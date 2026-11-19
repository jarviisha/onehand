//! The one box everything a row opens into is drawn in: what a command ran,
//! what a read or a search found, what an edit changed.
//!
//! **Never taller than its cap.** What is past it scrolls inside the box, in
//! both directions, under one bar on each of the box's edges, and the line
//! under the box says how much there is and offers it whole. A box that grew
//! in place pushed the rest of the turn off screen; one that is held to a size
//! keeps the reader where they were.

use super::*;
use gpui_component::button::ButtonVariants as _;

/// How many lines a box shows before it scrolls: the height it is drawn at.
pub(in crate::chat::transcript) const BOX_LINES: usize = 16;
/// How many lines a box draws at most, saying so when it leaves some out.
pub(in crate::chat::transcript) const BOX_MAX_LINES: usize = 400;
/// How many lines of a command a box shows before cutting it.
const COMMAND_LINES: usize = 4;
/// How long a one-line command runs before it is cut too: about four lines of
/// a box at its usual width.
const COMMAND_CUT_CHARS: usize = 320;

/// What one box holds.
pub(in crate::chat::transcript) struct DetailBox {
    /// Its key, which keeps its scroll offset from frame to frame.
    pub key: usize,
    /// What the step was told to do, over a hairline: a command.
    pub head: Option<gpui::AnyElement>,
    /// One element a line, each carrying its own side padding so a fill can
    /// run the box's whole width.
    pub lines: Vec<gpui::AnyElement>,
    /// How many lines there are in all, the ones not drawn included.
    pub total: usize,
    /// Opened at its end rather than its start: an output's result is its
    /// last lines.
    pub from_end: bool,
    /// What Copy hands over, whole.
    pub copy: Option<String>,
    /// Everything, and the language it is in, for *Open*.
    pub whole: Option<(SharedString, Option<SharedString>)>,
}

/// `detail` in its box, with the line under it where it holds more than shows.
pub(in crate::chat::transcript) fn detail_box(detail: DetailBox, cx: &App) -> gpui::Div {
    let key = detail.key;
    let total = detail.total;
    let from_end = detail.from_end;
    let whole = detail.whole;
    let lines = detail.lines;
    let head = detail.head;
    let copy = detail.copy;
    let has_lines = !lines.is_empty();
    let drawn = lines.len();
    let frame = plain_box(cx)
        .relative()
        .v_flex()
        .children(head.map(|head| {
            div()
                .px_3()
                .pt_2()
                .pb_2()
                .when(has_lines, |head| {
                    head.border_b_1().border_color(cx.theme().border)
                })
                // Clear of Copy in the corner: a control's width in from
                // the bar's.
                .child(
                    div()
                        .mr(SCROLLBAR_W)
                        .pr(crate::tokens::CONTROL_H)
                        .child(head),
                )
        }))
        .when(has_lines, |frame| {
            frame.child(ScrollBody {
                key,
                max_h: DETAIL_OPEN_H,
                from_end,
                children: vec![
                    // At least the box's width, so a line's fill reaches its
                    // edge however short the longest line is.
                    div()
                        .v_flex()
                        .min_w_full()
                        .py_2()
                        .children(lines)
                        .into_any_element(),
                ],
            })
        })
        // **Over the corner, clear of the bar**, on the box's own fill so a
        // long first line runs under it rather than through it.
        .children(copy.map(|text| {
            div()
                .absolute()
                .top(TIGHT_GAP)
                .right(SCROLLBAR_W)
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .child(copy_button(("detail-copy", key), text, "Copy this", cx))
        }));
    let foot = box_count(total, drawn, from_end).map(|words| {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(words)
            .children(whole.map(|(text, lang)| {
                crate::controls::action(("detail-open", key))
                    .secondary()
                    .short()
                    .label("Open")
                    .on_click(move |_, window, cx| {
                        super::super::code_view::open(text.clone(), lang.clone(), window, cx)
                    })
            }))
    });
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .min_w_0()
        .child(frame)
        .children(foot)
}

/// What the line under a box says: how many lines it holds, and how many of
/// them it draws where that is fewer; nothing for one that shows them all.
pub(in crate::chat::transcript) fn box_count(
    total: usize,
    drawn: usize,
    from_end: bool,
) -> Option<String> {
    if total <= BOX_LINES {
        return None;
    }
    Some(match (drawn < total, from_end) {
        (false, _) => format!("{total} lines"),
        (true, true) => format!("Last {drawn} of {total} lines"),
        (true, false) => format!("First {drawn} of {total} lines"),
    })
}

/// How many lines a command has, where it is long enough to be cut: past four
/// lines, or one line long enough to wrap past them.
pub(in crate::chat::transcript) fn command_cut(command: &str) -> Option<usize> {
    let lines = command.lines().count();
    (lines > COMMAND_LINES || command.chars().count() > COMMAND_CUT_CHARS).then_some(lines)
}

/// A command as a box's head: whole, wrapped so it reads as typed, cut at four
/// lines until `whole`, a dollar ahead of it.
pub(in crate::chat::transcript) fn command_head(command: &str, whole: bool, cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child("$"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(cx.theme().foreground)
                .when(!whole, |text| text.line_clamp(COMMAND_LINES))
                .child(command.to_string()),
        )
}

#[cfg(test)]
mod tests {
    use super::{BOX_LINES, box_count, command_cut};

    #[test]
    fn a_box_says_how_much_it_holds_only_past_its_cap() {
        assert_eq!(box_count(BOX_LINES, BOX_LINES, false), None);
        assert_eq!(box_count(90, 90, true).as_deref(), Some("90 lines"));
        assert_eq!(
            box_count(900, 400, true).as_deref(),
            Some("Last 400 of 900 lines")
        );
        assert_eq!(
            box_count(900, 400, false).as_deref(),
            Some("First 400 of 900 lines")
        );
    }

    #[test]
    fn a_command_is_cut_past_four_lines_or_one_long_line() {
        assert_eq!(command_cut("cargo test"), None);
        assert_eq!(command_cut("a\nb\nc\nd"), None);
        assert_eq!(command_cut("a\nb\nc\nd\ne"), Some(5));
        assert_eq!(command_cut(&"x".repeat(400)), Some(1));
    }
}
