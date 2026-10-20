//! What every page is built from: its head, a box with a title row, rows
//! parted by a line, and a pair of boxes set side by side while the column
//! holds two.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, IntoElement, ParentElement, Rems, SharedString, Styled, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt};

/// The hairline box every card on a page is drawn in, the UI crate's so
/// an issue's boxes are the same one.
pub(crate) use onehand_ui::card_box;

/// The width a page's box starts from before it grows into its row. Two fit
/// side by side within the page, and a narrower panel wraps them one under
/// the other.
const PAGE_CARD: Rems = rems(22.);

/// The height of a title row.
const PAGE_TITLE_H: Rems = rems(1.75);

/// A box on a page: its title in bold with a muted count beside it, an
/// optional control at the far end, and whatever it holds below.
pub(crate) fn page_card(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    card_box(cx).child(card_title(title, count, control, cx))
}

/// A page's head, in its body: the page's name at the title size, a line of
/// facts under it in the middle ink, and its actions at the end, each at the
/// head's size. The pane's header above it carries only chrome.
pub(crate) fn page_head(
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
                        .text_xl()
                        .font_semibold()
                        .child(title.into()),
                )
                .children(facts.map(|facts| {
                    div()
                        .text_xs()
                        .text_color(crate::theme::meta_ink(cx))
                        .child(facts)
                })),
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

/// Rows in a box, a hairline between each two: the line on a wrapper of the
/// row's own, so a row's focus ring moves nothing.
pub(crate) fn ruled(rows: impl IntoIterator<Item = AnyElement>, cx: &App) -> Div {
    let rule = cx.theme().border;
    div()
        .v_flex()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            div()
                .when(i > 0, |line| line.border_t_1().border_color(rule))
                .child(row)
                .into_any_element()
        }))
}

/// A page heading: the title in bold, a muted count beside it and an
/// optional control at the far end.
pub(super) fn card_title(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_2()
        // The control's own height must not change the title row's, or a card
        // with a filter sits taller than the one beside it.
        .h(PAGE_TITLE_H)
        .child(div().font_semibold().child(title.into()))
        .children(count.map(|n| div().text_xs().text_color(muted).child(n.to_string())))
        .child(div().flex_1())
        .children(control)
}

/// Two boxes or sections to a row where the column is wide enough, one under
/// the other where it is not, a box's gap apart.
pub(super) fn pair(left: Div, right: Div) -> Div {
    div()
        .h_flex()
        .flex_wrap()
        .items_start()
        .gap_4()
        .w_full()
        .child(left.flex_grow_1().flex_basis(PAGE_CARD))
        .child(right.flex_grow_1().flex_basis(PAGE_CARD))
}
