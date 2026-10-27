//! What every crate of Onehand draws with: a button that answers the
//! pointer, the derivation of status ink, the surface a dock is drawn on, the
//! shared sizes, and the pieces a page repeats.
//!
//! Shared rather than the app's own because the feature crates draw too and
//! cannot reach into the binary: one wrapper here is what keeps a button in
//! the Editor and a button in Settings the same button.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

mod choice_mark;
mod confirm;
mod list_detail;
mod menu;
mod page;
mod prose;
mod resize;
mod row_actions;
mod strip;
mod tabs;
pub mod toast;
mod toggle;
pub mod tokens;
pub use choice_mark::choice_mark;
pub use confirm::{Confirm, confirm_on};
pub use list_detail::{ListDetail, ListWidths, back_link, list_detail};
pub use menu::{
    menu_below, menu_command, menu_item, menu_label, menu_row, menu_still, split_action,
};
pub use onehand_core::worktree::removal::Process;
pub use page::{page_head, page_search, page_top};
pub use prose::{no_code_label, unedged_code};
pub use resize::{edge_handle, h_resizable, v_resizable};
pub use row_actions::{HoverSlot, hover_slot};
pub use strip::{mode_strip, strip, strip_control, workbench_strip};
pub use tabs::{
    Measured, TAB_MAX_W, TAB_MENU_CAP, TabStrip, TabWidths, measure_tab, measure_width, tab_chip,
    tab_close, tab_more, tab_select, tab_strip, tab_width, unmeasured,
};
pub use toggle::toggle;

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
/// **It lives here rather than in the app** because a feature crate draws
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

/// How much of the well's own fill a field's fill keeps.
const FIELD_FILL: f32 = 0.94;

/// A field's fill: the well, a step further toward the ink. The well alone sits
/// level with a floating panel's fill in the light palette, and a field on it
/// vanished; a step past it stands off the page, a floating panel and a well
/// alike, in both palettes by one rule.
fn field_fill(cx: &App) -> Hsla {
    cx.theme()
        .muted
        .mix_oklab(cx.theme().foreground, FIELD_FILL)
}

/// A text field at the theme's corner, on a fill with no edge: the corner
/// [`action`] gives a button, so a field and the button beside it are one
/// shape, and the fill alone marks it off the surface it sits on.
///
/// **Focus is the edge alone.** The library marks a focused field twice: its
/// edge turns the ring ink, and a ring is drawn round it, outside its bounds.
/// The field clips what lies outside itself, which takes the ring away and
/// leaves the edge. At rest the edge is drawn clear, so it still has
/// somewhere to show.
///
/// Here rather than in the app for the reason [`action`] is: a feature crate draws
/// fields too, and one wrapper is one place for the corner to hold.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state)
        .rounded(cx.theme().radius)
        .bg(field_fill(cx))
        .border_color(cx.theme().transparent)
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
/// Here rather than in the app for the reason [`action`] is: a feature crate
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

/// An icon with no words named twice: in its tooltip, for the pointer, and as
/// its accessible name, for a screen reader. The library names a button by its
/// label, and an icon alone has none, so a tooltip by itself left the button
/// announced as nothing. Every icon-only button says what it does through this,
/// never a bare `tooltip`.
///
/// A refusal still shows its reason in the tooltip ([`Refuses`]) and keeps the
/// name, which says what the button does rather than why it waits.
pub trait Named: Sized {
    /// The tooltip and the accessible name, one string.
    fn named(self, name: impl Into<SharedString>) -> Self;

    /// The same, with the key bound to `action` shown beside the tooltip.
    fn named_with_action(
        self,
        name: impl Into<SharedString>,
        action: &dyn gpui::Action,
        context: Option<&str>,
    ) -> Self;
}

impl Named for Button {
    fn named(self, name: impl Into<SharedString>) -> Self {
        let name = name.into();
        self.accessibility_label(name.clone()).tooltip(name)
    }

