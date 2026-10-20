//! The app's palette: VS Code's default themes, Dark Modern and Light Modern,
//! written into the component library's tokens.
//!
//! ## Where every value comes from
//!
//! Each step of the ramp is the value of one VS Code colour key, named beside
//! it, resolved the way VS Code resolves it: `dark_modern.json` over
//! `dark_plus.json` over `dark_vs.json` (and the light trio the same way), and
//! for a key none of those files sets, the default VS Code registers for it in
//! its own source. A value VS Code derives (a colour at an opacity, a darkened
//! colour) is written out already derived, and says what it was derived from.
//! A step is never tuned by eye here: changing one means choosing a different
//! key.
//!
//! The app names its steps by what they are *for* (the reading surface, a
//! well, the user's own message), so a view never names a VS Code key; the
//! mapping happens once, in [`paint`]. Every library token not written here
//! keeps the value the library shipped.
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
use onehand_terminal_ui::ansi::{self, Ansi};
use std::rc::Rc;

/// Status ink is the plugin host's, because a built-in plugin needs it too and
/// cannot reach in here. Named through this module all the same: every call
/// site in the app already says `crate::theme::status_ink`, and that is the
/// name the guard against using a raw status fill as text points at.
pub(crate) use onehand_ui::status_ink;

/// The surface a dock draws on is the plugin host's for the same reason, and
/// named through this module for the same one: the Neovim mode hands it to a
/// terminal grid as that grid's background, and a second copy of the answer is
/// a panel and the shell inside it disagreeing about what colour the panel is.
pub(crate) use onehand_ui::dock_surface;

/// The middle ink is the plugin host's too, since a plugin sets a detail beside
/// a name as well, and named through this module like the two above.
pub(crate) use onehand_ui::meta_ink;

/// One mode's surfaces, and the ink that has to be legible on each, each with
/// the VS Code key it is the value of.
struct Ramp {
    /// The reading surface, and the prose on it: `editor.background`,
    /// `foreground`.
    background: &'static str,
    foreground: &'static str,
    /// The docks and the rail: `panel.background`, which VS Code gives
    /// `sideBar.background` too.
    panel: &'static str,
    /// A well sunk into the surface: quoted commands, output, diffs, a folded
    /// thought. `textCodeBlock.background`.
    well: &'static str,
    /// Meta text: Dark Modern's `descriptionForeground`; Light Modern's
    /// `tab.inactiveForeground`, as its `descriptionForeground` is the prose
    /// ink itself and would leave meta text no quieter than what it annotates.
    well_ink: &'static str,
    /// A step past the well: code inside the user's message, a neutral label,
    /// a document's code on a dock. `textPreformat.background` and
    /// `textPreformat.foreground`.
    chip: &'static str,
    chip_ink: &'static str,
    /// Pointer feedback on a row or chip: `list.hoverBackground`.
    hover: &'static str,
    /// The one item selected among several, which no ring marks:
    /// `list.activeSelectionBackground` and its foreground.
    selected: &'static str,
    selected_ink: &'static str,
    /// A filled row on the rail, hovered: `list.inactiveSelectionBackground`.
    marked: &'static str,
    /// Every hairline and card border: `panel.border`.
    hairline: &'static str,
    /// A control floating over the transcript, a menu, a dialog:
    /// `editorWidget.background`.
    floating: &'static str,
    /// The composer card, and a field with no edge (the rail's search, its
    /// badges): `input.background`.
    raised: &'static str,
    inset: &'static str,
    /// A control's edge: `input.border`.
    control: &'static str,
    /// A secondary button at rest, pointed at or pressed, and its word:
    /// `button.secondaryBackground`, `button.secondaryHoverBackground`,
    /// `button.secondaryForeground`.
    button: &'static str,
    button_hover: &'static str,
    button_ink: &'static str,
    /// The scrollbar thumb, at rest and pointed at: `scrollbarSlider.background`,
    /// `scrollbarSlider.hoverBackground`.
    thumb: &'static str,
    thumb_hover: &'static str,
    /// The word on the open tab: `tab.activeForeground`.
    tab_ink: &'static str,
    /// Where a dragged panel would land: `editorGroup.dropBackground`.
    drop: &'static str,
    /// What a dialog dims behind it: `widget.shadow`.
    shadow: &'static str,
    /// Running and links: `textLink.foreground`.
    accent: &'static str,
    /// Keyboard focus: `focusBorder`.
    focus: &'static str,
    /// The text caret: `editorCursor.foreground`.
    caret: &'static str,
    /// Selected text: `editor.selectionBackground`. The library caps its
    /// alpha, so it is drawn thinned over the text.
    selection: &'static str,
    /// The base hues status ink is derived from: `editorWarning.foreground`,
    /// `testing.iconPassed`, `errorForeground`.
    warning: &'static str,
    success: &'static str,
    danger: &'static str,
    /// Danger as a fill, with `button.foreground` on it:
    /// `statusBarItem.errorBackground`.
    danger_fill: &'static str,
    /// The one primary action in a view: `button.background`,
    /// `button.hoverBackground`, `button.foreground`.
    primary: &'static str,
    primary_hover: &'static str,
    primary_ink: &'static str,
    /// A chart's series: `charts.blue`, `charts.green`, `charts.orange`,
    /// `charts.purple`, `charts.red` (`charts.yellow` is the warning hue).
    chart_blue: &'static str,
    chart_green: &'static str,
    chart_orange: &'static str,
    chart_purple: &'static str,
    chart_red: &'static str,
    /// The terminal's sixteen colours, whose magenta and cyan a label's kind
    /// takes too: `terminal.ansi*`.
    ansi: &'static Ansi,
}

