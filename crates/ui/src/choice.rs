//! Picking one of a few: a picker's trigger, a choice, the switch and the
//! track that hold choices side by side.

use crate::{ControlSize, Quiet as _, action, control_fill};
use gpui::{
    AnyElement, App, Div, ElementId, IntoElement, ParentElement as _, SharedString, Styled as _,
    Window, div,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Selectable as _, StyledExt as _};
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
/// `selected` on the choice that holds. Every pick-one row draws its choices
/// with this, the halves of a [`switch`] and a row of filter chips alike, so
/// one condition keeps one shape.
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

/// A choice between a few views of one panel: a [`choice`] per view, every
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
/// **One edged track.** Loose ghosts side by side read as two words, the
/// unchosen one as plain text: nothing said they were one control with one
/// choice standing. The choices sit in a control's edge, the chosen one
/// filled, so the track says "one of these" before anything is pressed.
///
/// **No two fills touch.** A hovered choice beside the chosen one would run
/// into it as one band, and either into the edge: the track pads its choices
/// off its edge and parts them by a gap, a step taller than the control it
/// holds for it.
///
/// `size` is the role's, so its choices stand at the height of that role's
/// buttons and inputs; the track around them stands a step taller.
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
        .border_1()
        .border_color(cx.theme().input)
        .p_0p5()
        .gap_0p5()
        .children(labels.iter().enumerate().map(|(i, label)| {
            let pick = pick.clone();
            choice((id, i), label.clone(), i == active, size, cx)
                .flex_1()
                .min_w_0()
                .on_click(move |_, window, cx| pick(&i, window, cx))
        }))
        .into_any_element()
}

/// One of more choices than a [`switch`] holds, each at its own width: the
/// same edged track, padded and parted as a switch's, its [`choice`]s
/// wrapping onto further lines in a narrow container. The edge is what
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
        .border_1()
        .border_color(cx.theme().input)
        .p_0p5()
        .gap_0p5()
        .children(choices)
}
