//! A table of an answer, whole.
//!
//! In the transcript a table scrolls sideways in its own track (the vertical
//! wheel still reaches the conversation), which reads a few columns past the
//! edge but not a wide table at once. Every table opens here too, in a dialog
//! as wide as the window allows, where its rows scroll outside the list.

use super::code_view::capped;
use super::metrics::TEXT;
use crate::controls::{Quiet as _, Short as _};
use crate::tokens::{CHROME_TEXT, LEADING};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClipboardItem, InteractiveElement as _, ParentElement, Rems, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, relative, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::text::{TableData, TextView};
use gpui_component::{ActiveTheme, StyledExt as _, WindowExt as _};

/// The most rows the dialog draws; past it, it says how many it left out and
/// Copy still takes them all.
const TABLE_VIEW_MAX_ROWS: usize = 1_000;
/// How wide the dialog asks to open: a wide table is the reason it exists, so
/// the window's width, less its margins, is what it usually gets.
const TABLE_VIEW_W: Rems = rems(96.);
/// How tall the dialog's table stands before it scrolls.
const TABLE_VIEW_MAX_H: Rems = rems(36.);

/// Put `table` up whole over the window: its first [`TABLE_VIEW_MAX_ROWS`]
/// rows scrolling both ways, saying how many it left out, and Copy taking the
/// whole table as Markdown.
pub(super) fn open(table: &TableData, window: &mut Window, cx: &mut App) {
    let width = crate::dialogs::width_in(window, TABLE_VIEW_W);
    let markdown = SharedString::from(table.markdown.clone());
    let rows = table.rows.len();
    // A pipe table is a line per row under its head and delimiter lines, so
    // the cut is by lines. Cut once: the dialog is built on every frame.
    let (shown, cut) = capped(&markdown, TABLE_VIEW_MAX_ROWS + 2);
    let shown = SharedString::from(shown.to_string());
    let held_back = cut.is_some();
    window.open_dialog(cx, move |dialog, window, cx| {
        let body = TextView::markdown("table-view-body", shown.clone())
            .selectable(true)
            .style(super::prose::prose_style(window, cx));
        let markdown = markdown.clone();
        dialog
            .w(width)
            .close_button(false)
            .child(crate::dialogs::title_row("Table", cx))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .id("table-view-scroll")
                            .max_h(TABLE_VIEW_MAX_H)
                            .overflow_y_scroll()
                            // At the transcript's reading size: the renderer
                            // sizes each column by text measured at the size
                            // it inherits, and the table reads as it did.
                            .text_size(TEXT)
                            .line_height(relative(LEADING))
                            .child(body),
                    )
                    .when(held_back, |col| {
                        col.child(
                            div()
                                .text_size(CHROME_TEXT)
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "Showing the first {TABLE_VIEW_MAX_ROWS} of {rows} rows. \
                                     Copy takes them all."
                                )),
                        )
                    }),
            )
            .footer(
                crate::dialogs::footer_row()
                    .justify_end()
                    .child(
                        crate::controls::action("table-view-close")
                            .short()
                            .quiet(cx)
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("table-view-copy")
                            .short()
                            .primary()
                            .label("Copy")
                            .on_click(move |_, _, cx: &mut App| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    markdown.to_string(),
                                ));
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::capped;

    #[test]
    fn the_cut_keeps_the_head_and_counts_rows_past_it() {
        let table = "| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |";
        let (shown, cut) = capped(table, 2 + 2);
        assert_eq!(shown, "| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |");
        assert!(cut.is_some());
        assert_eq!(capped(table, 3 + 2), (table, None));
    }
}