/// Light Modern.
const LIGHT: Ramp = Ramp {
    background: "#ffffff",
    foreground: "#3b3b3b",
    panel: "#f8f8f8",
    well: "#f8f8f8",
    well_ink: "#616161",
    // Black at 12% over `editor.background`.
    chip: "#e0e0e0",
    chip_ink: "#3b3b3b",
    hover: "#f2f2f2",
    selected: "#e8e8e8",
    selected_ink: "#000000",
    marked: "#e4e6f1",
    hairline: "#e5e5e5",
    floating: "#f8f8f8",
    raised: "#ffffff",
    inset: "#ffffff",
    control: "#cecece",
    button: "#e5e5e5",
    button_hover: "#cccccc",
    button_ink: "#3b3b3b",
    thumb: "#64646466",
    thumb_hover: "#646464b3",
    tab_ink: "#3b3b3b",
    drop: "#2677cb2e",
    shadow: "#00000029",
    accent: "#005fb8",
    focus: "#005fb8",
    caret: "#000000",
    selection: "#add6ff",
    warning: "#bf8803",
    success: "#73c991",
    danger: "#f85149",
    danger_fill: "#c72e0f",
    primary: "#005fb8",
    primary_hover: "#0258a8",
    primary_ink: "#ffffff",
    chart_blue: "#0063d3",
    chart_green: "#388a34",
    chart_orange: "#d18616",
    chart_purple: "#652d90",
    chart_red: "#e51400",
    ansi: &ansi::LIGHT,
};

/// Dark Modern.
const DARK: Ramp = Ramp {
    background: "#1f1f1f",
    foreground: "#cccccc",
    panel: "#181818",
    well: "#2b2b2b",
    well_ink: "#9d9d9d",
    chip: "#3c3c3c",
    chip_ink: "#d0d0d0",
    hover: "#2a2d2e",
    selected: "#04395e",
    selected_ink: "#ffffff",
    marked: "#37373d",
    hairline: "#2b2b2b",
    floating: "#202020",
    raised: "#313131",
    inset: "#313131",
    control: "#3c3c3c",
    // Dark Modern leaves `button.secondaryBackground` transparent and edges the
    // button with `button.border`; a button here draws no edge, so it rests on
    // Dark Modern's control fill, `input.background`, instead.
    button: "#313131",
    button_hover: "#2b2b2b",
    button_ink: "#cccccc",
    thumb: "#79797966",
    thumb_hover: "#646464b3",
    tab_ink: "#ffffff",
    drop: "#53595d80",
    shadow: "#0000005c",
    accent: "#4daafc",
    focus: "#0078d4",
    caret: "#aeafad",
    selection: "#264f78",
    warning: "#cca700",
    success: "#73c991",
    danger: "#f85149",
    // `errorForeground` darkened by 0.4, as VS Code derives it.
    danger_fill: "#b90f07",
    primary: "#0078d4",
    primary_hover: "#026ec1",
    primary_ink: "#ffffff",
    chart_blue: "#59a4f9",
    chart_green: "#89d185",
    chart_orange: "#d18616",
    chart_purple: "#b180d7",
    chart_red: "#f14c4c",
    ansi: &ansi::DARK,
};

