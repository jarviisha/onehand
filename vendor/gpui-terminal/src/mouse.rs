//! Mouse reporting: the bytes a full-screen program expects when the pointer
//! moves over it.
//!
//! *onehand patch*: this module used to hold a selection type, a pixel-to-cell
//! conversion and a scroll-delta helper as well, none of which had a caller --
//! the view had grown its own of each, against alacritty's `Selection` rather
//! than the one declared here, so the crate carried two types of the same name
//! meaning different things. What is left is the one thing the view genuinely
//! cannot do for itself: the wire format.
//!
//! # Who asks for this
//!
//! Nothing is reported unless the child turned reporting on. It does that with
//! a private mode, and there are three that decide *what* is worth reporting:
//!
//! | Mode | Flag | Reports |
//! |------|------|---------|
//! | 1000 | `MOUSE_REPORT_CLICK` | presses and releases |
//! | 1002 | `MOUSE_DRAG` | the above, plus motion while a button is held |
//! | 1003 | `MOUSE_MOTION` | the above, plus motion with no button at all |
//!
//! and two more that decide how the report is *spelled*:
//!
//! | Mode | Flag | Encoding |
//! |------|------|----------|
//! | 1006 | `SGR_MOUSE` | `ESC [ < b ; col ; row M` / `m` |
//! | 1005 | `UTF8_MOUSE` | the legacy form with the three fields as UTF-8 |
//! | — | neither | `ESC [ M` and three bytes, each offset by 32 |
//!
//! **Both halves have to be honoured, not just the first.** The legacy encoding
//! is the one a program gets if it asked for 1000 and nothing else, and sending
//! it SGR instead does not degrade quietly: the escape sequence it cannot parse
//! is delivered as text, so a click types `[<0;40;12M` into whatever is running.
//! That is the whole reason this is a table rather than one format.
//!
//! # Button encoding
//!
//! One number carries the button, the modifiers and what kind of event it is.
//!
//! | Field | Value |
//! |-------|-------|
//! | Left / Middle / Right | 0 / 1 / 2 |
//! | Shift / Alt / Control | +4 / +8 / +16 |
//! | Motion | +32 |
//! | Wheel up / down | 64 / 65 |
//!
//! The legacy encoding has no room for *which* button was let go, so a release
//! there is the low two bits set (3) with everything else intact; SGR says it
//! with the terminating `m` and keeps the button.

use alacritty_terminal::index::Point as AlacPoint;
use alacritty_terminal::term::TermMode;
use gpui::MouseButton;

/// How many wheel reports one scroll gesture may turn into.
///
/// A trackpad reports pixels and a flick can add up to a great many rows; every
/// one of them would be a separate escape sequence, and a program that redraws
/// per event spends the whole gesture catching up. Five is roughly a screenful
/// in a pager and is what the wheel of a real mouse produces anyway.
const SCROLL_REPORT_MAX: u32 = 5;

/// Encode modifier keys as a bitmask for mouse reporting.
///
/// # Arguments
///
/// * `shift` - Whether Shift is pressed
/// * `alt` - Whether Alt is pressed
/// * `control` - Whether Control is pressed
///
/// # Returns
///
/// A bitmask encoding the modifiers:
/// - Bit 2 (4): Shift
/// - Bit 3 (8): Alt/Meta
/// - Bit 4 (16): Control
///
/// # Examples
///
/// ```
/// use gpui_terminal::mouse::encode_modifiers;
///
/// assert_eq!(encode_modifiers(false, false, false), 0);
/// assert_eq!(encode_modifiers(true, false, false), 4);
/// assert_eq!(encode_modifiers(false, true, false), 8);
/// assert_eq!(encode_modifiers(false, false, true), 16);
/// assert_eq!(encode_modifiers(true, true, true), 28);
/// ```
pub fn encode_modifiers(shift: bool, alt: bool, control: bool) -> u8 {
    let mut modifiers = 0;
    if shift {
        modifiers |= 4;
    }
    if alt {
        modifiers |= 8;
    }
    if control {
        modifiers |= 16;
    }
    modifiers
}

