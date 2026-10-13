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
//! So the app names its own steps, and its own hues with them: neutral greys
//! carry everything, and a hue only ever means state. Where two roles must be
//! told apart (a bubble apart from a well, selected apart from hover, a marked
//! rail row), the step is the nearest grey that keeps the tests below.
//! Every token not written here keeps the value the library shipped.
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

/// The surface a dock draws on is the plugin host's for the same reason, and
/// named through this module for the same one: the Neovim mode hands it to a
/// terminal grid as that grid's background, and a second copy of the answer is
/// a panel and the shell inside it disagreeing about what colour the panel is.
pub(crate) use onehand_plugin_host::dock_surface;

/// The middle ink is the plugin host's too, since a plugin sets a detail beside
/// a name as well, and named through this module like the two above.
pub(crate) use onehand_plugin_host::meta_ink;

/// One mode's surfaces, and the ink that has to be legible on each.
///
/// Named by what the step is *for* rather than by the token it lands in: the
/// mapping onto token names happens once, in [`paint`], and the reason a value
/// is where it is belongs next to the value.
struct Ramp {
    /// The reading surface, and the prose on it.
    background: &'static str,
    foreground: &'static str,
    /// The docks: the Workbench and the terminal, one step off the reading
    /// surface, so each reads as a surface of its own beside the conversation
    /// rather than as a card inset in it.
    panel: &'static str,
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
    /// a row out of it. This sits between: the hover tint laid over the rail,
    /// so in the dark palette it lifts toward the light rather than sinking
    /// toward the near-black surface.
    ///
    /// It can afford to be quiet where a *surface* could not. A region has only
    /// its fill to be found by; a chosen row has its ink at full strength among
    /// rows in the muted one, so the fill is the second thing saying it and not
    /// the only one.
    marked: &'static str,
    /// Every hairline and card border.
    hairline: &'static str,
    /// A control floating over the transcript: the completion popup, the
    /// cards pinned above the composer, the jump-to-latest pill.
    floating: &'static str,
    /// The composer card, which draws no edge: its fill and its own lift are
    /// all that stand it off the page and off the wells beside it. White in the
    /// light palette, where the lift carries it; in the dark one a clear step
    /// above the well, because a shadow barely shows on near-black.
    raised: &'static str,
    /// A field with no edge sunk into the well: the rail's search, and the
    /// rail's badges. It has its fill alone to be found by, so it steps away
    /// from the well the other way from a row lifting off it. White in the
    /// light palette, where the panel step sat too close to the well to see; in
    /// the dark one the panel step, because the reading surface cut a hole the
    /// eye went to first.
    inset: &'static str,
    /// A control's edge, and the scrollbar thumb.
    control: &'static str,
    /// The border marking where the keyboard is.
    ring: &'static str,
    /// Running and links; the text selection is this, thinned.
    accent: &'static str,
    /// The base hues status ink is derived from.
    warning: &'static str,
    success: &'static str,
    danger: &'static str,
    /// Danger as a fill, with white on it: the same red in both modes, because
    /// the lighter red of the dark palette is only ever ink.
    danger_fill: &'static str,
    /// The one primary action in a view: ink on light, light on dark.
    primary: &'static str,
    primary_ink: &'static str,
}

/// Light steps *down* from a white surface, which is the only direction there
/// is: nothing is lighter than the surface, so a floating control stays white
/// and is separated by its shadow instead.
const LIGHT: Ramp = Ramp {
    background: "#ffffff",
    foreground: "#1f1f1f",
    panel: "#f3f3f3",
    well: "#eeeeee",
    // A secondary ink rather than a metadata grey: this one is drawn as small
    // text on every surface, and a lighter grey falls under AA on white.
    well_ink: "#616161",
    // A step past the well, so the user's own message is the block a reader
    // finds scanning back through a long conversation.
    bubble: "#dedede",
    bubble_ink: "#1f1f1f",
    // Dark enough that a hovered row shows on a dock as well as on the surface:
    // a lighter tint vanished on the panel.
    hover: "#e8e8e8",
    // Far enough past hover that a row both hovered and selected still reads
    // as selected.
    selected: "#d0d0d0",
    selected_ink: "#1f1f1f",
    // Its own step, darker than `hover`, so a marked rail row shows on the
    // well the rail is drawn in.
    marked: "#dedede",
    hairline: "#d4d4d4",
    floating: "#ffffff",
    raised: "#ffffff",
    inset: "#ffffff",
    control: "#c7c7c7",
    ring: "#888888",
    // Darker than the usual editor blue, which inline code tempered toward
    // the meta ink drew under AA in a well.
    accent: "#006eb8",
    warning: "#854f0b",
    success: "#16825d",
    danger: "#a32d2d",
    danger_fill: "#a32d2d",
    primary: "#1f1f1f",
    primary_ink: "#ffffff",
};

