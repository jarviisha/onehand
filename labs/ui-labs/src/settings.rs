//! Settings as a page, and the one destructive confirm: deleting a project.
//!
//! Settings keeps a nav column of `SETTINGS_NAV` while the page holds it beside
//! a form that can stay side by side; narrower, the nav becomes a select. A
//! form is at most `FORM_MAX`, and below `FORM_STACK` each row's label stacks
//! over its control.
use super::composer::{item, key};
use super::*;
use gpui::{Entity, Pixels};
use gpui_component::WindowExt as _;
use gpui_component::input::{Input, InputState};
use gpui_component::switch::Switch;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Section {
    Appearance,
    Workspace,
    Agents,
    Connections,
    Shortcuts,
}

const SECTIONS: [(Section, &str); 5] = [
    (Section::Appearance, "Appearance"),
    (Section::Workspace, "Workspace"),
    (Section::Agents, "Agents"),
    (Section::Connections, "Connections"),
    (Section::Shortcuts, "Shortcuts"),
];

const SHORTCUTS: [(&str, &str); 7] = [
    ("New session", "ctrl-n"),
    ("Show or hide the Workbench", "ctrl-\\"),
    ("Show or hide the terminal", "ctrl-`"),
    ("Show or hide the rail", "ctrl-shift-b"),
    ("Larger reading size", "ctrl-="),
    ("Smaller reading size", "ctrl--"),
    ("Reading size back to 100%", "ctrl-0"),
];

/// The reading sizes Settings offers; the keys step between any.
const READING: [(f32, &str); 3] = [(1.0, "100%"), (1.15, "115%"), (1.3, "130%")];

pub(super) struct Settings {
    section: Section,
    nav_menu: bool,
    notify: bool,
    keep_finished: bool,
    check: Entity<InputState>,
}

impl Settings {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Labs>) -> Self {
        Self {
            section: Section::Appearance,
            nav_menu: false,
            notify: true,
            keep_finished: false,
            check: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("cargo test")
                    .default_value("cargo test --workspace")
            }),
        }
    }
}

/// Whether the nav sits beside the form, and whether the form's rows stack, in
/// a page `avail` wide.
pub(super) fn settings_layout(avail: f32) -> (bool, bool) {
    let nav = avail >= SETTINGS_NAV + FORM_STACK + INSET * 3.0;
    let form = (avail - INSET * 2.0 - if nav { SETTINGS_NAV + INSET } else { 0.0 }).min(FORM_MAX);
    (nav, form < FORM_STACK)
}

