//! Keyboard input handling for the terminal emulator.
//!
//! This module provides [`keystroke_to_bytes`], which converts GPUI keyboard
//! events into terminal escape sequences that can be written to the PTY.
//!
//! # Key Mappings
//!
//! ## Special Keys
//!
//! | Key | Sequence | Notes |
//! |-----|----------|-------|
//! | Enter | `\r` (0x0D) | Carriage return |
//! | Escape | `\x1b` (0x1B) | ESC |
//! | Backspace | `\x7f` (0x7F) | DEL |
//! | Tab | `\t` (0x09) | Horizontal tab |
//! | Shift+Tab | `\x1b[Z` | Backtab |
//! | Space | ` ` (0x20) | Space |
//! | Ctrl+Space | `\x00` | NUL |
//!
//! ## Arrow Keys
//!
//! Arrow key sequences depend on application cursor mode:
//!
//! | Key | Normal Mode | App Cursor Mode |
//! |-----|-------------|-----------------|
//! | Up | `\x1b[A` | `\x1bOA` |
//! | Down | `\x1b[B` | `\x1bOB` |
//! | Right | `\x1b[C` | `\x1bOC` |
//! | Left | `\x1b[D` | `\x1bOD` |
//!
//! ## Navigation Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | Home | `\x1b[H` |
//! | End | `\x1b[F` |
//! | PageUp | `\x1b[5~` |
//! | PageDown | `\x1b[6~` |
//! | Insert | `\x1b[2~` |
//! | Delete | `\x1b[3~` |
//!
//! ## Function Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | F1-F4 | `\x1bOP` - `\x1bOS` |
//! | F5-F12 | `\x1b[15~` - `\x1b[24~` |
//!
//! ## Control Combinations
//!
//! Ctrl+A through Ctrl+Z map to ASCII control characters 0x01-0x1A:
//!
//! | Combination | Byte |
//! |-------------|------|
//! | Ctrl+A | 0x01 |
//! | Ctrl+C | 0x03 (interrupt) |
//! | Ctrl+D | 0x04 (EOF) |
//! | Ctrl+Z | 0x1A (suspend) |
//!
//! ## Alt Combinations
//!
//! Alt+key sends ESC followed by the key: `\x1b` + key
//!
//! ## Modified Cursor, Navigation and Function Keys
//!
//! *onehand patch*: every key in the three tables above can also be pressed with
//! Shift, Alt or Control, and the plain sequence says nothing about that. The
//! modified forms carry a parameter:
//!
//! | Key kind | Unmodified | Modified |
//! |----------|------------|----------|
//! | Arrows, Home, End | `\x1b[A`, `\x1bOA` | `\x1b[1;<m>A` |
//! | F1-F4 | `\x1bOP` | `\x1b[1;<m>P` |
//! | The numbered keys | `\x1b[5~` | `\x1b[5;<m>~` |
//!
//! where `<m>` is 1 plus 1 for Shift, 2 for Alt and 4 for Control, added
//! together. Without them a modified press arrives as the bare key, so an editor
//! told to move by word on `Ctrl+Right` receives a plain `Right` and moves by
//! one character -- a binding that appears to be configured and simply does the
//! wrong thing, which is harder to notice than one that does nothing.
//!
//! A modified arrow is always `CSI`, never `SS3`, even in application cursor
//! mode: `SS3` has no room for a parameter, so the mode only decides what the
//! *unmodified* form looks like.
//!
//! # Terminal Mode Effects
//!
//! The [`TermMode`] flags affect key sequences:
//!
//! - **APP_CURSOR**: Changes arrow key sequences from CSI to SS3 format
//!
//! # Example
//!
//! ```
//! use gpui::Keystroke;
//! use alacritty_terminal::term::TermMode;
//! use gpui_terminal::input::keystroke_to_bytes;
//!
//! // Enter key
//! let keystroke = Keystroke::parse("enter").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(b"\r".to_vec()));
//!
//! // Ctrl+C (interrupt)
//! let keystroke = Keystroke::parse("ctrl-c").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(vec![0x03]));
//! ```

use alacritty_terminal::term::TermMode;
use gpui::{Keystroke, Modifiers};

