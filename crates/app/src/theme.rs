//! The app's palette: gpui-kit's own Default Light and Default Dark, as the
//! component library ships them, with the app's fonts.
//!
//! ## Where every value comes from
//!
//! Every colour token is the library's: [`install`] clones the two shipped
//! configs and changes one colour in them, the primary (see
//! [`primary_overrides`]), so a library part and the app's own parts read one
//! palette. The few steps no library token holds (the
//! composer's lift, a well on a floating card, a hue tempered for ink) are
//! derived here from those tokens, never written as values of their own. The
//! terminal's sixteen colours are not the library's: they stay
//! `onehand_terminal_ui::ansi`, as a program's colours are its own.
//!
//! ## Status colour is still not a surface
//!
//! `danger`, `warning`, `success` and `info` are *fills*, each with a paired
//! foreground meant for text placed on that fill. Using the fill itself as
//! coloured text produces low-contrast amber and green on a light background.
//! That is a property of what the tokens mean, not of which palette is loaded,
//! so it is repaired once rather than at every call site: see [`status_ink`].

use gpui::{App, Hsla};
use gpui_component::{ActiveTheme as _, Colorize as _, Theme, ThemeRegistry};
use std::rc::Rc;

/// Status ink is the UI crate's, because a feature crate needs it too and
/// cannot reach in here. Named through this module all the same: every call
/// site in the app already says `crate::theme::status_ink`.
pub(crate) use onehand_ui::status_ink;

/// The surface a dock draws on is the UI crate's for the same reason, and
/// named through this module for the same one: the Neovim mode hands it to a
/// terminal grid as that grid's background, and a second copy of the answer is
/// a panel and the shell inside it disagreeing about what colour the panel is.
pub(crate) use onehand_ui::dock_surface;

/// The middle ink is the UI crate's too, since a feature crate sets a detail beside
/// a name as well, and named through this module like the two above.
pub(crate) use onehand_ui::meta_ink;

/// Take the library's two configs, named for the app and set in its fonts.
///
/// **The configs, not the theme.** Choosing a mode applies a whole config, so
/// a family set only on the theme would last until the first switch. Corners
/// are the library's too: its configs leave `radius` and `radius.lg` unset, so
/// they stay at its own 6 and 8.
///
/// Runs at boot, before a mode is chosen, because choosing one applies whichever
/// of these two configs the mode names.
pub(crate) fn install(cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let mut light = (**registry.default_light_theme()).clone();
    let mut dark = (**registry.default_dark_theme()).clone();

    light.name = "onehand Light".into();
    dark.name = "onehand Dark".into();
    for config in [&mut light, &mut dark] {
        config.font_family = Some(crate::fonts::UI_FAMILY.into());
        config.mono_font_family = Some(crate::fonts::MONO_FAMILY.into());
    }
    primary_overrides(&mut light.colors);
    primary_overrides(&mut dark.colors);

    let theme = Theme::global_mut(cx);
    theme.light_theme = Rc::new(light);
    theme.dark_theme = Rc::new(dark);
}

/// The one colour the app changes in the kit's palette: the primary, blue in
/// both modes.
///
/// The kit's primary is its neutral at the far end of the ramp, white on the
/// dark page and near black on the light one, and beside a secondary button
/// that block outweighed the whole row it stood in. A blue carries "the one
/// to press" at the weight of its neighbours. Named by the kit's own ramp, so
/// the value stays the library's colour and no hex is written here.
fn primary_overrides(colors: &mut gpui_component::ThemeConfigColors) {
    // Pointed at and pressed both go darker, in either mode: a lighter blue
    // under white words drops below what small text needs.
    let (rest, hover, pressed) = ("blue-600", "blue-700", "blue-800");
    colors.primary = Some(rest.into());
    colors.primary_hover = Some(hover.into());
    colors.primary_active = Some(pressed.into());
    colors.primary_foreground = Some("neutral-50".into());
}

/// A base hue tempered for use as ink where it is already legible.
///
/// **A different problem from status ink, so a different target.** A status
/// colour is pulled toward the *foreground* because it arrives as a fill and
/// has to be made bright or dark enough to read. The hue that marks inline code
/// is already legible; what it does beside neutral prose is glare. Pulling it
/// toward the *meta ink* instead, a neutral of about its own lightness, takes a
/// third of the saturation out and keeps the contrast where it was.
pub(crate) fn hue_ink(base: Hsla, cx: &App) -> Hsla {
    temper(base, cx.theme().muted_foreground)
}

