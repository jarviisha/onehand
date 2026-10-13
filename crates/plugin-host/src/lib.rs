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
mod strip;
mod tabs;
pub mod toast;
pub mod tokens;
mod workbench;
pub use confirm::{Confirm, confirm_on};
pub use list_detail::{ListDetail, ListWidths, back_link, list_detail};
pub use menu::{menu_below, menu_command, menu_item, menu_row};
pub use onehand_core::worktree::removal::Process;
pub use strip::{strip, strip_control};
pub use tabs::{
    Measured, TAB_MAX_W, TabStrip, measure_width, tab_chip, tab_close, tab_menu_rows, tab_select,
    tab_strip, unmeasured,
};
pub use workbench::{Ask, Request, WorkbenchMode};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Div, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt as _,
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
/// **It also sets the theme's corner**, the one every box, row and field
/// takes: the library's buttons default to it, but a size can change it, and
/// every button passes through here and nowhere else.
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
        .rounded(ButtonRounded::Medium)
}

/// A text field at the theme's corner, on no fill of its own: the corner
/// [`action`] gives a button, so a field and the button beside it are one
/// shape, and the surface it sits on, so its edge alone marks it. The library
/// draws a field on a fill it derives from the control edge rather than from
/// where the field sits.
///
/// **Focus is the edge alone.** The library marks a focused field twice: its
/// edge turns the ring ink, and a ring is drawn round it, outside its bounds.
/// The field clips what lies outside itself, which takes the ring away and
/// leaves the edge. A field drawn with no edge at rest keeps one, in the
/// surface's own colour, so the edge still has somewhere to show.
///
/// Here rather than in the app for the reason [`action`] is: a plugin draws
/// fields too, and one wrapper is one place for the corner to hold.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state)
        .rounded(cx.theme().radius)
        .bg(cx.theme().transparent)
        .overflow_hidden()
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
/// **It takes the reason, and shows it as the tooltip**, so a refusal cannot
/// go without saying why: a control that only goes quiet leaves the person
/// to guess what it waits for. So the refusal is the reason being there, and
/// it is called after the control's own tooltip, which it replaces for as
/// long as it refuses.
///
/// Here rather than in the app for the reason [`action`] is: a built-in plugin
/// draws buttons that refuse while they work.
pub trait Refuses: Sized {
    /// Refuse presses while there is a reason to, say so in the cursor, and
    /// say why in the tooltip.
    fn refuses(self, why: Option<impl Into<SharedString>>) -> Self;
}

impl Refuses for Button {
    fn refuses(self, why: Option<impl Into<SharedString>>) -> Self {
        use gpui_component::Disableable as _;

        match why {
            Some(why) => resting(self).disabled(true).tooltip(why.into()),
            None => self,
        }
    }
}

/// A sunk box of machine text: full width, the muted fill, the theme's corner,
/// the mono family at [`tokens::CODE_TEXT`], clipped so a long line cannot push
/// the column wider. The caller sets the padding, which differs by what is in it.
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
        .text_size(tokens::CODE_TEXT)
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
/// Here rather than in the app for the reason [`action`] is: a plugin sets a
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

/// One mark's slot: an icon, a state's mark or a dot, centred in a column one
/// icon wide. Every mark down a column sits in one, so the marks share a
/// centre line whatever each is drawn at, and the labels after them start on
/// one edge.
///
/// A control may stand in one too, wider than the slot: it stays centred on
/// the column's line, its hit area spilling past the slot on both sides.
pub fn mark_slot(mark: impl IntoElement) -> Div {
    div()
        .w_4()
        .flex_none()
        .h_flex()
        .items_center()
        .justify_center()
        .child(mark)
}

