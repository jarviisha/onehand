//! What the app hands its built-in plugins, and what they hand back.
//!
//! The things a plugin needs that it cannot reach into the binary for — a
//! button that answers the pointer, the derivation of status ink, and the
//! surface a dock is drawn on — plus the Workbench mode contract itself.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

mod confirm;
mod list_detail;
mod menu;
mod tabs;
pub mod tokens;
mod workbench;
pub use confirm::{Confirm, confirm_on};
pub use list_detail::{ListDetail, ListWidths, back_link, list_detail};
pub use menu::{menu_below, menu_item, menu_row};
pub use onehand_core::worktree::removal::Process;
pub use tabs::{
    Measured, TAB_MAX_W, TabStrip, measure_width, tab_chip, tab_close, tab_menu_rows, tab_select,
    tab_strip, unmeasured,
};
pub use workbench::{Ask, Request, WorkbenchMode};

use gpui::{
    AnyElement, App, Div, ElementId, Entity, Hsla, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Selectable as _, Sizable as _, Size,
    StyledExt as _,
};
use std::rc::Rc;

/// How a remote channel is opened, once its credential has been read.
pub type RemoteChannelFactory = fn(String) -> Box<dyn onehand_core::remote::types::RemoteChannel>;

/// A button that answers the pointer, which is every button Onehand draws.
///
/// The component library draws every button variant except `link` and `text`
/// with the **arrow** cursor. That is the platform convention this app is not
/// following: a session row, a completion candidate, a selector chip and an ask
/// choice are all hand-made `div`s that show a pointer, because that is the one
/// feedback a control gets *before* it is pressed. Half the actions on screen
/// answering the pointer and half not is worse than either rule applied whole —
/// the cursor stops meaning anything, and the only way left to find out whether
/// something is clickable is to click it.
///
/// The library re-applies the caller's own style refinement last, after its
/// `cursor_default`, so setting the cursor here wins — which is the whole reason
/// this can be a wrapper rather than a fork of the control.
///
/// **It also sets the small corner**, half the theme's: a control is pressed
/// or typed into, and the tighter corner tells it apart from the rows, chips
/// and blocks around it, which keep the theme's. Here, because every button
/// passes through here and nowhere else.
///
/// **It lives here rather than in the app** because a built-in plugin draws
/// buttons too and cannot reach into the binary that hosts it. A second copy in
/// each half is two places for the library's default to be let through, and the
/// bypass is one line and looks exactly like ordinary code — which is why a
/// guard counts the call sites and why there must be only one of these to count
/// against.
pub fn action(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .cursor_pointer()
        .rounded(ButtonRounded::Small)
}

/// A text field at a control's corner, on no fill of its own: the small
/// corner [`action`] gives a button, so a field and the button beside it are
/// one shape, and the surface it sits on, so its edge alone marks it. The
/// library draws a field at the theme's full corner, the one rows and blocks
/// keep, on a fill it derives from the control edge rather than from where
/// the field sits.
///
/// Here rather than in the app for the reason [`action`] is: a plugin draws
/// fields too, and one wrapper is one place for the corner to hold.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state)
        .rounded(cx.theme().radius_tokens().sm)
        .bg(cx.theme().transparent)
}

/// The cursor for an action that is currently refusing.
///
/// Applied on the disabled branch, so the pointer is a promise the control can
/// keep: it appears over things that will act on a click and nowhere else.
fn resting(button: Button) -> Button {
    button.cursor_default()
}

/// Refusing, and looking like it, in one call.
///
/// [`resting`] is the cursor half of a refusal and `disabled` is the behaviour
/// half, and while they were two separate calls six controls in the app had
/// made one without the other: a Save with nothing to save, a Submit with
/// nothing chosen, an Unbind with nothing bound, a Cancel already cancelling.
/// Every one of them sat under a pointer promising a press would do something.
///
/// It has to be explicit because the library re-applies the caller's own style
/// refinement *after* the cursor it picks, so [`action`]'s pointer outlives
/// being disabled — the control goes quiet, refuses the click, and still
/// beckons. Taking the pointer back in the same call that refuses is what stops
/// the two halves drifting apart again.
///
/// Here rather than in the app for the reason [`action`] is: a built-in plugin
/// draws buttons that refuse while they work.
pub trait Refuses: Sized {
    /// Refuse presses while `refusing` holds, and say so in the cursor.
    fn refuses(self, refusing: bool) -> Self;
}

