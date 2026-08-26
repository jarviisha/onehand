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
    // `accent` is the selected fill, not the hover one. That is the library's
    // own reading of it -- a list item falls back to `accent` for the selected
    // row whenever the highlight ring is off, which here it always is -- and it
    // leaves `list_hover` free to be the fainter of the two.
    set(&mut colors.accent, ramp.selected);
    set(&mut colors.accent_foreground, ramp.selected_ink);
    set(&mut colors.border, ramp.hairline);
    // The rail runs on the ramp like everything else.
    //
    // The library ships a whole second set of sidebar tokens with values of
    // their own rather than leaving them to fall back on the ones above, so a
    // ramp that stopped at `accent` left the most-clicked surface in the window
    // quietly running on somebody else's palette: the rail's fill sat one notch
    // off white in the light palette and dead level with the surface in the
    // dark one, its selected row was a paler fill than every other selection on
    // screen, and its ink was brighter than the prose beside it -- none of it
    // from anything either ramp says.
    //
    // Written from the steps already named rather than added to the `Ramp`,
    // because none of them is a new step. The rail *is* the reading surface,
    // separated from the conversation by the hairline down its edge; a row
    // selected there is selected in exactly the sense a row anywhere else is;
    // and the guide line down an expanded project is the same hairline as any
    // other. Naming them again with values of their own would be inventing a
    // second ramp for one panel and having to keep the two in step by hand.
    set(&mut colors.sidebar, ramp.background);
    set(&mut colors.sidebar_foreground, ramp.foreground);
    set(&mut colors.sidebar_accent, ramp.selected);
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

/// Status colours used as ink on the app's normal surfaces.
#[derive(Clone, Copy)]
pub(crate) struct StatusInk {
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
pub(crate) fn status_ink(cx: &App) -> StatusInk {
    let theme = cx.theme();
    StatusInk {
        danger: status_hue(theme.red, theme.foreground),
        warning: status_hue(theme.yellow, theme.foreground),
        success: status_hue(theme.green, theme.foreground),
    }
}

fn status_hue(base: Hsla, foreground: Hsla) -> Hsla {
    base.mix_oklab(foreground, 0.70)
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::{ThemeConfig, ThemeMode};

    /// The ratio small text has to clear to be readable.
    const AA: f32 = 4.5;

    /// The smallest ratio at which a fill still reads as a region of its own
    /// rather than as an artefact of the display.
    const STEP: f32 = 1.14;

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
    /// the reading surface in one mode and level with it in the other, its
    /// selected row paler than every other selection on screen, its ink
    /// brighter than the prose beside it.
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
                let ink = status_hue(base, theme.foreground);
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
