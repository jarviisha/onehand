//! A fenced block of an answer, whole.
//!
//! In the transcript a block stops at its height cap and cannot scroll: the
//! transcript is a list whose wheel runs first, so a block scrolling in place
//! would take the wheel from the conversation. A block the cap cuts says how
//! many lines it holds, and opens them in a dialog, where they scroll outside
//! the list and are highlighted as the block was.

use super::metrics::{CODE_BLOCK_PAD, MAX_CODE_BLOCK_H};
use crate::controls::{Quiet as _, Short as _};
use crate::tokens::{CHROME_TEXT, CODE_TEXT, LEADING};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClipboardItem, InteractiveElement as _, ParentElement, Rems, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, StyledExt as _, WindowExt as _};

/// The most lines the dialog draws; past it, it says how many it left out
/// and Copy still takes them all.
const CODE_VIEW_MAX_LINES: usize = 2_000;
/// How wide the dialog opens: code is wide, so wider than a form.
const CODE_VIEW_W: Rems = rems(60.);
/// How tall the dialog's code stands before it scrolls.
const CODE_VIEW_MAX_H: Rems = rems(36.);

/// How many lines of code the capped block draws, from the cap, the well's
/// padding and the code's line height.
pub(super) fn lines_shown() -> usize {
    ((MAX_CODE_BLOCK_H.0 - 2. * CODE_BLOCK_PAD.0) / (CODE_TEXT.0 * LEADING)).floor() as usize
}

/// How many lines `code` holds, when the capped block cannot show them all.
// ponytail: counts lines, not wrapped rows, so a block of a few very long
// lines is not offered whole; measure the laid-out block if that bites.
pub(super) fn held_back(code: &str) -> Option<usize> {
    let total = code.lines().count();
    (total > lines_shown()).then_some(total)
}

/// The first `cap` lines of `code`, and how many it holds when they are not
/// all of it.
fn capped(code: &str, cap: usize) -> (&str, Option<usize>) {
    let total = code.lines().count();
    if total <= cap {
        return (code, None);
    }
    let end = code
        .match_indices('\n')
        .nth(cap - 1)
        .map_or(code.len(), |(at, _)| at);
    // A line ends on `\r\n` as `lines` reads it, so the cut does too.
    (
        code[..end].strip_suffix('\r').unwrap_or(&code[..end]),
        Some(total),
    )
}

/// `code` as one fenced block of Markdown in `lang`, fenced longer than any
/// run of backticks inside it, so a fence it holds does not end it.
fn fence(code: &str, lang: Option<&str>) -> String {
    let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let run = "`".repeat((longest + 1).max(3));
    format!("{run}{}\n{code}\n{run}", lang.unwrap_or(""))
}

/// The transcript's prose style without the height cap on a fenced block:
/// the dialog is where the block is read whole, and its own box scrolls.
fn uncapped(mut style: TextViewStyle) -> TextViewStyle {
    style.code_block.max_size.height = None;
    style.code_block.overflow = Default::default();
    style
}

/// Put `code` up whole, in `lang`, over the window: its first
/// [`CODE_VIEW_MAX_LINES`] lines scrolling, saying how many it left out, and
/// Copy taking every line.
pub(super) fn open(
    code: SharedString,
    lang: Option<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let width = crate::dialogs::width_in(window, CODE_VIEW_W);
    // Fenced once: the dialog is built again on every frame it shows.
    let (shown, total) = capped(&code, CODE_VIEW_MAX_LINES);
    let fenced = SharedString::from(fence(shown, lang.as_deref()));
    window.open_dialog(cx, move |dialog, window, cx| {
        let body = TextView::markdown("code-view-body", fenced.clone())
            .selectable(true)
            .style(uncapped(super::prose::prose_style(window, cx)));
        let code = code.clone();
        let title = lang
            .clone()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| "Code".into());
        dialog
            .w(width)
            .close_button(false)
            .child(crate::dialogs::title_row(title, cx))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .id("code-view-scroll")
                            .max_h(CODE_VIEW_MAX_H)
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .when_some(total, |col, total| {
                        col.child(
                            div()
                                .text_size(CHROME_TEXT)
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "Showing the first {CODE_VIEW_MAX_LINES} of {total} lines. \
                                     Copy takes them all."
                                )),
                        )
                    }),
            )
            .footer(
                crate::dialogs::footer_row()
                    .justify_end()
                    .child(
                        crate::controls::action("code-view-close")
                            .short()
                            .quiet(cx)
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("code-view-copy")
                            .short()
                            .primary()
                            .label("Copy")
                            .on_click(move |_, _, cx: &mut App| {
                                cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests;