impl Refuses for Button {
    fn refuses(self, refusing: bool) -> Self {
        use gpui_component::Disableable as _;

        match refusing {
            true => resting(self).disabled(true),
            false => self,
        }
    }
}

/// A sunk box of machine text: full width, the muted fill, the theme's corner
/// and the mono family, clipped so a long line cannot push the column wider.
/// The caller sets the text size and the padding, which differ by what is in it.
///
/// Here rather than in the app for the reason [`action`] is: the Issues mode
/// draws the same well as the conversation's, and a second copy is a second
/// place for its fill to drift.
pub fn well(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(theme.radius)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
}

/// One setting, stacked: its name, a line about it in meta text, and the
/// control under both at the column's full width -- so a field is as wide as
/// what can be typed into it, and a long description wraps above it rather
/// than beside it.
///
/// Here rather than in the app for the reason [`action`] is: a plugin's form
/// asks for settings too, and a second copy is a second place for a field's
/// rhythm to drift.
pub fn field(
    title: impl Into<SharedString>,
    about: Option<AnyElement>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .child(div().text_sm().child(title.into()))
                .children(about.map(|about| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(about)
                })),
        )
        .child(control)
}

/// The surface a dock draws on: the palette's panel step, one off the reading
/// surface the conversation sits on, so a dock reads as a surface of its own
/// beside the conversation rather than as a card inset in it. The app
/// writes that step into the theme slot read here, with its contrast tested
/// beside the rest of its ramp.
///
/// Here rather than in the app for the reason [`action`] is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn dock_surface(cx: &App) -> Hsla {
    cx.theme().tiles
}

/// Status colours used as ink on the app's normal surfaces.
///
/// Here rather than in the app for the reason [`action`] is: a built-in plugin
/// draws change badges and cannot reach into the binary hosting it, and a
/// second copy of the derivation is a second place for the raw status fill to
/// be used as ink — which is the mistake this exists to prevent.
#[derive(Clone, Copy)]
pub struct StatusInk {
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
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

/// [`tint`] from the three colours it is made of. `pub` for the reason
/// [`status_hue`] is: the app's contrast test works from a resolved ramp.
pub fn tint_over(base: Hsla, surface: Hsla, foreground: Hsla) -> Tint {
    Tint {
        fill: base.mix_oklab(surface, TINT),
        ink: status_hue(base, foreground),
    }
}

/// A mark the app's own icon set draws for a plugin, where the bundled set
/// has no drawing: an open issue's circle with a dot, and a refresh. The app
/// serves these under its own prefix, and its registry test holds each path to
/// the one it serves.
#[derive(Clone, Copy)]
pub enum AppIcon {
    CircleDot,
    Refresh,
}

impl gpui_component::IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::CircleDot => "onehand/icons/circle-dot.svg".into(),
            Self::Refresh => "onehand/icons/refresh-cw.svg".into(),
        }
    }
}

/// A mode's empty state: one line of meta text, centred in the body it would
/// fill.
///
/// Here rather than copied into each mode for the reason [`action`] is. Four
/// copies of a centred muted line is four places for one of them to drift into
/// a different silence from its neighbours, in a panel where the user switches
/// between them with one click.
pub fn hint(text: &'static str, cx: &App) -> AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// The line a mode draws under its body for a standing condition.
///
/// **Deliberately not a notification**, unlike the rest of the app's transient
/// status. A save refused because the file changed on disk, a document that has
/// outgrown the read's size bound, an editor that would not start: none of them
/// is news that may be missed, and a toast that fades leaves the user believing
/// the thing went through. Each is cleared by whatever answers it.
///
/// Drawn by the mode and not by the panel, because the panel no longer knows
/// what any of them mean — which is what makes one definition worth having.
pub fn status_line(message: String, cx: &App) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(status_ink(cx).warning)
        .child(message)
        .into_any_element()
}

