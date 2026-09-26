//! The app's own surface ramp, and the one semantic role derived from it.
//!
//! ## Why there is a palette here at all
//!
//! The component library's neutral palette is built for panels and forms, where
//! two or three surfaces is plenty. A transcript needs more of them at once: the
//! reading surface, a well sunk into it for machine output, a filled bubble for
//! what the user said, a raised card for the composer floating over all of it —
//! and every pair of those is adjacent on screen at the same moment. The
//! shipped ramp does not have that many distinct steps. In the dark palette it
//! collapses hardest: hover, well fill, bubble fill and hairline are one and
//! the same value, so a quoted command and the user's own message are drawn
//! identically and the composer paints exactly the surface it floats above.
//!
//! So the app names its own steps. Only the surfaces and the greys that sit on
//! them are ours; hues, status colours, selection, the scrollbar and everything
//! else keep the values the library shipped.
//!
//! ## Status colour is still not a surface
//!
//! `danger`, `warning`, `success` and `info` are *fills*, each with a paired
//! foreground meant for text placed on that fill. Using the fill itself as
//! coloured text happened to work in dark mode, where those fills are bright,
//! but produces low-contrast amber and green on a light background. That is a
//! property of what the tokens mean, not of which palette is loaded, so it is
//! repaired here rather than in the ramp — see [`status_ink`].

use gpui::{App, Hsla, SharedString};
use gpui_component::{ActiveTheme as _, Colorize as _, Theme, ThemeConfigColors, ThemeRegistry};
use std::rc::Rc;

/// Status ink is the plugin host's, because a built-in plugin needs it too and
/// cannot reach in here. Named through this module all the same: every call
/// site in the app already says `crate::theme::status_ink`, and that is the
/// name the guard against using a raw status fill as text points at.
pub(crate) use onehand_plugin_host::status_ink;

/// The surface a dock panel's card draws on is the plugin host's for the same
/// reason, and named through this module for the same one: the Neovim mode
/// hands it to a terminal grid as that grid's background, and a second copy of
/// the answer is a panel and the shell inside it disagreeing about what colour
/// the panel is.
///
/// It was called `chrome` while it was a step off the reading surface. It is
/// that surface now, so the word had come to name the opposite of what the
/// function returns -- and its own first line said so.
pub(crate) use onehand_plugin_host::dock_surface;

/// One mode's surfaces, and the ink that has to be legible on each.
///
/// Named by what the step is *for* rather than by the token it lands in: the
/// mapping onto token names happens once, in [`paint`], and the reason a value
/// is where it is belongs next to the value.
struct Ramp {
    /// The reading surface, and the prose on it.
    background: &'static str,
    foreground: &'static str,
    /// A well sunk into the surface: quoted commands, output, diffs, a folded
    /// thought. Every one of them is small text, so the ink is chosen against
    /// *this* rather than against the surface.
    well: &'static str,
    well_ink: &'static str,
    /// The user's own message. One step further from the surface than a well,
    /// because it is the block a reader scans back through a long conversation
    /// looking for.
    bubble: &'static str,
    bubble_ink: &'static str,
    /// Pointer feedback on a row or chip. Deliberately the faintest step there
    /// is: the pointer resting somewhere is not a state the control is *in*.
    hover: &'static str,
    /// The one item selected among several — a completion candidate, the open
    /// tab of a question card, a chip whose popup is showing.
    ///
    /// **This has to carry the state by itself**, with no ring beside it, so it
    /// is a real step rather than a wash: clearly stronger than `hover`, since
    /// a row can be both at once, and the reader has to see which of the two is
    /// telling them where Enter will land.
    selected: &'static str,
    selected_ink: &'static str,
    /// A filled row *on chrome*: the rail's selected project or session.
    ///
    /// Its own step because none of the others fits. The rail is drawn in the
    /// well, and against the well `hover` is 1.04 apart in the light palette —
    /// a fill nobody can see — while the reading surface is 1.19 in the dark
    /// one, which punches a near-black hole through a panel rather than lifting
    /// a row out of it. This sits between, and it lifts: lighter than the rail
    /// in the dark palette, lighter again in the light one, because a selected
    /// row reads as raised and a hole reads as damage.
    ///
    /// It can afford to be quiet where a *surface* could not. A region has only
    /// its fill to be found by; a row has ink at full strength and a weight the
    /// library sets from the same token pair, so the fill is the third thing
    /// saying it and not the only one.
    marked: &'static str,
    /// Every hairline and card border.
    hairline: &'static str,
    /// A control floating over the transcript: the composer, the completion
    /// popup, the jump-to-latest pill.
    floating: &'static str,
}

