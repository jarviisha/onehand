//! The composer's popups: what each one lists and does, and where it opens.
//! Kept apart from drawing so Enter can take the same row the pointer would.
use super::composer::{Item, item};
use super::live::*;
use super::*;
use gpui::AnyElement;

pub(super) struct MenuSpec {
    pub header: &'static str,
    pub items: Vec<Item>,
    pub width: Option<f32>,
    pub right: bool,
    /// A completion: Enter inserts rather than chooses.
    pub footer: bool,
    pub empty: Option<String>,
}

impl Labs {
    /// What a popup lists and how it sits, apart from drawing it, so Enter
    /// can take the same row the pointer would.
    pub(super) fn menu_spec(&self, menu: Menu) -> MenuSpec {
        let live = &self.live;
        let (header, items, width, right, footer, empty): (
            &'static str,
            Vec<Item>,
            Option<f32>,
            bool,
            bool,
            Option<String>,
        ) = match menu {
            Menu::Plus => (
                "Add to the prompt",
                vec![
                    item("Attach files…", "")
                        .icon(IconName::Plus)
                        .on(act(|this, _, cx| {
                            let next = this.live.tray.len() % ATTACHABLE.len();
                            this.live.tray.push(next);
                            this.live.open = None;
                            cx.notify();
                        })),
                    item("Mention a file", "@")
                        .icon(IconName::File)
                        .on(act(|this, w, cx| {
                            let text = this.text(cx);
                            let sep = if text.is_empty() || text.ends_with(' ') {
                                ""
                            } else {
                                " "
                            };
                            this.set_text(&format!("{text}{sep}@"), w, cx);
                            this.focus_field(w, cx);
                        })),
                    item("Run a command", "/")
                        .icon(IconName::SquareTerminal)
                        .on(act(|this, w, cx| {
                            this.set_text("/", w, cx);
                            this.focus_field(w, cx);
                        })),
                    item("Run a workflow…", "")
                        .icon(IconName::GalleryVerticalEnd)
                        .on(act(|this, _, cx| {
                            this.live.open = None;
                            this.live.said.push(Said::Notice(
                                "The workflow launcher is not drawn in the lab yet.",
                            ));
                            cx.notify();
                        })),
                ],
                Some(MENU_W),
                false,
                false,
                None,
            ),
            Menu::Model => {
                let mut items: Vec<Item> = MODELS
                    .iter()
                    .enumerate()
                    .map(|(i, (name, note))| {
                        let it = item(name, note).on(act(move |this, _, cx| {
                            this.live.model = i;
                            this.live.open = None;
                            cx.notify();
                        }));
                        let it = if i == 0 { it.group("Model") } else { it };
                        if i == live.model { it.current() } else { it }
                    })
                    .collect();
                items.extend(EFFORTS.iter().enumerate().map(|(i, name)| {
                    let it = item(name, "").on(act(move |this, _, cx| {
                        this.live.effort = i;
                        this.live.open = None;
                        cx.notify();
                    }));
                    let it = if i == 0 { it.group("Effort") } else { it };
                    if i == live.effort { it.current() } else { it }
                }));
                ("Settings", items, Some(MENU_WIDE_W), false, false, None)
            }
            Menu::Mode => (
                "Mode",
                MODES
                    .iter()
                    .enumerate()
                    .map(|(i, (name, note))| {
                        let it = item(name, note).on(act(move |this, _, cx| {
                            this.live.mode = i;
                            this.live.open = None;
                            cx.notify();
                        }));
                        if i == live.mode { it.current() } else { it }
                    })
                    .collect(),
                Some(MENU_WIDE_W),
                true,
                false,
                None,
            ),
            Menu::Branch => (
                "Branch",
                BRANCHES
                    .iter()
                    .enumerate()
                    .map(|(i, (name, note))| {
                        let it = item(name, note).mono().on(act(move |this, _, cx| {
                            this.live.branch = i;
                            this.live.open = None;
                            cx.notify();
                        }));
                        if i == live.branch { it.current() } else { it }
                    })
                    .collect(),
                Some(MENU_W),
                false,
                false,
                None,
            ),
            Menu::Mention | Menu::Command => {
                let mention = menu == Menu::Mention;
                let items: Vec<Item> = self
                    .matches()
                    .into_iter()
                    .map(|(name, note)| {
                        let it = item(name, note)
                            .mono()
                            .on(act(move |this, w, cx| this.complete(name, w, cx)));
                        if mention { it.icon(IconName::File) } else { it }
                    })
                    .collect();
                let empty = items
                    .is_empty()
                    .then(|| format!("No matches for \u{201c}{}\u{201d}", live.query));
                (
                    if mention {
                        "Mention a file"
                    } else {
                        "Run a command"
                    },
                    items,
                    None,
                    false,
                    true,
                    empty,
                )
            }
        };
        MenuSpec {
            header,
            items,
            width,
            right,
            footer,
            empty,
        }
    }

    /// The open popup, positioned over the field: completions span the stack,
    /// menus sit over the control that opened them.
    pub(super) fn live_popup(
        &self,
        menu: Menu,
        p: &Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let MenuSpec {
            header,
            items,
            width,
            right,
            footer,
            empty,
        } = self.menu_spec(menu);
        let list = Self::popup(
            cx,
            p,
            mono,
            width,
            header,
            items,
            self.live.highlight,
            Some(if footer {
                vec![("up", "move"), ("enter", "insert"), ("escape", "close")]
            } else {
                vec![("up", "move"), ("enter", "choose"), ("escape", "close")]
            }),
            empty,
        );
        // A menu starts at the card's inset, under `+`, the model chip or the
        // branch on the left, or at the right for the mode. The model chip sits
        // past Fast, whose word changes width, so its menu starts at the inset
        // too. A completion spans the stack.
        let left = match menu {
            Menu::Plus | Menu::Model | Menu::Branch => COMPOSER_PAD,
            Menu::Mode | Menu::Mention | Menu::Command => 0.0,
        };
        div()
            .absolute()
            .bottom(gpui::relative(1.))
            .left_0()
            .right_0()
            .pb(rems(CONTROL))
            .flex()
            .pl(rems(left))
            .when(right, |d| d.justify_end().pr(rems(COMPOSER_PAD)))
            .occlude()
            .child(list)
            .into_any_element()
    }
}