/// onehand patch: the modifier parameter a `CSI` sequence carries, or `None`
/// when there is nothing to say.
///
/// The encoding is one plus a bit per modifier, which is why an unmodified key
/// would be `1` -- and a sequence carrying `;1` is not what any terminal sends,
/// so the absence is meaningful rather than a shortcut.
///
/// The platform key (Super, Command) is deliberately left out. It has a bit in
/// this scheme, but it is also the key the desktop reserves for itself, so a
/// terminal that forwarded it would be reporting presses the window manager
/// already acted on.
fn modifier_param(modifiers: &Modifiers) -> Option<u8> {
    let mut value = 1;
    if modifiers.shift {
        value += 1;
    }
    if modifiers.alt {
        value += 2;
    }
    if modifiers.control {
        value += 4;
    }
    (value > 1).then_some(value)
}

/// onehand patch: a cursor or edit key whose sequence ends in a letter.
///
/// The arrows, Home and End. `SS3` for the unmodified form under application
/// cursor mode, `CSI` otherwise, and always `CSI` once there is a parameter to
/// carry.
fn letter_key(final_byte: u8, modifiers: &Modifiers, mode: TermMode) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(param) => format!("\x1b[1;{param}{}", final_byte as char).into_bytes(),
        None if mode.contains(TermMode::APP_CURSOR) => vec![0x1b, b'O', final_byte],
        None => vec![0x1b, b'[', final_byte],
    }
}

/// onehand patch: F1 through F4, which are `SS3` whatever the cursor mode is.
fn ss3_function_key(final_byte: u8, modifiers: &Modifiers) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(param) => format!("\x1b[1;{param}{}", final_byte as char).into_bytes(),
        None => vec![0x1b, b'O', final_byte],
    }
}

/// onehand patch: a key named by a number and terminated with `~`.
///
/// Page Up and Page Down, Insert and Delete, and F5 upwards.
fn numbered_key(number: u8, modifiers: &Modifiers) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(param) => format!("\x1b[{number};{param}~").into_bytes(),
        None => format!("\x1b[{number}~").into_bytes(),
    }
}

/// onehand patch: put a key behind ESC, which is how Alt is spelled.
///
/// A terminal has no modifier byte for Alt on an ordinary character, so it sends
/// the escape character first and the program reads the pair. This applies to
/// the handful of keys that are a single byte -- the ones with an escape
/// sequence of their own carry Alt in that sequence's parameter instead.
fn with_alt(bytes: Vec<u8>, modifiers: &Modifiers) -> Vec<u8> {
    if !modifiers.alt {
        return bytes;
    }
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(0x1b);
    out.extend_from_slice(&bytes);
    out
}