/// Light steps *down* from a white surface, which is the only direction there
/// is: nothing is lighter than the surface, so a floating control stays white
/// and is separated by its shadow instead.
///
/// The ink here is left at full strength on purpose. What glares in a light
/// palette is the *surface*, not the text on it, so dimming the text buys no
/// comfort and spends legibility to do it — the softening the dark ramp needed
/// would be a straight loss here.
const LIGHT: Ramp = Ramp {
    background: "#ffffff",
    foreground: "#0a0a0a",
    well: "#efefef",
    well_ink: "#636363",
    bubble: "#e0e0e0",
    bubble_ink: "#171717",
    hover: "#ebebeb",
    selected: "#d3d3d3",
    selected_ink: "#171717",
    // 1.12 against the well the rail is drawn in.
    marked: "#fcfcfc",
    hairline: "#dcdcdc",
    floating: "#ffffff",
};

/// Dark steps *up* from a near-black surface, for the same reason in reverse —
/// and here the floating step is not optional, because a drop shadow over
/// near-black is invisible and would leave a card divided from the conversation
/// by one hairline.
const DARK: Ramp = Ramp {
    background: "#0a0a0a",
    // **Not white.** On a near-black surface the ink is the bright thing in the
    // room, and near-white prose against it runs about 19:1 -- roughly four
    // times what a body of text needs and enough to leave an afterimage on a
    // long conversation read in a dark room. Stepped down to a soft grey it is
    // still comfortably past AAA against every surface it lands on, and the
    // ramp's *relative* steps are all unchanged: meta ink stays quieter than
    // prose, and prose stays quieter than the ink on a selected row.
    foreground: "#d6d6d6",
    well: "#1e1e1e",
    // Left where it was. It is already grey rather than white, so it is not
    // what glares -- and it is the ink with the least room to give: dimmed one
    // step further it fell under AA against the bubble fill, which the ramp's
    // own test caught.
    well_ink: "#a3a3a3",
    bubble: "#303030",
    // A step up from prose, because the bubble fill is a step up from the
    // surface: the same ink on both would make the user's own message the
    // dimmest text on screen.
    bubble_ink: "#e3e3e3",
    hover: "#232323",
    selected: "#3d3d3d",
    selected_ink: "#f0f0f0",
    // 1.12 against the well, the same step the light palette takes, and on the
    // same side of it: up from the rail rather than down toward the near-black
    // reading surface.
    marked: "#272727",
    hairline: "#333333",
    floating: "#1e1e1e",
};

