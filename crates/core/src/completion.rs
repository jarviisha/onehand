//! Pure `@`-file-mention and `/`-command completion logic. The composer popup
//! is driven entirely by these GUI-free functions: detect an active trigger at
//! the caret, filter candidates, and apply a chosen completion back into the
//! text.

/// What the caret is currently completing, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTrigger {
    pub kind: TriggerKind,
    /// Byte index of the trigger char (`@` or `/`) in the source string.
    pub start: usize,
    /// The query text typed after the trigger (may be empty).
    pub query: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    /// `@` — file mention.
    File,
    /// `/` — slash command (only valid at the very start of the input).
    Command,
}

/// Detect a completion trigger ending at byte offset `caret` in `text`.
///
/// Rules:
/// - An `@` opens a file mention; the query runs from just after `@` to the
///   caret and must not contain whitespace.
/// - A leading `/` (the input starts with it) opens a command; the query is the
///   rest, no whitespace.
pub fn detect(text: &str, caret: usize) -> Option<ActiveTrigger> {
    let caret = caret.min(text.len());
    let before = &text[..caret];

    // Slash command: only when the whole input begins with '/'.
    if let Some(query) = before.strip_prefix('/') {
        if !query.contains(char::is_whitespace) {
            return Some(ActiveTrigger {
                kind: TriggerKind::Command,
                start: 0,
                query: query.to_string(),
            });
        }
    }

    // File mention: nearest '@' to the left with no whitespace between it and
    // the caret, and either at the start or preceded by whitespace.
    if let Some(at) = before.rfind('@') {
        let query = &before[at + 1..];
        let preceded_ok = at == 0
            || before[..at]
                .chars()
                .next_back()
                .map(char::is_whitespace)
                .unwrap_or(false);
        if preceded_ok && !query.contains(char::is_whitespace) {
            return Some(ActiveTrigger {
                kind: TriggerKind::File,
                start: at,
                query: query.to_string(),
            });
        }
    }
    None
}

/// Case-insensitive substring filter over candidate labels, preserving order.
pub fn filter<'a>(candidates: &'a [String], query: &str) -> Vec<&'a String> {
    let q = query.to_lowercase();
    candidates
        .iter()
        .filter(|c| q.is_empty() || c.to_lowercase().contains(&q))
        .collect()
}

/// Apply `choice` for `trigger` to `text`, replacing the trigger+query span.
/// Returns the new text and the new caret byte offset.
///
/// A file mention keeps its `@` prefix and gets a trailing space; a command
/// replaces the whole `/query` with `/choice` and a trailing space.
pub fn apply(text: &str, caret: usize, trigger: &ActiveTrigger, choice: &str) -> (String, usize) {
    let caret = caret.min(text.len());
    let replacement = match trigger.kind {
        TriggerKind::File => format!("@{choice} "),
        TriggerKind::Command => format!("/{choice} "),
    };
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..trigger.start]);
    out.push_str(&replacement);
    out.push_str(&text[caret..]);
    let new_caret = trigger.start + replacement.len();
    (out, new_caret)
}

/// Directories never descended into when scanning for `@`-mention candidates.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".jj",
    "target",
    "node_modules",
    ".venv",
    "dist",
    "build",
];

/// Scan `root` recursively for files, returning their root-relative paths
/// (forward-slashed), capped at `limit`. Hidden and heavy build dirs are
/// skipped. This is the `@`-file-mention candidate source; it does IO, so it is
/// kept out of the pure detect/filter/apply core above.
pub fn scan_files(root: &std::path::Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                        if out.len() >= limit {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_mention() {
        let t = detect("hello @ma", 9).unwrap();
        assert_eq!(t.kind, TriggerKind::File);
        assert_eq!(t.start, 6);
        assert_eq!(t.query, "ma");
    }

    #[test]
    fn at_must_be_word_start() {
        // '@' inside a word (no preceding whitespace) is not a trigger.
        assert_eq!(detect("foo@bar", 7), None);
    }

    #[test]
    fn whitespace_in_query_cancels() {
        assert_eq!(detect("@foo bar", 8), None);
    }

    #[test]
    fn detects_leading_command() {
        let t = detect("/he", 3).unwrap();
        assert_eq!(t.kind, TriggerKind::Command);
        assert_eq!(t.start, 0);
        assert_eq!(t.query, "he");
    }

    #[test]
    fn slash_not_at_start_is_not_command() {
        // A '/' mid-line is not a command (no '@' either) → nothing.
        assert_eq!(detect("a /b", 4), None);
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        let cands = vec!["README.md".to_string(), "src/main.rs".to_string()];
        let got = filter(&cands, "main");
        assert_eq!(got, vec![&"src/main.rs".to_string()]);
    }

    #[test]
    fn apply_file_mention() {
        let trig = detect("see @ma", 7).unwrap();
        let (text, caret) = apply("see @ma", 7, &trig, "src/main.rs");
        assert_eq!(text, "see @src/main.rs ");
        assert_eq!(caret, text.len());
    }

    #[test]
    fn apply_command_replaces_whole() {
        let trig = detect("/he", 3).unwrap();
        let (text, caret) = apply("/he", 3, &trig, "help");
        assert_eq!(text, "/help ");
        assert_eq!(caret, "/help ".len());
    }
}
