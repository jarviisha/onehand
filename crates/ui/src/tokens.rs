//! The sizes and the lift the app and its feature crates share.
//!
//! A size used in one place is a `const` beside that use. One that two places
//! have to agree on lives here, because the second place is often a feature
//! crate, which cannot reach into the binary hosting it — and two copies of
//! one number are two places for it to drift.

use gpui::{App, BoxShadow, Pixels, Rems, px, rems};
use gpui_component::ActiveTheme as _;

/// The height every bar along the top of a region shares: the conversation's
/// header, the Workbench's mode strip, the rail's workspace bar and foot, and
/// a page's column heads, so bars side by side line up across the window.
pub const BAR_H: Rems = rems(2.75);

/// A mode's own bar, under the Workbench's strip: the editor's tabs, the
/// reader's bar. A step under [`BAR_H`], so the second bar reads as the
/// mode's rather than as the panel's a second time; its controls keep their
/// size and stand on it with a little room.
pub const MODE_BAR_H: Rems = rems(2.);

/// A single-line list row: a rail row, a choice in the composer's popup. A
/// Workbench tree takes [`TREE_ROW_H`] instead. A little over a control's height, so a
/// long list of names reads as rows with air between them rather than a
/// block of text, and every list in the window stands at one pitch.
pub const ROW_H: Rems = rems(1.875);

/// A small control the pointer has to hit: a transcript button, a composer
/// chip, a blocking card's answers.
///
/// Fixed, because otherwise the *content* decides it and the content is not
/// the same shape: a chip with a word in it is as tall as that word's line
/// box, one holding only an icon as tall as the icon, and side by side they
/// came out about seven pixels apart on the same row.
///
/// The component library's **small** row, carried as a number because the
/// library hard-codes its row heights in a sizing branch and offers no way to
/// ask for one. Worth matching exactly: controls a few inches apart with
/// floors a pixel or two off read as a line of controls that do not agree.
pub const CONTROL_H: Rems = rems(1.5);

/// A control a step over [`CONTROL_H`], as tall as a list's row: the Issues
/// page's head, its choices and its actions, over the filter bar. Set by
/// [`crate::Tall`] over the small size rather than reached through the
/// library's default one, which letters a button at the reading size.
pub const FIELD_H: Rems = rems(1.875);

/// The one corner, in pixels as the theme holds it: every box, control,
/// field, row, well and floating surface, the library's own included, which
/// reads it as `radius` (and `radius_lg`, set to the same).
pub const CORNER: usize = 4;

/// Where a tree row's content starts from a dock's edge, and how far each
/// level of the tree steps in: Files and Markdown share both, so their trees
/// stand on one axis.
/// The inset is also where a Workbench bar's words start, so a bar and the
/// rows under it read down one edge.
pub const ROW_INSET: Rems = rems(1.);
pub const TREE_INDENT: Rems = rems(0.875);

/// How far a tree row's fill stands off the dock's side edges, so a hovered or
/// picked row reads as a block inside the dock rather than a band across it.
/// Inside [`ROW_INSET`], which leaves the words a row's own inset from the fill.
pub const ROW_GUTTER: Rems = rems(0.5);

/// A tree row's pitch (Files, Markdown): tighter than [`ROW_H`], since a tree
/// is a long run of one-line names scanned down a narrow dock.
pub const TREE_ROW_H: Rems = rems(1.5);

/// A tree row's fill, an odd number of pixels at the base rem. The text
/// system lays the baseline on a whole pixel, rounding down: at the chrome
/// size an even fill loses most of a pixel and the name sits a pixel high in
/// it, an odd one loses about a quarter. Only the base rem lands on whole
/// pixels; under another zoom the name may still sit a pixel off.
pub const TREE_ROW_FILL: Rems = rems(1.3125);

/// The room over and under a tree row's fill. Rows stack, so every gap between
/// two fills is the sum and no fill touches its neighbour; split one and two
/// rather than in halves, which would land the fill's edges between pixels.
pub const TREE_ROW_ABOVE: Rems = rems(0.0625);
pub const TREE_ROW_BELOW: Rems = rems(0.125);

/// The room held clear beside text the library's scrollbar scrolls, so the
/// thumb never lies over the words. In pixels, as the library draws the thumb:
/// its width and inset do not follow a panel's zoom, so neither does this.
pub const SCROLLBAR_W: Pixels = px(16.);

/// The scrollbar's thumb at rest, and under the pointer or dragged. A step
/// thinner than the library's own, which drew a bar heavier than the text it
/// scrolls; in pixels, as [`SCROLLBAR_W`] is, since the library draws it so.
pub const SCROLLBAR_THUMB: Pixels = px(4.);
pub const SCROLLBAR_THUMB_ACTIVE: Pixels = px(6.);