/// Write one ramp into the token names the library actually reads.
fn paint(colors: &mut ThemeConfigColors, ramp: &Ramp) {
    fn set(slot: &mut Option<SharedString>, value: &'static str) {
        *slot = Some(value.into());
    }

    set(&mut colors.background, ramp.background);
    set(&mut colors.foreground, ramp.foreground);
    set(&mut colors.muted, ramp.well);
    set(&mut colors.muted_foreground, ramp.well_ink);
    set(&mut colors.secondary, ramp.bubble);
    set(&mut colors.secondary_foreground, ramp.bubble_ink);
    // The two states a *filled button* takes, which the library keeps as slots
    // of their own rather than deriving from the fill above. Left alone they
    // stayed on the shipped palette, and in the dark one that palette's hover
    // is *darker* than this ramp's bubble -- so the rail's one filled control
    // receded toward the well under the pointer, and its pressed step landed
    // 1.04 from the well, which is a hole rather than a button. The same
    // collapse the sidebar tokens above are written out to avoid, on a second
    // triple.
    //
    // **Both take the one step this ramp has above the bubble fill**, and that
    // is deliberate rather than a shortage: `selected` is already the answer to
    // "this control is the one being acted on", it moves the right way in both
    // palettes (lighter in the dark, darker in the light), and inventing a
    // third shade so that *held open* could differ from *pointed at* would be a
    // ramp step existing for one caret.
    //
    // Their being equal is also what keeps the open state visible at all. gpui
    // refines a hover style over the base, and `hover_style` is crate-private,
    // so a trigger that fills itself while its menu is open cannot outrank the
    // hover underneath it -- with two different shades the pressed one would
    // vanish for as long as the pointer stayed on the control that opened it.
    set(&mut colors.secondary_hover, ramp.selected);
    set(&mut colors.secondary_active, ramp.selected);
    // `accent` is the selected fill, not the hover one. That is the library's
    // own reading of it -- a list item falls back to `accent` for the selected
    // row whenever the highlight ring is off, which here it always is -- and it
    // leaves `list_hover` free to be the fainter of the two.
    set(&mut colors.accent, ramp.selected);
    set(&mut colors.accent_foreground, ramp.selected_ink);
    set(&mut colors.border, ramp.hairline);
    // The rail runs on the ramp too, but one notch quieter than the
    // conversation, because it is chrome rather than a second thing to read.
    //
    // Every one of these has to be written out. The library ships a whole
    // second set of sidebar tokens with values of their own rather than letting
    // them fall back on the ones above, so a ramp that stopped at `accent` left
    // the most-clicked panel in the window running on somebody else's palette:
    // its fill sat a notch off white in the light palette and dead level with
    // the surface in the dark one, and its ink and its selected row were
    // whatever that other palette happened to say.
    //
    // Written from the steps already named rather than added to the `Ramp`,
    // because none of them is a new step:
    //
    // - The rail's own fill is no longer this: it names the well at its call
    //   site, because this token is the reading surface and the library applies
    //   it before the caller's refinement, so left alone it would bring the
    //   panel up level with the conversation beside it. What this token still
    //   decides is the fallback for anything in the library that reads it
    //   without going through the rail.
    // - Its ink is the ramp's quiet ink. A rail row is a name to aim at, not a
    //   sentence to read, and at prose strength a column of thirty of them
    //   out-shouted the conversation they exist to get you to.
    // - A filled row takes `marked`, which is the one step here that is the
    //   rail's own. It was `hover`, chosen while the rail sat on the reading
    //   surface and 1.04 from the well once the rail moved onto it; then the
    //   reading surface, which reads at 1.19 in the dark palette and punches a
    //   near-black hole through the panel rather than lifting a row out of it.
    //   `marked` is 1.12 either way and lifts in both. The library draws a
    //   hovered row at 0.8 of this token and a selected one at full, so the two
    //   stay apart without a second token.
    // - The guide line down an expanded project is the same hairline as any
    //   other.
    //
    // Naming any of it again with values of its own would be inventing a second
    // ramp for one panel and having to keep the two in step by hand.
    set(&mut colors.sidebar, ramp.background);
    set(&mut colors.sidebar_foreground, ramp.well_ink);
    set(&mut colors.sidebar_accent, ramp.marked);
    set(&mut colors.sidebar_accent_foreground, ramp.selected_ink);
    set(&mut colors.sidebar_border, ramp.hairline);
    set(&mut colors.popover, ramp.floating);
    set(&mut colors.popover_foreground, ramp.foreground);
    // Left to its own devices this one is derived as a fraction of the selected
    // fill, which lands close enough to the surface to read as nothing.
    set(&mut colors.list_hover, ramp.hover);
}

