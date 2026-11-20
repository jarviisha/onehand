//! Picking one of a few: a picker's trigger, a choice, a segment, and the
//! switch and the track that hold segments side by side.

use crate::{ControlSize, Quiet as _, action, control_fill};
use gpui::{
    AnyElement, App, BoxShadow, Div, ElementId, Hsla, IntoElement, ParentElement as _, Rems,
    SharedString, Styled as _, Window, div, point, px, rems,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Colorize as _, Selectable as _, StyledExt as _, Theme};
use std::rc::Rc;

/// A picker's trigger: a button naming what it is set to, then the caret that
/// opens its menu, so it reads as a control. Unset, it names what it picks,
/// as a ghost in the muted ink, its caret with it, as a label at rest is;
/// `set` away from where it starts, it names the choice on the secondary fill
/// in that fill's ink, so the picker narrowing a list is the one that stands
/// out. `size` is the role's.
pub fn picker(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    set: bool,
    size: ControlSize,
    cx: &App,
) -> Button {
    let button = action(id).label(label.into()).dropdown_caret(true);
    size.on(match set {
        true => button.secondary(),
        // The variant's own ink rather than one laid over it, so the caret,
        // which the library inks from the variant, rests with the words.
        false => {
            let theme = cx.theme();
            button.custom(
                ButtonCustomVariant::new(cx)
                    .foreground(theme.muted_foreground)
                    .hover(control_fill(cx))
                    .active(control_fill(cx)),
            )
        }
    })
}

/// One of a few choices, the one taken among them filled: a ghost button,
/// `selected` on the choice that holds. A row of filter chips draws its
/// choices with this; a choice inside a track is a [`segment`].
///
/// A [`Quiet`](crate::Quiet) ghost, so a selected choice and a hovered one share the
/// selected step, the one `accent` is too; selected, it has no edge and no
/// reaction to the pointer: a fill that differs lifts by itself, and pressing
/// what is already chosen changes nothing.
pub fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    size: ControlSize,
    cx: &App,
) -> Button {
    size.on(action(id).quiet(cx).label(label.into()).selected(on))
}

/// One choice inside a track, a [`switch`]'s or a [`choice_track`]'s: chosen,
/// it stands on the pick's fill ([`segment_fills`]), in the prose ink, under a
/// tight lift, and
/// pressing it changes nothing; the others are bare words in the muted ink
/// on the track's sunk fill, a hover a step between the two fills, never the
/// chosen look.
///
/// A custom ghost through [`action`], so the cursor, the focus ring and a
/// refusal stay the library's.
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    size: ControlSize,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    let fills = segment_fills(theme);
    let button = action(id).label(label.into()).selected(on);
    let button = size.on(match on {
        true => button
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(fills.pick)
                    .foreground(theme.foreground)
                    .hover(fills.pick)
                    .active(fills.pick),
            )
            .shadow(segment_lift(cx)),
        false => {
            let step = fills.track.mix_oklab(fills.pick, 0.5);
            button.custom(
                ButtonCustomVariant::new(cx)
                    .foreground(theme.muted_foreground)
                    .hover(step)
                    .active(step),
            )
        }
    });
    // The track stands at the role's height; a segment fills it less its inset.
    button.h(rems(size.height().0 - 2. * SEGMENT_INSET.0))
}

/// How far a track holds its segments in from its rim: enough that the pick
/// reads as standing on the track rather than filling it.
const SEGMENT_INSET: Rems = rems(0.1875);

/// A pick-one track's two fills.
pub struct SegmentFills {
    /// The track's: the well, a step off the page and off a floating surface.
    pub track: Hsla,
    /// The chosen segment's: a step lighter than the track in both palettes,
    /// so it reads as raised. The page fill by day; by night the page is
    /// darker than the well, and the pick takes the selected step instead.
    pub pick: Hsla,
}

/// [`SegmentFills`] from a resolved theme. `pub` for the reason
/// [`control_fill_of`](crate::control_fill_of) is: the app's contrast test
/// works from a resolved ramp.
pub fn segment_fills(theme: &Theme) -> SegmentFills {
    SegmentFills {
        track: theme.muted,
        pick: match theme.mode.is_dark() {
            true => theme.accent,
            false => theme.background,
        },
    }
}

/// The chosen segment's lift, in pixels as a hairline is: one tight shadow
/// where an edge would be. The prose ink, thinned, on the light page; on the
/// dark one, where no shadow shows, the hairline's colour.
fn segment_lift(cx: &App) -> Vec<BoxShadow> {
    let theme = cx.theme();
    let color = match theme.mode.is_dark() {
        true => theme.border,
        false => theme.foreground.alpha(0.16),
    };
    vec![BoxShadow {
        color,
        offset: point(px(0.), px(0.5)),
        blur_radius: px(1.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// A choice between a few views of one panel: a [`segment`] per view, every
/// one the same width across the row.
///
/// **Drawn here, after both of the library's answers were tried.** A
/// `ButtonGroup` joins its buttons into one outlined block. A segmented
/// `TabBar` sizes every tab to its own label inside a `flex_shrink_0` nothing
/// outside the library can stretch, so one label came out two thirds the width
/// of the other, both against the left edge of a bar as wide as its panel. So
/// each view is a choice at `flex_1`.
///
/// **Here rather than in the app** for the reason [`action`] is: the app and a
/// feature crate both draw one, and a feature crate cannot reach into the binary
/// hosting it.
///
/// **One filled track.** Loose ghosts side by side read as two words, the
/// unchosen one as plain text: nothing said they were one control with one
/// choice standing. The choices sit on one sunk fill, no edge, the chosen one
/// raised off it, so the track says "one of these" before anything is
/// pressed; an edge inside a settings box read as a box in a box.
///
/// **No two fills touch.** A hovered choice beside the chosen one would run
/// into it as one band, and either into the track's rim: the track pads its
/// choices in from its rim and parts them by a gap.
///
/// `size` is the role's, so the track stands at the height of that role's
/// buttons and inputs and its choices inside it, held in by its inset.
pub fn switch(
    id: &'static str,
    labels: &[SharedString],
    active: usize,
    size: ControlSize,
    pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let pick = Rc::new(pick);

    div()
        .h_flex()
        .w_full()
        .rounded(cx.theme().radius)
        .bg(segment_fills(cx.theme()).track)
        .p(SEGMENT_INSET)
        .gap_0p5()
        .children(labels.iter().enumerate().map(|(i, label)| {
            let pick = pick.clone();
            segment((id, i), label.clone(), i == active, size, cx)
                .flex_1()
                .min_w_0()
                .on_click(move |_, window, cx| pick(&i, window, cx))
        }))
        .into_any_element()
}

/// One of more choices than a [`switch`] holds, each at its own width: the
/// same filled track, padded and parted as a switch's, its [`segment`]s
/// wrapping onto further lines in a narrow container. The fill is what
/// makes a row of loose words read as one setting; stacked settings stand
/// apart as boxes rather than running into one paragraph. The track holds
/// its choices and no more: run on past the last, it is an empty stretch
/// that reads as room for one still to come.
pub fn choice_track(choices: impl IntoIterator<Item = impl IntoElement>, cx: &App) -> Div {
    div()
        .h_flex()
        .flex_wrap()
        .self_start()
        .max_w_full()
        .rounded(cx.theme().radius)
        .bg(segment_fills(cx.theme()).track)
        .p(SEGMENT_INSET)
        .gap_0p5()
        .children(choices)
}
