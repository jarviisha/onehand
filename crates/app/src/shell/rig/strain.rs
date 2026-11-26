//! Drawing a view under strain and reading its parts back: the window at a
//! width, the words a name can take, and the bounds of the parts a view
//! marks with `debug_selector`, which only a debug test build records.
//!
//! The test platform measures every character at the same advance, so what
//! these hold is structure (a height, a part that stays put or inside its
//! container), not where the app's own font breaks a line.

use gpui::{Bounds, Pixels, VisualTestContext, px, size};

/// A name of the length most names are.
pub(crate) const SHORT: &str = "Notes";

/// Two hundred characters of words, which a line may break between.
pub(crate) fn long() -> String {
    "word ".repeat(40)
}

/// Two hundred characters with nowhere to break.
pub(crate) fn unbroken() -> String {
    "x".repeat(200)
}

/// How far two edges may differ and still be the same: layout rounds to the
/// pixel's fractions.
const SLACK: f32 = 0.5;

/// Draw the window at `width`, as tall as a laptop's, and let it settle.
pub(crate) fn width(cx: &mut VisualTestContext, width: Pixels) {
    cx.simulate_resize(size(width, px(800.)));
    cx.run_until_parked();
}

/// A selector built at run time, as the repeated parts are named.
pub(crate) fn selector(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// Where the part marked `sel` was drawn on the last frame.
#[track_caller]
pub(crate) fn bounds(cx: &mut VisualTestContext, sel: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(sel)
        .unwrap_or_else(|| panic!("nothing drawn as {sel}"))
}

/// Whether `a` and `b` are the same length, give or take the rounding.
pub(crate) fn same(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= SLACK
}

#[track_caller]
pub(crate) fn assert_inside(part: Bounds<Pixels>, within: Bounds<Pixels>, what: &str) {
    let slack = px(SLACK);
    assert!(
        part.left() + slack >= within.left()
            && part.top() + slack >= within.top()
            && part.right() <= within.right() + slack
            && part.bottom() <= within.bottom() + slack,
        "{what}: {part:?} stands outside {within:?}"
    );
}

#[track_caller]
pub(crate) fn assert_one_height(parts: &[Bounds<Pixels>], what: &str) {
    let Some(first) = parts.first() else { return };
    for part in parts {
        assert!(
            same(part.size.height, first.size.height),
            "{what}: heights differ, {parts:?}"
        );
    }
}

/// Every part on one line: one top and one height.
#[track_caller]
pub(crate) fn assert_one_line(parts: &[Bounds<Pixels>], what: &str) {
    assert_one_height(parts, what);
    let Some(first) = parts.first() else { return };
    for part in parts {
        assert!(
            same(part.top(), first.top()),
            "{what}: not on one line, {parts:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, div,
    };

    struct Marked;

    impl Render for Marked {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .child(div().debug_selector(|| "strain-probe".into()).flex_1())
        }
    }

    #[gpui::test]
    fn the_bounds_of_a_marked_part_follow_the_window(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Marked);
        width(cx, px(700.));
        assert!(same(bounds(cx, "strain-probe").size.width, px(700.)));
        width(cx, px(1200.));
        assert!(same(bounds(cx, "strain-probe").size.width, px(1200.)));
    }

    #[gpui::test]
    #[should_panic(expected = "nothing drawn as strain-missing")]
    fn an_unmarked_part_is_said_by_name(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Marked);
        bounds(cx, "strain-missing");
    }
}
