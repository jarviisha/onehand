//! The project's files an issue's body names, found so the body can open them.
//!
//! Found in the text rather than in the parsed document because the renderer
//! keeps its tree to itself: what it does offer is a hook on a link being
//! pressed, so a path that exists is rewritten into a link of its own scheme
//! before the body is parsed, and the hook opens it.

use std::ops::Range;

/// The scheme of a link the body gains for a file, which the link hook opens
/// in the editor instead of handing to the system.
pub(super) const FILE_LINK: &str = "onehand-file:";

/// A path the body names, where it says it, and whether it says it as code.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Mention {
    /// The bytes it takes in the body: a code span with its backticks, or a
    /// bare word with the punctuation round it left out.
    pub(super) range: Range<usize>,
    /// The path relative to the project, without a `./` or a `:line`.
    pub(super) path: String,
}

/// Every path-shaped thing in `body`, in order. Fenced and indented code
/// blocks are skipped — they are quoted text, not references — and so are
/// links and addresses, which already go somewhere.
pub(super) fn mentions(body: &str) -> Vec<Mention> {
    let mut found = Vec::new();
    let mut fenced = false;
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        scan_line(line, start, &mut found);
    }
    found
}

/// `body` with every mention whose path is in `exists` made a link, drawn as
/// code so a bare path and a quoted one read the same.
pub(super) fn linked(body: &str, mentions: &[Mention], exists: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut at = 0;
    for mention in mentions.iter().filter(|m| exists.contains(&m.path)) {
        let said = &body[mention.range.clone()];
        out.push_str(&body[at..mention.range.start]);
        if said.starts_with('`') {
            out.push_str(&format!("[{said}](<{FILE_LINK}{}>)", mention.path));
        } else {
            out.push_str(&format!("[`{said}`](<{FILE_LINK}{}>)", mention.path));
        }
        at = mention.range.end;
    }
    out.push_str(&body[at..]);
    out
}

/// One line: its code spans, and the bare words between them.
fn scan_line(line: &str, start: usize, found: &mut Vec<Mention>) {
    let mut at = 0;
    while let Some(tick) = line[at..].find('`') {
        let open = at + tick;
        bare(&line[at..open], start + at, found);
        let run = line[open..].chars().take_while(|c| *c == '`').count();
        let fence = &line[open..open + run];
        let Some(close) = line[open + run..].find(fence) else {
            // An unclosed backtick is a backtick, and what follows is prose.
            at = open + run;
            break;
        };
        let inner = &line[open + run..open + run + close];
        // Code that is a link's text already goes somewhere.
        let in_link = line[..open].ends_with('[');
        if let Some(path) = path_like(inner.trim()).filter(|_| !in_link) {
            found.push(Mention {
                range: start + open..start + open + run + close + run,
                path,
            });
        }
        at = open + run + close + run;
    }
    bare(&line[at..], start + at, found);
}

/// The words of `text`, which starts `start` bytes into the body, that are
/// paths once the punctuation round them is taken off.
fn bare(text: &str, start: usize, found: &mut Vec<Mention>) {
    const LEADING: &[char] = &['(', '[', '"', '\'', '*', '_', '<'];
    const TRAILING: &[char] = &[
        '.', ',', ';', ':', '!', '?', ')', ']', '"', '\'', '*', '_', '>',
    ];
    for word in text.split(char::is_whitespace).filter(|w| !w.is_empty()) {
        if word.contains("://") || word.contains("](") || word.starts_with("![") {
            continue;
        }
        let offset = word.as_ptr() as usize - text.as_ptr() as usize;
        let lead = word.len() - word.trim_start_matches(LEADING).len();
        let core = word[lead..].trim_end_matches(TRAILING);
        if let Some(path) = path_like(core) {
            let from = start + offset + lead;
            found.push(Mention {
                range: from..from + core.len(),
                path,
            });
        }
    }
}

/// `text` as a path relative to the project, if it is shaped like one: at
/// least one `/`, not absolute, nothing that climbs out, and only the
/// characters file names here are made of. `./` and a `:line[:col]` are
/// taken off. Whether it exists is asked later, off the UI loop.
fn path_like(text: &str) -> Option<String> {
    let mut path = text.strip_prefix("./").unwrap_or(text);
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => {
                path = head
            }
            _ => break,
        }
    }
    let shaped = path.contains('/')
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains("//")
        && !path.split('/').any(|part| part == "..")
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '@' | '+'));
    shaped.then(|| path.to_string())
}

#[cfg(test)]
mod tests;
