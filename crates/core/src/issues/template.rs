//! Issue templates, and what a body written from one leaves out.
//!
//! A template only fills a new issue's body in: it adds no field and no state,
//! and an issue written without one is worked as it is. What a body lacks is
//! read against the template it matches, and only that one: an issue written
//! its own way, or under other headings, is never told it lacks anything.
//! It is advice, never a refusal; whether the work meets what the issue asks
//! stays the reviewer's to judge.

/// A template a new issue can be written from: its name, the body it fills
/// in, and the labels it puts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueTemplate {
    pub name: String,
    pub body: String,
    pub labels: Vec<String>,
}

/// The templates onehand ships: the same four headings, each worded for its
/// kind, with a hint under each that the reader ignores.
pub fn shipped() -> Vec<IssueTemplate> {
    let body = |problem: &str, scope: &str, acceptance: &str, check: &str| {
        format!(
            "## Problem\n<!-- {problem} -->\n\n## Scope\n<!-- {scope} -->\n\n\
             ## Acceptance\n<!-- {acceptance} -->\n\n## How to check\n<!-- {check} -->\n"
        )
    };
    vec![
        IssueTemplate {
            name: "Bug".to_string(),
            body: body(
                "What happens, what should, and who meets it.",
                "What may change, and what must not.",
                "How you will judge the fix. One line each.",
                "The command, the steps or the screen that shows it is fixed.",
            ),
            labels: vec!["bug".to_string()],
        },
        IssueTemplate {
            name: "Feature".to_string(),
            body: body(
                "What is missing, and who needs it.",
                "What may change, and what must not.",
                "How you will judge the work. One line each.",
                "The command, the steps or the screen that shows it works.",
            ),
            labels: Vec::new(),
        },
        IssueTemplate {
            name: "Refactor".to_string(),
            body: body(
                "What is hard to change today, and why it matters.",
                "What may move, and what must behave exactly as before.",
                "How you will judge the result. One line each.",
                "The tests or checks that show nothing changed for a user.",
            ),
            labels: Vec::new(),
        },
    ]
}

/// Where a project keeps issue templates of its own, as its forge reads them.
const PROJECT_TEMPLATES: &str = ".github/ISSUE_TEMPLATE";

/// The templates a new issue in the project at `root` is offered: its own,
/// whole, when it keeps any that read; the shipped three otherwise. Reads
/// the disk, so never on the UI thread.
pub fn for_project_blocking(root: &std::path::Path) -> Vec<IssueTemplate> {
    let mut files: Vec<_> = std::fs::read_dir(root.join(PROJECT_TEMPLATES))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    own_or_shipped(
        files
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|text| from_file(&text))
            .collect(),
    )
}

/// A project's own templates when there are any, or the shipped three: one
/// of its own replaces them all, never mixes with them.
fn own_or_shipped(own: Vec<IssueTemplate>) -> Vec<IssueTemplate> {
    if own.is_empty() {
        shipped()
    } else {
        own
    }
}

/// A Markdown issue template as a forge keeps it: front matter giving its
/// `name` and `labels`, the rest its body. `None` for a file with no front
/// matter, none closed, or no name, which a forge would not offer either.
fn from_file(text: &str) -> Option<IssueTemplate> {
    let rest = text.strip_prefix("---")?.trim_start_matches([' ', '\t']);
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let mut lines = rest.split_inclusive('\n');
    let mut front = Vec::new();
    let mut read = 0;
    loop {
        let line = lines.next()?;
        read += line.len();
        if line.trim_end() == "---" {
            break;
        }
        front.push(line.trim_end());
    }
    let body = rest[read..].trim_start_matches(['\r', '\n']).to_string();
    let (mut name, mut labels) = (None, Vec::new());
    let mut at = 0;
    while at < front.len() {
        let line = front[at];
        at += 1;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "name" => name = Some(unquoted(value)).filter(|n| !n.is_empty()),
            "labels" if value.is_empty() => {
                while let Some(item) = front.get(at).and_then(|l| l.trim().strip_prefix('-')) {
                    labels.push(unquoted(item.trim()));
                    at += 1;
                }
            }
            "labels" => {
                let listed = value.strip_prefix('[').and_then(|v| v.strip_suffix(']'));
                labels = listed.unwrap_or(value).split(',').map(unquoted).collect();
            }
            _ => {}
        }
    }
    labels.retain(|label| !label.is_empty());
    Some(IssueTemplate {
        name: name?,
        body,
        labels,
    })
}

/// A front matter value with its quotes taken off.
fn unquoted(value: &str) -> String {
    let value = value.trim();
    ['"', '\'']
        .into_iter()
        .find_map(|q| value.strip_prefix(q).and_then(|v| v.strip_suffix(q)))
        .unwrap_or(value)
        .to_string()
}

/// The sections a body leaves empty or out, by the template it matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lacking {
    /// The matched template's headings the body lacks, as the template writes them.
    missing: Vec<String>,
}