/// Dark steps *up* from a near-black surface, for the same reason in reverse —
/// and here the floating step is not optional, because a drop shadow over
/// near-black is invisible and would leave a card divided from the conversation
/// by one hairline.
const DARK: Ramp = Ramp {
    background: "#181818",
    foreground: "#ececec",
    panel: "#202020",
    well: "#282828",
    // Light enough to clear AA on the bubble as well as the well, which the
    // ramp's own test checks.
    well_ink: "#a0a0a0",
    bubble: "#333333",
    bubble_ink: "#ececec",
    hover: "#2a2a2a",
    selected: "#3e3e3e",
    selected_ink: "#ececec",
    // Past the light palette's step: at the same 1.12 from the well a hovered
    // rail row did not show on a dark screen at all.
    marked: "#404040",
    hairline: "#404040",
    // The well's grey: `panel` sits too close to the surface for a
    // card that has no visible shadow to stand on.
    floating: "#282828",
    raised: "#303030",
    inset: "#202020",
    control: "#404040",
    ring: "#767676",
    // Blue in both modes, lifted to read on near-black: running and waiting
    // must never be told apart by a shade, and a yellow sat beside the amber.
    accent: "#3794ff",
    warning: "#fac775",
    success: "#1fbd53",
    danger: "#f09595",
    danger_fill: "#a32d2d",
    primary: "#ececec",
    primary_ink: "#181818",
};

