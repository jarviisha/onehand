//! Every value the proposed UI needs that gpui and gpui-component do not give:
//! chrome heights, reading type, the width budgets and the two palettes.
//!
//! All sizes are rems. Chrome (bars, rows, controls, rail) is meant to stay
//! fixed when the reading zoom changes; only the `TEXT_READ*` sizes and the
//! chat's minimum width scale with it.

use gpui::{Hsla, rgb, rgba};

// ---- what the library already gives ----------------------------------------

// Gaps, paddings, the UI's text sizes, control heights, icon sizes and radii
// come from gpui's scale (`gap_2`, `text_xs`, `h_6`), the library's sizes
// (`small`, `xsmall`) and `cx.theme().radius`/`radius_lg`, as in the app. Only
// what neither gives is named here.

/// The page gutter that `px_4` draws, for the width sums that subtract it.
pub const GUTTER: f32 = 1.0;

// ---- chrome heights: outside the reading zoom -----------------------------

/// The agent header, the Workbench mode strip, the terminal tabs and every
/// page header share this height, so bars side by side line up.
pub const BAR_H: f32 = 2.75;
/// A detail header inside a dock: the `← Files` line.
pub const SUBBAR_H: f32 = 2.25;
/// A single-line rail or nav row.
pub const ROW_H: f32 = 1.875;
/// A tab never grows past this; a longer name truncates.
pub const TAB_MAX_W: f32 = 10.0;

// ---- reading type: the sizes the reading zoom multiplies ------------------

/// Reading sizes: the transcript, the composer, documents. These are the
/// ones the reading zoom multiplies, so they are numbers rather than gpui's
/// `text_*` calls, held to that scale: `text_base`, a step over the UI's
/// `text_sm`.
pub const TEXT_READ: f32 = 1.0;
/// Activity lines and code: `text_sm`.
pub const TEXT_READ_SM: f32 = 0.875;
/// Headings in the agent's prose: `text_xl` and `text_lg`. A third level
/// keeps the body size and only takes weight.
pub const TEXT_READ_H1: f32 = 1.25;
pub const TEXT_READ_H2: f32 = 1.125;
/// The space between paragraphs in the agent's prose: `gap_2`.
pub const PARAGRAPH_GAP: f32 = 0.5;
/// Line height as a multiple of the size.
pub const LEADING_UI: f32 = 1.45;
pub const LEADING_READ: f32 = 1.6;
pub const LEADING_DOC: f32 = 1.65;
/// The reading zoom: one step per key press, snapped so stepping up and back
/// returns to exactly 1, and bounded both ways.
pub const ZOOM_STEP: f32 = 0.1;
pub const ZOOM_MIN: f32 = 0.7;
pub const ZOOM_MAX: f32 = 2.0;

// ---- lines: device pixels on purpose --------------------------------------

/// A hairline is a property of the screen, not of the reading size, so it is
/// the one length given in pixels; gpui rounds it up to one device pixel.
pub const HAIRLINE_PX: f32 = 0.5;
/// A seam's line while it is being dragged.
pub const SEAM_DRAG_PX: f32 = 2.0;
/// The composer's lift, in pixels as a hairline is: the tight shadow's blur,
/// and the soft one's drop and blur.
pub const LIFT_EDGE_BLUR_PX: f32 = 1.5;
pub const LIFT_DROP_PX: f32 = 4.0;
pub const LIFT_BLUR_PX: f32 = 16.0;

// ---- marks ----------------------------------------------------------------

/// A state dot.
pub const DOT: f32 = 0.4375;
/// The stable column a status mark or a row's icon sits in, so names never
/// shift beside it.
pub const DOT_COLUMN: f32 = 1.0;
/// The dot on a header button saying something behind it is alive: smaller
/// than a row's, because it sits on a glyph, and inset from the corner so it
/// stays inside the button.
pub const BADGE_DOT: f32 = DOT * 0.75;
/// The band at the end of a name too long for its room, where the text fades
/// into the row instead of ending on an ellipsis.
pub const FADE_W: f32 = 1.25;
/// A session row's text starts under its project's name: icon plus gap.
pub const RAIL_INDENT: f32 = 1.75;
/// Each level of a file tree steps in this far: half a rail session's indent,
/// because a tree runs many levels deep and a rail only one.
pub const TREE_INDENT: f32 = 0.875;

