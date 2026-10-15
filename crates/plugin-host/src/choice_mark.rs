//! The radio or checkbox at the head of a choice row, drawn in ink.
//!
//! The library's radio rings its mark in the field's edge colour, which is
//! the selected step's own grey in the dark palette, so the mark vanished on
//! the very row being hovered or taken. In the muted ink it shows on a row's
//! rest, hover and selected fills alike, and in full ink once taken.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, IntoElement, ParentElement as _, Pixels, Rems, Styled as _, div, px, rems};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// The mark, and the dot inside it once the choice is taken.
///
/// Two numbers because the inner one is not a fraction of the outer: the ring
/// has a border of its own, and a dot derived from the outside measurement
/// would grow into it at one size and float inside it at another.
const MARK: Rems = rems(1.0);
const DOT: Rems = rems(0.5);
/// The ring around the mark, in pixels and not rems: a hairline's job done a
/// touch heavier, and a hairline must not scale with a panel's zoom.
const RING: Pixels = px(1.5);

/// Round for a single-select and square for a multi-select, the one
/// convention a reader already knows; filled once `on`.
pub fn choice_mark(on: bool, single: bool, cx: &App) -> impl IntoElement + use<> {
    let ink = match on {
        true => cx.theme().foreground,
        false => cx.theme().muted_foreground,
    };
    let corner = cx.theme().radius;
    div()
        .flex_none()
        .size(MARK)
        .h_flex()
        .items_center()
        .justify_center()
        .border(RING)
        .border_color(ink)
        .map(|mark| match single {
            true => mark.rounded_full(),
            false => mark.rounded(corner),
        })
        .when(on, |mark| {
            mark.child(div().size(DOT).bg(ink).map(|dot| match single {
                true => dot.rounded_full(),
                false => dot.rounded(corner),
            }))
        })
}
