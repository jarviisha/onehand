//! The terminal's sixteen colours: VS Code's `terminal.ansi*` defaults, as VS
//! Code registers them in its own source for its dark and light themes.
//!
//! Held here rather than in the app's palette because the grid is drawn by
//! this crate for the shell dock and the Neovim mode alike, and nothing below
//! the app can reach the app. The app's palette reads its magenta, its cyan
//! and the theme's bright base hues from this table, so a terminal colour is
//! written once.

/// One mode's sixteen colours, each `0xRRGGBB`.
pub struct Ansi {
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub yellow: u32,
    pub blue: u32,
    pub magenta: u32,
    pub cyan: u32,
    pub white: u32,
    pub bright_black: u32,
    pub bright_red: u32,
    pub bright_green: u32,
    pub bright_yellow: u32,
    pub bright_blue: u32,
    pub bright_magenta: u32,
    pub bright_cyan: u32,
    pub bright_white: u32,
}

pub const DARK: Ansi = Ansi {
    black: 0x000000,
    red: 0xcd3131,
    green: 0x0dbc79,
    yellow: 0xe5e510,
    blue: 0x2472c8,
    magenta: 0xbc3fbc,
    cyan: 0x11a8cd,
    white: 0xe5e5e5,
    bright_black: 0x666666,
    bright_red: 0xf14c4c,
    bright_green: 0x23d18b,
    bright_yellow: 0xf5f543,
    bright_blue: 0x3b8eea,
    bright_magenta: 0xd670d6,
    bright_cyan: 0x29b8db,
    bright_white: 0xe5e5e5,
};

pub const LIGHT: Ansi = Ansi {
    black: 0x000000,
    red: 0xcd3131,
    green: 0x107c10,
    yellow: 0x949800,
    blue: 0x0451a5,
    magenta: 0xbc05bc,
    cyan: 0x0598bc,
    white: 0x555555,
    bright_black: 0x666666,
    bright_red: 0xf14c4c,
    bright_green: 0x14ce14,
    bright_yellow: 0xb5ba00,
    bright_blue: 0x3b8eea,
    bright_magenta: 0xd670d6,
    bright_cyan: 0x29b8db,
    bright_white: 0xa5a5a5,
};

/// The table for a mode.
pub fn of(dark: bool) -> &'static Ansi {
    match dark {
        true => &DARK,
        false => &LIGHT,
    }
}

/// A colour as `#rrggbb`, the form a theme file gives it.
pub fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

/// A colour as its three channels.
pub(crate) fn rgb(color: u32) -> (u8, u8, u8) {
    ((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

#[cfg(test)]
mod tests {
    use super::{hex, rgb};

    #[test]
    fn a_colour_reads_back_as_hex_and_channels() {
        assert_eq!(hex(0x0451a5), "#0451a5");
        assert_eq!(hex(0x000000), "#000000");
        assert_eq!(rgb(0xcd3131), (0xcd, 0x31, 0x31));
    }
}