// ---- width budgets, measured after the rail -------------------------------

/// How wide a seam is to grab. The line drawn in it stays a hairline.
pub const SEAM_GRAB_W: f32 = 0.375;
/// The rail opens at `RAIL_W`, room for a session's two lines beside its
/// status and actions, and resizes between the other two; it hides
/// completely, never to icons.
pub const RAIL_W: f32 = 20.0;
pub const RAIL_MIN_W: f32 = 14.5;
pub const RAIL_MAX_W: f32 = 28.0;
/// The chat's minimum beside a dock, times the reading zoom. Below it the
/// Workbench takes the content area instead of leaving a thin chat strip.
pub const CHAT_MIN: f32 = 30.0;
/// The transcript column, inset included, centred with equal gutters.
pub const READ_MAX: f32 = 44.0;
/// The narrowest useful Workbench.
pub const DOCK_MIN: f32 = 24.0;
/// The Workbench's opening width; below it the mode strip becomes a select.
pub const DOCK_PREF: f32 = 30.0;
/// The terminal's opening height, and the least it is drawn at.
pub const TERM_H: f32 = 15.0;
pub const TERM_MIN_H: f32 = 6.0;
/// What the conversation always keeps above the terminal: enough to read a
/// few lines and reach the composer.
pub const READING_MIN_H: f32 = 16.0;
/// How much more room a split needs to come back than to hold, so a window
/// resting on the threshold does not flicker between presentations.
pub const SPLIT_SLACK: f32 = 1.0;
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
/// A text field at the end of a form row: room for a command, and half the
/// row's stacking width, so its label still has the other half beside it.
pub const FIELD_W: f32 = FORM_STACK / 2.0;
/// The composer, everything pinned on it and the popups opening from it share
/// this width, so the stack reads as one object rather than panels that fail
/// to line up. The same cap as the app's composer: a message being written is
/// a few lines and one row of controls, and much wider than this those
/// controls end up a hand's width apart with nothing between them.
pub const COMPOSER_MAX: f32 = 44.0;
/// Below this the strip under the composer takes two lines, the branch over
/// the mode.
pub const COMPOSER_SPLIT: f32 = 36.0;
/// The user's bubble, so a long prompt wraps well short of the left axis.
pub const BUBBLE_MAX: f32 = 28.0;
/// An overview project tile; tiles wrap rather than shrink.
pub const TILE_W: f32 = 14.0;
/// A dialog never widens past this; a long name wraps in its body.
pub const DIALOG_MAX: f32 = 28.0;
/// The composer cards gallery: a stack plus its specimen frame.
pub const GALLERY_MAX: f32 = 48.0;
/// A popup opening from a chip, rather than spanning the composer.
pub const MENU_W: f32 = 17.0;
pub const MENU_WIDE_W: f32 = 20.0;
/// Sessions a project, or a flat list in the rail, shows before it says how
/// many more there are: about half a rail's height of two-line rows.
pub const SESSION_CAP: usize = 6;
/// Rows a popup shows before it says how many more there are.
pub const POPUP_LIST_CAP: usize = 6;
/// A command's output shows this many last lines until asked for the rest.
pub const OUTPUT_TAIL: usize = 5;

// ---- state steps for filled controls -------------------------------------

/// How far a filled control's hover moves away from the surface, and its press.
pub const HOVER_STEP: f32 = 0.12;
pub const PRESS_STEP: f32 = 0.2;
/// The solid danger fill darkens in both modes, so white stays legible on it.
pub const DANGER_HOVER_STEP: f32 = 0.06;
pub const DANGER_PRESS_STEP: f32 = 0.12;
/// Selected text: the accent, thinned so the text under it still reads.
pub const SELECTION_ALPHA: f32 = 0.25;
/// A state's ink thinned to a fill: a diff line's green or red, an error's
/// banner.
pub const STATE_TINT: f32 = 0.1;

// ---- colour ---------------------------------------------------------------