/// The report for a button going down or coming back up.
///
/// # Arguments
///
/// * `button` - The mouse button that was pressed or released
/// * `pressed` - `true` for a press, `false` for a release
/// * `point` - Where the pointer is, as a **viewport** cell, zero-based
/// * `modifiers` - The bitmask from [`encode_modifiers`]
/// * `mode` - The terminal's current mode flags
///
/// # Returns
///
/// The bytes to write to the PTY, or `None` if the child has not asked to hear
/// about the mouse at all -- which is also the caller's signal that the press
/// belongs to the terminal itself, to start a selection with.
///
/// # Examples
///
/// ```
/// use gpui::MouseButton;
/// use alacritty_terminal::term::TermMode;
/// use alacritty_terminal::index::{Point, Line, Column};
/// use gpui_terminal::mouse::button_report;
///
/// let at = Point::new(Line(5), Column(10));
/// let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
///
/// assert_eq!(button_report(MouseButton::Left, true, at, 0, mode).unwrap(), b"\x1b[<0;11;6M");
/// assert!(button_report(MouseButton::Left, true, at, 0, TermMode::empty()).is_none());
/// ```
pub fn button_report(
    button: MouseButton,
    pressed: bool,
    point: AlacPoint,
    modifiers: u8,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    encode(button_code(button)? | modifiers, point, pressed, mode)
}

/// The report for the pointer moving.
///
/// # Arguments
///
/// * `held` - The button being held, or `None` if the pointer is moving free
/// * `point` - Where the pointer now is, as a **viewport** cell, zero-based
/// * `modifiers` - The bitmask from [`encode_modifiers`]
/// * `mode` - The terminal's current mode flags
///
/// # Returns
///
/// The bytes to write, or `None` when this motion is not one the child asked
/// about: 1002 wants motion only while a button is down, and plain 1000 wants
/// none of it.
///
/// The caller owes one thing this cannot check for itself -- **only call it when
/// the pointer has changed cell**. Motion arrives per pixel and a report is per
/// cell, so reporting every event sends the same coordinates over and over, at
/// the rate the mouse is sampled, to a program that redraws on each one.
pub fn motion_report(
    held: Option<MouseButton>,
    point: AlacPoint,
    modifiers: u8,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let wanted = mode.contains(TermMode::MOUSE_MOTION)
        || (mode.contains(TermMode::MOUSE_DRAG) && held.is_some());
    if !wanted {
        return None;
    }
    // With no button down there is nothing to name, and the legacy encoding's
    // "no button" value is the same 3 it uses for a release.
    let code = match held {
        Some(button) => button_code(button)?,
        None => 3,
    };
    encode((code + 32) | modifiers, point, true, mode)
}

