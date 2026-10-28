//! The colours derived from the palette: status ink, the middle ink, the
//! faint rule, the control fill and the pale tints.

use gpui::{App, Hsla};
use gpui_component::{ActiveTheme as _, Colorize as _};

/// Status colours used as ink on the app's normal surfaces.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: a feature crate
/// draws change badges and cannot reach into the binary hosting it, and a
/// second copy of the derivation is a second place for the raw status fill to
/// be used as ink — which is the mistake this exists to prevent.
#[derive(Clone, Copy)]
pub struct StatusInk {
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    /// What is over and settled, such as a closed issue: magenta, the hue a
    /// closed issue takes on the forges people know.
    pub closed: Hsla,
}

/// Resolve status ink from the active palette.
///
/// Base hues already switch between darker 600-level colours in light mode and
/// brighter 400-level colours in dark mode. Pulling them part of the way toward
/// the theme foreground gives small labels and thin icons enough contrast
/// without inventing a second set of hues beside the ramp.
///
/// **How far is set by the well, not by the reading surface.** A tool's status
/// word, a diff's added and removed lines and a terminal's exit code are all
/// drawn on the sunk fill rather than on the surface, and light amber is the
/// one that runs out of margin there first.
pub fn status_ink(cx: &App) -> StatusInk {
    let theme = cx.theme();
    StatusInk {
        danger: status_hue(theme.red, theme.foreground),
        warning: status_hue(theme.yellow, theme.foreground),
        success: status_hue(theme.green, theme.foreground),
        closed: status_hue(theme.magenta, theme.foreground),
    }
}

/// One hue, pulled toward the foreground.
///
/// `pub` because the app's contrast test asserts this derivation directly: it
/// works from a resolved ramp rather than from the theme in force, so it cannot
/// go through [`status_ink`], which reads a global. Not a way in for anything
/// else — a call site wanting status ink wants all three at once.
pub fn status_hue(base: Hsla, foreground: Hsla) -> Hsla {
    base.mix_oklab(foreground, 0.70)
}

/// The middle ink, the step between prose and meta ink, on a neutral surface.
///
/// **Why two steps are not enough in one place.** A completion row stacks three
/// kinds of text and they are not equal: the name, which is what the row is;
/// the detail lying on the same line beside it — a folder, a description, a
/// type icon, an issue's number — which is context for that name; and the label
/// naming the run of rows the whole thing sits in.
///
/// The name has to be at full strength, because a list is read as a column of
/// names and everything else second. The label has to be at the bottom, because
/// it is found when looked for and ignored the rest of the time. That leaves
/// the detail, and it can be at neither end: level with the name it competes
/// with the thing it is describing, and down at the label it is the same ink as
/// a heading two lines up while sitting immediately beside a name at full
/// strength — a gap that reads as the row trailing off.
///
/// So it sits here, and this step is what makes the row three things in order
/// rather than two things and a repeat.
///
/// (The run of a name a query matched is *not* one of these. It has nowhere
/// above full strength to go, so it is carried by weight instead — which is
/// why four roles fit in three inks.)
///
/// Derived rather than named as a ramp step: both palettes get it from values
/// they already hold, so there is no second ramp to keep in step by hand and no
/// library token borrowed for a meaning it does not have.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: a feature crate sets a
/// detail beside a name too (an issue's number beside its title, a plugin's
/// version beside its name), and a second copy of the derivation is a second
/// place for the step to drift.
pub fn meta_ink(cx: &App) -> Hsla {
    middle_ink(cx.theme().foreground, cx.theme().muted_foreground)
}

/// The step halfway from `full` to `meta`, mixed rather than thinned so it is
/// one solid colour wherever it lands. On a fill, `full` is the ink paired with
/// that fill, so the step stays a step there too.
///
/// `pub` for the reason [`status_hue`] is: the app's contrast test asserts this
/// derivation from a resolved ramp.
pub fn middle_ink(full: Hsla, meta: Hsla) -> Hsla {
    full.mix_oklab(meta, 0.5)
}

/// A hairline a step fainter than the theme's `border`, halfway to the
/// surface it lies on: the lines a list running the pane's whole width is
/// parted by, from the header over it and the foot under it, which the full
/// hairline would draw as a frame round the page.
pub fn faint_rule(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.border.mix_oklab(theme.background, 0.5)
}

/// How much of the hover step the control fill keeps; the rest is the
/// selected step.
const CONTROL_FILL: f32 = 0.8;

/// The fill a ghost control takes hovered, pressed or chosen, and a picker
/// pointed at. Between the hover step a row takes and the selected step a
/// chosen row does, nearer the hover: at the selected step every such control
/// was the heaviest fill on screen. The selected step is left to a chosen row,
/// beside the hovered rows it has to stand out from.
pub fn control_fill(cx: &App) -> Hsla {
    let theme = cx.theme();
    control_fill_of(theme.list_hover, theme.accent)
}

/// [`control_fill`] from the two steps it lies between. `pub` for the reason
/// [`status_hue`] is: the app's contrast test works from a resolved ramp.
pub fn control_fill_of(hover: Hsla, selected: Hsla) -> Hsla {
    hover.mix_oklab(selected, CONTROL_FILL)
}

/// How much of a hue a pale fill keeps; the rest is the surface it sits on.
const TINT: f32 = 0.16;

/// A hue as a pale solid fill, and the ink that reads on it: a badge, or a
/// label that asks for something.
#[derive(Clone, Copy)]
pub struct Tint {
    pub fill: Hsla,
    pub ink: Hsla,
}

/// `base` tinted over the reading surface.
///
/// **Solid, never a hue at partial opacity.** A see-through fill takes
/// whatever is under it, so the same badge reads one shade on a row and
/// another on that row hovered or selected, and its ink is tested against
/// neither. Mixed here, the fill is one colour wherever it lands, and the
/// app's contrast test holds the ink to it.
pub fn tint(base: Hsla, cx: &App) -> Tint {
    tint_over(base, cx.theme().background, cx.theme().foreground)
}

/// The tint of a badge that says nothing about state: a plain tag, or a
/// count of nothing, which in a state's hue would draw the eye to it.
pub fn neutral_tint(cx: &App) -> Tint {
    let theme = cx.theme();
    Tint {
        fill: theme.secondary,
        ink: theme.secondary_foreground,
    }
}

/// [`tint`] from the three colours it is made of. `pub` for the reason
/// [`status_hue`] is: the app's contrast test works from a resolved ramp.
pub fn tint_over(base: Hsla, surface: Hsla, foreground: Hsla) -> Tint {
    Tint {
        fill: base.mix_oklab(surface, TINT),
        ink: status_hue(base, foreground),
    }
}