impl Lacking {
    /// As a person is told: *No acceptance written*.
    pub fn said(&self) -> String {
        format!("No {} written", self.listed())
    }

    /// As a report says it: *The issue has no acceptance written.*
    pub fn note(&self) -> String {
        format!("The issue has no {} written.", self.listed())
    }

    fn listed(&self) -> String {
        let lower: Vec<String> = self.missing.iter().map(|m| m.to_lowercase()).collect();
        match lower.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        }
    }
}

/// What `body` lacks of the template among `templates` it matches best, or
/// `None` when it matches none or lacks nothing.
///
/// A body matches a template carrying at least half its headings; the one
/// carrying the most wins, the first on a tie. A section is empty when, once
/// HTML comments and whitespace are taken out, nothing is left between its
/// heading and the next of the same or a higher level but sub-headings.
pub fn lacking(body: &str, templates: &[IssueTemplate]) -> Option<Lacking> {
    let lines = read(body);
    let found: Vec<&Heading> = lines
        .iter()
        .filter_map(|line| line.heading.as_ref())
        .collect();
    let wanted = templates
        .iter()
        .map(|template| (template, headings(&template.body)))
        .filter(|(_, wanted)| !wanted.is_empty())
        .map(|(template, wanted)| {
            let carried = wanted
                .iter()
                .filter(|w| found.iter().any(|f| f.text == w.text))
                .count();
            (template, wanted, carried)
        })
        .filter(|(_, wanted, carried)| *carried > 0 && carried * 2 >= wanted.len())
        .fold(
            None::<(&IssueTemplate, Vec<Heading>, usize)>,
            |best, next| match best {
                Some(best) if best.2 >= next.2 => Some(best),
                _ => Some(next),
            },
        )
        .map(|(_, wanted, _)| wanted)?;
    let missing: Vec<String> = wanted
        .into_iter()
        .filter(|w| !written(&lines, &w.text))
        .map(|w| w.raw)
        .collect();
    (!missing.is_empty()).then_some(Lacking { missing })
}

/// A heading as read: its level, its text compared lowercase, and its text
/// as written.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Heading {
    level: usize,
    text: String,
    raw: String,
}

/// A line of a body, and the heading it is, if it is one outside a fence.
struct Line<'a> {
    text: &'a str,
    heading: Option<Heading>,
}

/// The headings of `body`, in order, skipping fenced code.
fn headings(body: &str) -> Vec<Heading> {
    read(body)
        .into_iter()
        .filter_map(|line| line.heading)
        .collect()
}

/// `body`'s lines, each with the heading it is. A line inside a fenced code
/// block, fences included, is never a heading: a `# comment` in a shell
/// sample is code.
fn read(body: &str) -> Vec<Line<'_>> {
    let mut fence: Option<char> = None;
    body.lines()
        .map(|text| {
            let trimmed = text.trim_start();
            let opens = ['`', '~']
                .into_iter()
                .find(|c| trimmed.starts_with(&c.to_string().repeat(3)));
            match (fence, opens) {
                (Some(open), Some(c)) if c == open => fence = None,
                (Some(_), _) => {}
                (None, Some(c)) => fence = Some(c),
                (None, None) => {
                    return Line {
                        text,
                        heading: heading(text),
                    };
                }
            }
            Line {
                text,
                heading: None,
            }
        })
        .collect()
}

/// `line` as an ATX heading, `#` to `######` and up to three spaces in.
fn heading(line: &str) -> Option<Heading> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    let rest = &trimmed[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let raw = rest.trim().trim_end_matches('#').trim().to_string();
    Some(Heading {
        level,
        text: raw.to_lowercase(),
        raw,
    })
}

/// Whether the section headed `text` in `lines` holds anything.
fn written(lines: &[Line<'_>], text: &str) -> bool {
    section_of(lines, text).is_some()
}

/// What `body` writes under the heading `heading`, matched as a reader would
/// (any level, any case), its hints left out; `None` when it is absent or
/// holds nothing.
pub fn section(body: &str, heading: &str) -> Option<String> {
    section_of(&read(body), &heading.to_lowercase())
}

/// What the section headed `text` in `lines` holds, without its comments;
/// `None` when it is absent or empty.
fn section_of(lines: &[Line<'_>], text: &str) -> Option<String> {
    let at = lines
        .iter()
        .position(|line| line.heading.as_ref().is_some_and(|h| h.text == text))?;
    let level = lines[at].heading.as_ref().map_or(0, |h| h.level);
    let section: Vec<&str> = lines[at + 1..]
        .iter()
        .take_while(|line| line.heading.as_ref().is_none_or(|h| h.level > level))
        .filter(|line| line.heading.is_none())
        .map(|line| line.text)
        .collect();
    let kept = uncommented(&section.join("\n")).trim().to_string();
    (!kept.is_empty()).then_some(kept)
}

/// `text` with its HTML comments taken out; one never closed runs to the end.
fn uncommented(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 4..].find("-->") {
            Some(end) => &rest[start + 4 + end + 3..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