/// The report for a scroll gesture, or the keys that stand in for one.
///
/// # Arguments
///
/// * `delta` - Rows to scroll; positive is up, negative is down
/// * `point` - Where the pointer is, as a **viewport** cell, zero-based
/// * `modifiers` - The bitmask from [`encode_modifiers`]
/// * `mode` - The terminal's current mode flags
///
/// # Returns
///
/// Wheel reports where the child is tracking the mouse; **arrow keys** where it
/// is on the alternate screen and is not. That second case is not a convenience:
/// the alternate screen has no scrollback to move through, so a wheel the
/// terminal keeps for itself there does nothing at all, and a pager or an editor
/// that never enabled mouse tracking is exactly the program someone expects to
/// be able to scroll. `None` means the gesture is the terminal's own, to move
/// the scrollback with.
///
/// # Examples
///
/// ```
/// use alacritty_terminal::term::TermMode;
/// use alacritty_terminal::index::{Point, Line, Column};
/// use gpui_terminal::mouse::scroll_report;
///
/// let at = Point::new(Line(5), Column(10));
///
/// // Tracking the mouse: one wheel report per row.
/// let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
/// assert_eq!(scroll_report(2, at, 0, mode).unwrap(), b"\x1b[<64;11;6M\x1b[<64;11;6M");
///
/// // On the alternate screen without it: arrow keys.
/// assert_eq!(scroll_report(2, at, 0, TermMode::ALT_SCREEN).unwrap(), b"\x1b[A\x1b[A");
///
/// // Otherwise the terminal keeps it.
/// assert!(scroll_report(2, at, 0, TermMode::empty()).is_none());
/// ```
pub fn scroll_report(
    delta: i32,
    point: AlacPoint,
    modifiers: u8,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if delta == 0 {
        return None;
    }
    let count = delta.unsigned_abs().min(SCROLL_REPORT_MAX);

    if mode.intersects(TermMode::MOUSE_MODE) {
        let code = if delta > 0 { 64 } else { 65 } | modifiers;
        // A wheel has nothing to release, so every notch is a press.
        let report = encode(code, point, true, mode)?;
        return Some(report.repeat(count as usize));
    }

    if mode.contains(TermMode::ALT_SCREEN) {
        let arrow: &[u8] = match (delta > 0, mode.contains(TermMode::APP_CURSOR)) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return Some(arrow.repeat(count as usize));
    }

    None
}

/// The number a button is spelled with, or `None` for one the protocol has no
/// room for.
///
/// The navigation buttons are the ones with nowhere to go: their codes were
/// taken by the wheel long before a mouse had thumb buttons, and a program that
/// received one would read it as a scroll.
fn button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Navigate(_) => None,
    }
}

/// Spell one report in whichever encoding the child asked for.
fn encode(code: u8, point: AlacPoint, pressed: bool, mode: TermMode) -> Option<Vec<u8>> {
    // Both encodings count from one, and a report is about the viewport, so a
    // caller that handed us a scrolled-back coordinate would name a row the
    // child cannot see. Clamped rather than dropped: a drag that runs off the
    // top should report the first row, which is what it is over.
    let col = point.column.0 + 1;
    let row = point.line.0.max(0) as usize + 1;

    if mode.contains(TermMode::SGR_MOUSE) {
        let action = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{col};{row}{action}").into_bytes());
    }

    // The legacy form cannot say which button was released, so it says only that
    // one was. The upper bits -- modifiers, motion -- are kept.
    let code = if pressed { code } else { (code & !0b11) | 0b11 };

    let mut out = b"\x1b[M".to_vec();
    push_field(&mut out, code as usize + 32, mode)?;
    push_field(&mut out, col + 32, mode)?;
    push_field(&mut out, row + 32, mode)?;
    Some(out)
}

