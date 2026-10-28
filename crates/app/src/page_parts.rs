//! What every page is built from: its head, a box with a title row, rows
//! parted by a line, and a pair of boxes set side by side while the column
//! holds two; and the projects a page about more than one of them lists.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, IntoElement, ParentElement, Rems, SharedString, Styled, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt};
use std::path::PathBuf;

/// What a project filter reads while it is not narrowing the list at all:
/// the workspace page's issues, or the Tasks page.
pub(crate) const ALL_PROJECTS: &str = "All projects";

/// One project as the workspace page lists it.
pub struct PageProject {
    pub label: SharedString,
    pub root: PathBuf,
    /// Its sessions, each with what to call it before its conversation has a
    /// name of its own.
    pub sessions: Vec<(u64, SharedString)>,
    /// Its branch and how much is changed, as the last `git status` sweep
    /// found them — `None` for a folder that is not a repository.
    pub git: Option<String>,
    /// The file its issues are kept in — `None` for a workspace bound to no
    /// storage, which keeps none.
    pub issues: Option<PathBuf>,
}

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

/// A page's head and the part that stays put over what scrolls, the UI
/// crate's so the Issues page starts as these pages do.
pub(crate) use onehand_ui::{page_head, page_search, page_top};

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
pub(crate) fn card_title(
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
/// the other where it is not, a page section's gap apart: the gap the column
/// leaves between pairs, so the boxes stand evenly whether they sit two to a
/// row or stack.
pub(crate) fn pair(left: Div, right: Div) -> Div {
    div()
        .h_flex()
        .flex_wrap()
        .items_start()
        .gap_6()
        .w_full()
        .child(left.flex_grow_1().flex_basis(PAGE_CARD))
        .child(right.flex_grow_1().flex_basis(PAGE_CARD))
}

/// A bar along a page's foot, the page's whole width, on the tint of `base`,
/// led by `icon`: what a form tells its person while it stands (its problems,
/// or that it is only read), one line each.
pub(crate) fn foot_bar(
    icon: gpui_component::IconName,
    base: gpui::Hsla,
    lines: Vec<String>,
    cx: &App,
) -> AnyElement {
    // Across the page's foot, on its kind's tint, so it reads as one standing
    // condition of the whole form rather than of the column it sits in.
    tinted_lines(icon, base, lines, cx)
        .flex_none()
        .w_full()
        .px_8()
        .py_3()
        .into_any_element()
}

/// What stands in a page's body, above the thing it is about: the foot
/// bar's tint, mark and lines in a box of their own, at a box's corner.
pub(crate) fn notice_box(
    icon: gpui_component::IconName,
    base: gpui::Hsla,
    lines: Vec<String>,
    cx: &App,
) -> AnyElement {
    tinted_lines(icon, base, lines, cx)
        .w_full()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .into_any_element()
}

/// Lines on the tint of `base`, `icon` slotted on the first.
fn tinted_lines(
    icon: gpui_component::IconName,
    base: gpui::Hsla,
    lines: Vec<String>,
    cx: &App,
) -> Div {
    let tint = onehand_ui::tint(base, cx);
    // One line's height, set on the box and on the mark's slot, so the mark
    // stands centred on the first line however many follow it.
    let line = gpui::rems(crate::tokens::CHROME_TEXT.0 * crate::tokens::LEADING);
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .text_size(crate::tokens::CHROME_TEXT)
        .line_height(line)
        .bg(tint.fill)
        .text_color(tint.ink)
        .child(
            onehand_ui::mark_slot(gpui_component::Sizable::small(gpui_component::Icon::new(
                icon,
            )))
            .h(line),
        )
        .child(
            div()
                .v_flex()
                .gap_1()
                .min_w_0()
                .children(lines.into_iter().map(|line| div().child(line))),
        )
}
