//! Every value the proposed UI is built from, in one place: spacing roles,
//! chrome sizes, type, radii, the width budgets and the two palettes.
//!
//! All sizes are rems. Chrome (bars, rows, controls, rail) is meant to stay
//! fixed when the reading zoom changes; only the `TEXT_READ*` sizes and the
//! chat's minimum width scale with it.
//!
//! Some values are not drawn by any screen here yet; they are kept because the
//! screens still to come (see the README's backlog) are specified in them.
#![allow(dead_code)]

use gpui::{Hsla, rgb, rgba};

// ---- spacing: chosen by relationship, never by global replacement ---------

/// Closely related metadata; an icon beside its label.
pub const TIGHT: f32 = 0.25;
/// Adjacent controls; the content of one compact row.
pub const CONTROL: f32 = 0.5;
/// Related blocks: field groups, a card's insides, stacked controls.
pub const RELATED: f32 = 0.75;
/// The default gutter of a page or a reader. One owner per outer padding: a
/// page inset, a card inset and a list inset never stack on the same label.
pub const INSET: f32 = 1.0;
/// Between the distinct groups of a page.
pub const SECTION: f32 = 1.5;
/// Only between substantially different page sections, never between fields.
pub const MAJOR: f32 = 2.0;

// ---- chrome heights: outside the reading zoom -----------------------------

/// The agent header, the Workbench mode strip, the terminal tabs and every
/// page header share this height, so bars side by side line up.
pub const BAR_H: f32 = 2.75;
/// A detail header inside a dock: the `← Files` line.
pub const SUBBAR_H: f32 = 2.25;
/// A single-line rail or nav row.
pub const ROW_H: f32 = 1.875;
/// Buttons, inputs, tabs.
pub const CONTROL_H: f32 = 1.75;
pub const CONTROL_H_SM: f32 = 1.5;
/// A tab never grows past this; a longer name truncates.
pub const TAB_MAX_W: f32 = 10.0;

// ---- type: two weights only, 400 and 500; sentence case everywhere --------

/// Metadata, chips, sub-lines.
pub const TEXT_XS: f32 = 0.75;
/// The UI default: rows, buttons, bars.
pub const TEXT_SM: f32 = 0.8125;
/// Section headings.
pub const TEXT_MD: f32 = 0.875;
/// Dialog and form titles.
pub const TEXT_LG: f32 = 0.9375;
/// A page title (the task detail).
pub const TEXT_XL: f32 = 1.25;
/// Reading sizes: the transcript, the composer, documents. These are the
/// ones the reading zoom multiplies.
pub const TEXT_READ: f32 = 0.9375;
/// Activity lines and code.
pub const TEXT_READ_SM: f32 = 0.8125;
/// The composer's context strip.
pub const TEXT_READ_XS: f32 = 0.75;
/// Line height as a multiple of the size.
pub const LEADING_UI: f32 = 1.45;
pub const LEADING_READ: f32 = 1.6;
pub const LEADING_DOC: f32 = 1.65;

// ---- marks ----------------------------------------------------------------

pub const ICON: f32 = 0.875;
pub const ICON_SM: f32 = 0.75;
/// A state dot, in a stable 1rem column so names never shift beside it.
pub const DOT: f32 = 0.4375;
pub const DOT_COLUMN: f32 = 1.0;
/// A session row's text starts under its project's name: icon plus gap.
pub const RAIL_INDENT: f32 = 1.75;

// ---- radii: small; a pill only for a status badge -------------------------

/// Inline code, a key cap.
pub const RADIUS_XS: f32 = 0.25;
/// Buttons, rows, inputs, tabs.
pub const RADIUS_SM: f32 = 0.375;
/// Code blocks, list boxes, popups.
pub const RADIUS_MD: f32 = 0.5;
/// A pinned card on the composer.
pub const RADIUS_LG: f32 = 0.625;
/// The composer, the user's bubble, a dialog.
pub const RADIUS_XL: f32 = 0.75;

// ---- width budgets, measured after the rail -------------------------------

