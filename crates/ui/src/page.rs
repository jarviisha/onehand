//! The top of a page: its head, and the search and filters a page that has
//! them keeps in reach over what scrolls.
//!
//! Every workspace-wide page starts the same way, whether its body is a
//! column or a list the pane's whole width, so moving between them moves
//! nothing but the width.

use crate::Tall as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, Entity, ParentElement as _, Rems, SharedString, Styled as _, div, rems,
};
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// A page's head, in its body: the page's name at the title size, a line of
/// facts under it in the muted ink, and its actions at the end, each at the
/// head's size. The pane's header above it carries only chrome.
pub fn page_head(
    title: impl Into<SharedString>,
    facts: Option<AnyElement>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_4()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_2xl()
                        .font_semibold()
                        .child(title.into()),
                )
                .children(
                    facts.map(|facts| div().text_color(cx.theme().muted_foreground).child(facts)),
                ),
        )
        .child(
            div()
                .flex_none()
                .h_flex()
                .items_center()
                .gap_2()
                .children(actions),
        )
}

/// The part of a page that stays put while the rest scrolls: `parts` (the
/// head, then the search and filters) in a column `width` wide, centred, or
/// the pane's whole width with none; inset as a page's body is at its sides,
/// close under the pane's header, which already stands clear of it. No line
/// parts it from what scrolls: the head's own size says where the page begins.
pub fn page_top(width: Option<Rems>, parts: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .flex_none()
        .h_flex()
        .justify_center()
        .text_size(crate::tokens::CHROME_TEXT)
        .px_8()
        .pt_2()
        .pb_4()
        .child(
            div()
                .v_flex()
                .gap_6()
                .w_full()
                .when_some(width, |column, width| column.max_w(width))
                .children(parts),
        )
}

/// The least a page's search gives way to: room for a title's few words or
/// a `#number`.
const SEARCH_W: Rems = rems(18.);

/// The widest a page's search gets: past it the row's free width opens
/// between the search and the filters, which keep to the row's end.
const SEARCH_MAX: Rems = rems(28.);

/// A page's search, leading the row under its head: [`crate::search_input`]
/// at the head's height. It keeps its widest and gives way only when the row
/// runs short.
pub fn page_search(state: &Entity<InputState>, cx: &App) -> Div {
    div()
        .flex_basis(SEARCH_MAX)
        .min_w(SEARCH_W)
        .max_w(SEARCH_MAX)
        .child(crate::search_input(state, cx).tall())
}
