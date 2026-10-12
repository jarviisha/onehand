//! The four kinds of toast, and the one shape every toast's words take.
//!
//! The kind is chosen by what the news means: an action that failed, one done
//! only in part or that now needs the person, a long or background action
//! finished, or news that asks for nothing. A toast is one sentence with no
//! trailing period, which is trimmed here rather than at every source: much of
//! what a toast says is a reason written for other places too (a log line, an
//! issue comment), where the sentence keeps its period.

use gpui_component::notification::Notification;
use std::fmt::Display;

/// An action a person started that failed: *<What> failed: <why>*.
pub(crate) fn failed(what: &str, why: impl Display) -> Notification {
    Notification::error(failed_line(what, &why.to_string()))
}

/// An action that failed with no reason to give beyond its own name.
pub(crate) fn error(text: impl Display) -> Notification {
    Notification::error(line(&text.to_string()))
}

/// Done only in part, or something that now needs the person.
pub(crate) fn warning(text: impl Display) -> Notification {
    Notification::warning(line(&text.to_string()))
}

/// A long or background action finished, which the screen does not already
/// show.
pub(crate) fn success(text: impl Display) -> Notification {
    Notification::success(line(&text.to_string()))
}

/// News that asks for nothing.
pub(crate) fn info(text: impl Display) -> Notification {
    Notification::info(line(&text.to_string()))
}

fn failed_line(what: &str, why: &str) -> String {
    line(&format!("{what} failed: {why}"))
}

/// `text` as a toast's one sentence: trimmed, its trailing period gone.
fn line(text: &str) -> String {
    let text = text.trim();
    text.strip_suffix('.').unwrap_or(text).to_string()
}

#[cfg(test)]
mod tests {
    use super::{failed_line, line};

    #[test]
    fn a_toast_is_one_sentence_without_its_period() {
        assert_eq!(line("Removed the folder."), "Removed the folder");
        assert_eq!(line("  Reading…  "), "Reading…");
        assert_eq!(line("No period"), "No period");
        assert_eq!(
            failed_line("Saving the shortcuts", "permission denied."),
            "Saving the shortcuts failed: permission denied"
        );
    }
}
