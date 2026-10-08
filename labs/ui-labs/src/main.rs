//! ui-labs: a proposed onehand UI drawn with gpui-component, on static data.
//! Nothing here is the app; `make showcase` opens the app as it is.
//!
//! - Resize the window: the chat keeps 30rem beside a 30rem Workbench, and
//!   below that the Workbench takes the content area with a way back.
//! - The dock's Files list and the file sit side by side from 36rem, one at a
//!   time under a back link below it.
//! - The palette button swaps light and dark through the theme config, so the
//!   library's own buttons follow.
//! - The rail opens the overview, Tasks, Issues and the composer cards; a
//!   session returns to the chat.
use gpui::{
    App, AppContext, Context, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, rems,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, Selectable as _, Sizable as _, StyledExt as _, Theme,
    ThemeMode,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

mod tokens;
use tokens::*;

fn hex(h: Hsla) -> SharedString {
    let c = gpui::Rgba::from(h);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b), b(c.a)).into()
}

/// One step lighter or darker, for a filled control's hover and press.
fn step(h: Hsla, by: f32) -> Hsla {
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
fn paint(config: &mut gpui_component::ThemeConfig, p: &Palette, dark: bool) {
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
    set(&mut c.selection, p.accent.opacity(0.25));
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
    let primary_hover = step(p.primary_bg, 0.12 * away);
    let primary_active = step(p.primary_bg, 0.2 * away);
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
    let danger_hover = step(p.danger_solid, -0.06);
    let danger_active = step(p.danger_solid, -0.12);
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
fn install(cx: &mut App) {
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

fn set_mode(dark: bool, window: &mut Window, cx: &mut App) {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, Some(window), cx);
}

#[derive(PartialEq)]
enum Presentation {
    Conversation,
    Split,
    FocusWorkbench,
}

/// The pure rule from the proposal: split only while the chat keeps its minimum.
fn presentation(avail_rem: f32, workbench_open: bool, zoom: f32) -> Presentation {
    if !workbench_open {
        Presentation::Conversation
    } else if avail_rem - DOCK_PREF >= CHAT_MIN * zoom {
        Presentation::Split
    } else {
        Presentation::FocusWorkbench
    }
}

mod composer;
mod pages;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Chat,
    Overview,
    Tasks,
    Issues,
    Composer,
}

struct Labs {
    dark: bool,
    workbench: bool,
    file_open: bool,
    page: Page,
    task_open: bool,
    issue: Option<usize>,
    choice: usize,
    picks: [bool; 4],
}

impl Labs {
    fn palette(&self) -> Palette {
        if self.dark { dark() } else { light() }
    }

    fn hairline_v(p: &Palette) -> gpui::Div {
        div().w(gpui::px(0.5)).h_full().bg(p.hairline)
    }

    fn rail(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.page;
        let nav = |cx: &mut Context<Self>,
                   page: Page,
                   label: &'static str,
                   icon: IconName,
                   count: Option<&'static str>| {
            h_flex()
                .id(label)
                .when(current == page, |d| d.bg(p.selected).text_color(p.text))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    this.task_open = false;
                    cx.notify();
                }))
                .h(rems(ROW_H))
                .px(rems(CONTROL))
                .gap(rems(CONTROL))
                .rounded(rems(RADIUS_SM))
                .when(current != page, |d| d.text_color(p.text2))
                .child(Icon::new(icon).size(rems(ICON)))
                .child(div().flex_1().child(label))
                .children(
                    count.map(|n| div().text_size(rems(TEXT_XS)).text_color(p.muted).child(n)),
                )
        };
        let session = |cx: &mut Context<Self>,
                       title: &'static str,
                       foot: &'static str,
                       dot: Option<Hsla>,
                       sel: bool| {
            let sel = sel && current == Page::Chat;
            h_flex()
                .id(title)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Chat;
                    cx.notify();
                }))
                .py(rems(TIGHT))
                .pl(rems(RAIL_INDENT))
                .pr(rems(CONTROL))
                .gap(rems(CONTROL))
                .rounded(rems(RADIUS_SM))
                .when(sel, |d| d.bg(p.selected))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().text_color(p.text).child(title))
                        .child(
                            div()
                                .truncate()
                                .text_size(rems(TEXT_XS))
                                .text_color(p.muted)
                                .child(foot),
                        ),
                )
                .child(
                    div()
                        .w(rems(DOT_COLUMN))
                        .flex()
                        .justify_center()
                        .children(dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                )
        };
        v_flex()
            .w(rems(RAIL_W))
            .h_full()
            .flex_none()
            .bg(p.sunken)
            .child(
                h_flex()
                    .h(rems(BAR_H))
                    .px(rems(RELATED))
                    .font_medium()
                    .text_color(p.text)
                    .child("Audit workspace"),
            )
            .child(
                v_flex()
                    .px(rems(CONTROL))
                    .gap(rems(TIGHT * 0.5))
                    .child(nav(
                        cx,
                        Page::Overview,
                        "Workspace overview",
                        IconName::LayoutDashboard,
                        None,
                    ))
                    .child(nav(cx, Page::Tasks, "Tasks", IconName::Inbox, Some("2")))
                    .child(nav(cx, Page::Issues, "Issues", IconName::CircleCheck, None))
                    .child(nav(
                        cx,
                        Page::Composer,
                        "Composer cards",
                        IconName::Frame,
                        None,
                    ))
                    .child(
                        div().py(rems(CONTROL)).child(
                            Button::new("new-session")
                                .primary()
                                .small()
                                .w_full()
                                .icon(IconName::Plus)
                                .label("New session"),
                        ),
                    ),
            )
            .child(div().h(gpui::px(0.5)).bg(p.hairline))
            .child(
                v_flex()
                    .p(rems(CONTROL))
                    .gap(rems(TIGHT * 0.5))
                    .child(
                        h_flex()
                            .h(rems(ROW_H))
                            .px(rems(CONTROL))
                            .gap(rems(CONTROL))
                            .text_color(p.text)
                            .font_medium()
                            .child(Icon::new(IconName::FolderOpen).size(rems(ICON)))
                            .child(div().flex_1().child("atlas-api"))
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size(rems(ICON_SM))
                                    .text_color(p.muted),
                            ),
                    )
                    .child(
                        div()
                            .pl(rems(RAIL_INDENT))
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child("main · 3 changes"),
                    )
                    .child(session(
                        cx,
                        "Inspect dashboard",
                        "claude",
                        Some(p.warning),
                        true,
                    ))
                    .child(session(
                        cx,
                        "Fix flaky retry test",
                        "claude",
                        Some(p.accent),
                        false,
                    ))
                    .child(session(
                        cx,
                        "Audit interface fast",
                        "codex",
                        Some(p.danger),
                        false,
                    ))
                    .child(session(cx, "Rename config keys", "claude", None, false)),
            )
    }

    fn header(&self, p: &Palette, title: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(rems(BAR_H))
            .flex_none()
            .px(rems(INSET))
            .gap(rems(CONTROL))
            .border_b_1()
            .border_color(p.hairline)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_medium()
                    .text_color(p.text)
                    .child(title),
            )
            .child(
                Button::new("theme")
                    .ghost()
                    .small()
                    .icon(IconName::Palette)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dark = !this.dark;
                        set_mode(this.dark, window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("term")
                    .ghost()
                    .small()
                    .icon(IconName::SquareTerminal),
            )
            .child(
                Button::new("wb")
                    .ghost()
                    .small()
                    .icon(IconName::PanelRight)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.workbench = !this.workbench;
                        cx.notify();
                    })),
            )
    }

    fn chat(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let code = |text: &'static str| {
            div()
                .p(rems(RELATED))
                .rounded(rems(RADIUS_MD))
                .bg(p.sunken)
                .font_family(mono.clone())
                .text_size(rems(TEXT_READ_SM))
                .text_color(p.text2)
                .child(text)
        };
        let column = v_flex()
            .w_full()
            .max_w(rems(READ_MAX))
            .px(rems(INSET))
            .py(rems(SECTION))
            .gap(rems(RELATED))
            .text_size(rems(TEXT_READ))
            .line_height(rems(TEXT_READ * LEADING_READ))
            .text_color(p.text)
            .child(
                h_flex().justify_end().child(
                    div()
                        .max_w(rems(BUBBLE_MAX))
                        .px(rems(RELATED))
                        .py(rems(CONTROL))
                        .rounded(rems(RADIUS_XL))
                        .bg(p.sunken)
                        .child("The retry test fails about one run in five. Find out why and fix it."),
                ),
            )
            .child("Mình đã đọc test và hàm retry. Lỗi nằm ở chỗ backoff dùng thời gian thật, nên khi máy chậm thì lần thử thứ ba vượt quá timeout của test.")
            .child(
                h_flex()
                    .gap(rems(CONTROL))
                    .text_size(rems(TEXT_READ_SM))
                    .text_color(p.text2)
                    .child(Icon::new(IconName::ChevronRight).size(rems(ICON_SM)))
                    .child("Ran 2 commands")
                    .child(div().text_color(p.muted).child("·"))
                    .child(div().text_color(p.danger).child("1 failed")),
            )
            .child(code("cargo test -p retry -- flaky\nthread 'backoff_caps' panicked at 'elapsed 5.2s > 5s'"))
            .child("Mình sẽ đổi backoff sang đồng hồ giả trong test, rồi chạy lại.");

        let permission = v_flex()
            .w_full()
            .max_w(rems(COMPOSER_MAX))
            .rounded(rems(RADIUS_LG))
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .child(
                v_flex()
                    .p(rems(RELATED))
                    .gap(rems(CONTROL))
                    .child(
                        div()
                            .font_medium()
                            .text_color(p.text)
                            .child("Run a command?"),
                    )
                    .child(code("cargo test -p retry")),
            )
            .child(
                h_flex()
                    .px(rems(RELATED))
                    .py(rems(CONTROL))
                    .gap(rems(CONTROL))
                    .border_t_1()
                    .border_color(p.hairline)
                    .child(
                        div()
                            .flex_1()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child("Enter allows once · Esc denies"),
                    )
                    .child(Button::new("deny").ghost().small().label("Deny"))
                    .child(
                        Button::new("always")
                            .outline()
                            .small()
                            .label("Always allow"),
                    )
                    .child(Button::new("once").primary().small().label("Allow once")),
            );

        let composer = Self::composer_card(p, composer::ComposerLook::default());

        v_flex()
            .flex_1()
            .min_w(rems(CHAT_MIN))
            .h_full()
            .child(self.header(p, "Fix flaky retry test", cx))
            .child(
                div()
                    .id("transcript")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(h_flex().justify_center().child(column)),
            )
            .child(
                v_flex()
                    .items_center()
                    .px(rems(INSET))
                    .pb(rems(RELATED))
                    .gap(rems(CONTROL))
                    .child(permission)
                    .child(composer),
            )
    }

    fn workbench(
        &self,
        p: &Palette,
        width_rem: f32,
        focus: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let side_by_side = width_rem >= SPLIT_MIN;
        let mode = |id: &'static str, label: &'static str, sel: bool| {
            Button::new(id).ghost().small().label(label).selected(sel)
        };
        let files = [
            "src/retry.rs",
            "src/backoff.rs",
            "tests/flaky.rs",
            "Cargo.toml",
            "README.md",
        ];
        let list = v_flex().p(rems(CONTROL)).gap(rems(TIGHT * 0.5)).children(
            files.iter().enumerate().map(|(i, f)| {
                h_flex()
                    .id(SharedString::from(*f))
                    .h(rems(ROW_H))
                    .px(rems(CONTROL))
                    .gap(rems(CONTROL))
                    .rounded(rems(RADIUS_SM))
                    .text_color(p.text2)
                    .when(i == 1 && self.file_open, |d| {
                        d.bg(p.selected).text_color(p.text)
                    })
                    .child(
                        Icon::new(IconName::File)
                            .size(rems(ICON))
                            .text_color(p.muted),
                    )
                    .child(div().truncate().child(*f))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.file_open = true;
                        cx.notify();
                    }))
            }),
        );
        let reader = v_flex()
            .flex_1()
            .min_w_0()
            .when(!side_by_side, |d| {
                d.child(
                    h_flex()
                        .h(rems(SUBBAR_H))
                        .px(rems(CONTROL))
                        .border_b_1()
                        .border_color(p.hairline)
                        .child(
                            Button::new("back-files")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowLeft)
                                .label("Files")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.file_open = false;
                                    cx.notify();
                                })),
                        )
                        .child(div().text_size(rems(TEXT_XS)).text_color(p.muted).child("src/backoff.rs")),
                )
            })
            .child(
                div()
                    .p(rems(INSET))
                    .font_family(mono)
                    .text_size(rems(TEXT_READ_SM))
                    .line_height(rems(TEXT_READ_SM * LEADING_READ))
                    .text_color(p.text2)
                    .child("pub fn backoff(attempt: u32, clock: &dyn Clock) -> Duration {\n    let base = Duration::from_millis(200);\n    base * 2u32.pow(attempt.min(5))\n}"),
            );
        let body = if side_by_side {
            h_flex()
                .flex_1()
                .items_start()
                .child(div().w(rems(LIST_W)).h_full().flex_none().child(list))
                .child(Self::hairline_v(p))
                .child(reader)
        } else if self.file_open {
            h_flex().flex_1().items_start().child(reader)
        } else {
            h_flex()
                .flex_1()
                .items_start()
                .child(div().flex_1().child(list))
        };

        v_flex()
            .when(focus, |d| d.flex_1())
            .when(!focus, |d| d.w(rems(DOCK_PREF)).flex_none())
            .h_full()
            .bg(p.panel)
            .child(
                h_flex()
                    .h(rems(BAR_H))
                    .flex_none()
                    .px(rems(CONTROL))
                    .gap(rems(TIGHT))
                    .border_b_1()
                    .border_color(p.hairline)
                    .when(focus, |d| {
                        d.child(
                            Button::new("back-chat")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowLeft)
                                .label("Conversation")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workbench = false;
                                    cx.notify();
                                })),
                        )
                        .child(Self::hairline_v(p).h(rems(ICON)))
                    })
                    .child(mode("m-editor", "Editor", true))
                    .child(mode("m-md", "Markdown", false))
                    .child(mode("m-issues", "Issues", false))
                    .child(div().flex_1())
                    .child(Button::new("max").ghost().small().icon(IconName::Maximize))
                    .child(
                        Button::new("hide")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.workbench = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(body)
            .child(
                div()
                    .px(rems(INSET))
                    .py(rems(CONTROL))
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child(format!(
                        "dock {width_rem:.1}rem · {}",
                        if side_by_side {
                            "side by side (≥ 36rem)"
                        } else {
                            "list → detail (< 36rem)"
                        }
                    )),
            )
    }
}

