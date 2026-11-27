//! The rule every list beside its detail follows in a dock.
//!
//! Side by side only while the container holds the list at the width it is
//! dragged to and [`DETAIL_MIN`] beside it; otherwise one at a time, the detail
//! under a link back naming the list. The width is the container's, measured
//! each frame, never the window's: a dock is dragged and zoomed apart from the
//! window it sits in. Going back keeps what was picked.

use crate::Quiet as _;
use crate::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Div, IntoElement, ParentElement as _, Pixels, Rems, SharedString, Styled as _, div,
};
use gpui_component::ResizableState;
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};
use std::ops::Range;

/// The least room a detail is still readable in beside its list.
const DETAIL_MIN: Rems = gpui::rems(24.);

/// A list's width before anybody drags it, and the range a drag may take it
/// through.
pub struct ListWidths {
    pub start: Rems,
    pub min: Rems,
    pub max: Rems,
    /// Whether the list is the split's last panel, on the right of its
    /// detail, rather than its first.
    pub last: bool,
}

impl ListWidths {
    /// The three widths, in rems.
    pub const fn new(start: f32, min: f32, max: f32) -> Self {
        Self {
            start: gpui::rems(start),
            min: gpui::rems(min),
            max: gpui::rems(max),
            last: false,
        }
    }

    /// The same widths for a list on the right of its detail.
    pub const fn last(self) -> Self {
        Self { last: true, ..self }
    }
}

/// How a list and its detail are drawn this frame.
pub struct ListDetail {
    /// Side by side, rather than one at a time.
    pub side_by_side: bool,
    /// The split's starting width for the list, and the range a drag may take
    /// it through: never far enough to leave the detail under its minimum,
    /// so a drag does not flip the two into one at a time by itself.
    pub start: Pixels,
    pub range: Range<Pixels>,
    /// The range a drag may take the detail through: the list's, mirrored
    /// across the container. A list after its detail sits after the split's
    /// handle, and the split clamps only the panel before it, so the detail
    /// carries this one or a drag takes it, and the list, past either bound.
    pub detail: Range<Pixels>,
}

/// Lay out a list beside its detail in a container `container` wide, as last
/// measured, the list at the width `split` was dragged to. `rem` is the base in
/// force, which the split's pixels are read and written against.
pub fn list_detail(
    split: &ResizableState,
    widths: &ListWidths,
    container: Rems,
    rem: Pixels,
) -> ListDetail {
    // A size outside the range is the split seeding its panels before anything
    // was measured, not a width anybody dragged to.
    let dragged = list_size(split.sizes(), widths.last).map(|w| w / rem);
    let (side_by_side, max) = fit(dragged, widths, container.0);
    let detail = detail_range(widths.min.0..max, container.0);
    ListDetail {
        side_by_side,
        start: rem * widths.start.0,
        range: rem * widths.min.0..rem * max,
        detail: rem * detail.start..rem * detail.end,
    }
}

/// The list's own size among the split's, which is its last panel's when it
/// stands after its detail.
fn list_size(sizes: &[Pixels], last: bool) -> Option<Pixels> {
    match last {
        true => sizes.last(),
        false => sizes.first(),
    }
    .copied()
}

/// The detail's range, in rems, for a list taking `list` of a container
/// `container` wide. Unbounded until the container is measured.
fn detail_range(list: Range<f32>, container: f32) -> Range<f32> {
    match container.is_finite() {
        true => (container - list.end).max(0.)..(container - list.start).max(0.),
        false => 0.0..f32::MAX,
    }
}

/// Whether a list `list` wide and its detail fit side by side in a container
/// `container` wide, for a list nobody drags: [`list_detail`]'s decision
/// without its split.
pub fn fits_beside(list: Rems, container: Rems) -> bool {
    container.0 >= list.0 + DETAIL_MIN.0
}

