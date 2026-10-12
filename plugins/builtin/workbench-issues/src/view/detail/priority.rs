//! What an issue's body says about its priority, and how hard that presses.

use gpui::App;
use gpui_component::ActiveTheme;
use onehand_plugin_host::status_ink;

/// How hard a priority presses: 2 for one that cannot wait, 1 for a high
/// one, 0 for the rest, read off its words whatever the team calls them.
fn urgency(priority: &str) -> u8 {
    let lower = priority.to_lowercase();
    let says = |words: &[&str]| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| words.contains(&token))
    };
    if says(&["critical", "urgent", "blocker", "highest", "p0"]) {
        2
    } else if says(&["high", "p1"]) {
        1
    } else {
        0
    }
}

/// A priority's ink: danger for what cannot wait, warning for high, and
/// muted for the rest, so only what presses draws the eye.
pub(in crate::view) fn priority_ink(priority: &str, cx: &App) -> gpui::Hsla {
    let ink = status_ink(cx);
    match urgency(priority) {
        2 => ink.danger,
        1 => ink.warning,
        _ => cx.theme().muted_foreground,
    }
}

/// A priority's name, without what the body says after it: *High* of
/// *High: the footer lies*.
pub(in crate::view) fn priority_name(priority: &str) -> &str {
    priority.split(':').next().unwrap_or(priority).trim()
}

/// The first line under a *Priority* heading in `body`, list marker and bold
/// taken off, or `None` where the body has no such section or it is empty.
pub(in crate::view) fn priority(body: &str) -> Option<String> {
    let heading = |line: &str| line.trim_start().starts_with('#');
    let mut lines = body.lines();
    lines.find(|line| {
        heading(line)
            && line
                .trim()
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .eq_ignore_ascii_case("priority")
    })?;
    let first = lines
        .take_while(|line| !heading(line))
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let first = first
        .strip_prefix("- ")
        .or_else(|| first.strip_prefix("* "))
        .or_else(|| first.strip_prefix("+ "))
        .unwrap_or(first)
        .replace("**", "")
        .replace("__", "");
    let first = first.trim();
    (!first.is_empty()).then(|| first.to_string())
}

#[cfg(test)]
mod tests;