/// Append one field of a legacy report.
///
/// `None` where the value will not fit, which is the whole point of mode 1005:
/// a single byte offset by 32 runs out at column 223, and a terminal wider than
/// that would otherwise report a wrapped-around column with perfect confidence.
/// Saying nothing is the honest answer -- the click is lost, rather than
/// delivered somewhere the user did not click.
fn push_field(out: &mut Vec<u8>, value: usize, mode: TermMode) -> Option<()> {
    if mode.contains(TermMode::UTF8_MOUSE) {
        let mut buf = [0u8; 4];
        let encoded = char::from_u32(value as u32)?.encode_utf8(&mut buf);
        out.extend_from_slice(encoded.as_bytes());
        return Some(());
    }
    if value > u8::MAX as usize {
        return None;
    }
    out.push(value as u8);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::{Column, Line};

    /// The cell a report is about, one row and one column in from the corner.
    fn at() -> AlacPoint {
        AlacPoint::new(Line(1), Column(2))
    }

    #[test]
    fn silent_until_the_child_asks() {
        assert!(button_report(MouseButton::Left, true, at(), 0, TermMode::empty()).is_none());
        assert!(motion_report(Some(MouseButton::Left), at(), 0, TermMode::empty()).is_none());
    }

    #[test]
    fn sgr_press_and_release_name_the_button() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            button_report(MouseButton::Right, true, at(), 0, mode).unwrap(),
            b"\x1b[<2;3;2M"
        );
        assert_eq!(
            button_report(MouseButton::Right, false, at(), 0, mode).unwrap(),
            b"\x1b[<2;3;2m"
        );
    }

    /// The legacy form has one release for every button, and the difference is
    /// what makes sending SGR to a client that did not ask for it unsafe.
    #[test]
    fn legacy_release_forgets_which_button() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            button_report(MouseButton::Left, true, at(), 0, mode).unwrap(),
            b"\x1b[M\x20\x23\x22"
        );
        assert_eq!(
            button_report(MouseButton::Right, false, at(), 0, mode).unwrap(),
            b"\x1b[M\x23\x23\x22"
        );
    }

    #[test]
    fn modifiers_ride_in_the_button_field() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let modifiers = encode_modifiers(true, false, true);
        assert_eq!(
            button_report(MouseButton::Left, true, at(), modifiers, mode).unwrap(),
            b"\x1b[<20;3;2M"
        );
    }

    #[test]
    fn motion_needs_a_mode_that_wants_it() {
        let held = Some(MouseButton::Left);
        // 1000 reports clicks and nothing else.
        let click_only = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert!(motion_report(held, at(), 0, click_only).is_none());

        // 1002 reports a drag but not a free-moving pointer.
        let drag = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(
            motion_report(held, at(), 0, drag).unwrap(),
            b"\x1b[<32;3;2M"
        );
        assert!(motion_report(None, at(), 0, drag).is_none());

        // 1003 reports both, and a pointer with no button held is a 3.
        let any = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        assert_eq!(motion_report(None, at(), 0, any).unwrap(), b"\x1b[<35;3;2M");
    }

    #[test]
    fn scroll_is_one_report_per_row_and_capped() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            scroll_report(-2, at(), 0, mode).unwrap(),
            b"\x1b[<65;3;2M\x1b[<65;3;2M"
        );
        let flick = scroll_report(40, at(), 0, mode).unwrap();
        assert_eq!(
            flick.len(),
            b"\x1b[<64;3;2M".len() * SCROLL_REPORT_MAX as usize
        );
    }

    /// The alternate screen has no scrollback, so a wheel there has to become
    /// something the program can act on.
    #[test]
    fn alt_screen_scrolls_with_arrow_keys() {
        assert_eq!(
            scroll_report(2, at(), 0, TermMode::ALT_SCREEN).unwrap(),
            b"\x1b[A\x1b[A"
        );
        assert_eq!(
            scroll_report(-1, at(), 0, TermMode::ALT_SCREEN | TermMode::APP_CURSOR).unwrap(),
            b"\x1bOB"
        );
        // On the normal screen it stays the terminal's own gesture.
        assert!(scroll_report(2, at(), 0, TermMode::empty()).is_none());
    }

    #[test]
    fn a_column_the_legacy_form_cannot_hold_is_not_guessed_at() {
        let wide = AlacPoint::new(Line(0), Column(300));
        assert!(
            button_report(
                MouseButton::Left,
                true,
                wide,
                0,
                TermMode::MOUSE_REPORT_CLICK
            )
            .is_none()
        );

        // Mode 1005 is what a program enables to lift that ceiling.
        let utf8 = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        assert!(button_report(MouseButton::Left, true, wide, 0, utf8).is_some());
    }

    #[test]
    fn navigation_buttons_have_nowhere_to_go() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let back = MouseButton::Navigate(gpui::NavigationDirection::Back);
        assert!(button_report(back, true, at(), 0, mode).is_none());
    }
}