/// The arithmetic of [`list_detail`], in rems: whether the two fit side by
/// side, and how wide a drag may take the list.
fn fit(dragged: Option<f32>, widths: &ListWidths, container: f32) -> (bool, f32) {
    let (min, max) = (widths.min.0, widths.max.0);
    let list = dragged
        .filter(|w| (min..=max).contains(w))
        .unwrap_or(widths.start.0);
    (
        fits_beside(gpui::rems(list), gpui::rems(container)),
        (container - DETAIL_MIN.0).clamp(min, max),
    )
}

/// The way back from a detail shown alone to the list it was picked from,
/// named for that list. The caller says what a press does.
pub fn back_link(id: &'static str, list: &'static str, cx: &App) -> Button {
    crate::action(id)
        .quiet(cx)
        .short()
        .flex_none()
        .icon(Icon::new(IconName::ArrowLeft))
        .label(list)
}

/// The first step of a page's trail ([`crumbs`]): the list the page was
/// opened from, by name, in the muted ink the trail's steps take, pressed to
/// go back to it. A button, so the keyboard reaches it; the caller says what a
/// press does.
pub fn crumb_link(id: &'static str, list: &'static str, cx: &App) -> Button {
    crate::action(id)
        .quiet(cx)
        .short()
        .flex_none()
        .text_color(cx.theme().muted_foreground)
        .label(list)
}

/// A page's trail, as the library's breadcrumb draws one: `back` (a
/// [`crumb_link`]), then each of `steps` after a chevron, the steps muted but
/// the last, which is the page itself. A step that runs long truncates; the
/// way back never does.
pub fn crumbs(back: Button, steps: impl IntoIterator<Item = SharedString>, cx: &App) -> Div {
    let steps: Vec<SharedString> = steps.into_iter().collect();
    let last = steps.len().saturating_sub(1);
    let (muted, ink) = (cx.theme().muted_foreground, cx.theme().foreground);
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_color(muted)
        .child(back)
        .children(steps.into_iter().enumerate().map(move |(i, step)| {
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .child(Icon::new(IconName::ChevronRight).xsmall().text_color(muted))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .when(i == last, |step| step.text_color(ink))
                        .child(step),
                )
                .into_any_element()
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: ListWidths = ListWidths::new(12., 9., 26.);

    #[test]
    fn a_list_and_its_detail_share_a_container_only_when_both_fit() {
        let edge = 12. + DETAIL_MIN.0;
        assert!(fit(None, &WIDTHS, 40.).0);
        assert!(fit(None, &WIDTHS, edge).0);
        assert!(!fit(None, &WIDTHS, edge - 0.1).0);
        // A list dragged wider needs a wider container.
        assert!(!fit(Some(20.), &WIDTHS, 40.).0);
        // A size outside the range is not a drag: the start width answers.
        assert!(fit(Some(2.), &WIDTHS, 40.).0);
    }

    #[test]
    fn a_list_after_its_detail_is_read_from_the_last_panel() {
        let sizes = [gpui::px(500.), gpui::px(200.)];
        assert_eq!(list_size(&sizes, true), Some(gpui::px(200.)));
        assert_eq!(list_size(&sizes, false), Some(gpui::px(500.)));
        assert_eq!(list_size(&[], true), None);
    }

    #[test]
    fn the_detail_mirrors_the_lists_range_across_the_container() {
        let max = fit(None, &WIDTHS.last(), 40.).1;
        assert_eq!(detail_range(WIDTHS.min.0..max, 40.), 24.0..31.0);
        // Unmeasured, the detail is not bound at all.
        assert_eq!(
            detail_range(WIDTHS.min.0..max, f32::INFINITY),
            0.0..f32::MAX
        );
    }

    #[test]
    fn a_drag_never_takes_the_detail_under_its_minimum() {
        assert_eq!(fit(None, &WIDTHS, 40.).1, 16.);
        assert_eq!(fit(None, &WIDTHS, 100.).1, WIDTHS.max.0);
        // Never under the list's own floor, whatever the container.
        assert_eq!(fit(None, &WIDTHS, 10.).1, WIDTHS.min.0);
    }
}