impl Render for Labs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let rem = window.rem_size();
        let avail = window.viewport_size().width / rem - RAIL_W;
        let shown = presentation(avail, self.workbench, 1.0);

        h_flex()
            .size_full()
            .bg(p.page)
            .text_size(rems(TEXT_SM))
            .text_color(p.text)
            .child(self.rail(&p, cx))
            .child(Self::hairline_v(&p))
            .when(self.page != Page::Chat, |d| {
                d.child(self.page_view(&p, avail, cx))
            })
            .when(
                self.page == Page::Chat && shown != Presentation::FocusWorkbench,
                |d| d.child(self.chat(&p, cx)),
            )
            .when(
                self.page == Page::Chat && shown == Presentation::Split,
                |d| {
                    d.child(Self::hairline_v(&p))
                        .child(self.workbench(&p, DOCK_PREF, false, cx))
                },
            )
            .when(
                self.page == Page::Chat && shown == Presentation::FocusWorkbench,
                |d| d.child(self.workbench(&p, avail, true, cx)),
            )
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            install(cx);
            cx.open_window(Default::default(), |window, cx| {
                let view = cx.new(|_| Labs {
                    dark: false,
                    workbench: true,
                    file_open: true,
                    page: Page::Overview,
                    task_open: false,
                    issue: Some(0),
                    choice: 0,
                    picks: [true, false, false, false],
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("window");
            cx.activate(true);
        });
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

    #[test]
    fn presentation_follows_the_budget() {
        // 800px / 16 = 50rem; minus the rail leaves 35.5rem: no room for 30 + 30.
        assert!(presentation(800.0 / 16.0 - RAIL_W, true, 1.0) == Presentation::FocusWorkbench);
        assert!(presentation(1600.0 / 16.0 - RAIL_W, true, 1.0) == Presentation::Split);
        // 85.5rem fits 30 + 30 at 100% but not at 200% zoom.
        assert!(presentation(85.5, true, 2.0) == Presentation::FocusWorkbench);
        assert!(presentation(35.5, false, 1.0) == Presentation::Conversation);
    }
}