/// Replace the two configs the mode switch chooses between with ours.
///
/// **Built on the library's own configs, not on an empty one.** A key a config
/// leaves unset does not fall back to the shipped palette — it falls back to a
/// value *computed* from whatever base colours are in force. Starting from an
/// empty config would therefore recolour about eighty things nobody asked to
/// change, including the scrollbar thumb, the text selection, the tab bar and
/// the focus ring, each of them silently and none of them here. Cloning the
/// shipped config and overwriting a dozen fields keeps every other value
/// exactly as it arrived.
///
/// Runs at boot, before a mode is chosen, because choosing one applies whichever
/// of these two configs the mode names.
pub(crate) fn install(cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let mut light = (**registry.default_light_theme()).clone();
    let mut dark = (**registry.default_dark_theme()).clone();

    light.name = "onehand Light".into();
    dark.name = "onehand Dark".into();
    paint(&mut light.colors, &LIGHT);
    paint(&mut dark.colors, &DARK);

    let theme = Theme::global_mut(cx);
    theme.light_theme = Rc::new(light);
    theme.dark_theme = Rc::new(dark);
    // No ring around a selected row, anywhere — including inside the library's
    // own lists and tables, which draw one by default. A selection here is a
    // fill and only a fill, and the ramp gives that fill enough of a step to
    // say so on its own. Left on, this setting would also swap the fill for a
    // wash the library clamps to a fifth of its opacity, so the two disagree
    // about what a selection looks like *and* the survivor is the fainter one.
    theme.list.active_highlight = false;
}

/// A base hue tempered for use as ink where it is already legible.
///
/// **A different problem from status ink, so a different target.** A status
/// colour is pulled toward the *foreground* because it arrives as a fill and
/// has to be made bright or dark enough to read. The hue that marks inline code
/// has no such trouble — the shipped dark blue already clears AA comfortably on
/// every surface here. What it does is **glare**: a 94%-saturated blue beside
/// neutral grey prose on a near-black surface vibrates, and the eye is pulled
/// to it over the sentence it belongs to. Pulling it toward the *meta ink*
/// instead — a neutral of about its own lightness — takes a third of the
/// saturation out and leaves the contrast where it was, which is the axis the
/// complaint is actually about.
///
/// Derived rather than named, so the light palette gets the same treatment of
/// its own darker hue and neither has to be tuned by hand.
pub(crate) fn hue_ink(base: Hsla, cx: &App) -> Hsla {
    temper(base, cx.theme().muted_foreground)
}

fn temper(base: Hsla, neutral: Hsla) -> Hsla {
    base.mix_oklab(neutral, 0.70)
}

/// The step between prose and meta ink.
///
/// **Why two steps are not enough in one place.** A completion row stacks three
/// kinds of text and they are not equal: the name, which is what the row is;
/// the detail lying on the same line beside it — a folder, a description, a
/// type icon — which is context for that name; and the label naming the run of
/// rows the whole thing sits in.
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
/// Derived rather than named as a ramp step, for the reason [`hue_ink`] is
/// derived: both palettes get it from values they already hold, so there is no
/// second ramp to keep in step by hand and no library token borrowed for a
/// meaning it does not have.
pub(crate) fn meta_ink(cx: &App) -> Hsla {
    between(cx.theme().foreground, cx.theme().muted_foreground)
}

fn between(prose: Hsla, meta: Hsla) -> Hsla {
    prose.mix_oklab(meta, 0.5)
}

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
/// lands on the surface underneath — which is what
/// `the_lift_can_be_seen_on_both_palettes` measures.
pub(crate) fn lift(cx: &App) -> Vec<gpui::BoxShadow> {
    let alpha = lift_alpha(cx.theme().mode.is_dark());
    let shadow = |y: f32, blur: f32, spread: f32, alpha: f32| gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., alpha),
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