/// Convert a GPUI keystroke to terminal escape sequence bytes.
///
/// This function translates GPUI keyboard events into the appropriate byte sequences
/// expected by terminal applications. It handles special keys, control characters,
/// and application cursor mode.
///
/// # Arguments
///
/// * `keystroke` - The GPUI keystroke to convert
/// * `mode` - The current terminal mode (affects arrow key sequences)
///
/// # Returns
///
/// An optional vector of bytes representing the terminal escape sequence.
/// Returns `None` if the keystroke should not produce any output.
///
/// # Examples
///
/// ```
/// use gpui::Keystroke;
/// use alacritty_terminal::term::TermMode;
/// use gpui_terminal::input::keystroke_to_bytes;
///
/// let keystroke = Keystroke::parse("enter").unwrap();
/// let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
/// assert_eq!(bytes, Some(b"\r".to_vec()));
/// ```
pub fn keystroke_to_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;

    // Handle special keys first
    match keystroke.key.as_str() {
        // Basic control characters. onehand patch: each of these is one byte
        // with nowhere to carry a modifier, so Alt goes in front of it.
        "space" => {
            if m.control {
                return Some(with_alt(b"\x00".to_vec(), m)); // Ctrl+Space = NUL
            }
            return Some(with_alt(b" ".to_vec(), m));
        }
        "enter" => return Some(with_alt(b"\r".to_vec(), m)),
        "escape" => return Some(with_alt(b"\x1b".to_vec(), m)),
        "backspace" => {
            // onehand patch: `Ctrl+Backspace` is the backspace character rather
            // than DEL, which is the pair a shell reads as "delete the word"
            // and "delete the character".
            let base = if m.control {
                b"\x08".to_vec()
            } else {
                b"\x7f".to_vec()
            };
            return Some(with_alt(base, m));
        }
        "tab" => {
            // Shift+Tab sends a different sequence
            if m.shift {
                return Some(b"\x1b[Z".to_vec());
            }
            return Some(with_alt(b"\t".to_vec(), m));
        }

        // Arrow keys - check APP_CURSOR mode
        "up" => return Some(letter_key(b'A', m, mode)),
        "down" => return Some(letter_key(b'B', m, mode)),
        "right" => return Some(letter_key(b'C', m, mode)),
        "left" => return Some(letter_key(b'D', m, mode)),

        // Navigation keys
        "home" => return Some(letter_key(b'H', m, mode)),
        "end" => return Some(letter_key(b'F', m, mode)),
        "pageup" => return Some(numbered_key(5, m)),
        "pagedown" => return Some(numbered_key(6, m)),
        "insert" => return Some(numbered_key(2, m)),
        "delete" => return Some(numbered_key(3, m)),

        // Function keys
        "f1" => return Some(ss3_function_key(b'P', m)),
        "f2" => return Some(ss3_function_key(b'Q', m)),
        "f3" => return Some(ss3_function_key(b'R', m)),
        "f4" => return Some(ss3_function_key(b'S', m)),
        "f5" => return Some(numbered_key(15, m)),
        "f6" => return Some(numbered_key(17, m)),
        "f7" => return Some(numbered_key(18, m)),
        "f8" => return Some(numbered_key(19, m)),
        "f9" => return Some(numbered_key(20, m)),
        "f10" => return Some(numbered_key(21, m)),
        "f11" => return Some(numbered_key(23, m)),
        "f12" => return Some(numbered_key(24, m)),

        _ => {}
    }

    // Handle Ctrl+key combinations
    if m.control {
        let key = keystroke.key.as_str();

        // Ctrl+A through Ctrl+Z map to 0x01 through 0x1a
        if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if ch.is_ascii_alphabetic() {
                // Convert to uppercase and then to control character
                let upper = ch.to_ascii_uppercase();
                let ctrl_char = (upper as u8) - b'@';
                // onehand patch: `Ctrl+Alt+key` is the control character behind
                // ESC. Dropping Alt here made every one of those arrive as the
                // plain `Ctrl` press, so a binding on one silently fired the
                // other.
                return Some(with_alt(vec![ctrl_char], m));
            }

            // Special Ctrl combinations
            let special = match ch {
                '[' => Some(b"\x1b".to_vec()),  // Ctrl+[
                '\\' => Some(b"\x1c".to_vec()), // Ctrl+\
                ']' => Some(b"\x1d".to_vec()),  // Ctrl+]
                '^' => Some(b"\x1e".to_vec()),  // Ctrl+^
                '_' => Some(b"\x1f".to_vec()),  // Ctrl+_
                '?' => Some(b"\x7f".to_vec()),  // Ctrl+?
                _ => None,
            };
            if let Some(bytes) = special {
                return Some(with_alt(bytes, m));
            }
        }
    }

    // Handle Alt+key combinations
    if m.alt {
        // onehand patch: the whole character, not its first byte.
        //
        // This used to require the key be ASCII, which silently dropped Alt on
        // every layout whose letters are not -- so on a Vietnamese or Greek or
        // Cyrillic keyboard the modifier a program's bindings are built on
        // simply did not exist.
        let typed = keystroke
            .key_char
            .as_deref()
            .unwrap_or(keystroke.key.as_str());
        if typed.chars().count() == 1 {
            return Some(with_alt(typed.as_bytes().to_vec(), m));
        }
    }

    // Handle regular printable characters
    // Use key_char if available (contains the actual typed character with modifiers like Shift)
    if let Some(key_char) = &keystroke.key_char
        && !m.control
        && !m.alt
    {
        return Some(key_char.as_bytes().to_vec());
    }

    // Fallback to key for single characters
    let key = keystroke.key.as_str();
    if key.len() == 1 {
        let ch = key.chars().next().unwrap();
        if ch.is_ascii() && !m.control {
            // Handle shift modifier for uppercase
            let ch = if m.shift { ch.to_ascii_uppercase() } else { ch };
            return Some(vec![ch as u8]);
        }
        // For non-ASCII characters, encode as UTF-8
        if !m.control && !m.alt {
            return Some(key.as_bytes().to_vec());
        }
    }

    // If we get here, the keystroke doesn't produce any output
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_key() {
        let keystroke = Keystroke::parse("enter").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\r".to_vec()));
    }

    #[test]
    fn test_escape_key() {
        let keystroke = Keystroke::parse("escape").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b".to_vec()));
    }

    #[test]
    fn test_backspace_key() {
        let keystroke = Keystroke::parse("backspace").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x7f".to_vec()));
    }

    #[test]
    fn test_tab_key() {
        let keystroke = Keystroke::parse("tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\t".to_vec()));
    }

    #[test]
    fn test_shift_tab() {
        let keystroke = Keystroke::parse("shift-tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn test_arrow_keys_normal_mode() {
        let mode = TermMode::empty();

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1b[A".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1b[B".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1b[C".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn test_arrow_keys_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1bOA".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1bOB".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1bOC".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1bOD".to_vec()));
    }

    #[test]
    fn test_navigation_keys() {
        let mode = TermMode::empty();

        let home = Keystroke::parse("home").unwrap();
        assert_eq!(keystroke_to_bytes(&home, mode), Some(b"\x1b[H".to_vec()));

        let end = Keystroke::parse("end").unwrap();
        assert_eq!(keystroke_to_bytes(&end, mode), Some(b"\x1b[F".to_vec()));

        let pageup = Keystroke::parse("pageup").unwrap();
        assert_eq!(keystroke_to_bytes(&pageup, mode), Some(b"\x1b[5~".to_vec()));

        let pagedown = Keystroke::parse("pagedown").unwrap();
        assert_eq!(
            keystroke_to_bytes(&pagedown, mode),
            Some(b"\x1b[6~".to_vec())
        );

        let insert = Keystroke::parse("insert").unwrap();
        assert_eq!(keystroke_to_bytes(&insert, mode), Some(b"\x1b[2~".to_vec()));

        let delete = Keystroke::parse("delete").unwrap();
        assert_eq!(keystroke_to_bytes(&delete, mode), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn test_function_keys() {
        let mode = TermMode::empty();

        let f1 = Keystroke::parse("f1").unwrap();
        assert_eq!(keystroke_to_bytes(&f1, mode), Some(b"\x1bOP".to_vec()));

        let f2 = Keystroke::parse("f2").unwrap();
        assert_eq!(keystroke_to_bytes(&f2, mode), Some(b"\x1bOQ".to_vec()));

        let f5 = Keystroke::parse("f5").unwrap();
        assert_eq!(keystroke_to_bytes(&f5, mode), Some(b"\x1b[15~".to_vec()));

        let f12 = Keystroke::parse("f12").unwrap();
        assert_eq!(keystroke_to_bytes(&f12, mode), Some(b"\x1b[24~".to_vec()));
    }

    #[test]
    fn test_ctrl_combinations() {
        let mode = TermMode::empty();

        // Ctrl+A = 0x01
        let ctrl_a = Keystroke::parse("ctrl-a").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_a, mode), Some(vec![0x01]));

        // Ctrl+C = 0x03
        let ctrl_c = Keystroke::parse("ctrl-c").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_c, mode), Some(vec![0x03]));

        // Ctrl+Z = 0x1a
        let ctrl_z = Keystroke::parse("ctrl-z").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_z, mode), Some(vec![0x1a]));

        // Ctrl+Space = 0x00
        let ctrl_space = Keystroke::parse("ctrl-space").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_space, mode), Some(vec![0x00]));
    }

    #[test]
    fn test_alt_combinations() {
        let mode = TermMode::empty();

        // Alt+a sends ESC followed by 'a'
        let alt_a = Keystroke::parse("alt-a").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_a, mode), Some(b"\x1ba".to_vec()));

        // Alt+x sends ESC followed by 'x'
        let alt_x = Keystroke::parse("alt-x").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_x, mode), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn test_regular_characters() {
        let mode = TermMode::empty();

        let a = Keystroke::parse("a").unwrap();
        assert_eq!(keystroke_to_bytes(&a, mode), Some(b"a".to_vec()));

        let z = Keystroke::parse("z").unwrap();
        assert_eq!(keystroke_to_bytes(&z, mode), Some(b"z".to_vec()));

        let zero = Keystroke::parse("0").unwrap();
        assert_eq!(keystroke_to_bytes(&zero, mode), Some(b"0".to_vec()));
    }

    #[test]
    fn test_space_key() {
        let mode = TermMode::empty();

        let space = Keystroke::parse("space").unwrap();
        assert_eq!(keystroke_to_bytes(&space, mode), Some(b" ".to_vec()));
    }

    // ── onehand patch: the modified forms ───────────────────────────────────

    /// The parameter is 1 plus a bit each for Shift, Alt and Control.
    #[test]
    fn a_modifier_rides_in_the_sequence() {
        let mode = TermMode::empty();

        let shift_right = Keystroke::parse("shift-right").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_right, mode),
            Some(b"\x1b[1;2C".to_vec())
        );

        let ctrl_right = Keystroke::parse("ctrl-right").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_right, mode),
            Some(b"\x1b[1;5C".to_vec())
        );

        let all = Keystroke::parse("ctrl-shift-alt-left").unwrap();
        assert_eq!(keystroke_to_bytes(&all, mode), Some(b"\x1b[1;8D".to_vec()));
    }

    /// `SS3` has no room for a parameter, so a modified arrow leaves application
    /// cursor mode behind while an unmodified one does not.
    #[test]
    fn a_modified_arrow_is_csi_even_in_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1bOA".to_vec()));

        let ctrl_up = Keystroke::parse("ctrl-up").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_up, mode),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn numbered_keys_carry_the_parameter_before_the_tilde() {
        let mode = TermMode::empty();

        let shift_pageup = Keystroke::parse("shift-pageup").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_pageup, mode),
            Some(b"\x1b[5;2~".to_vec())
        );

        let ctrl_delete = Keystroke::parse("ctrl-delete").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_delete, mode),
            Some(b"\x1b[3;5~".to_vec())
        );

        let shift_f5 = Keystroke::parse("shift-f5").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_f5, mode),
            Some(b"\x1b[15;2~".to_vec())
        );
    }

    /// F1 through F4 are `SS3` unmodified and `CSI` once there is a parameter,
    /// which is the one place the two families cross.
    #[test]
    fn the_first_four_function_keys_switch_family_when_modified() {
        let mode = TermMode::empty();

        let f1 = Keystroke::parse("f1").unwrap();
        assert_eq!(keystroke_to_bytes(&f1, mode), Some(b"\x1bOP".to_vec()));

        let ctrl_f1 = Keystroke::parse("ctrl-f1").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_f1, mode),
            Some(b"\x1b[1;5P".to_vec())
        );
    }

    /// A one-byte key carries Alt in front of it, because it has nowhere else to
    /// put it.
    #[test]
    fn alt_goes_in_front_of_the_single_byte_keys() {
        let mode = TermMode::empty();

        let alt_enter = Keystroke::parse("alt-enter").unwrap();
        assert_eq!(
            keystroke_to_bytes(&alt_enter, mode),
            Some(b"\x1b\r".to_vec())
        );

        let alt_backspace = Keystroke::parse("alt-backspace").unwrap();
        assert_eq!(
            keystroke_to_bytes(&alt_backspace, mode),
            Some(b"\x1b\x7f".to_vec())
        );

        // Ctrl+Backspace is the backspace character, not DEL.
        let ctrl_backspace = Keystroke::parse("ctrl-backspace").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_backspace, mode), Some(vec![0x08]));

        // And the two together are that character behind ESC.
        let both = Keystroke::parse("ctrl-alt-backspace").unwrap();
        assert_eq!(keystroke_to_bytes(&both, mode), Some(vec![0x1b, 0x08]));
    }

    /// `Ctrl+Alt+key` used to arrive as the plain `Ctrl` press, so a program
    /// bound to one of them fired on the other.
    #[test]
    fn alt_survives_a_control_character() {
        let mode = TermMode::empty();

        let ctrl_alt_a = Keystroke::parse("ctrl-alt-a").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_alt_a, mode),
            Some(vec![0x1b, 0x01])
        );
    }
}