impl Labs {
    pub(super) fn settings_page(
        &self,
        p: &Palette,
        avail: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let s = &self.settings;
        let (nav_beside, stacked) = settings_layout(avail);
        let current = SECTIONS
            .iter()
            .find(|(sec, _)| *sec == s.section)
            .map(|(_, l)| *l)
            .unwrap_or("");

        let nav = v_flex()
            .w(rems(SETTINGS_NAV))
            .flex_none()
            .gap(rems(ROW_GAP))
            .children(SECTIONS.iter().map(|&(sec, label)| {
                let on = sec == s.section;
                h_flex()
                    .id(label)
                    .h(rems(ROW_H))
                    .px(rems(CONTROL))
                    .rounded(rems(RADIUS_SM))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.selected))
                    .text_color(if on { p.text } else { p.text2 })
                    .when(on, |d| d.bg(p.selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.section = sec;
                        cx.notify();
                    }))
                    .child(label)
            }));

        // Narrow: one control naming the section, opening the others.
        let select = div()
            .relative()
            .child(
                action("section-select")
                    .ghost()
                    .small()
                    .label(current)
                    .icon(IconName::ChevronDown)
                    .selected(s.nav_menu)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.nav_menu = !this.settings.nav_menu;
                        cx.notify();
                    })),
            )
            .when(s.nav_menu, |d| {
                let mono = cx.theme().mono_font_family.clone();
                let items = SECTIONS
                    .iter()
                    .map(|&(sec, label)| {
                        let it = item(label, "").on(Rc::new(
                            move |this: &mut Labs, _: &mut Window, cx: &mut Context<Labs>| {
                                this.settings.section = sec;
                                this.settings.nav_menu = false;
                                cx.notify();
                            },
                        ));
                        if sec == s.section { it.current() } else { it }
                    })
                    .collect();
                d.child(
                    div()
                        .absolute()
                        .top(gpui::relative(1.))
                        .left_0()
                        .pt(rems(TIGHT))
                        .occlude()
                        .child(Self::popup(
                            cx,
                            p,
                            mono,
                            Some(MENU_W),
                            "Settings",
                            items,
                            usize::MAX,
                            Some(vec![("escape", "close")]),
                            None,
                        )),
                )
            })
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.settings.nav_menu {
                    this.settings.nav_menu = false;
                    cx.notify();
                }
            }));

        let form = v_flex()
            .flex_1()
            .min_w_0()
            .max_w(rems(FORM_MAX))
            .gap(rems(SECTION))
            .child(
                div()
                    .text_size(rems(TEXT_LG))
                    .font_medium()
                    .text_color(p.text)
                    .child(current),
            )
            .child(self.section_rows(p, stacked, cx));

        let page = v_flex()
            .w_full()
            .max_w(rems(PAGE_MAX))
            .mx_auto()
            .px(rems(INSET))
            .py(rems(SECTION))
            .gap(rems(SECTION));
        let page = if nav_beside {
            page.child(
                h_flex()
                    .items_start()
                    .gap(rems(INSET))
                    .child(nav)
                    .child(form),
            )
        } else {
            page.child(select).child(form)
        };
        Self::column("settings").child(page)
    }

    fn section_rows(&self, p: &Palette, stacked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        let group = v_flex().border_t_1().border_color(p.hairline);
        match s.section {
            Section::Appearance => {
                let theme = segmented(
                    p,
                    "theme",
                    &[("Light", !self.dark), ("Dark", self.dark)],
                    cx,
                    |this, i, window, cx| {
                        this.dark = i == 1;
                        set_mode(this.dark, window, cx);
                    },
                );
                let reading = segmented(
                    p,
                    "reading",
                    &READING.map(|(z, l)| (l, (self.zoom - z).abs() < 0.01)),
                    cx,
                    |this, i, _, _| this.zoom = READING[i].0,
                );
                group
                    .child(form_row(p, "Theme", "Light or dark; the layout is the same in both.", theme, stacked))
                    .child(form_row(
                        p,
                        "Reading size",
                        "Scales the conversation, the composer and documents. Bars, the rail and the terminal keep their size.",
                        reading,
                        stacked,
                    ))
                    .child(form_row(
                        p,
                        "Interface font",
                        "The theme's default. A family that is not installed fails silently, so a font has to ship with the app to be offered here.",
                        action("font").outline().small().label("Default").icon(IconName::ChevronDown),
                        stacked,
                    ))
                    .into_any_element()
            }
            Section::Workspace => group
                .child(form_row(
                    p,
                    "Check command",
                    "Run after a step that changed files, and by Run check on a project with no session.",
                    div().w(rems(FIELD_W)).child(Input::new(&s.check).small()),
                    stacked,
                ))
                .child(form_row(
                    p,
                    "Notify when an agent waits",
                    "A desktop notification when a session asks for permission or an answer while the window is not in front.",
                    Switch::new("notify").cursor_pointer().checked(s.notify).on_click(cx.listener(
                        |this, on: &bool, _, cx| {
                            this.settings.notify = *on;
                            cx.notify();
                        },
                    )),
                    stacked,
                ))
                .child(form_row(
                    p,
                    "Keep finished tasks",
                    "Finished tasks stay on the Tasks page until dismissed, instead of leaving after a day.",
                    Switch::new("keep-finished").cursor_pointer().checked(s.keep_finished).on_click(cx.listener(
                        |this, on: &bool, _, cx| {
                            this.settings.keep_finished = *on;
                            cx.notify();
                        },
                    )),
                    stacked,
                ))
                .into_any_element(),
            Section::Agents => group
                .children(
                    [("Claude Code", "claude-code-acp · found"), ("Mock UI", "node crates/core/examples/mock_ui_agent.js · found")]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (name, about))| {
                            form_row(
                                p,
                                name,
                                about,
                                action(("test-agent", i)).ghost().small().label("Test"),
                                stacked,
                            )
                        }),
                )
                .child(
                    div().pt(rems(RELATED)).child(
                        action("add-agent").outline().small().icon(IconName::Plus).label("Add agent"),
                    ),
                )
                .into_any_element(),
            Section::Connections => group
                .child(form_row(
                    p,
                    "GitHub",
                    "Signed in through gh as jarviisha. Issues and pull requests are read with it.",
                    action("gh").ghost().small().label("Sign out"),
                    stacked,
                ))
                .child(form_row(
                    p,
                    "Telegram",
                    "Not connected. The token is read from ONEHAND_TELEGRAM_TOKEN or a file of its own, never from the settings file.",
                    action("tg").outline().small().label("Connect…"),
                    stacked,
                ))
                .into_any_element(),
            Section::Shortcuts => group
                .children(SHORTCUTS.iter().map(|(what, keys)| {
                    form_row(p, what, "", key(keys), stacked)
                }))
                .into_any_element(),
        }
    }

    /// Ask before removing a project: a dialog naming it, the long name
    /// wrapping in the body, *Keep* first and a solid danger *Delete*.
    pub(super) fn confirm_delete(
        &mut self,
        name: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let p = self.palette();
        let width: Pixels = rems(DIALOG_MAX).to_pixels(window.rem_size());
        window.open_dialog(cx, move |dialog, _, _| {
            let delete = this.clone();
            dialog
                .w(width)
                .title(div().text_size(rems(TEXT_LG)).font_medium().child("Delete project?"))
                .child(
                    v_flex()
                        .gap(rems(RELATED))
                        .text_color(p.text2)
                        .child(
                            div()
                                .text_color(p.text)
                                .font_medium()
                                .whitespace_normal()
                                .child(name),
                        )
                        .child("leaves onehand with its sessions and conversations. Its files stay on disk."),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap(rems(CONTROL))
                        .child(
                            action("keep")
                                .outline()
                                .small()
                                .label("Keep")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            action("delete")
                                .danger()
                                .small()
                                .label("Delete")
                                .on_click(move |_, window, cx| {
                                    delete
                                        .update(cx, |labs, cx| {
                                            labs.removed.push(name);
                                            cx.notify();
                                        })
                                        .ok();
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }
}

/// One settings row: its label and description, and the control at its end,
/// or under them when the form is too narrow to hold both.
fn form_row(
    p: &Palette,
    label: &'static str,
    description: &'static str,
    control: impl IntoElement,
    stacked: bool,
) -> gpui::Div {
    let text = v_flex()
        .flex_1()
        .min_w_0()
        .gap(rems(SUBLINE))
        .child(div().text_color(p.text).child(label))
        .when(!description.is_empty(), |d| {
            d.child(
                div()
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child(description),
            )
        });
    let row = if stacked {
        v_flex()
            .items_start()
            .gap(rems(CONTROL))
            .child(text)
            .child(control)
    } else {
        h_flex()
            .items_center()
            .gap(rems(INSET))
            .child(text)
            .child(div().flex_none().child(control))
    };
    row.py(rems(RELATED)).border_b_1().border_color(p.hairline)
}

/// Two or three short choices; the chosen one takes the `selected` fill.
fn segmented(
    p: &Palette,
    id: &'static str,
    options: &[(&'static str, bool)],
    cx: &mut Context<Labs>,
    pick: impl Fn(&mut Labs, usize, &mut Window, &mut Context<Labs>) + 'static,
) -> gpui::Div {
    let pick = Rc::new(pick);
    h_flex()
        .p(rems(ROW_GAP))
        .gap(rems(ROW_GAP))
        .rounded(rems(RADIUS_SM))
        .border_1()
        .border_color(p.control)
        .children(options.iter().enumerate().map(|(i, (label, on))| {
            let pick = pick.clone();
            action((id, i))
                .ghost()
                .xsmall()
                .label(*label)
                .selected(*on)
                .on_click(cx.listener(move |this, _, window, cx| {
                    pick(this, i, window, cx);
                    cx.notify();
                }))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_the_nav_beside_a_form_and_stack_rows_only_when_narrow() {
        // A wide page: nav beside, rows side by side.
        assert_eq!(settings_layout(60.0), (true, false));
        // Too narrow for nav and a side-by-side form: the nav becomes a select.
        assert!(!settings_layout(40.0).0);
        // Narrow enough that a row cannot hold label and control: rows stack.
        assert_eq!(settings_layout(30.0), (false, true));
    }
}