/// The rail resizes between these; it hides completely, never to icons.
pub const RAIL_W: f32 = 14.5;
pub const RAIL_MAX_W: f32 = 20.0;
/// The chat's minimum beside a dock, times the reading zoom. Below it the
/// Workbench takes the content area instead of leaving a thin chat strip.
pub const CHAT_MIN: f32 = 30.0;
/// The transcript column, inset included, centred with equal gutters.
pub const READ_MAX: f32 = 44.0;
/// The narrowest useful Workbench.
pub const DOCK_MIN: f32 = 24.0;
/// The Workbench's opening width; below it the mode strip becomes a select.
pub const DOCK_PREF: f32 = 30.0;
/// The terminal's opening height.
pub const TERM_H: f32 = 15.0;
/// A list beside its detail: side by side only when the container holds
/// `LIST_W + DETAIL_MIN`, else one at a time under a labelled back link.
pub const LIST_W: f32 = 12.0;
pub const DETAIL_MIN: f32 = 24.0;
pub const SPLIT_MIN: f32 = LIST_W + DETAIL_MIN;
/// The Issues page's list, wider than a dock's because its rows carry more.
pub const ISSUE_LIST_W: f32 = 22.0;
/// A Markdown document's comfortable measure.
pub const DOC_MEASURE: f32 = 34.0;
/// A page's column.
pub const PAGE_MAX: f32 = 64.0;
/// Settings: the nav column while it fits, a select when it does not.
pub const SETTINGS_NAV: f32 = 11.0;
/// A form's width, and below `FORM_STACK` a row's label stacks over its control.
pub const FORM_MAX: f32 = 40.0;
pub const FORM_STACK: f32 = 32.0;
/// The composer and everything pinned on it share this width.
pub const COMPOSER_MAX: f32 = 40.0;
/// Below this the composer's context strip takes two lines.
pub const COMPOSER_SPLIT: f32 = 36.0;
/// The user's bubble, so a long prompt wraps well short of the left axis.
pub const BUBBLE_MAX: f32 = 28.0;
/// The composer's field before anything is typed.
pub const INPUT_MIN_H: f32 = 2.5;
/// Between a pinned card and the composer: a step over the gap inside a
/// stack, because two objects need a seam that reads as one.
pub const STACK_GAP: f32 = 0.625;
/// An overview project tile; tiles wrap rather than shrink.
pub const TILE_W: f32 = 14.0;
/// A dialog never widens past this; a long name wraps in its body.
pub const DIALOG_MAX: f32 = 28.0;
/// A popup opening from a chip, rather than spanning the composer.
pub const MENU_W: f32 = 17.0;
pub const MENU_WIDE_W: f32 = 20.0;
/// Rows a popup shows before it says how many more there are.
pub const POPUP_LIST_CAP: usize = 6;

// ---- colour ---------------------------------------------------------------

/// One palette. Warm neutrals carry everything; a hue only ever means state:
/// blue is running (and links), amber waits on the person, green is done, red
/// is danger. A selected row is a faint ink tint, never the accent.
///
/// Solid fills only: no gradients, no textures. Shadows only on what floats
/// (dialogs, menus, popups).
#[derive(Clone, Copy)]
pub struct Palette {
    /// The reading surface.
    pub page: Hsla,
    /// The rail, code wells, the user's bubble.
    pub sunken: Hsla,
    /// Docks, the composer, cards, dialogs.
    pub panel: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    /// Metadata, line numbers.
    pub muted: Hsla,
    /// Dividers.
    pub hairline: Hsla,
    /// A control's edge.
    pub control: Hsla,
    /// A selected or hovered row.
    pub selected: Hsla,
    /// A chip whose popup is open: a step above `selected`, so it still shows
    /// under the pointer.
    pub chip_on: Hsla,
    pub accent: Hsla,
    pub accent_bg: Hsla,
    pub warning: Hsla,
    pub warning_bg: Hsla,
    pub success: Hsla,
    pub success_bg: Hsla,
    /// Danger as ink on a surface.
    pub danger: Hsla,
    /// Danger as a fill, with `on_danger` on it: the same red in both modes.
    pub danger_solid: Hsla,
    pub on_danger: Hsla,
    /// The one primary action in a region; never two side by side.
    pub primary_bg: Hsla,
    pub primary_fg: Hsla,
    /// Under a dialog.
    pub scrim: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}
fn ca(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub fn light() -> Palette {
    Palette {
        page: c(0xFAF9F5),
        sunken: c(0xF1EFE8),
        panel: c(0xFFFFFF),
        text: c(0x2C2C2A),
        text2: c(0x5F5E5A),
        muted: c(0x888780),
        hairline: ca(0x8887804D),
        control: ca(0x88878073),
        selected: ca(0x2C2C2A0E),
        chip_on: ca(0x2C2C2A1C),
        accent: c(0x185FA5),
        accent_bg: c(0xE6F1FB),
        warning: c(0x854F0B),
        warning_bg: c(0xFAEEDA),
        success: c(0x3B6D11),
        success_bg: c(0xEAF3DE),
        danger: c(0xA32D2D),
        danger_solid: c(0xA32D2D),
        on_danger: c(0xFFFFFF),
        primary_bg: c(0x2C2C2A),
        primary_fg: c(0xFFFFFF),
        scrim: ca(0x2C2C2A52),
    }
}

/// Derived from the light palette rather than specified: the same hierarchy
/// and the same geometry, on warm near-black surfaces.
pub fn dark() -> Palette {
    Palette {
        page: c(0x1E1D1B),
        sunken: c(0x191817),
        panel: c(0x262523),
        text: c(0xECEAE3),
        text2: c(0xB4B2A9),
        muted: c(0x8C8A83),
        hairline: ca(0xB4B2A933),
        control: ca(0xB4B2A952),
        selected: ca(0xECEAE312),
        chip_on: ca(0xECEAE324),
        accent: c(0x85B7EB),
        accent_bg: ca(0x378ADD2E),
        warning: c(0xFAC775),
        warning_bg: ca(0xBA751733),
        success: c(0x97C459),
        success_bg: ca(0x6399222E),
        danger: c(0xF09595),
        danger_solid: c(0xA32D2D),
        on_danger: c(0xFFFFFF),
        primary_bg: c(0xECEAE3),
        primary_fg: c(0x1E1D1B),
        scrim: ca(0x0000008C),
    }
}