/// Write one ramp into the token names the library actually reads.
fn paint(colors: &mut ThemeConfigColors, ramp: &Ramp) {
    fn set(slot: &mut Option<SharedString>, value: &'static str) {
        *slot = Some(value.into());
    }

    set(&mut colors.background, ramp.background);
    set(&mut colors.foreground, ramp.foreground);
    // The docks' surface rides in the slot the library gives a dock of tiles,
    // which the app never draws: a built-in plugin hands it to a terminal grid
    // and can only reach it through the theme.
    set(&mut colors.tiles, ramp.panel);
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
    // A secondary button is filled at the selected step, the fill an open
    // tab or a taken choice shows, rather than the bubble the library falls
    // back to: one filled grey for a control, wherever a control is filled.
    set(&mut colors.button_secondary, ramp.selected);
    set(&mut colors.button_secondary_foreground, ramp.selected_ink);
    set(&mut colors.accent_foreground, ramp.selected_ink);
    set(&mut colors.border, ramp.hairline);
    // A switch that is off is a track and a thumb, and both have to be seen
    // for the control to say anything: the library draws the track in
    // `secondary_active` — the selected step above, a fill meant to be quiet —
    // and the thumb in the surface, so an off switch was a faint smudge with an
    // invisible knob. A control's parts are held to 3:1 against what they sit
    // on, and the well's ink is already the ramp's answer to "readable on the
    // surface": as the track, the surface-coloured thumb stands off it too.
    set(&mut colors.switch, ramp.well_ink);
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
    //   `marked` lifts in both, and is what a hovered rail row takes whole; the
    //   chosen row is that step pulled toward its ink ([`rail_chosen`]).
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

    set(&mut colors.input, ramp.control);
    set(&mut colors.scrollbar_thumb, ramp.control);
    set(&mut colors.ring, ramp.ring);
    set(&mut colors.caret, ramp.foreground);
    set(&mut colors.link, ramp.accent);
    set(&mut colors.info, ramp.accent);
    set(&mut colors.success, ramp.success);
    set(&mut colors.warning, ramp.warning);
    set(&mut colors.primary, ramp.primary);
    set(&mut colors.primary_foreground, ramp.primary_ink);
    set(&mut colors.button_primary, ramp.primary);
    set(&mut colors.button_primary_foreground, ramp.primary_ink);
    // Danger is solid red with white on it in both modes; the lighter red is
    // only ever ink, through the base hue below.
    for slot in [&mut colors.danger, &mut colors.button_danger] {
        set(slot, ramp.danger_fill);
    }
    for slot in [
        &mut colors.danger_foreground,
        &mut colors.button_danger_foreground,
    ] {
        set(slot, "#ffffff");
    }
    // The text selection: the accent, thinned so the text under it still reads.
    colors.selection = Some(format!("{}40", ramp.accent).into());

    // The base hues status ink and inline code are derived from. The library
    // keeps these slots private, so they go in through the config's serde form,
    // under the names a theme file would give them.
    let mut value = serde_json::to_value(&*colors).expect("a theme config serializes");
    for (key, hue) in [
        ("base.blue", ramp.accent),
        ("base.yellow", ramp.warning),
        ("base.green", ramp.success),
        ("base.red", ramp.danger),
    ] {
        value[key] = hue.into();
    }
    *colors = serde_json::from_value(value).expect("a theme config reads back");
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
    // In the configs, not on the theme: choosing a mode applies a whole config,
    // so a family set only on the theme would last until the first switch.
    for config in [&mut light, &mut dark] {
        config.font_family = Some(crate::fonts::UI_FAMILY.into());
        config.mono_font_family = Some(crate::fonts::MONO_FAMILY.into());
    }

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

/// The fill of the rail's chosen row: the row on screen, or the keyboard's.
///
/// One step past the hovered row's `marked`, so the two stay apart, and
/// derived rather than named for the reason [`meta_ink`] is: both palettes get
/// it from values they already hold.
pub(crate) fn rail_chosen(cx: &App) -> Hsla {
    chosen_over(
        cx.theme().sidebar_accent,
        cx.theme().sidebar_accent_foreground,
    )
}

fn chosen_over(marked: Hsla, ink: Hsla) -> Hsla {
    marked.mix_oklab(ink, 0.95)
}

/// The composer card's fill. No library token holds it, so it is read off the
/// ramp by mode, the way [`crate::tokens::lift`] chooses its alpha.
pub(crate) fn raised(cx: &App) -> Hsla {
    let ramp = match cx.theme().mode.is_dark() {
        true => &DARK,
        false => &LIGHT,
    };
    Hsla::parse_hex(ramp.raised).unwrap_or(cx.theme().popover)
}

/// The fill of a field with no edge sunk into the well, and of a badge in the
/// rail, which has to stand off the well, a hovered row and a chosen one at
/// once. No library token holds it, so it is read off the ramp by mode, as
/// [`raised`] is.
pub(crate) fn inset(cx: &App) -> Hsla {
    let ramp = match cx.theme().mode.is_dark() {
        true => &DARK,
        false => &LIGHT,
    };
    Hsla::parse_hex(ramp.inset).unwrap_or(cx.theme().background)
}

/// A well on a floating card: the command a pinned permission asks to run.
///
/// Its own step because the dark palette floats its cards on the well's own
/// grey, where a well drawn in `muted` would vanish into the card. Mixed from
/// the card's fill toward the ink, so both palettes sink it by one rule.
pub(crate) fn floating_well(cx: &App) -> Hsla {
    floating_well_on(cx.theme().popover, cx.theme().foreground)
}

fn floating_well_on(card: Hsla, ink: Hsla) -> Hsla {
    card.mix_oklab(ink, FLOATING_WELL)
}

/// How much of the card's own fill a floating well keeps.
const FLOATING_WELL: f32 = 0.93;

/// The composer's lift, in pixels as a hairline is: the tight shadow's blur,
/// then the soft one's drop and blur.
const COMPOSER_EDGE_BLUR: f32 = 1.5;
const COMPOSER_DROP: f32 = 4.;
const COMPOSER_BLUR: f32 = 16.;

/// The composer card's lift: a tight shadow drawn where an edge would be, and a
/// soft one under it.
///
/// **Its own rather than [`crate::tokens::lift`]**, because the card has no border: the tight
/// shadow is the whole of its outline on a white page, so it has to be dark
/// enough to read as one, where the floating cards keep a hairline and only
/// need the elevation.
pub(crate) fn composer_lift(cx: &App) -> Vec<gpui::BoxShadow> {
    let (edge, soft) = composer_shades(cx.theme().mode.is_dark());
    let shadow = |color: Hsla, y: f32, blur: f32| gpui::BoxShadow {
        color,
        offset: gpui::point(gpui::px(0.), gpui::px(y)),
        blur_radius: gpui::px(blur),
        spread_radius: gpui::px(0.),
        inset: false,
    };
    vec![
        shadow(edge, 0., COMPOSER_EDGE_BLUR),
        shadow(soft, COMPOSER_DROP, COMPOSER_BLUR),
    ]
}

/// The two shadows' colours. Near-black prose ink on the light page, where
/// pure black reads as a smudge; pure black on the dark one, which needs far
/// more of it to show at all.
fn composer_shades(dark: bool) -> (Hsla, Hsla) {
    match dark {
        true => (gpui::hsla(0., 0., 0., 0.6), gpui::hsla(0., 0., 0., 0.4)),
        false => {
            let ink = Hsla::parse_hex(LIGHT.foreground).unwrap_or(gpui::black());
            (ink.alpha(0.18), ink.alpha(0.10))
        }
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
    /// strength among rows in the muted one, so the fill is the second thing
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

        // An off switch: its track against the surface it sits on, and its
        // thumb (drawn in the surface) against the track. Graphical parts of a
        // control are held to 3:1, not the 4.5 text is.
        check(
            "an off switch's track against the surface",
            theme.switch,
            theme.background,
            3.0,
        );
        check(
            "an off switch's thumb against its track",
            theme.switch_thumb,
            theme.switch,
            3.0,
        );

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

    /// The base hues are private slots reached through the config's serde form;
    /// a renamed key there would leave them on the shipped palette in silence.
    #[test]
    fn the_base_hues_run_on_the_ramp() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (hue, got, want) in [
                ("blue", theme.blue, ramp.accent),
                ("yellow", theme.yellow, ramp.warning),
                ("green", theme.green, ramp.success),
                ("red", theme.red, ramp.danger),
            ] {
                assert_eq!(
                    got,
                    Hsla::parse_hex(want).unwrap(),
                    "{name}: base {hue} is not the ramp's"
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
    /// selected is the ink on it rather than a slab of colour.
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

    /// A dock is a surface of its own, a step off the reading surface, and
    /// everything drawn in it still shows: the hairline it meets the
    /// conversation at, a hovered row, a selected tab, and its ink. Its step
    /// from the surface is under a region's, on purpose: the hairline is what
    /// says where a dock begins, and the fill only keeps it from reading as
    /// the conversation running on.
    #[test]
    fn a_dock_stands_on_its_own_step() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let panel = theme.tiles;
            assert_eq!(panel, Hsla::parse_hex(ramp.panel).unwrap());
            assert_ne!(
                panel, theme.background,
                "{name}: a dock is the reading surface"
            );
            for (label, fill, floor) in [
                ("the hairline", theme.border, STEP),
                ("a hovered row", theme.list_hover, ROW),
                ("the selected fill", theme.accent, STEP),
                ("an off switch's track", theme.switch, 3.0),
                ("prose", theme.foreground, AA),
                ("meta ink", theme.muted_foreground, AA),
            ] {
                let ratio = contrast(fill, panel);
                assert!(
                    ratio >= floor,
                    "{name}: {label} on a dock is {ratio:.2}, under {floor}"
                );
            }
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
                let mut on = vec![
                    ("surface", theme.background),
                    ("well", theme.muted),
                    ("dock", theme.tiles),
                ];
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

    /// A tinted badge or label carries words, so its ink has to read on its
    /// own fill, for every hue one is drawn in.
    #[test]
    fn a_tint_reads_on_its_own_fill() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (hue, base) in [
                ("accent", theme.blue),
                ("success", theme.green),
                ("warning", theme.yellow),
                // A failure's banner.
                ("danger", theme.red),
                // A label about the code's structure, and an agent's report.
                ("violet", theme.magenta),
                ("cyan", theme.cyan),
            ] {
                let tint = onehand_plugin_host::tint_over(base, theme.background, theme.foreground);
                let ratio = contrast(tint.ink, tint.fill);
                assert!(
                    ratio >= AA,
                    "{name}: {hue} ink on its tint is {ratio:.2}, under {AA}"
                );
                assert!(
                    contrast(tint.fill, theme.background) > 1.0,
                    "{name}: the {hue} tint is the surface"
                );
            }
        }
    }

    /// A changed line in a diff lies on its hue's wash, mixed into the well:
    /// the wash shows against the well, and the line's words and its sign
    /// read on it.
    #[test]
    fn a_diff_line_reads_on_its_wash() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (side, base) in [("added", theme.green), ("removed", theme.red)] {
                let wash = onehand_plugin_host::tint_over(base, theme.muted, theme.foreground);
                assert!(
                    contrast(wash.fill, theme.muted) > 1.0,
                    "{name}: the {side} wash is the well"
                );
                for (label, ink) in [("words", theme.foreground), ("sign", wash.ink)] {
                    let ratio = contrast(ink, wash.fill);
                    assert!(
                        ratio >= AA,
                        "{name}: an {side} line's {label} are {ratio:.2} on its wash"
                    );
                }
            }
        }
    }

    /// A neutral label is drawn on the bubble, and on a picked row, whose fill
    /// is a step past the bubble, on the well instead: either way its fill has
    /// to show against the row it sits in, or it reads as loose words.
    #[test]
    fn a_neutral_label_shows_on_its_row() {
        // Enough to see an edge; a fill is not text, so AA does not apply.
        const APART: f32 = 1.05;
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (row, fill, under) in [
                ("a row", theme.secondary, theme.background),
                ("a hovered row", theme.secondary, theme.list_hover),
                ("the picked row", theme.muted, theme.accent),
            ] {
                let ratio = contrast(fill, under);
                assert!(
                    ratio >= APART,
                    "{name}: a neutral label on {row} is {ratio:.2} from it"
                );
            }
        }
    }

    /// The accent is ink as well as a link: it marks a running session in the
    /// rail, on the well and on the rail's marked fill. A mark is a graphic,
    /// so it is held to the 3:1 a control's parts are.
    #[test]
    fn the_accent_reads_as_a_mark_in_the_rail() {
        // The step a hovered row needs to show. A dark screen needs the wider
        // one: at the light palette's ratio a hovered row there did not show.
        for (name, ramp, mode, floor) in [
            ("light", &LIGHT, ThemeMode::Light, 1.15),
            ("dark", &DARK, ThemeMode::Dark, 1.35),
        ] {
            let theme = resolve(ramp, mode);
            let chosen = chosen_over(theme.sidebar_accent, theme.sidebar_accent_foreground);
            for (surface, fill) in [
                ("well", theme.muted),
                ("hovered row", theme.sidebar_accent),
                ("chosen row", chosen),
            ] {
                let ratio = contrast(theme.link, fill);
                assert!(
                    ratio >= 3.,
                    "{name}: the accent on the {surface} is {ratio:.2}"
                );
            }
            // The rail's chosen row: its text reads, and it stands off the
            // well further than a hovered one does.
            let ink = contrast(theme.sidebar_accent_foreground, chosen);
            assert!(ink >= AA, "{name}: ink on the chosen row is {ink:.2}");
            let (hovered, picked) = (
                contrast(theme.sidebar_accent, theme.muted),
                contrast(chosen, theme.muted),
            );
            assert!(
                hovered >= floor,
                "{name}: a hovered row is {hovered:.2} from the rail, too faint to see"
            );
            assert!(
                picked > hovered + 0.1,
                "{name}: the chosen row ({picked:.2}) is not past a hovered one ({hovered:.2})"
            );
        }
    }

    /// Everything a rail row writes on the fills it takes: at rest the well,
    /// hovered `sidebar_accent`, chosen `rail_chosen`.
    ///
    /// A badge sinks on the inset step, which has to stand off all three or it
    /// reads as loose words on the row it sits in; the words in it read. A
    /// state's word waiting on the person reads on every fill, and on the
    /// selected step of the chip that filters by it. The chosen row's meta
    /// line is the step between its name's ink and muted, so it reads and
    /// stays under the name.
    #[test]
    fn the_rail_reads_on_every_fill_a_row_takes() {
        const APART: f32 = 1.05;
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let chosen = chosen_over(theme.sidebar_accent, theme.sidebar_accent_foreground);
            let inset = Hsla::parse_hex(ramp.inset).unwrap();
            let fills = [
                ("the well", theme.muted),
                ("a hovered row", theme.sidebar_accent),
                ("the chosen row", chosen),
            ];
            let warning = onehand_plugin_host::status_hue(theme.yellow, theme.foreground);
            let danger = onehand_plugin_host::status_hue(theme.red, theme.foreground);

            for (row, fill) in fills {
                let ratio = contrast(inset, fill);
                assert!(
                    ratio >= APART,
                    "{name}: a badge on {row} is {ratio:.2} from it"
                );
                for (state, ink) in [("warning", warning), ("danger", danger)] {
                    let ratio = contrast(ink, fill);
                    assert!(
                        ratio >= AA,
                        "{name}: a {state} word on {row} is {ratio:.2}, under {AA}"
                    );
                }
            }
            for (label, ink) in [
                ("its words", theme.secondary_foreground),
                ("a stuck pill's words", warning),
            ] {
                let ratio = contrast(ink, inset);
                assert!(ratio >= AA, "{name}: {label} on a badge are {ratio:.2}");
            }
            let on_chip = contrast(warning, theme.secondary_active);
            assert!(
                on_chip >= AA,
                "{name}: the attention chip's words while on are {on_chip:.2}"
            );

            let meta = onehand_plugin_host::middle_ink(
                theme.sidebar_accent_foreground,
                theme.muted_foreground,
            );
            let (line, title) = (
                contrast(meta, chosen),
                contrast(theme.sidebar_accent_foreground, chosen),
            );
            assert!(
                line >= AA,
                "{name}: the chosen row's meta line is {line:.2}, under {AA}"
            );
            assert!(
                line < title,
                "{name}: the chosen row's meta line ({line:.2}) is as loud as its name ({title:.2})"
            );
        }
    }

    /// The middle ink step has to read, and has to be *in the middle*.
    ///
    /// Both halves are the whole of what it is for. A completion row spends the
    /// two ends of the ramp on saying which characters matched a query, and
    /// this is the third value the description beside that name is drawn in —
    /// so a step landing on either end would make the description
    /// indistinguishable from one half of the name, which is the state it
    /// exists to prevent. And it is small text on a floating card, on the
    /// reading surface, and on the selected fill under that fill's own ink, so
    /// it owes the same legibility every other ink here does on each.
    #[test]
    fn the_middle_ink_reads_and_stays_between_the_two_it_divides() {
        use onehand_plugin_host::middle_ink;

        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let middle = middle_ink(theme.foreground, theme.muted_foreground);

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

            let on_surface = contrast(middle, theme.background);
            assert!(
                on_surface >= AA,
                "{name}: the middle ink on the reading surface is {on_surface:.2}, under {AA}"
            );

            // On the selected fill the step is taken under that fill's own ink.
            let picked = middle_ink(theme.accent_foreground, theme.muted_foreground);
            let on_picked = contrast(picked, theme.accent);
            assert!(
                on_picked >= AA,
                "{name}: the middle ink on the selected fill is {on_picked:.2}, under {AA}"
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
            let alpha = onehand_plugin_host::tokens::lift_alpha(mode == ThemeMode::Dark);
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

    /// The composer card has no edge, so its fill has to stand off what lies
    /// beside it: the page in the dark palette, where its lift barely shows,
    /// and the well in both. And everything written on it has to read.
    #[test]
    fn the_raised_card_stands_off_the_page_and_the_well() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let raised = Hsla::parse_hex(ramp.raised).unwrap();
            if mode == ThemeMode::Dark {
                let ratio = contrast(raised, theme.background);
                assert!(
                    ratio >= STEP,
                    "{name}: the card is {ratio:.2} from the page"
                );
            }
            // A notch under a surface step: the card's lift draws the edge a
            // fill would otherwise have to, so the fill only has to differ.
            const CARD_STEP: f32 = 1.1;
            let ratio = contrast(raised, theme.muted);
            assert!(
                ratio >= CARD_STEP,
                "{name}: the card is {ratio:.2} from the well"
            );
            for (label, ink) in [
                ("prose", theme.foreground),
                ("meta ink", theme.muted_foreground),
            ] {
                let ratio = contrast(ink, raised);
                assert!(ratio >= AA, "{name}: {label} on the card is {ratio:.2}");
            }
        }
    }

    /// An edgeless field in the well has its fill alone to be found by, so the
    /// fill has to differ from the well it is sunk into, and what is written in
    /// it, the typed words and the placeholder in meta ink, has to read.
    #[test]
    fn an_inset_field_stands_off_the_well() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let inset = Hsla::parse_hex(ramp.inset).unwrap();
            // A notch under a surface step, as the composer card's: the field
            // is small and has a mark and words in it, so the fill only has
            // to differ.
            const FIELD_STEP: f32 = 1.1;
            let ratio = contrast(inset, theme.muted);
            assert!(
                ratio >= FIELD_STEP,
                "{name}: the inset field is {ratio:.2} from the well"
            );
            for (label, ink) in [
                ("prose", theme.foreground),
                ("meta ink", theme.muted_foreground),
            ] {
                let ratio = contrast(ink, inset);
                assert!(
                    ratio >= AA,
                    "{name}: {label} on the inset field is {ratio:.2}"
                );
            }
        }
    }

    /// A well on a floating card has its fill alone to be found by, as the
    /// inset field has, and the command and the gutter in it have to read.
    #[test]
    fn a_floating_well_stands_off_its_card() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let well = floating_well_on(theme.popover, theme.foreground);
            let ratio = contrast(well, theme.popover);
            assert!(
                ratio >= 1.1,
                "{name}: the floating well is {ratio:.2} from its card"
            );
            for (label, ink) in [
                ("prose", theme.foreground),
                ("meta ink", theme.muted_foreground),
            ] {
                let ratio = contrast(ink, well);
                assert!(
                    ratio >= AA,
                    "{name}: {label} on the floating well is {ratio:.2}"
                );
            }
        }
    }

    /// The tight shadow is the card's whole outline, so composited onto the
    /// page under it, it has to be a difference a reader can see.
    #[test]
    fn the_composer_lift_outlines_the_card_on_both_palettes() {
        fn over(fill: Hsla, under: Hsla) -> Hsla {
            let (f, u) = (gpui::Rgba::from(fill), gpui::Rgba::from(under));
            let mix = |a: f32, b: f32| b + (a - b) * f.a;
            gpui::Rgba {
                r: mix(f.r, u.r),
                g: mix(f.g, u.g),
                b: mix(f.b, u.b),
                a: 1.,
            }
            .into()
        }
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let (edge, soft) = composer_shades(mode == ThemeMode::Dark);
            let ratio = contrast(over(edge, theme.background), theme.background);
            assert!(
                ratio >= ROW,
                "{name}: the card's edge is {ratio:.2} on the page"
            );
            assert!(
                soft.a < edge.a,
                "{name}: the soft shadow outweighs the edge"
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