/// A small label on a fill, the shape every badge takes: the library's `Tag`
/// at its small size, on [`tokens::BADGE_CORNER`], in the `tint`'s fill and ink.
pub fn badge(text: impl Into<SharedString>, tint: Tint) -> gpui_component::tag::Tag {
    gpui_component::tag::Tag::custom(tint.fill, tint.ink, tint.fill)
        .small()
        .rounded(crate::tokens::BADGE_CORNER)
        .child(text.into())
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
///
/// In the ink of the state it reports, one of [`status_ink`]'s, and on the
/// strip's inset, so its words start where the strip's above them do.
pub fn status_line(message: impl Into<SharedString>, ink: Hsla) -> AnyElement {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .text_color(ink)
        .child(message.into())
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
/// gives a field of one: the library draws a textarea on a fill of its own,
/// and beside a single-line field in the same
/// form the two read as different controls.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    Textarea::new(state)
        .rounded(cx.theme().radius)
        .bg(cx.theme().transparent)
        .overflow_hidden()
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

    /// The ghost of an icon with no words: the muted ink at rest, as an icon
    /// at rest is, and its fill's ink when hovered or `on`.
    ///
    /// **Its own call, because a bare icon is no label.** A word in a ghost
    /// reads at full ink; the same ink on a lone mark makes it the brightest
    /// thing on a chrome row. The library's hover takes the variant's ink, so
    /// the mark comes up under the pointer by itself, but it replays the
    /// caller's ink inside `selected`, which is why `on` is taken here and
    /// set with it: a muted mark would otherwise sit on the selected fill.
    /// A menu's trigger, which the library marks open on its own, keeps the
    /// muted mark while its menu shows, for that same replay.
    fn quiet_icon(self, on: bool, cx: &App) -> Self;
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

    fn quiet_icon(self, on: bool, cx: &App) -> Self {
        let rest = cx.theme().muted_foreground;
        self.quiet(cx)
            .selected(on)
            .when(!on, |button| button.text_color(rest))
    }
}

/// A control at the scale of a form, a filter bar, a page's head and a
/// dialog's footer: [`tokens::FIELD_H`] high, with its words and its icon at
/// the chrome size.
///
/// The library ties a control's words and icon to its size, and not the same
/// way in every component: a default-size button letters its label at the
/// reading size and grows its icon, a step above a field of the same size,
/// and nothing outside it reaches the box those words sit in. Its height and
/// padding are on the control itself, though, and the caller's own land after
/// the library's. So this is the small size, lettered as chrome and keeping the
/// small icon, given back the default size's height, and padding on the spacing
/// steps (`px_3`, where the default's `px_2p5` falls between two).
///
/// The one correction a size needs, made here and nowhere else: the two other
/// sizes the app uses, `small` and `xsmall`, already measure the same in a
/// button and a field, and the library's default is never used.
pub trait Tall {
    /// The small size at the height of a form, a filter bar, a page's head
    /// and a dialog's footer.
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
        // `Styled::h` by name: the field's own `h` sets a multi-line field's
        // height only, and a one-line field kept the small size's.
        gpui::Styled::h(self.small(), tokens::FIELD_H)
    }
}

/// The three sizes a control takes, each the role it serves: `.xsmall()` for
/// a secondary action inside dense content, `.small()` the compact default,
/// `.tall()` in a form, a filter bar, a page's head and a dialog's footer.
///
/// For a builder that sizes the controls it makes, so a caller names one of
/// the three: the library's own scale also holds its default size, which
/// letters a button at the reading size and is never used.
#[derive(Clone, Copy)]
pub enum ControlSize {
    XSmall,
    Small,
    Tall,
}

impl ControlSize {
    fn on(self, button: Button) -> Button {
        match self {
            Self::XSmall => button.xsmall(),
            Self::Small => button.small(),
            Self::Tall => button.tall(),
        }
    }
}

/// A picker's trigger: a button naming what it is set to, then the caret that
/// opens its menu, so it reads as a control. Unset, it names what it picks,
/// as a quiet ghost; `set` away from where it starts, it names the choice on
/// the secondary fill, so the picker narrowing a list is the one that stands
/// out. Its ink is the one its variant is paired with. `size` is the role's.
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
        false => button.quiet(cx),
    })
}

/// A list row's surface: at least [`tokens::ROW_H`] tall, inset `px_2`, at
/// the theme's corner, answering the pointer. The hover fills it with
/// `list_hover` and leaves its inks; the picked row stands on `accent` in
/// that fill's paired ink, a quieter ink on it taken through [`middle_ink`]
/// under that pair. The caller lays it out, a line or a line over a second,
/// and adds the ring where the keyboard is.
pub fn row(picked: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let hover = theme.list_hover;
    div()
        .min_h(tokens::ROW_H)
        .px_2()
        .rounded(theme.radius)
        .cursor_pointer()
        .map(|row| match picked {
            true => row.bg(theme.accent).text_color(theme.accent_foreground),
            false => row.hover(move |row| row.bg(hover)),
        })
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
/// built-in plugin both draw one, and a plugin cannot reach into the binary
/// hosting it.
///
/// `size` is the role's, so a switch sits level with the buttons and inputs
/// of that role beside it.
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
        .gap_1()
        .children(labels.iter().enumerate().map(|(i, label)| {
            let pick = pick.clone();
            choice((id, i), label.clone(), i == active, size, cx)
                .flex_1()
                .min_w_0()
                .on_click(move |_, window, cx| pick(&i, window, cx))
        }))
        .into_any_element()
}
