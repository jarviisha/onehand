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
    Animation, AnimationExt as _, Div, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Rems, SharedString, Styled as _, div, ease_in_out,
};
use std::time::Duration;

/// How long the slide takes; a caller holds a press's count for this long.
pub const TUCKED_SLIDE: Duration = Duration::from_millis(200);

/// How far open a tucked card stands `elapsed` into a slide toward `open`
/// that began `from` open: what the next press starts from.
pub fn tucked_openness(from: f32, open: bool, elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / TUCKED_SLIDE.as_secs_f32()).min(1.);
    let to = if open { 1. } else { 0. };
    from + (to - from) * ease_in_out(t)
}

/// The card `card`, tucked: `edge` tall while closed, at most `full` tall
/// while open, its top edge first. `sliding` is the press that moved it,
/// counted, and how far open the card stood when it came (0 closed, 1 open),
/// for [`TUCKED_SLIDE`] after it, and `None` otherwise: each count plays the
/// slide once, from where the card stood, so a press mid-slide turns it
/// back without a jump; with none the card stands where it is, so it never
/// moves when it is only drawn again.
pub fn tucked(
    id: impl Into<SharedString>,
    open: bool,
    sliding: Option<(usize, f32)>,
    edge: Rems,
    full: Rems,
    card: Div,
) -> impl IntoElement {
    let at = move |openness: f32| gpui::rems(edge.0 + (full.0 - edge.0) * openness);
    let to = match open {
        true => full,
        false => edge,
    };
    let body = div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .overflow_hidden()
        // What it covers answers none of the pointer's presses.
        .occlude()
        .child(card);
    let slid = match sliding {
        None => body.max_h(to).into_any_element(),
        Some((turns, from)) => body
            .max_h(at(from))
            .with_animation(
                ElementId::NamedInteger(id.into(), turns as u64),
                Animation::new(TUCKED_SLIDE).with_easing(ease_in_out),
                move |body, delta| body.max_h(gpui::rems(at(from).0 + (to.0 - at(from).0) * delta)),
            )
            .into_any_element(),
    };
    div().relative().w_full().h(edge).child(slid)
}

#[cfg(test)]
mod tests {
    use super::{TUCKED_SLIDE, tucked_openness};
    use std::time::Duration;

    #[test]
    fn a_slide_ends_where_it_goes() {
        assert_eq!(tucked_openness(0., true, TUCKED_SLIDE), 1.);
        assert_eq!(tucked_openness(1., false, TUCKED_SLIDE * 2), 0.);
    }

    /// Turned back halfway, it starts from halfway.
    #[test]
    fn halfway_is_halfway() {
        let half = tucked_openness(0., true, TUCKED_SLIDE / 2);
        assert!((half - 0.5).abs() < 1e-3, "{half}");
        assert_eq!(tucked_openness(0.3, true, Duration::ZERO), 0.3);
    }
}