/// Chrome text: a panel's rows, boxes, forms and strips. A step under the
/// reading size, so dense chrome holds more and reading text stands apart.
pub const CHROME_TEXT: Rems = rems(0.8125);

/// Text inside a small object, a step under the meta size: a plate's caption.
/// The floor of chrome is the meta size, but what a small object holds scales
/// with the object rather than stopping at that floor.
pub const MICRO_TEXT: Rems = rems(0.6875);

/// Machine text: code, a command, a tool's output, a path.
///
/// A step under the secondary size, so what a program printed reads quieter
/// than what a person or the agent wrote, and one figure wherever it appears.
pub const CODE_TEXT: Rems = rems(0.8125);

/// Line height for reading text, as a multiple of its size: set as a ratio so
/// a well a step smaller keeps the same rhythm as the prose around it.
pub const LEADING: f32 = 1.6;

/// Line height for a title or a question that may wrap: looser than a
/// control's and tighter than prose, because it is one sentence read once —
/// and enough that a clipped line keeps its descenders.
pub const LEADING_TIGHT: f32 = 1.4;

/// The column a one-column page keeps to: the resume picker and the
/// workspace page's list. One figure, so moving between them
/// does not move the column.
pub const PAGE_NARROW: Rems = rems(35.);

/// The column for one thing read or written in full: a workflow's editor, an
/// issue, a project's page. Wider than [`PAGE_NARROW`], since what is in it reads as prose, and
/// no wider, or a line runs too long to find the start of the next.
pub const PAGE_PROSE: Rems = rems(48.);

/// The widest a workspace-wide page grows (the overview, Tasks, the Issues
/// list): room for two cards side by side, or a row's facts in columns,
/// without lines too long to read. One figure, so moving between the pages
/// does not move the column.
pub const PAGE_WIDE: Rems = rems(64.);

/// How wide a dialog that starts a run stands: it says what the workflow
/// does, where it works and what blocks it before its fields, and at the
/// library's own width that reads as a narrow column of wrapped lines.
/// A dialog of a field or two keeps the library's width.
pub const DIALOG_FORM: Rems = rems(40.);

/// The shadow a surface takes when it floats over another surface.
///
/// **Because the component ladder's shadows cannot be seen on this palette.**
/// Every step of gpui's `shadow_*` is pure black at a tenth of an alpha, which
/// is a sensible cue on a white page and nothing at all on a dark one: against
/// the floating surface that is a difference of three parts in 255, and the
/// popup drawn over a parked card came out looking like one tall panel with a
/// rule across it. That is the whole of what "a drop shadow over near-black is
/// invisible" has always meant here — not that shadow is the wrong idea, but
/// that *that* shadow is too faint to be one.
///
/// So the alpha is chosen per palette rather than shared. It has to be, and
/// this is the direction that surprises: the dark palette needs **more** than
/// four times the light one, because a black shadow on white has the whole
/// range to fall through while on near-black it has almost none. One value
/// tuned by eye in either mode is invisible in the other — the same trap the
/// selected fill fell into, in the other direction.
///
/// Two shadows for the reason the component library uses two: the tight one
/// draws the edge and the broad one carries the elevation. Blur costs the peak
/// opacity, so the number that matters is not the alpha written here but what
/// lands on the surface underneath — which is what the app's palette test
/// measures.
pub fn lift(cx: &App) -> Vec<BoxShadow> {
    let alpha = lift_alpha(cx.theme().mode.is_dark());
    let shadow = |y: f32, blur: f32, spread: f32, alpha: f32| BoxShadow {
        color: gpui::black().alpha(alpha),
        offset: gpui::point(gpui::px(0.), gpui::px(y)),
        blur_radius: gpui::px(blur),
        spread_radius: gpui::px(spread),
        inset: false,
    };
    vec![
        shadow(2., 4., -1., alpha),
        shadow(10., 20., -4., alpha * 0.8),
    ]
}

/// The tight shadow's alpha on the dark palette or the light one. Exposed for
/// the palette's contrast test, which works from a resolved ramp rather than
/// the theme in force; a call site wants [`lift`].
pub fn lift_alpha(dark: bool) -> f32 {
    match dark {
        true => 0.6,
        false => 0.14,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the base rem of 16px.
    fn px(r: Rems) -> f32 {
        r.0 * 16.
    }

    #[test]
    fn tree_row_fill_is_odd_and_rows_stack_at_the_pitch() {
        assert_eq!(px(TREE_ROW_FILL), 21.);
        assert_eq!(
            px(TREE_ROW_ABOVE) + px(TREE_ROW_FILL) + px(TREE_ROW_BELOW),
            24.
        );
        assert_eq!(px(TREE_ROW_H), 24.);
    }

    #[test]
    fn words_stand_inside_the_fill() {
        assert_eq!(px(ROW_GUTTER), 8.);
        assert_eq!(px(ROW_INSET), 16.);
    }
}