    fn named_with_action(
        self,
        name: impl Into<SharedString>,
        action: &dyn gpui::Action,
        context: Option<&str>,
    ) -> Self {
        let name = name.into();
        self.accessibility_label(name.clone())
            .tooltip_with_action(name, action, context)
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
/// Here rather than in the app for the reason [`action`] is: a feature crate's form
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
                .child(
                    div()
                        .text_size(crate::tokens::CHROME_TEXT)
                        .child(title.into()),
                )
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
/// beside the conversation rather than as a card inset in it. The rail stands
/// on the same step, so the app writes it into the sidebar slot read here,
/// with its contrast tested beside the rest of its ramp.
///
/// Here rather than in the app for the reason [`action`] is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn dock_surface(cx: &App) -> Hsla {
    cx.theme().sidebar
}

/// Status colours used as ink on the app's normal surfaces.
///
/// Here rather than in the app for the reason [`action`] is: a feature crate
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
/// Here rather than in the app for the reason [`action`] is: a feature crate sets a
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

/// An issue's label as a badge, in its [`label_tint`]. Every badge of a label
/// is drawn through this, so a label reads one way wherever it is.
pub fn label_badge(label: impl Into<SharedString>, cx: &App) -> gpui_component::tag::Tag {
    let label = label.into();
    let kind = label_tint(&label, cx);
    badge(label, kind).flex_none()
}

/// The tint of an issue's label by its kind: a label that asks for something
/// is blue, an enhancement green, one about the code's structure violet, one
/// about the documents yellow, an agent's report cyan; one that only files the
/// issue stays neutral. A badge takes the whole tint; a label written inside a
/// line of meta text takes only its ink, so both read the kind in one hue.
pub fn label_tint(label: &str, cx: &App) -> Tint {
    use onehand_core::issues::LabelKind;
    let theme = cx.theme();
    match LabelKind::of(label) {
        LabelKind::Category => Tint {
            fill: theme.secondary,
            ink: theme.secondary_foreground,
        },
        LabelKind::Action => tint(theme.blue, cx),
        LabelKind::Enhancement => tint(theme.green, cx),
        LabelKind::Structure => tint(theme.magenta, cx),
        LabelKind::Docs => tint(theme.yellow, cx),
        LabelKind::Report => tint(theme.cyan, cx),
    }
}

/// A small label on a fill, the shape every badge takes: the library's `Tag`
/// at its small size, fully round so it reads as a tag rather than as a button
/// beside it, in the `tint`'s fill and ink, with room above and below its
/// words (`py_1`) where the library leaves them a hair off its edge, and an
/// edge of its own ([`badge_edge`]). Its words are the small text, set solid,
/// rather than the library's smaller size for a small tag.
pub fn badge(text: impl Into<SharedString>, tint: Tint) -> gpui_component::tag::Tag {
    gpui_component::tag::Tag::custom(tint.fill, tint.ink, badge_edge(tint))
        .small()
        .text_xs()
        .line_height(gpui::relative(1.))
        .rounded_full()
        .py_1()
        .child(text.into())
}

/// A badge's edge: halfway from its fill to its ink, in its own hue, so the
/// badge stands out of the row it sits in rather than lying on it as a smudge
/// of colour, and reads the same on any surface under it.
pub fn badge_edge(tint: Tint) -> Hsla {
    tint.ink.mix_oklab(tint.fill, 0.5)
}

/// A mark the app picks from the kit's catalog for a feature crate, where the
/// default bundle has no drawing: an open issue's circle with a dot, and a
/// refresh. The app serves these, and its registry test holds each path to the
/// one it serves.
#[derive(Clone, Copy)]
pub enum AppIcon {
    CircleDot,
    Refresh,
}

impl gpui_component::IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::CircleDot => "icons/circle-dot.svg".into(),
            Self::Refresh => "icons/refresh-cw.svg".into(),
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
/// library's clear button, so every search in the app and its feature crates is one
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

/// A field of several lines, on the corner and the fill [`text_input`]
/// gives a field of one: the library draws a textarea on a fill of its own,
/// and beside a single-line field in the same form the two read as different
/// controls.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    Textarea::new(state)
        .rounded(cx.theme().radius)
        .bg(field_fill(cx))
        .border_color(cx.theme().transparent)
        .overflow_hidden()
}