/// One palette. Neutrals carry everything; a hue only ever means state:
/// blue is running (and links), amber waits on the person, green is done, red
/// is danger. A selected row is a faint ink tint, never the accent.
///
/// Solid fills only: no gradients, no textures. Shadows only on what floats
/// (the composer card, dialogs, menus, popups).
#[derive(Clone, Copy)]
pub struct Palette {
    /// The reading surface.
    pub page: Hsla,
    /// The rail, code wells, the user's bubble.
    pub sunken: Hsla,
    /// Docks, cards, dialogs.
    pub panel: Hsla,
    /// The composer card: it has no edge, so its fill and its lift stand it
    /// clear of the page and of `sunken`, the wells' and the bubble's.
    pub raised: Hsla,
    /// The composer's lift: a tight shadow that draws its outline where a
    /// border would, and a soft one under it.
    pub lift_edge: Hsla,
    pub lift: Hsla,
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
    // The running and done badges' fills: no badge here is in either state yet.
    #[allow(dead_code)]
    pub accent_bg: Hsla,
    pub warning: Hsla,
    pub warning_bg: Hsla,
    pub success: Hsla,
    // The running and done badges' fills: no badge here is in either state yet.
    #[allow(dead_code)]
    pub success_bg: Hsla,
    /// Danger as ink on a surface.
    pub danger: Hsla,
    /// Danger as a fill, with `on_danger` on it: the same red in both modes.
    pub danger_solid: Hsla,
    pub on_danger: Hsla,
    /// The one primary action in a region; never two side by side.
    pub primary_bg: Hsla,
    pub primary_fg: Hsla,
    /// Under a dialog. The lab still draws the library's own overlay there.
    #[allow(dead_code)]
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
        page: c(0xFFFFFF),
        sunken: c(0xEEEEEE),
        panel: c(0xF3F3F3),
        // White, as the page: a grey card read as a well. The lift draws it.
        raised: c(0xFFFFFF),
        lift_edge: ca(0x1F1F1F2E),
        lift: ca(0x1F1F1F1A),
        text: c(0x1F1F1F),
        text2: c(0x616161),
        muted: c(0x888888),
        hairline: c(0xD4D4D4),
        control: c(0xC7C7C7),
        // Thick enough that a ghost control's hover shows on a panel, where
        // the tabs and the composer's chips sit: #E3E3E3 there, #EEEEEE on
        // the page. An open chip lands near #DADADA, a step above it.
        selected: ca(0x1F1F1F14),
        chip_on: ca(0x1F1F1F1E),
        accent: c(0x007ACC),
        accent_bg: ca(0x007ACC1A),
        // No waiting or danger state in the reference set: these keep the
        // previous hues.
        warning: c(0x854F0B),
        warning_bg: c(0xFAEEDA),
        success: c(0x16825D),
        success_bg: ca(0x16825D1A),
        danger: c(0xA32D2D),
        danger_solid: c(0xA32D2D),
        on_danger: c(0xFFFFFF),
        primary_bg: c(0x1F1F1F),
        primary_fg: c(0xFFFFFF),
        scrim: ca(0x1F1F1F52),
    }
}

/// Its own set rather than the light one inverted: the same hierarchy and
/// the same geometry, on near-black greys.
pub fn dark() -> Palette {
    Palette {
        page: c(0x181818),
        sunken: c(0x282828),
        panel: c(0x202020),
        // A shadow barely shows on near-black, so the fill does the work: a
        // clear step above `sunken`, the lift only deepening it.
        raised: c(0x303030),
        lift_edge: ca(0x00000099),
        lift: ca(0x00000066),
        text: c(0xECECEC),
        text2: c(0x979797),
        muted: c(0x767676),
        hairline: c(0x404040),
        control: c(0x404040),
        // Thicker than the light set's: a light tint on a dark surface shows
        // far less than the same share of ink on white. This lands near
        // #323232 on a panel, the reference's chosen tab, and #2A2A2A on the
        // page; an open chip near #3E3E3E.
        selected: ca(0xECECEC16),
        chip_on: ca(0xECECEC26),
        // Blue in both modes, the light set's hue lifted to read on near-black:
        // the reference's yellow sat beside the waiting amber, and running and
        // waiting must never be told apart by a shade.
        accent: c(0x3794FF),
        accent_bg: ca(0x3794FF2E),
        // No waiting or danger state in the reference set: these keep the
        // previous hues.
        warning: c(0xFAC775),
        warning_bg: ca(0xBA751733),
        success: c(0x1FBD53),
        success_bg: ca(0x1FBD532E),
        danger: c(0xF09595),
        danger_solid: c(0xA32D2D),
        on_danger: c(0xFFFFFF),
        primary_bg: c(0xECECEC),
        primary_fg: c(0x181818),
        scrim: ca(0x0000008C),
    }
}