fn temper(base: Hsla, neutral: Hsla) -> Hsla {
    base.mix_oklab(neutral, 0.70)
}

/// The fill of the rail's chosen row: the row on screen, or the keyboard's.
/// The library's sidebar marks its active item on `sidebar_accent`.
pub(crate) fn rail_chosen(cx: &App) -> Hsla {
    cx.theme().sidebar_accent
}

/// The composer card's fill: the page's own, as the card is told apart by its
/// edge and its lift.
pub(crate) fn raised(cx: &App) -> Hsla {
    cx.theme().background
}

/// The fill of a field on the rail, and of a badge in the rail, which has to
/// stand off the rail, a hovered row and a chosen one at once: the page's.
pub(crate) fn inset(cx: &App) -> Hsla {
    cx.theme().background
}

/// A well on a floating card: the command a pinned permission asks to run.
/// The library's own well, as its floating surfaces sit on the page's fill.
pub(crate) fn floating_well(cx: &App) -> Hsla {
    cx.theme().muted
}

/// The composer's lift, in pixels as a hairline is: the tight shadow's blur,
/// then the soft one's drop and blur.
const COMPOSER_EDGE_BLUR: f32 = 1.5;
const COMPOSER_DROP: f32 = 4.;
const COMPOSER_BLUR: f32 = 16.;