/// A step of the ramp as a colour, to derive another from. Every step is a
/// literal the ramp's tests parse, so the fallback is never reached.
fn hex(step: &str) -> Hsla {
    Hsla::parse_hex(step).unwrap_or_default()
}

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
    set(&mut colors.secondary, ramp.chip);
    set(&mut colors.secondary_foreground, ramp.chip_ink);
    // A filled button pointed at or held open: VS Code's secondary hover, in
    // every slot the library keeps for it, since a slot left alone stays on
    // the config the ramp is written over.
    //
    // Pointed at and held open are one fill on purpose. gpui refines a hover
    // style over the base, and `hover_style` is crate-private, so a trigger
    // that fills itself while its menu is open cannot outrank the hover
    // underneath it -- with two different shades the pressed one would vanish
    // for as long as the pointer stayed on the control that opened it.
    for slot in [
        &mut colors.secondary_hover,
        &mut colors.secondary_active,
        &mut colors.button_secondary_hover,
        &mut colors.button_secondary_active,
    ] {
        set(slot, ramp.button_hover);
    }
    // `accent` is the selected fill, not the hover one. That is the library's
    // own reading of it -- a list item falls back to `accent` for the selected
    // row whenever the highlight ring is off, which here it always is -- and it
    // leaves `list_hover` free to be the fainter of the two.
    set(&mut colors.accent, ramp.selected);
    set(&mut colors.button_secondary, ramp.button);
    set(&mut colors.button_secondary_foreground, ramp.button_ink);
    set(&mut colors.accent_foreground, ramp.selected_ink);
    set(&mut colors.border, ramp.hairline);
    // A switch that is off is a track and a thumb, and both have to be seen
    // for the control to say anything: the library draws the track in
    // `secondary_active`, a fill meant to be quiet, and the thumb in the
    // surface, so an off switch was a faint smudge with an
    // invisible knob. A control's parts are held to 3:1 against what they sit
    // on, and the well's ink is already the ramp's answer to "readable on the
    // surface": as the track, the surface-coloured thumb stands off it too.
    set(&mut colors.switch, ramp.well_ink);
    // The rail is VS Code's side bar. Every one of these has to be written
    // out: the library ships a whole second set of sidebar tokens with values
    // of their own rather than letting them fall back on the ones above, so a
    // ramp that stopped at `accent` left the rail on somebody else's palette.
    //
    // - Its fill is `sideBar.background`, which VS Code gives the panel too.
    // - Its ink is `sideBar.foreground`, the prose ink; the rail's names are
    //   drawn in meta ink where it lays them out, as chrome rather than a
    //   second thing to read.
    // - A hovered row takes `marked`; the chosen row is that step pulled toward
    //   its ink ([`rail_chosen`]).
    // - The guide line down an expanded project is the same hairline as any
    //   other.
    set(&mut colors.sidebar, ramp.panel);
    set(&mut colors.sidebar_foreground, ramp.foreground);
    set(&mut colors.sidebar_accent, ramp.marked);
    set(&mut colors.sidebar_accent_foreground, ramp.selected_ink);
    set(&mut colors.sidebar_border, ramp.hairline);
    set(&mut colors.popover, ramp.floating);
    set(&mut colors.popover_foreground, ramp.foreground);
    // Left to its own devices this one is derived as a fraction of the selected
    // fill, which lands close enough to the surface to read as nothing.
    set(&mut colors.list_hover, ramp.hover);

    set(&mut colors.input, ramp.control);
    set(&mut colors.scrollbar_thumb, ramp.thumb);
    set(&mut colors.scrollbar_thumb_hover, ramp.thumb_hover);
    set(&mut colors.ring, ramp.focus);
    set(&mut colors.caret, ramp.caret);
    // A link keeps its blue pointed at and pressed, as `textLink.activeForeground`
    // does; left alone these were the neutral steps of the config under the ramp.
    for slot in [
        &mut colors.link,
        &mut colors.link_hover,
        &mut colors.link_active,
    ] {
        set(slot, ramp.accent);
    }
    set(&mut colors.info, ramp.accent);
    set(&mut colors.success, ramp.success);
    set(&mut colors.warning, ramp.warning);
    set(&mut colors.primary, ramp.primary);
    set(&mut colors.primary_foreground, ramp.primary_ink);
    set(&mut colors.button_primary, ramp.primary);
    set(&mut colors.button_primary_foreground, ramp.primary_ink);
    // Pressed takes the hover fill, as VS Code draws a pressed button. Every
    // one of these is written: the config the ramp is written over carries a
    // neutral pressed step of its own, which flashed a blue button near-black
    // or near-white under the click.
    for slot in [
        &mut colors.primary_hover,
        &mut colors.primary_active,
        &mut colors.button_primary_hover,
        &mut colors.button_primary_active,
    ] {
        set(slot, ramp.primary_hover);
    }
    // Danger is a solid red with the button ink on it; the lighter red is only
    // ever ink, through the base hue below.
    for slot in [&mut colors.danger, &mut colors.button_danger] {
        set(slot, ramp.danger_fill);
    }
    for slot in [
        &mut colors.danger_foreground,
        &mut colors.button_danger_foreground,
    ] {
        set(slot, ramp.primary_ink);
    }
    // Its hover and pressed fills. VS Code has no danger button to take them
    // from, and the library would thin the red to a third, under which white
    // reads at under 2 on a light dialog. Derived instead: the fill mixed 15%
    // and 25% toward Light Modern's ink, so the word gains contrast as it is
    // acted on.
    let danger = hex(ramp.danger_fill);
    let ink = hex(LIGHT.foreground);
    colors.button_danger_hover = Some(danger.mix_oklab(ink, 0.85).to_hex().into());
    colors.button_danger_active = Some(danger.mix_oklab(ink, 0.75).to_hex().into());
    set(&mut colors.selection, ramp.selection);

    // The rest of what the config under the ramp sets a value of its own for,
    // each on the VS Code key its part is. A slot left alone here keeps the
    // library's palette, whether or not this app draws the part today.
    for (slot, value) in [
        // The title bar and the status bar: `titleBar.*`, `statusBar.*`; the
        // window's edge is `titleBar.border`, as VS Code sets no
        // `window.activeBorder` outside its high-contrast themes.
        (&mut colors.title_bar, ramp.panel),
        (&mut colors.title_bar_border, ramp.hairline),
        (&mut colors.status_bar, ramp.panel),
        (&mut colors.status_bar_border, ramp.hairline),
        (&mut colors.window_border, ramp.hairline),
        // Tabs: `editorGroupHeader.tabsBackground`, `tab.inactive*`,
        // `tab.active*`.
        (&mut colors.tab_bar, ramp.panel),
        (&mut colors.tab_bar_segmented, ramp.panel),
        (&mut colors.tab, ramp.panel),
        (&mut colors.tab_foreground, ramp.well_ink),
        (&mut colors.tab_active, ramp.background),
        (&mut colors.tab_active_foreground, ramp.tab_ink),
        // Dragging a panel: `sash.hoverBorder`, which is `focusBorder`, and
        // `editorGroup.dropBackground`; a dialog's backdrop, `widget.shadow`.
        (&mut colors.drag_border, ramp.focus),
        (&mut colors.drop_target, ramp.drop),
        (&mut colors.overlay, ramp.shadow),
        // The library's lists and tables, on the editor with VS Code's list
        // keys; their heads on `sideBarSectionHeader.background`.
        (&mut colors.list, ramp.background),
        (&mut colors.list_even, ramp.background),
        (&mut colors.list_head, ramp.panel),
        (&mut colors.list_active, ramp.selected),
        (&mut colors.list_active_border, ramp.focus),
        (&mut colors.table, ramp.background),
        (&mut colors.table_even, ramp.background),
        (&mut colors.table_head, ramp.panel),
        (&mut colors.table_head_foreground, ramp.well_ink),
        (&mut colors.table_hover, ramp.hover),
        (&mut colors.table_active, ramp.selected),
        (&mut colors.table_active_border, ramp.focus),
        (&mut colors.table_row_border, ramp.hairline),
        // Grouping: `sideBarSectionHeader.*`.
        (&mut colors.accordion, ramp.panel),
        (&mut colors.group_box, ramp.panel),
        (&mut colors.group_box_foreground, ramp.foreground),
        (&mut colors.description_list_label, ramp.panel),
        (
            &mut colors.description_list_label_foreground,
            ramp.foreground,
        ),
        // Progress: `progressBar.background`. VS Code has no slider or
        // skeleton; the slider takes the progress bar's blue on the editor,
        // and a skeleton the code block's grey.
        (&mut colors.progress_bar, ramp.primary),
        (&mut colors.slider_bar, ramp.primary),
        (&mut colors.slider_thumb, ramp.background),
        (&mut colors.skeleton, ramp.well),
        // The rail's own primary: `button.*`.
        (&mut colors.sidebar_primary, ramp.primary),
        (&mut colors.sidebar_primary_foreground, ramp.primary_ink),
        // The word on a status fill a library part writes, which the app does
        // not draw: the editor's own surface, so it is the page's colour and
        // not the library's.
        (&mut colors.info_foreground, ramp.background),
        (&mut colors.success_foreground, ramp.background),
        (&mut colors.warning_foreground, ramp.background),
        // Charts: `charts.*`, rising green and falling red.
        (&mut colors.chart_1, ramp.chart_blue),
        (&mut colors.chart_2, ramp.chart_green),
        (&mut colors.chart_3, ramp.warning),
        (&mut colors.chart_4, ramp.chart_orange),
        (&mut colors.chart_5, ramp.chart_purple),
        (&mut colors.chart_bullish, ramp.chart_green),
        (&mut colors.chart_bearish, ramp.chart_red),
    ] {
        set(slot, value);
    }

    // The base hues status ink, inline code and a label's kind are derived
    // from, and their light halves, which are the terminal's bright colours.
    // The library keeps these slots private, so they go in through the
    // config's serde form, under the names a theme file would give them.
    let mut value = serde_json::to_value(&*colors).expect("a theme config serializes");
    let a = ramp.ansi;
    for (key, hue) in [
        ("base.blue", ramp.accent.to_string()),
        ("base.yellow", ramp.warning.to_string()),
        ("base.green", ramp.success.to_string()),
        ("base.red", ramp.danger.to_string()),
        ("base.magenta", ansi::hex(a.magenta)),
        ("base.cyan", ansi::hex(a.cyan)),
        ("base.red.light", ansi::hex(a.bright_red)),
        ("base.green.light", ansi::hex(a.bright_green)),
        ("base.yellow.light", ansi::hex(a.bright_yellow)),
        ("base.blue.light", ansi::hex(a.bright_blue)),
        ("base.magenta.light", ansi::hex(a.bright_magenta)),
        ("base.cyan.light", ansi::hex(a.bright_cyan)),
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
/// change, silently. Cloning the shipped config and overwriting every field it
/// sets a value of its own for keeps the derived ones derived from the ramp.
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
        // One corner for everything, large surfaces too; a badge keeps its
        // own, set where it is drawn.
        config.radius = Some(crate::tokens::CORNER);
        config.radius_lg = Some(crate::tokens::CORNER);
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
/// has no such trouble: the link blue already clears AA on the surface and the
/// well, which the tests below hold. What it does is **glare**: a 94%-saturated blue beside
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

/// The ramp of the mode in force, for a step no library token holds, the way
/// [`crate::tokens::lift`] chooses its alpha.
fn ramp_in_force(cx: &App) -> &'static Ramp {
    match cx.theme().mode.is_dark() {
        true => &DARK,
        false => &LIGHT,
    }
}

