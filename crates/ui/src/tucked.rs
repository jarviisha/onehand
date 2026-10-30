//! A card tucked behind another, its edge showing past it: pressed, it
//! slides out from under that card to its open height, and back when closed.
//!
//! **The one motion beside a spinner.** It moves only when the person opens
//! or closes it, never by itself, about 200ms and eased, and stands still
//! under reduced motion, which gpui's animations respect on their own.
//!
//! In flow it takes only its edge's height, so what lies above it never
//! moves as it opens: the card grows upward over it, anchored at its foot.

use gpui::{
    Animation, AnimationExt as _, Div, ElementId, IntoElement, ParentElement as _, Rems,
    SharedString, Styled as _, div, ease_in_out,
};
use std::time::Duration;

/// How long the slide takes.
const SLIDE: Duration = Duration::from_millis(200);

/// The card `card`, tucked: `edge` tall while closed, at most `full` tall
/// while open, its top edge first. `turns` counts the presses so far: a new
/// count plays the slide once, and none yet means it was never moved, so it
/// lands where it is without moving.
pub fn tucked(
    id: impl Into<SharedString>,
    open: bool,
    turns: usize,
    edge: Rems,
    full: Rems,
    card: Div,
) -> impl IntoElement {
    let (from, to) = match open {
        true => (edge, full),
        false => (full, edge),
    };
    let body = div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .overflow_hidden()
        .child(card);
    let slid = match turns {
        0 => body.max_h(to).into_any_element(),
        _ => body
            .max_h(from)
            .with_animation(
                ElementId::NamedInteger(id.into(), turns as u64),
                Animation::new(SLIDE).with_easing(ease_in_out),
                move |body, delta| body.max_h(gpui::rems(from.0 + (to.0 - from.0) * delta)),
            )
            .into_any_element(),
    };
    div().relative().w_full().h(edge).child(slid)
}