/// A vertical scrollbar at the app's weight: every scrolling region draws its
/// thumb through this, so none of them is heavier than the next.
pub fn scrollbar<H: gpui_component::scroll::ScrollbarHandle + Clone>(
    handle: &H,
) -> gpui_component::scroll::Scrollbar {
    gpui_component::scroll::Scrollbar::vertical(handle).styles(|styles| {
        styles
            .thumb(|thumb| thumb.width(tokens::SCROLLBAR_THUMB))
            .thumb_hover(|thumb| thumb.width(tokens::SCROLLBAR_THUMB_ACTIVE))
            .thumb_active(|thumb| thumb.width(tokens::SCROLLBAR_THUMB_ACTIVE))
    })
}

/// A ghost button whose hover is the fill a selected one shows.
///
/// The library's ghost hovers on a fill it mixes from the bubble's grey at
/// four fifths opacity, which is no step of the app's ramp: beside a selected
/// ghost in the same strip, the hovered one came out a shade fainter, its edge
/// lost in the surface, and read as a smaller box. So the ghost is drawn here
/// as a custom variant on the ramp's own steps: no fill at rest, the
/// [`control_fill`] on hover, on press and when selected, so hovered and
/// selected are one shape. Every ghost button in the app and its feature crates is this, never a bare
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
                .hover(control_fill(cx))
                .active(control_fill(cx)),
        )
    }

    fn quiet_icon(self, on: bool, cx: &App) -> Self {
        let rest = cx.theme().muted_foreground;
        self.quiet(cx)
            .selected(on)
            .when(!on, |button| button.text_color(rest))
    }
}

/// A control a step over [`Short`]: [`tokens::FIELD_H`] high, its words and
/// its icon at the small size's, the reading size, given that height and
/// padding on the spacing steps. The library ties a control's words and icon
/// to its size and its default size letters a button at the reading size, so
/// this is the small size made taller rather than the default one.
pub trait Tall {
    /// The small size at [`tokens::FIELD_H`].
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

/// An icon alone at [`Tall`]'s height: square at [`tokens::FIELD_H`], with
/// no padding of its own.
///
/// Not [`Tall`] on an icon: the library sizes an icon alone by a box of its
/// own, and `Tall`'s padding, meant for words, taken inside that box left
/// the icon a few pixels wide.
pub trait TallIcon {
    /// Square at [`tokens::FIELD_H`].
    fn tall_icon(self) -> Self;
}

impl TallIcon for Button {
    fn tall_icon(self) -> Self {
        self.small().size(tokens::FIELD_H).px_0()
    }
}

impl Tall for Input {
    fn tall(self) -> Self {
        // `Styled::h` by name: the field's own `h` sets a multi-line field's
        // height only, and a one-line field kept the small size's.
        gpui::Styled::h(self.small(), tokens::FIELD_H)
    }
}

/// A control at a form's scale: a form's fields and buttons, a dialog's
/// footer, a page's filter bar. The small size's height, its words a step
/// down at the meta size, so they sit in the box with room above and below
/// rather than filling it edge to edge.
///
/// A button takes the extra small size for its words, the only way to reach
/// the box the library letters them in, and is given back the small height
/// and inset; its icon comes down with the words, and an icon alone stays
/// square. A field's text size is its own style, which lands after the
/// library's.
pub trait Short {
    /// The small size, lettered at the meta size.
    fn short(self) -> Self;
}

impl Short for Button {
    fn short(self) -> Self {
        // `min_w` rather than `w`, so an icon alone stays square and a label
        // keeps its own width.
        self.xsmall()
            .h(tokens::CONTROL_H)
            .min_w(tokens::CONTROL_H)
            .px_2()
    }
}

impl Short for Input {
    fn short(self) -> Self {
        self.small().text_xs()
    }
}

/// The sizes a builder's controls take, each the role it serves:
/// `.short()` the default, `.tall()` in a list page's head.
///
/// For a builder that sizes the controls it makes, so a caller names one of
/// these: the library's own scale also holds its default size, which
/// letters a button at the reading size and is never used.
#[derive(Clone, Copy)]
pub enum ControlSize {
    Short,
    Tall,
}

impl ControlSize {
    fn on(self, button: Button) -> Button {
        match self {
            Self::Short => button.short(),
            Self::Tall => button.tall(),
        }
    }
}

/// The box a page groups one thing's facts in: a card on the overview or
/// Tasks, a project tile, a task's row, an issue's conflict or review.
/// Padded `p_3`, its parts `gap_2` apart, edged in a control's edge: the
/// hairline is too near the page's fill in the dark palette to group
/// anything.
///
/// Here rather than in the app for the reason [`action`] is: the Issues page
/// draws the same box as the overview's.
pub fn card_box(cx: &App) -> Div {
    div()
        .v_flex()
        .gap_2()
        .min_w_0()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().input)
}