/// The composer card's lift: a tight shadow drawn where an edge would be, and a
/// soft one under it.
pub(crate) fn composer_lift(cx: &App) -> Vec<gpui::BoxShadow> {
    let theme = cx.theme();
    let (edge, soft) = composer_shades(theme.mode.is_dark(), theme.foreground, theme.border);
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

/// The two shadows' colours. The prose ink, thinned, on the light page, where
/// pure black reads as a smudge. On the dark page, near black, no shadow
/// shows, so the tight one takes the hairline's colour and draws the edge.
fn composer_shades(dark: bool, ink: Hsla, hairline: Hsla) -> (Hsla, Hsla) {
    match dark {
        true => (hairline, gpui::hsla(0., 0., 0., 0.4)),
        false => (ink.alpha(0.18), ink.alpha(0.10)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::{ThemeColor, ThemeConfig, ThemeMode};

    /// The ratio small text has to clear to be readable.
    const AA: f32 = 4.5;

    /// The floor for hue ink: status words and marks, a tint's words, a diff
    /// line's sign.
    ///
    /// Under AA on purpose: the hues are the library's and are not lightened
    /// here. Held at this, so a hue that drops further shows.
    const HUE_INK: f32 = 2.7;

    /// The smallest ratio at which a shadow still reads as one rather than
    /// as an artefact of the display.
    const SHADOW: f32 = 1.10;

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

    /// A colour as it lands on screen: `fill` laid over the opaque `under`
    /// at its own alpha, which is how a see-through fill is actually seen.
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

    /// The library's own palette in one mode, as [`install`] leaves it.
    fn shipped(mode: ThemeMode) -> Theme {
        let colors = match mode {
            ThemeMode::Dark => ThemeColor::dark(),
            ThemeMode::Light => ThemeColor::light(),
        };
        let mut theme = Theme::from(&*colors);
        theme.mode = mode;
        theme
    }

    /// The palette as [`install`] leaves it: the library's, under the app's
    /// own overrides.
    fn installed(mode: ThemeMode) -> Theme {
        let mut theme = shipped(mode);
        let mut config = ThemeConfig {
            mode,
            ..Default::default()
        };
        primary_overrides(&mut config.colors);
        theme.apply_config(&Rc::new(config));
        theme
    }

    /// The primary button's word reads on its own fill in both modes, at
    /// rest, pointed at and pressed, and the fill is a blue rather than the
    /// kit's neutral.
    #[test]
    fn the_primary_word_reads_on_the_primary_fill() {
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = installed(mode);
            for (state, fill) in [
                ("rest", theme.primary),
                ("hover", theme.primary_hover),
                ("pressed", theme.primary_active),
            ] {
                let ratio = contrast(theme.primary_foreground, fill);
                assert!(ratio >= AA, "{name}: primary word on {state} is {ratio:.2}");
            }
            assert!(
                theme.primary.s > 0.5,
                "{name}: primary is not a blue but {:?}",
                theme.primary
            );
        }
    }

    /// Status words and marks are small, and most of them land inside a well —
    /// a tool's status word, a diff's added and removed lines, an exit code.
    ///
    /// The step past the well is checked for `danger` alone because that is the
    /// only status drawn there: an attachment the agent never received is
    /// marked on its caption bar inside the user's own message. Asserting the other two against a surface they are
    /// never drawn on would be inventing a requirement.
    #[test]
    fn status_ink_is_readable_on_every_surface_it_lands_on() {
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
            for (role, base) in [
                ("danger", theme.red),
                ("warning", theme.yellow),
                ("success", theme.green),
                ("closed", theme.magenta),
            ] {
                let ink = onehand_ui::status_hue(base, theme.foreground);
                let mut on = vec![
                    ("surface", theme.background),
                    ("well", theme.muted),
                    ("dock", theme.sidebar),
                ];
                // A removal on a secondary button is its word in the danger
                // ink, on the button's own fill.
                if role == "danger" {
                    on.push(("caption bar", theme.secondary));
                    on.push(("secondary button", theme.button_secondary));
                }
                for (surface, fill) in on {
                    let ratio = contrast(ink, fill);
                    assert!(
                        ratio >= HUE_INK,
                        "{name}: {role} ink on the {surface} is {ratio:.2}, under {HUE_INK}"
                    );
                }
            }
        }
    }

    /// A tinted badge or label carries words, so its ink has to read on its
    /// own fill, for every hue one is drawn in.
    #[test]
    fn a_tint_reads_on_its_own_fill() {
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
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
                let tint = onehand_ui::tint_over(base, theme.background, theme.foreground);
                let ratio = contrast(tint.ink, tint.fill);
                assert!(
                    ratio >= HUE_INK,
                    "{name}: {hue} ink on its tint is {ratio:.2}, under {HUE_INK}"
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
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
            for (side, base) in [("added", theme.green), ("removed", theme.red)] {
                let wash = onehand_ui::tint_over(base, theme.muted, theme.foreground);
                assert!(
                    contrast(wash.fill, theme.muted) > 1.0,
                    "{name}: the {side} wash is the well"
                );
                for (label, ink, floor) in
                    [("words", theme.foreground, AA), ("sign", wash.ink, HUE_INK)]
                {
                    let ratio = contrast(ink, wash.fill);
                    assert!(
                        ratio >= floor,
                        "{name}: an {side} line's {label} are {ratio:.2} on its wash"
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
    /// exists to prevent. And it is small text on a floating card, on the
    /// reading surface, and on the selected fill under that fill's own ink, so
    /// it owes the same legibility every other ink here does on each.
    #[test]
    fn the_middle_ink_reads_and_stays_between_the_two_it_divides() {
        use onehand_ui::middle_ink;

        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
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

    /// A floating surface has to be told from the card it falls on: by its
    /// lift on the light palette, and on the dark one, whose surfaces are near
    /// black and show no shadow, by its edge, which the library draws round
    /// every floating surface.
    #[test]
    fn a_floating_surface_stands_off_what_it_floats_on() {
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

        let light = shipped(ThemeMode::Light);
        let alpha = onehand_ui::tokens::lift_alpha(false);
        let ratio = contrast(under(light.popover, alpha), light.popover);
        assert!(
            ratio >= SHADOW,
            "light: the lift is {ratio:.2} against the card it falls on"
        );

        let dark = shipped(ThemeMode::Dark);
        let ratio = contrast(dark.border, dark.popover);
        assert!(
            ratio >= SHADOW,
            "dark: a floating surface's edge is {ratio:.2} against it"
        );
    }

    /// The tight shadow is the card's whole outline, so composited onto the
    /// page under it, it has to be a difference a reader can see.
    #[test]
    fn the_composer_lift_outlines_the_card_on_both_palettes() {
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
            let (edge, soft) =
                composer_shades(mode == ThemeMode::Dark, theme.foreground, theme.border);
            let ratio = contrast(over(edge, theme.background), theme.background);
            assert!(
                ratio >= SHADOW,
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
        for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
            let theme = shipped(mode);
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
