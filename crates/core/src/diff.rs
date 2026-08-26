//! Line diffing for the transcript's `EDIT` sections.
//!
//! ACP hands over a tool call's edit as `{ path, old_text, new_text }` — two
//! whole files, with no hunk information. The transcript used to render that
//! literally: every line of `old` with a `-`, then every line of `new` with a
//! `+`. A one-line change in a 300-line file read as "the whole file was
//! deleted and a different one written", and because the render budget is spent
//! in order, a file longer than the budget showed a screen of nothing but
//! deletions and a "truncated" note — the actual change never reached the
//! screen at all.
//!
//! So the hunks are computed here. Pure and GUI-free, which is where this kind
//! of rule belongs (shared rules live in core).

/// One line of a rendered diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line<'a> {
    Context(&'a str),
    Removed(&'a str),
    Added(&'a str),
    /// `n` unchanged lines skipped between two hunks.
    Skipped(usize),
}

/// One line of a rendered diff, owning its text.
///
/// The borrowed [`Line`] is what the algorithm produces; this is what a caller
/// can *keep*. Computing hunks is quadratic in the worst case and the answer
/// only changes when the edit does, so it belongs beside the edit — held once,
/// not recomputed by whoever happens to be drawing it. Elision means a row list
/// is roughly the size of the change rather than the size of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Context(String),
    Removed(String),
    Added(String),
    Skipped(usize),
}

/// Diff `old` against `new` and keep the result.
pub fn rows(old: &str, new: &str) -> Vec<Row> {
    lines(old, new)
        .into_iter()
        .map(|line| match line {
            Line::Context(l) => Row::Context(l.to_string()),
            Line::Removed(l) => Row::Removed(l.to_string()),
            Line::Added(l) => Row::Added(l.to_string()),
            Line::Skipped(n) => Row::Skipped(n),
        })
        .collect()
}

/// How many unchanged lines to keep either side of a change.
pub const CONTEXT: usize = 3;

/// Diff `old` against `new`, line by line, with [`CONTEXT`] lines of context
/// and a [`Line::Skipped`] marker standing in for each elided run.
///
/// The whole point is that the caller's render budget is spent on *changes*, so
/// a long file with a small edit still shows the edit.
pub fn lines<'a>(old: &'a str, new: &'a str) -> Vec<Line<'a>> {
    let a: Vec<&str> = split(old);
    let b: Vec<&str> = split(new);
    with_context(&script(&a, &b))
}

/// Split into lines, treating "" as no lines rather than one empty one — a
/// created file has no `old` side, and an empty first row would render as a
/// spurious deletion.
fn split(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.lines().collect()
    }
}

/// Cap on the LCS table. Above this the quadratic table is both slow and
/// pointless — the render budget is a couple of hundred lines either way — so
/// the whole-file fallback stands in, exactly as before.
const MAX_LCS_LINES: usize = 3000;

/// The edit script: `Context` / `Removed` / `Added`, no elision yet.
fn script<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Line<'a>> {
    // Trimming the common head and tail first is what keeps the table small for
    // the case this exists for: a large file with a small edit in the middle.
    let head = a
        .iter()
        .zip(b.iter())
        .take_while(|(x, y)| x == y)
        .count()
        .min(a.len().min(b.len()));
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();

    let (mid_a, mid_b) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);

    let mut out: Vec<Line<'a>> = a[..head].iter().copied().map(Line::Context).collect();

    if mid_a.len().max(mid_b.len()) > MAX_LCS_LINES {
        // Too big to diff properly: fall back to replace-the-block. Still far
        // better than before, because the untouched head and tail stay context.
        out.extend(mid_a.iter().copied().map(Line::Removed));
        out.extend(mid_b.iter().copied().map(Line::Added));
    } else {
        out.extend(lcs_script(mid_a, mid_b));
    }

    out.extend(a[a.len() - tail..].iter().copied().map(Line::Context));
    out
}