/// What opens a block held folded under it: its chevron first, then its
/// words, a button at a control's size, so the keyboard reaches it too.
/// The caller says what a press does, and refuses it where there is nothing
/// to open.
///
/// On the secondary fill rather than a ghost: it starts its own line at the
/// block's edge, where a ghost's padding would set its chevron in from the
/// text above it with nothing drawn to say where it begins.
pub fn fold_toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, open: bool) -> Button {
    action(id)
        .secondary()
        .short()
        .icon(Icon::new(match open {
            true => IconName::ChevronDown,
            false => IconName::ChevronRight,
        }))
        .label(label.into())
}

/// A tree's row, as Files and Markdown draw theirs: a list row at
/// [`tokens::TREE_ROW_H`] a pitch, its fill [`tokens::ROW_GUTTER`] off both of
/// the dock's side edges and [`tokens::TREE_ROW_ABOVE`] and
/// [`tokens::TREE_ROW_BELOW`] off the rows around it, so a hovered fill never
/// touches the picked one beside it; its content inset [`tokens::ROW_INSET`] and stepped in
/// [`tokens::TREE_INDENT`] per level, the picked row on the selected fill.
/// It stretches itself across the tree's column, less the gutter, rather than
/// taking a width: a full width and a margin would overrun the column.
/// The caller adds its mark in a [`mark_slot`], its name, and its marks in
/// one trailing slot.
pub fn tree_row(
    id: impl Into<ElementId>,
    depth: usize,
    picked: bool,
    cx: &App,
) -> gpui::Stateful<Div> {
    row(picked, cx)
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .self_stretch()
        .mx(tokens::ROW_GUTTER)
        .mt(tokens::TREE_ROW_ABOVE)
        .mb(tokens::TREE_ROW_BELOW)
        .min_h(tokens::TREE_ROW_FILL)
        .h(tokens::TREE_ROW_FILL)
        .text_size(crate::tokens::CHROME_TEXT)
        .pl(gpui::rems(
            tokens::ROW_INSET.0 - tokens::ROW_GUTTER.0 + depth as f32 * tokens::TREE_INDENT.0,
        ))
        .pr(gpui::rems(tokens::ROW_INSET.0 - tokens::ROW_GUTTER.0))
}

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
/// apart as boxes rather than running into one paragraph.
pub fn choice_track(choices: impl IntoIterator<Item = impl IntoElement>, cx: &App) -> Div {
    div()
        .h_flex()
        .flex_wrap()
        .w_full()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().input)
        .p_0p5()
        .gap_0p5()
        .children(choices)
}