/// The composer card's fill.
pub(crate) fn raised(cx: &App) -> Hsla {
    Hsla::parse_hex(ramp_in_force(cx).raised).unwrap_or(cx.theme().popover)
}

/// The fill of a field with no edge on the rail, and of a badge in the rail,
/// which has to stand off the rail, a hovered row and a chosen one at once.
pub(crate) fn inset(cx: &App) -> Hsla {
    Hsla::parse_hex(ramp_in_force(cx).inset).unwrap_or(cx.theme().background)
}

/// A well on a floating card: the command a pinned permission asks to run.
///
/// Its own step because a card floats on a fill a step from the page, where a
/// well drawn in `muted` would sit level with or under the card. Mixed from the
/// card's fill toward the ink, so both palettes sink it by one rule.
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

    /// The smallest ratio at which two fills are still two colours.
    ///
    /// VS Code's surfaces sit this close: a widget is 1.01 off the editor
    /// under it, a code block 1.06 off a Light Modern page, and a hairline or
    /// a shadow says where one ends. A fill under this is the one beside it.
    const STEP: f32 = 1.01;

    /// The floor for hue ink: status words and marks, a tint's words, a diff
    /// line's sign.
    ///
    /// Under AA on purpose. VS Code's status hues are its own and are not
    /// lightened here, and the lowest of them on a surface it lands on is
    /// Light Modern's passed green at 2.75. Held at that, so a hue that drops
    /// further shows.
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
            "code in a message against the well",
            theme.secondary,
            theme.muted,
            STEP,
        );
        check(
            "code in a message against the surface",
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
        // The rail is drawn on the side bar's fill, so its marked row is
        // measured against that and not against the reading surface.
        check(
            "a marked row against the rail it sits in",
            theme.sidebar_accent,
            theme.sidebar,
            STEP,
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
        // Under AA in Dark Modern, where `descriptionForeground` on
        // `textPreformat.background` is 4.07: an attachment's name on its
        // caption bar. Held there, so it does not drop further.
        check(
            "meta ink on code in a message",
            theme.muted_foreground,
            theme.secondary,
            4.0,
        );
        check("prose in a well", theme.foreground, theme.muted, AA);
        check(
            "prose on code in a message",
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
    /// The fill alone is not enough: `secondary_hover` and `secondary_active`
    /// are slots of their own, so a ramp that wrote only `secondary` left a
    /// filled control's other two states on the config under the ramp.
    ///
    /// Both halves are checked: that the steps are ours at all, and that each
    /// shows against the resting fill and against the surfaces a filled
    /// control sits on, the page and the rail.
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
            for (label, state) in [
                ("hovered", theme.secondary_hover),
                ("held open", theme.secondary_active),
            ] {
                assert!(
                    contrast(state, theme.secondary) >= STEP,
                    "{name}: a {label} filled button is its own resting fill"
                );
                for (surface, under) in [("page", theme.background), ("rail", theme.sidebar)] {
                    assert!(
                        contrast(state, under) >= STEP,
                        "{name}: a {label} filled button is the {surface} behind it"
                    );
                }
            }
        }
    }

    /// A primary button and a link keep their own blue in every state: the slots are all
    /// written, so none is left to the neutral steps of the config the ramp is
    /// written over, and its word reads on each.
    #[test]
    fn a_primary_button_stays_on_the_ramp_in_every_state() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let mut colors = ThemeConfigColors::default();
            paint(&mut colors, ramp);
            for (state, slot) in [
                ("the primary fill hovered", &colors.primary_hover),
                ("the primary fill pressed", &colors.primary_active),
                ("a button hovered", &colors.button_primary_hover),
                ("a button pressed", &colors.button_primary_active),
                ("a link hovered", &colors.link_hover),
                ("a link pressed", &colors.link_active),
            ] {
                assert!(
                    slot.is_some(),
                    "{name}: the blue of {state} is left to the config under the ramp"
                );
            }
            let theme = resolve(ramp, mode);
            for (state, fill) in [
                ("at rest", theme.button_primary),
                ("hovered", theme.button_primary_hover),
                ("pressed", theme.button_primary_active),
            ] {
                let ratio = contrast(theme.button_primary_foreground, fill);
                assert!(
                    ratio >= AA,
                    "{name}: a primary button's word {state} is {ratio:.2}"
                );
            }
        }
    }

    /// A danger button's word stays readable in every state it takes: at rest,
    /// under the pointer and pressed. It is the one button that ends or
    /// removes something, so it cannot fade at the moment it is acted on, and
    /// it is drawn on a dialog (a removal's confirmation) and on the composer
    /// card (*Stop*), so each state is judged as it lands on both.
    #[test]
    fn a_danger_button_reads_in_every_state() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (surface, under) in [
                ("a dialog", theme.popover),
                ("the composer", hex(ramp.raised)),
            ] {
                for (state, fill) in [
                    ("at rest", theme.button_danger),
                    ("hovered", theme.button_danger_hover),
                    ("pressed", theme.button_danger_active),
                ] {
                    let ratio = contrast(theme.button_danger_foreground, over(fill, under));
                    assert!(
                        ratio >= AA,
                        "{name}: a danger button's word {state} on {surface} is {ratio:.2}"
                    );
                }
            }
        }
    }

    /// The keyboard's ring shows against every surface a control sits on, as
    /// it is drawn: the library lays it round a focused control at half the
    /// ring's strength, a figure it keeps to itself, and the app's own rings,
    /// a field's focused edge among them, draw it whole.
    ///
    /// Under the 3:1 a control's parts are held to, on purpose: the ring is
    /// VS Code's `focusBorder`, which clears 3:1 drawn whole on the editor but
    /// not at half strength, nor on the input fill the composer is drawn in.
    /// Held at what it reaches, so a ring that fades further shows.
    #[test]
    fn the_focus_ring_shows_on_every_surface() {
        // The library's `FOCUS_RING_OPACITY`, which it does not export.
        const DRAWN: f32 = 0.5;
        const AT_HALF: f32 = 1.6;
        const WHOLE: f32 = 2.8;
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            for (surface, under) in [
                ("the reading surface", theme.background),
                ("the composer", hex(ramp.raised)),
                ("a dock", hex(ramp.panel)),
                ("the rail", theme.sidebar),
                ("a floating surface", theme.popover),
            ] {
                for (how, ring, floor) in [
                    ("as a control's ring", theme.ring.alpha(DRAWN), AT_HALF),
                    ("whole", theme.ring, WHOLE),
                ] {
                    let ratio = contrast(over(ring, under), under);
                    assert!(
                        ratio >= floor,
                        "{name}: the focus ring {how} on {surface} is {ratio:.2}"
                    );
                }
            }
        }
    }

    /// A ghost control's fill sits between a hovered row and a chosen one, and
    /// a secondary button rests on VS Code's secondary fill and steps off it
    /// under the pointer.
    #[test]
    fn a_control_fill_sits_between_hover_and_selected() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let fill = onehand_ui::control_fill_of(theme.list_hover, theme.accent);
            let off = |c: Hsla| contrast(c, theme.background);
            assert!(
                off(fill) > off(theme.list_hover) && off(fill) < off(theme.accent),
                "{name}: the control fill ({:.2}) is not between hover ({:.2}) and selected ({:.2})",
                off(fill),
                off(theme.list_hover),
                off(theme.accent)
            );
            assert!(
                off(fill) >= STEP,
                "{name}: the control fill is {:.2} off the surface, under {STEP}",
                off(fill)
            );
            assert_eq!(
                theme.button_secondary,
                hex(ramp.button),
                "{name}: a secondary button does not rest on its own fill"
            );
            assert!(
                contrast(theme.button_secondary_hover, theme.button_secondary) >= STEP,
                "{name}: a secondary button does not change under the pointer"
            );
            let ratio = contrast(theme.button_secondary_foreground, theme.button_secondary);
            assert!(
                ratio >= AA,
                "{name}: a secondary button's word is {ratio:.2}"
            );
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
            let a = ramp.ansi;
            for (hue, got, want) in [
                ("blue", theme.blue, ramp.accent.to_string()),
                ("yellow", theme.yellow, ramp.warning.to_string()),
                ("green", theme.green, ramp.success.to_string()),
                ("red", theme.red, ramp.danger.to_string()),
                ("magenta", theme.magenta, ansi::hex(a.magenta)),
                ("cyan", theme.cyan, ansi::hex(a.cyan)),
                ("light red", theme.red_light, ansi::hex(a.bright_red)),
                ("light green", theme.green_light, ansi::hex(a.bright_green)),
                (
                    "light yellow",
                    theme.yellow_light,
                    ansi::hex(a.bright_yellow),
                ),
                ("light blue", theme.blue_light, ansi::hex(a.bright_blue)),
                (
                    "light magenta",
                    theme.magenta_light,
                    ansi::hex(a.bright_magenta),
                ),
                ("light cyan", theme.cyan_light, ansi::hex(a.bright_cyan)),
            ] {
                assert_eq!(
                    got,
                    Hsla::parse_hex(&want).unwrap(),
                    "{name}: base {hue} is not the ramp's"
                );
            }
        }
    }

    /// The rail is quieter than the conversation, and the marked row is still
    /// the loud thing in it.
    ///
    /// Both halves matter. The rail lays its names out in meta ink, below
    /// prose strength — a column of names to aim at, not a body of text — and
    /// the fill under a
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
                contrast(theme.muted_foreground, theme.sidebar)
                    < contrast(theme.foreground, theme.background),
                "{name}: the rail's ink is as loud as the conversation's prose"
            );
            assert!(
                contrast(theme.sidebar_accent_foreground, theme.sidebar)
                    > contrast(theme.muted_foreground, theme.sidebar),
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
                ("a hovered row", theme.list_hover, STEP),
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

    /// The library's own dark palette puts hover, the well and the step past
    /// it on one colour, so a quoted command and the code inside the user's
    /// message were the same block; the ramp keeps them apart. A hairline is not among them: Dark
    /// Modern draws its borders in its code block's grey.
    #[test]
    fn the_dark_ramp_keeps_its_surfaces_apart() {
        let theme = resolve(&DARK, ThemeMode::Dark);
        let surfaces = [
            ("the well", theme.muted),
            ("the step past the well", theme.secondary),
            ("hover", theme.list_hover),
            ("the selected fill", theme.accent),
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
    /// The step past the well is checked for `danger` alone because that is the
    /// only status drawn there: an attachment the agent never received is
    /// marked on its caption bar inside the user's own message. Asserting the other two against a surface they are
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
                ("closed", theme.magenta),
            ] {
                let ink = onehand_ui::status_hue(base, theme.foreground);
                let mut on = vec![
                    ("surface", theme.background),
                    ("well", theme.muted),
                    ("dock", theme.tiles),
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
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
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

    /// A neutral label is drawn on the step past the well, and on a picked row,
    /// whose fill is a step past that, on the well instead: either way its fill has
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
    /// rail, on the rail's fill and on its marked one. A mark is a graphic,
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
                ("rail", theme.sidebar),
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
            // rail further than a hovered one does.
            let ink = contrast(theme.sidebar_accent_foreground, chosen);
            assert!(ink >= AA, "{name}: ink on the chosen row is {ink:.2}");
            let (hovered, picked) = (
                contrast(theme.sidebar_accent, theme.sidebar),
                contrast(chosen, theme.sidebar),
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

    /// Everything a rail row writes on the fills it takes: at rest the rail,
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
                ("the rail", theme.sidebar),
                ("a hovered row", theme.sidebar_accent),
                ("the chosen row", chosen),
            ];
            let warning = onehand_ui::status_hue(theme.yellow, theme.foreground);
            let danger = onehand_ui::status_hue(theme.red, theme.foreground);

            for (row, fill) in fills {
                let ratio = contrast(inset, fill);
                assert!(
                    ratio >= APART,
                    "{name}: a badge on {row} is {ratio:.2} from it"
                );
                for (state, ink) in [("warning", warning), ("danger", danger)] {
                    let ratio = contrast(ink, fill);
                    assert!(
                        ratio >= HUE_INK,
                        "{name}: a {state} word on {row} is {ratio:.2}, under {HUE_INK}"
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
                on_chip >= HUE_INK,
                "{name}: the attention chip's words while on are {on_chip:.2}"
            );

            let meta =
                onehand_ui::middle_ink(theme.sidebar_accent_foreground, theme.muted_foreground);
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
        use onehand_ui::middle_ink;

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
            let alpha = onehand_ui::tokens::lift_alpha(mode == ThemeMode::Dark);
            let ratio = contrast(under(theme.popover, alpha), theme.popover);
            assert!(
                ratio >= SHADOW,
                "{name}: the lift is {ratio:.2} against the card it falls on, which is not a shadow"
            );

            // The ladder this replaced, at the value it would have used, so the
            // reason for replacing it is on the record rather than in a commit
            // message.
            let shipped = contrast(under(theme.popover, 0.1), theme.popover);
            assert!(
                mode == ThemeMode::Light || shipped < SHADOW,
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
            // The card's lift draws the edge a fill would otherwise have to, so
            // the fill only has to differ.
            let ratio = contrast(raised, theme.muted);
            assert!(
                ratio >= STEP,
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

    /// An edgeless field in the rail has its fill alone to be found by, so the
    /// fill has to differ from the rail it is sunk into, and what is written in
    /// it, the typed words and the placeholder in meta ink, has to read.
    #[test]
    fn an_inset_field_stands_off_the_rail() {
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let inset = Hsla::parse_hex(ramp.inset).unwrap();
            let ratio = contrast(inset, theme.sidebar);
            assert!(
                ratio >= STEP,
                "{name}: the inset field is {ratio:.2} from the rail"
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
        for (name, ramp, mode) in [
            ("light", &LIGHT, ThemeMode::Light),
            ("dark", &DARK, ThemeMode::Dark),
        ] {
            let theme = resolve(ramp, mode);
            let (edge, soft) = composer_shades(mode == ThemeMode::Dark);
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
