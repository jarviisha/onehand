//! The palette poured into gpui-component's theme configs, and the mode switch.
use super::*;

pub(super) fn hex(h: Hsla) -> SharedString {
    let c = gpui::Rgba::from(h);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b), b(c.a)).into()
}

/// One step lighter or darker, for a filled control's hover and press.
pub(super) fn step(h: Hsla, by: f32) -> Hsla {
    Hsla {
        l: (h.l + by).clamp(0.0, 1.0),
        ..h
    }
}

/// Write a palette into a theme *config*, the way the app's own `theme.rs`
/// does. gpui-component resolves its component tokens (every button state,
/// the radio, the checkbox, the kbd) from the config when a mode is applied,
/// so writing resolved colours straight onto `Theme::colors` leaves them on the
/// shipped palette.
pub(super) fn paint(config: &mut gpui_component::ThemeConfig, p: &Palette, dark: bool) {
    let c = &mut config.colors;
    // Hover moves away from the surface, press a step further.
    let away = if dark { -1.0 } else { 1.0 };
    let set = |slot: &mut Option<SharedString>, v: Hsla| *slot = Some(hex(v));

    set(&mut c.background, p.page);
    set(&mut c.foreground, p.text);
    set(&mut c.border, p.hairline);
    set(&mut c.input, p.control);
    set(&mut c.ring, p.muted);
    set(&mut c.caret, p.text);
    set(&mut c.selection, p.accent.opacity(SELECTION_ALPHA));
    set(&mut c.link, p.accent);
    set(&mut c.muted, p.sunken);
    set(&mut c.muted_foreground, p.muted);
    set(&mut c.popover, p.panel);
    set(&mut c.popover_foreground, p.text);
    set(&mut c.list_hover, p.selected);
    set(&mut c.list_active, p.chip_on);
    set(&mut c.accent, p.chip_on);
    set(&mut c.accent_foreground, p.text);

    // The one primary per region: ink on light, light on dark.
    let primary_hover = step(p.primary_bg, HOVER_STEP * away);
    let primary_active = step(p.primary_bg, PRESS_STEP * away);
    set(&mut c.primary, p.primary_bg);
    set(&mut c.primary_foreground, p.primary_fg);
    set(&mut c.primary_hover, primary_hover);
    set(&mut c.primary_active, primary_active);
    set(&mut c.button_primary, p.primary_bg);
    set(&mut c.button_primary_foreground, p.primary_fg);
    set(&mut c.button_primary_hover, primary_hover);
    set(&mut c.button_primary_active, primary_active);

    // Ghost and outline controls: the panel, tinted under the pointer.
    set(&mut c.secondary, p.panel);
    set(&mut c.secondary_foreground, p.text);
    set(&mut c.secondary_hover, p.selected);
    set(&mut c.secondary_active, p.chip_on);
    set(&mut c.button, p.panel);
    set(&mut c.button_foreground, p.text);
    set(&mut c.button_hover, p.selected);
    set(&mut c.button_active, p.chip_on);
    set(&mut c.button_secondary, p.panel);
    set(&mut c.button_secondary_foreground, p.text);
    set(&mut c.button_secondary_hover, p.selected);
    set(&mut c.button_secondary_active, p.chip_on);

    // Danger is solid red with white text in both modes; the lighter red is
    // only ever ink on a surface.
    let danger_hover = step(p.danger_solid, -DANGER_HOVER_STEP);
    let danger_active = step(p.danger_solid, -DANGER_PRESS_STEP);
    for (slot, v) in [
        (&mut c.danger, p.danger_solid),
        (&mut c.danger_hover, danger_hover),
        (&mut c.danger_active, danger_active),
        (&mut c.button_danger, p.danger_solid),
        (&mut c.button_danger_hover, danger_hover),
        (&mut c.button_danger_active, danger_active),
    ] {
        set(slot, v);
    }
    set(&mut c.danger_foreground, p.on_danger);
    set(&mut c.button_danger_foreground, p.on_danger);

    set(&mut c.success, p.success);
    set(&mut c.warning, p.warning);
    set(&mut c.info, p.accent);
    set(&mut c.switch, p.muted);
    set(&mut c.scrollbar_thumb, p.control);
}

/// Install both palettes as the configs the mode switch chooses between.
pub(super) fn install(cx: &mut App) {
    let registry = gpui_component::ThemeRegistry::global(cx);
    let mut light_cfg = (**registry.default_light_theme()).clone();
    let mut dark_cfg = (**registry.default_dark_theme()).clone();
    paint(&mut light_cfg, &light(), false);
    paint(&mut dark_cfg, &dark(), true);
    let theme = Theme::global_mut(cx);
    theme.light_theme = std::rc::Rc::new(light_cfg);
    theme.dark_theme = std::rc::Rc::new(dark_cfg);
    theme.list.active_highlight = false;
    Theme::change(ThemeMode::Light, None, cx);
}

pub(super) fn set_mode(dark: bool, window: &mut Window, cx: &mut App) {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, Some(window), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buttons follow the mode: what gpui-component resolves for each
    /// button state, after a mode is applied, is the palette of that mode.
    #[test]
    fn button_tokens_follow_the_mode() {
        let resolve = |p: &Palette, mode: ThemeMode| {
            let mut config = gpui_component::ThemeConfig {
                mode,
                ..Default::default()
            };
            paint(&mut config, p, mode == ThemeMode::Dark);
            let mut theme = Theme::default();
            theme.apply_config(&std::rc::Rc::new(config));
            theme
        };
        for (p, mode) in [(light(), ThemeMode::Light), (dark(), ThemeMode::Dark)] {
            let t = resolve(&p, mode);
            let k = &t.tokens;
            assert_eq!(
                hex(k.button_primary.color),
                hex(p.primary_bg),
                "{mode:?} primary"
            );
            assert_eq!(
                hex(t.button_primary_foreground),
                hex(p.primary_fg),
                "{mode:?} primary ink"
            );
            assert_eq!(
                hex(k.button_danger.color),
                hex(p.danger_solid),
                "{mode:?} danger"
            );
            assert_eq!(
                hex(t.button_danger_foreground),
                hex(p.on_danger),
                "{mode:?} danger ink"
            );
            assert_eq!(
                hex(k.secondary_active.color),
                hex(p.chip_on),
                "{mode:?} selected chip"
            );
            assert_eq!(hex(t.foreground), hex(p.text), "{mode:?} ghost ink");
            assert_ne!(
                hex(k.button_primary_hover.color),
                hex(p.primary_bg),
                "{mode:?} hover moves"
            );
        }
    }
}