fn lift_alpha(dark: bool) -> f32 {
    match dark {
        true => 0.6,
        false => 0.14,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::{ThemeConfig, ThemeMode};

    /// The ratio small text has to clear to be readable.
    const AA: f32 = 4.5;

    /// The smallest ratio at which a fill still reads as a region of its own
    /// rather than as an artefact of the display.
    const STEP: f32 = 1.14;

    /// The same question for a *row* rather than a region, which is a lower
    /// floor on purpose.
    ///
    /// A region has only its fill to be found by. A marked row has ink at full
    /// strength and a weight to go with it, so the fill is the third thing
    /// saying which row it is and not the only one — and it is read at a glance
    /// down a column of thirty, where the loud answer is worse than the quiet
    /// one. Below this it stops being a fill at all: `hover` against the well is
    /// 1.04, which is what the rail drew for a while and nobody could see.
    const ROW: f32 = 1.10;

    /// Relative luminance, as the contrast ratio defines it.
    fn luminance(color: Hsla) -> f32 {
        let rgba = gpui::Rgba::from(color);
        let channel = |c: f32| {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.r) + 0.7152 * channel(rgba.g) + 0.0722 * channel(rgba.b)
    }

    /// How far apart two opaque colours are, from 1.0 (identical) to 21.0.
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// A ramp resolved the way the running app resolves it.
    ///
    /// Every colour asserted on below is one the ramp sets outright, so starting
    /// the resolution from an empty config — rather than from the library's,
    /// which [`install`] uses and which cannot be reached without an `App` —
    /// arrives at the same value for all of them.
    fn resolve(ramp: &Ramp, mode: ThemeMode) -> Theme {
        let mut colors = ThemeConfigColors::default();
        paint(&mut colors, ramp);
        let mut theme = Theme::default();
        theme.apply_config(&Rc::new(ThemeConfig {
            mode,
            colors,
            ..ThemeConfig::default()
        }));
        theme
    }

    /// Every pair of surfaces that is adjacent on screen, and every ink against
    /// the surface it is actually drawn on.
    ///
    /// `floating_floor` differs by mode on purpose: a light surface cannot be
    /// raised above white, so there the drop shadow carries the elevation and
    /// the fill is allowed to match. On dark that shadow is invisible, so the
    /// fill has to be a real step.
    fn assert_ramp(name: &str, theme: &Theme, floating_floor: f32) {
        let check = |label: &str, a: Hsla, b: Hsla, floor: f32| {
            let ratio = contrast(a, b);
            assert!(
                ratio >= floor,
                "{name}: {label} is {ratio:.2}, under {floor}"
            );
        };

        // Surfaces, against whatever sits next to them.
        check(
            "the well against the surface",
            theme.muted,
            theme.background,
            STEP,
        );
        check(
            "the bubble against the well",
            theme.secondary,
            theme.muted,
            STEP,
        );
        check(
            "the bubble against the surface",
            theme.secondary,
            theme.background,
            STEP,
        );
        check(
            "hover against the surface",
            theme.list_hover,
            theme.background,
            STEP,
        );
        check(
            "the hairline against the surface",
            theme.border,
            theme.background,
            STEP,
        );
        // The rail is drawn in the well, so its marked row is measured against
        // *that* and not against the reading surface. Both of these have been
        // wrong: the fill was invisible when it was `hover`, and shouted when it
        // was the reading surface.
        check(
            "a marked row against the rail it sits in",
            theme.sidebar_accent,
            theme.muted,
            ROW,
        );
        check(
            "the ink on a marked row",
            theme.sidebar_accent_foreground,
            theme.sidebar_accent,
            AA,
        );
        check(
            "a floating control against the surface",
            theme.popover,
            theme.background,
            floating_floor,
        );

        // Nothing rings a selected item, so its fill answers for it alone: on
        // the reading surface, on a floating card, and far enough past hover
        // that a row which is both does not read as merely hovered.
        check(
            "the selected fill against the surface",
            theme.accent,
            theme.background,
            STEP,
        );
        check(
            "the selected fill on a floating card",
            theme.accent,
            theme.popover,
            STEP,
        );
        check(
            "the selected fill against hover",
            theme.accent,
            theme.list_hover,
            STEP,
        );
        check(
            "ink on the selected fill",
            theme.accent_foreground,
            theme.accent,
            AA,
        );

        // Ink, against every surface it lands on.
        check(
            "meta ink on the surface",
            theme.muted_foreground,
            theme.background,
            AA,
        );
        check(
            "meta ink in a well",
            theme.muted_foreground,
            theme.muted,
            AA,
        );
        check(
            "meta ink on a bubble",
            theme.muted_foreground,
            theme.secondary,
            AA,
        );
        check("prose in a well", theme.foreground, theme.muted, AA);
        check(
            "prose on a bubble",
            theme.secondary_foreground,
            theme.secondary,
            AA,
        );
        check(
            "prose on a floating control",
            theme.popover_foreground,
            theme.popover,
            AA,
        );

        // Quiet has to stay quieter than loud, or the ramp says nothing.
        assert!(
            contrast(theme.muted_foreground, theme.background)
                < contrast(theme.foreground, theme.background),
            "{name}: meta ink is as loud as prose"
        );
    }

    #[test]
    fn the_light_ramp_holds_every_step_and_every_ink() {
        assert_ramp("light", &resolve(&LIGHT, ThemeMode::Light), 1.0);
    }

    #[test]
    fn the_dark_ramp_holds_every_step_and_every_ink() {
        assert_ramp("dark", &resolve(&DARK, ThemeMode::Dark), STEP);
    }

    /// The rail's tokens have to be *written*, not left to fall back.
    ///
    /// This is the one family where an unset slot is not a fallback at all. The
    /// configs the ramp is written over carry a whole second palette for the
    /// sidebar — a fill, an ink, a selected fill and its ink, a hairline — so a
    /// slot left alone keeps the value that arrived rather than deriving one
    /// from the steps above it. That is how the most-clicked panel in the
    /// window came to run on a palette nobody here chose: its fill a notch off
    /// the reading surface in one mode and level with it in the other, its ink
    /// and its selected row whatever that other palette happened to say.
    #[test]
    fn the_rail_runs_on_the_ramp() {
        for (name, ramp) in [("light", &LIGHT), ("dark", &DARK)] {
            let mut colors = ThemeConfigColors::default();
            paint(&mut colors, ramp);
            for (token, slot) in [
                ("the fill", &colors.sidebar),
                ("the ink", &colors.sidebar_foreground),
                ("the selected fill", &colors.sidebar_accent),
                (
                    "ink on the selected fill",
                    &colors.sidebar_accent_foreground,
                ),
                ("the hairline", &colors.sidebar_border),
            ] {
                assert!(
                    slot.is_some(),
                    "{name}: {token} in the rail is left to whatever config the ramp is written over"
                );
            }
        }
    }

    /// A filled button runs on the ramp in all three of its states.
    ///
    /// The fill alone is not enough, and that is the whole finding this
    /// records: `secondary_hover` and `secondary_active` are slots of their
    /// own, so a ramp that wrote only `secondary` left the app's one filled
    /// control taking the shipped palette's other two -- whose dark hover is
    /// *darker* than this ramp's bubble, so the button receded under the
    /// pointer instead of lifting.
    ///
    /// Both halves are checked: that the steps are ours at all, and that each
    /// moves away from the fill rather than toward the well behind it.
    #[test]
    fn a_filled_button_runs_on_the_ramp_in_every_state() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let mut colors = ThemeConfigColors::default();
            paint(&mut colors, ramp);
            for (token, slot) in [
                ("the fill", &colors.secondary),
                ("the ink on it", &colors.secondary_foreground),
                ("the hover step", &colors.secondary_hover),
                ("the pressed step", &colors.secondary_active),
            ] {
                assert!(
                    slot.is_some(),
                    "{name}: {token} of a filled button is left to whatever config the ramp is written over"
                );
            }

            let theme = resolve(ramp, mode);
            // Away from the well, not toward it. A hover that closes on the
            // surface behind the control is the shipped palette's failure, and
            // it reads as the button going away when it is pointed at.
            for (label, state) in [
                ("hovered", theme.secondary_hover),
                ("held open", theme.secondary_active),
            ] {
                assert!(
                    contrast(state, theme.muted) > contrast(theme.secondary, theme.muted),
                    "{name}: a {label} filled button sits closer to the well than its own resting fill"
                );
            }
        }
    }

    /// The rail is quieter than the conversation, and the marked row is still
    /// the loud thing in it.
    ///
    /// Both halves matter. The rail's ink is deliberately below prose strength
    /// — a column of names to aim at, not a body of text — and the fill under a
    /// marked row is the faintest step there is, so what says *which* row is
    /// selected is the ink and the weight on it rather than a slab of colour.
    /// That only works while that ink is legible on that fill and clearly
    /// louder than the rows around it, neither of which the pairing gets for
    /// free: it is the one place in the app where the selected ink lands on the
    /// hover step.
    #[test]
    fn the_rail_is_quiet_and_its_marked_row_is_not() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);

            let ratio = contrast(theme.sidebar_accent_foreground, theme.sidebar_accent);
            assert!(
                ratio >= AA,
                "{name}: ink on the rail's marked row is {ratio:.2}, under {AA}"
            );
            assert!(
                contrast(theme.sidebar_foreground, theme.sidebar)
                    < contrast(theme.foreground, theme.background),
                "{name}: the rail's ink is as loud as the conversation's prose"
            );
            assert!(
                contrast(theme.sidebar_accent_foreground, theme.sidebar)
                    > contrast(theme.sidebar_foreground, theme.sidebar),
                "{name}: the rail's marked row is no louder than the rows around it"
            );
        }
    }

    /// The collapse that made the app own a palette in the first place: the
    /// shipped dark values put hover, well, bubble and hairline on one colour,
    /// so a quoted command and the user's own message were the same block.
    #[test]
    fn the_dark_ramp_keeps_its_surfaces_apart() {
        let theme = resolve(&DARK, ThemeMode::Dark);
        let surfaces = [
            ("the well", theme.muted),
            ("the bubble", theme.secondary),
            ("hover", theme.list_hover),
            ("the selected fill", theme.accent),
            ("the hairline", theme.border),
        ];
        for (i, (a_name, a)) in surfaces.iter().enumerate() {
            for (b_name, b) in surfaces.iter().skip(i + 1) {
                assert_ne!(a, b, "dark: {a_name} and {b_name} are the same colour");
            }
        }
    }

    /// Status words and marks are small, and most of them land inside a well —
    /// a tool's status word, a diff's added and removed lines, an exit code.
    ///
    /// The bubble is checked for `danger` alone because that is the only status
    /// drawn there: an attachment the agent never received is marked inside the
    /// user's own message. Asserting the other two against a surface they are
    /// never drawn on would be inventing a requirement.
    #[test]
    fn status_ink_is_readable_on_every_surface_it_lands_on() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (role, base) in [
                ("danger", theme.red),
                ("warning", theme.yellow),
                ("success", theme.green),
            ] {
                let ink = onehand_plugin_host::status_hue(base, theme.foreground);
                let mut on = vec![("surface", theme.background), ("well", theme.muted)];
                if role == "danger" {
                    on.push(("bubble", theme.secondary));
                }
                for (surface, fill) in on {
                    let ratio = contrast(ink, fill);
                    assert!(
                        ratio >= AA,
                        "{name}: {role} ink on the {surface} is {ratio:.2}, under {AA}"
                    );
                }
            }
        }
    }

    /// The middle ink step has to read, and has to be *in the middle*.
    ///
    /// Both halves are the whole of what it is for. A completion row spends the
    /// two ends of the ramp on saying which characters matched a query, and
    /// this is the third value the description beside that name is drawn in —
    /// so a step landing on either end would make the description
    /// indistinguishable from one half of the name, which is the state it
    /// exists to prevent. And it is small text on a floating card, so it owes
    /// the same legibility every other ink here does.
    #[test]
    fn the_middle_ink_reads_and_stays_between_the_two_it_divides() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let middle = between(theme.foreground, theme.muted_foreground);

            let ratio = contrast(middle, theme.popover);
            assert!(
                ratio >= AA,
                "{name}: the middle ink on a floating card is {ratio:.2}, under {AA}"
            );

            let (prose, meta) = (
                contrast(theme.foreground, theme.popover),
                contrast(theme.muted_foreground, theme.popover),
            );
            assert!(
                ratio < prose && ratio > meta,
                "{name}: the middle ink is not between prose ({prose:.2}) and meta ({meta:.2}), it is {ratio:.2}"
            );
        }
    }

    /// A selected row drawn at partial opacity still has to be a selected row.
    ///
    /// **The one fill in the completion popup that carries meaning** — it says
    /// where `Enter` will land, with no ring, no bar and no mark beside it. It
    /// is drawn under 1.0 alpha because at full strength it read as a text
    /// field holding the caret rather than as a row picked out of a list, and
    /// thinning it is what takes that weight off.
    ///
    /// What that must not do is walk it into the hover step. A row can be
    /// hovered and selected at once, and the reader has to be able to see which
    /// of the two fills is telling them what the keyboard is pointing at. So
    /// the comparison here is against the *composited* colour rather than
    /// against `accent`, because the token the fill came from is no longer the
    /// colour on the glass.
    ///
    /// **The light palette is what this is really guarding.** It has about 1.15
    /// between white and the well to divide among every step, so its selected
    /// and hover fills start far closer together than the dark palette's, and
    /// thinning the selected one closes that gap several times faster. A number
    /// chosen by eye in dark mode is a selection nobody can find in light mode.
    #[test]
    fn the_selection_stays_clear_of_hover() {
        /// What the compositor puts on the glass for `fill` over `under`.
        fn over(fill: Hsla, alpha: f32, under: Hsla) -> Hsla {
            let (fill, under) = (gpui::Rgba::from(fill), gpui::Rgba::from(under));
            let mix = |a: f32, b: f32| b + (a - b) * alpha;
            gpui::Rgba {
                r: mix(fill.r, under.r),
                g: mix(fill.g, under.g),
                b: mix(fill.b, under.b),
                a: 1.,
            }
            .into()
        }

        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let drawn = over(
                theme.accent,
                crate::chat::composer::SELECTED_ALPHA,
                theme.popover,
            );

            let apart = contrast(drawn, theme.list_hover);
            assert!(
                apart >= ROW,
                "{name}: the drawn selection is {apart:.2} from hover, which is not a difference"
            );
            assert!(
                contrast(drawn, theme.popover) >= ROW,
                "{name}: the drawn selection does not lift off the card it is on"
            );
            assert!(
                contrast(theme.accent_foreground, drawn) >= AA,
                "{name}: the ink on the drawn selection is not readable"
            );
        }
    }

    /// A shadow that cannot be seen is not a cue, it is a line of code.
    ///
    /// This is the check the component library's own ladder fails here, and it
    /// failed silently: `shadow_xl` is black at a tenth, which against the dark
    /// palette's floating surface moves it by three parts in 255. The popup
    /// drawn over a parked card had a shadow the whole time and read as one
    /// tall panel.
    ///
    /// Measured against the surface the shadow actually falls on — another
    /// floating card, since that is the case it exists for — and in both
    /// palettes, because the alpha differs between them by more than four times
    /// and a number tuned by eye in one mode is invisible in the other.
    #[test]
    fn the_lift_can_be_seen_on_both_palettes() {
        fn under(surface: Hsla, alpha: f32) -> Hsla {
            let s = gpui::Rgba::from(surface);
            gpui::Rgba {
                r: s.r * (1. - alpha),
                g: s.g * (1. - alpha),
                b: s.b * (1. - alpha),
                a: 1.,
            }
            .into()
        }

        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let alpha = lift_alpha(mode == ThemeMode::Dark);
            let ratio = contrast(under(theme.popover, alpha), theme.popover);
            assert!(
                ratio >= ROW,
                "{name}: the lift is {ratio:.2} against the card it falls on, which is not a shadow"
            );

            // The ladder this replaced, at the value it would have used, so the
            // reason for replacing it is on the record rather than in a commit
            // message.
            let shipped = contrast(under(theme.popover, 0.1), theme.popover);
            assert!(
                mode == ThemeMode::Light || shipped < ROW,
                "dark: the component ladder's shadow is visible after all, so this is not needed"
            );
        }
    }

    /// Tempering a hue for ink has to cost saturation and *not* contrast.
    ///
    /// That is the whole point of pulling it toward the meta ink rather than
    /// toward the foreground: the complaint it answers is glare, and a fix that
    /// bought calm by making the mark harder to read would be trading the wrong
    /// thing away.
    #[test]
    fn a_tempered_hue_loses_saturation_and_keeps_its_contrast() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let ink = temper(theme.blue, theme.muted_foreground);

            assert!(
                ink.s < theme.blue.s * 0.85,
                "{name}: tempering took almost no saturation out ({:.2} from {:.2})",
                ink.s,
                theme.blue.s
            );
            for (surface, fill) in [("surface", theme.background), ("well", theme.muted)] {
                let (before, after) = (contrast(theme.blue, fill), contrast(ink, fill));
                assert!(
                    after >= AA,
                    "{name}: tempered ink on the {surface} is {after:.2}, under {AA}"
                );
                assert!(
                    after >= before * 0.95,
                    "{name}: tempering cost contrast on the {surface} ({before:.2} to {after:.2})"
                );
            }
        }
    }
}
