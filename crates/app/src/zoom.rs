//! Per-panel zoom.
//!
//! `Window::with_rem_size` overrides the rem base for one subtree, so a panel's
//! zoom scales **everything sized in rems** — including the `text_xs` labels
//! that a font-size-only zoom leaves stranded at their original size next to
//! doubled body text. This is also why sizes
//! must be written in rems: a hand-written `px(13.)` is exactly what refuses to
//! come along.
//!
//! The terminal is deliberately not zoomed this way: its glyph grid is sized
//! in pixels by `TerminalConfig::font_size`, and changing that is what
//! re-measures the cell and resizes the PTY (see [`crate::terminal`]).

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, px,
};

/// The factor range: enough to read across a room, enough to fit a wide diff,
/// in steps big enough to be worth a keystroke.
pub const MIN: f32 = 0.6;
pub const MAX: f32 = 2.0;
pub const STEP: f32 = 0.1;

/// One panel's zoom factor.
///
/// Not persisted: zoom is a reading posture for the moment, not a setting, and
/// a workspace that reopens at 180% because of one afternoon is a bug report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom(f32);

impl Default for Zoom {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Zoom {
    pub fn factor(self) -> f32 {
        self.0
    }

    pub fn zoom_in(&mut self) {
        self.set(self.0 + STEP);
    }

    pub fn zoom_out(&mut self) {
        self.set(self.0 - STEP);
    }

    pub fn reset(&mut self) {
        self.0 = 1.0;
    }

    /// Rounded to the step so repeated presses cannot drift: `1.0 - 0.1` is not
    /// `0.9` in binary floating point, and after a dozen presses the factor
    /// would no longer land back on 1.0.
    fn set(&mut self, factor: f32) {
        self.0 = ((factor / STEP).round() * STEP).clamp(MIN, MAX);
    }

    /// The rem base this factor implies, given the window's own.
    ///
    /// Named for `Window::rem_size` / `Window::with_rem_size`, which is what it
    /// feeds; a bare `rem` reads as a remainder at the call site.
    pub fn rem_size(self, window: &Window) -> Pixels {
        window.rem_size() * self.0
    }

    /// Wrap `child` so everything inside it lays out against this zoom.
    pub fn scale(self, window: &Window, child: impl IntoElement) -> Zoomed {
        Zoomed {
            rem: self.rem_size(window),
            child: child.into_any_element(),
        }
    }
}

/// An element that lays its child out against an overridden rem base.
///
/// All three phases have to set it: `request_layout` is where rem-sized text
/// and padding become numbers, and `prepaint`/`paint` re-resolve some of them
/// (a text run's line height, for one). Overriding only in one phase produces
/// a subtree measured at one size and drawn at another.
pub struct Zoomed {
    rem: Pixels,
    child: AnyElement,
}

impl IntoElement for Zoomed {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Zoomed {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let id = window.with_rem_size(Some(self.rem), |window| {
            self.child.request_layout(window, cx)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem), |window| self.child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem), |window| self.child.paint(window, cx));
    }
}

/// The terminal's own zoom: a font size in pixels, because a glyph grid is
/// measured, not laid out. The base is `TerminalConfig`'s default.
pub const TERM_FONT: Pixels = px(14.);

pub fn term_font_size(zoom: Zoom) -> Pixels {
    TERM_FONT * zoom.factor()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binary floating point does not round-trip `1.0 - 0.1 + 0.1`, so the
    /// factor is snapped to the step. Stays inside the range on purpose:
    /// clamping is what the next test covers, and stepping into the floor
    /// would swallow one of these presses.
    #[test]
    fn steps_land_back_on_one() {
        let mut zoom = Zoom::default();
        for _ in 0..3 {
            zoom.zoom_out();
        }
        for _ in 0..3 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.factor(), 1.0);
    }

    #[test]
    fn clamps_both_ends() {
        let mut zoom = Zoom::default();
        for _ in 0..50 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.factor(), MAX);
        for _ in 0..50 {
            zoom.zoom_out();
        }
        assert_eq!(zoom.factor(), MIN);
    }

    #[test]
    fn reset_returns_to_unzoomed() {
        let mut zoom = Zoom::default();
        zoom.zoom_in();
        zoom.reset();
        assert_eq!(zoom.factor(), 1.0);
    }
}