/// A search field: a [`text_input`] led by the search mark and ended by the
/// library's clear button, so every search in the app and its plugins is one
/// shape. The caller sizes it, as it would a field.
pub fn search_input(state: &Entity<InputState>, cx: &App) -> Input {
    text_input(state, cx)
        .prefix(
            Icon::new(IconName::Search)
                .small()
                .text_color(cx.theme().muted_foreground),
        )
        .cleanable(true)
}

/// A field of several lines, on the corner and the surface [`text_input`]
/// gives a field of one: the library draws a textarea at the theme's full
/// corner on a fill of its own, and beside a single-line field in the same
/// form the two read as different controls.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    Textarea::new(state)
        .rounded(cx.theme().radius_tokens().sm)
        .bg(cx.theme().transparent)
}

/// A ghost button whose hover is the fill a selected one shows.
///
/// The library's ghost hovers on a fill it mixes from the bubble's grey at
/// four fifths opacity, which is no step of the app's ramp: beside a selected
/// ghost in the same strip, the hovered one came out a shade fainter, its edge
/// lost in the surface, and read as a smaller box. So the ghost is drawn here
/// as a custom variant on the ramp's own steps: no fill at rest, the selected
/// step on hover, on press and when selected, so hovered and selected are one
/// shape. Every ghost button in the app and its plugins is this, never a bare
/// `ghost()`.
pub trait Quiet {
    /// The ghost variant, hovering on the selected step.
    fn quiet(self, cx: &App) -> Self;
}

impl Quiet for Button {
    fn quiet(self, cx: &App) -> Self {
        let theme = cx.theme();
        self.custom(
            ButtonCustomVariant::new(cx)
                .foreground(theme.secondary_foreground)
                .hover(theme.secondary_active)
                .active(theme.secondary_active),
        )
    }
}

/// A control at a dialog's or a form's scale: [`tokens::FIELD_H`] high with
/// its words at the chrome size.
///
/// The library ties a control's words to its size: a default-size button
/// letters its label at the reading size, a step above a field of the same
/// size, and nothing outside it reaches the box those words sit in. Its height
/// and padding are on the control itself, though, and the caller's own land
/// after the library's. So this is the small size, lettered as chrome, given
/// back the default size's height, and padding on the spacing steps (`px_3`,
/// where the default's `px_2p5` falls between two).
pub trait Tall {
    /// The small size at a dialog's or a form's height.
    fn tall(self) -> Self;
}

impl Tall for Button {
    fn tall(self) -> Self {
        // `min_w` rather than `w`, so an icon alone stays square and a label
        // keeps its own width.
        self.small()
            .h(tokens::FIELD_H)
            .min_w(tokens::FIELD_H)
            .px_3()
    }
}

impl Tall for Input {
    fn tall(self) -> Self {
        self.small().h(tokens::FIELD_H)
    }
}

/// One of a few choices, the one taken among them filled: a ghost button,
/// `selected` on the choice that holds. Every pick-one row draws its choices
/// with this, the halves of a [`switch`] and a row of filter chips alike, so
/// one condition keeps one shape.
///
/// A [`Quiet`] ghost, so a selected choice and a hovered one share the
/// selected step, the one `accent` is too; selected, it has no edge and no
/// reaction to the pointer: a fill that differs lifts by itself, and pressing
/// what is already chosen changes nothing.
pub fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    cx: &App,
) -> Button {
    action(id).quiet(cx).label(label.into()).selected(on)
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
/// built-in plugin both draw one, and a plugin cannot reach into the binary
/// hosting it.
///
/// `size` is the library's own scale, so a switch sits level with the buttons
/// and inputs of that size beside it.
pub fn switch(
    id: &'static str,
    labels: &[SharedString],
    active: usize,
    size: Size,
    pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let pick = Rc::new(pick);

    div()
        .h_flex()
        .w_full()
        .gap_1()
        .children(labels.iter().enumerate().map(|(i, label)| {
            let pick = pick.clone();
            choice((id, i), label.clone(), i == active, cx)
                .flex_1()
                .min_w_0()
                .with_size(size)
                .on_click(move |_, window, cx| pick(&i, window, cx))
        }))
        .into_any_element()
}