/// Classic LCS-table diff over the trimmed middle.
fn lcs_script<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Line<'a>> {
    let (n, m) = (a.len(), b.len());
    // `table[i][j]` = length of the LCS of `a[i..]` and `b[j..]`.
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(Line::Context(a[i]));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            out.push(Line::Removed(a[i]));
            i += 1;
        } else {
            out.push(Line::Added(b[j]));
            j += 1;
        }
    }
    out.extend(a[i..].iter().copied().map(Line::Removed));
    out.extend(b[j..].iter().copied().map(Line::Added));
    out
}

/// Collapse runs of context longer than `2 * CONTEXT` into a [`Line::Skipped`].
fn with_context<'a>(script: &[Line<'a>]) -> Vec<Line<'a>> {
    let changed: Vec<bool> = script
        .iter()
        .map(|l| !matches!(l, Line::Context(_)))
        .collect();
    if !changed.iter().any(|c| *c) {
        // Identical sides. ACP sends these (a tool that rewrote a file with the
        // same bytes), and showing the whole file as context is noise.
        return Vec::new();
    }

    // A line is kept when a change is within CONTEXT of it.
    let keep: Vec<bool> = (0..script.len())
        .map(|i| {
            let lo = i.saturating_sub(CONTEXT);
            let hi = (i + CONTEXT + 1).min(script.len());
            changed[lo..hi].iter().any(|c| *c)
        })
        .collect();

    let mut out = Vec::new();
    let mut skipped = 0usize;
    for (i, line) in script.iter().enumerate() {
        if keep[i] {
            if skipped > 0 {
                out.push(Line::Skipped(skipped));
                skipped = 0;
            }
            out.push(line.clone());
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(Line::Skipped(skipped));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_line_change_in_a_long_file_shows_the_change() {
        // The case the old renderer got wrong: 300 lines, one edit. It used to
        // emit 600 rows, the first 300 of them deletions, and the render budget
        // ran out long before the edit.
        let old: String = (0..300)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat();
        let new = old.replace("line 150\n", "line 150 CHANGED\n");

        let diff = lines(&old, &new);
        assert!(
            diff.len() < 20,
            "expected a small hunk, got {} rows",
            diff.len()
        );
        assert!(diff.contains(&Line::Removed("line 150")));
        assert!(diff.contains(&Line::Added("line 150 CHANGED")));
        assert!(diff.contains(&Line::Context("line 149")));
        // And the 290-odd untouched lines are accounted for, not dropped
        // silently.
        let skipped: usize = diff
            .iter()
            .filter_map(|l| match l {
                Line::Skipped(n) => Some(*n),
                _ => None,
            })
            .sum();
        assert_eq!(skipped, 300 - (diff.len() - 3));
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let diff = lines("", "a\nb\n");
        assert_eq!(diff, vec![Line::Added("a"), Line::Added("b")]);
    }

    #[test]
    fn a_deleted_body_is_all_removals() {
        let diff = lines("a\nb\n", "");
        assert_eq!(diff, vec![Line::Removed("a"), Line::Removed("b")]);
    }

    #[test]
    fn identical_sides_produce_nothing() {
        assert!(lines("a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn an_insertion_is_not_reported_as_a_rewrite() {
        let diff = lines("a\nb\nc\n", "a\nb\nx\nc\n");
        assert_eq!(
            diff,
            vec![
                Line::Context("a"),
                Line::Context("b"),
                Line::Added("x"),
                Line::Context("c"),
            ]
        );
    }

    #[test]
    fn separate_edits_become_separate_hunks() {
        let old: String = (0..60).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l5\n", "l5!\n").replace("l50\n", "l50!\n");
        let diff = lines(&old, &new);
        let gaps = diff
            .iter()
            .filter(|l| matches!(l, Line::Skipped(_)))
            .count();
        // Head gap, middle gap, tail gap -- the middle one is the point: two
        // hunks, not one run spanning the file.
        assert!(gaps >= 2, "expected separate hunks, got {diff:?}");
    }
}
