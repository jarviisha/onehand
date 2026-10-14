//! What every page about more than one project is built from: a box with a
//! title row, a section with the same title row and no box, and a pair of
//! either set side by side while the column holds two.

use gpui::{App, Div, ParentElement, Rems, SharedString, Styled, div, rems};
use gpui_component::{ActiveTheme, StyledExt};

/// The hairline box every card on a page is drawn in, the plugin host's so
/// an issue's boxes are the same one.
pub(super) use onehand_plugin_host::card_box;

/// The width a page's box starts from before it grows into its row. Two fit
/// side by side within the page, and a narrower panel wraps them one under
/// the other.
const PAGE_CARD: Rems = rems(22.);

/// The height of a title row.
const PAGE_TITLE_H: Rems = rems(1.75);

/// A box on a page: its title in bold with a muted count beside it, an
/// optional control at the far end, and whatever it holds below.
pub(super) fn page_card(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    card_box(cx).child(card_title(title, count, control, cx))
}

/// A section of a page that is a list rather than a box: its title row over
/// what it holds, on the page's own surface.
pub(super) fn page_section(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    div()
        .v_flex()
        .gap_2()
        .min_w_0()
        .child(card_title(title, count, control, cx))
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
