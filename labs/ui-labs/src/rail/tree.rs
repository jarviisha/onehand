//! The rail's list: projects with their sessions, or the sessions alone under
//! a flat filter, and the menus their rows open from `⋯` and right-click.
use super::model::{Filter, PROJECTS};
use super::*;
use gpui::{AnyElement, ClipboardItem, WeakEntity};

impl Labs {
    /// The projects left in the workspace, pinned first, each with its index.
    pub(super) fn project_names(&self) -> Vec<(usize, &'static str)> {
        self.projects().map(|i| (i, PROJECTS[i].name)).collect()
    }

    pub(super) fn projects(&self) -> impl Iterator<Item = usize> + '_ {
        let rest = (0..PROJECTS.len()).filter(|i| !self.rail.pinned.contains(i));
        self.rail
            .pinned
            .iter()
            .copied()
            .chain(rest)
            .filter(|i| !self.removed.contains(&PROJECTS[*i].name))
    }

    /// The project folded unless a search is on, which opens every project so
    /// no match hides behind a fold.
    fn is_open(&self, project: usize, searching: bool) -> bool {
        searching || !self.rail.folded.contains(&project)
    }

    /// The sessions in the order the list draws them, which the arrows walk.
    pub(super) fn order(&self, cx: &App) -> Vec<usize> {
        let query = self.rail.search.read(cx).value();
        let shown = model::visible(&self.rail.sessions, self.rail.filter, &query);
        if self.rail.filter != Filter::ByProject {
            return shown;
        }
        let searching = !query.trim().is_empty();
        self.projects()
            .filter(|p| self.is_open(*p, searching))
            .flat_map(|p| {
                shown
                    .iter()
                    .copied()
                    .filter(move |s| self.rail.sessions[*s].project == p)
            })
            .collect()
    }

    pub(super) fn list(
        &self,
        p: &Palette,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let query = self.rail.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let shown = model::visible(&self.rail.sessions, self.rail.filter, &query);
        let mut rows = Vec::new();
        if self.rail.filter == Filter::ByProject {
            let projects: Vec<usize> = self.projects().collect();
            for i in projects {
                let mine: Vec<usize> = shown
                    .iter()
                    .copied()
                    .filter(|s| self.rail.sessions[*s].project == i)
                    .collect();
                if searching && mine.is_empty() && !PROJECTS[i].name.contains(&query) {
                    continue;
                }
                let open = self.is_open(i, searching);
                // Air between projects, so each reads as its own group.
                if !rows.is_empty() {
                    rows.push(div().h_2().into_any_element());
                }
                rows.push(self.project_header(p, i, open, focused, cx));
                if !open {
                    continue;
                }
                if mine.is_empty() {
                    rows.push(note(p, "Empty"));
                }
                for s in mine {
                    rows.push(self.session_row(p, s, false, focused, cx));
                }
            }
        } else {
            for s in shown {
                rows.push(self.session_row(p, s, true, focused, cx));
            }
        }
        if rows.is_empty() {
            rows.push(note(p, "No sessions match"));
        }
        rows.push(self.add_project(p, cx));
        rows
    }

    /// The list's last row: the way to grow it.
    fn add_project(&self, p: &Palette, cx: &App) -> AnyElement {
        h_flex()
            .id("add-project")
            .h(rems(ROW_H))
            .mt_3()
            .px_1p5()
            .gap_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|d| d.bg(p.selected))
            .text_color(p.text2)
            .tooltip(|window, cx| {
                Tooltip::new("Add a project root to this workspace").build(window, cx)
            })
            .child(
                div()
                    .w(rems(DOT_COLUMN))
                    .flex()
                    .justify_center()
                    .child(Icon::new(IconName::Plus).small()),
            )
            .child("Add project…")
            .into_any_element()
    }
}

/// A muted line in the list where rows would be.
fn note(p: &Palette, text: &'static str) -> AnyElement {
    div()
        .h(rems(ROW_H))
        .pl(rems(RAIL_INDENT))
        .flex()
        .items_center()
        .text_xs()
        .text_color(p.muted)
        .child(text)
        .into_any_element()
}

/// A menu row in danger ink, for the one entry that takes something away.
fn danger(p: Palette, label: &'static str, icon: IconName) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| div().text_color(p.danger).child(label))
        .icon(Icon::new(icon).text_color(p.danger))
}

pub(super) fn project_menu(
    menu: PopupMenu,
    i: usize,
    pinned: bool,
    this: WeakEntity<Labs>,
    p: Palette,
) -> PopupMenu {
    let name = PROJECTS[i].name;
    let unattended = PROJECTS[i].auto.is_some();
    let (pin, new, terminal, remove) = (this.clone(), this.clone(), this.clone(), this);
    menu.item(
        PopupMenuItem::new(if pinned { "Unpin" } else { "Pin to top" })
            .icon(IconName::Star)
            .on_click(move |_, _, cx| {
                pin.update(cx, |labs, cx| {
                    if labs.rail.pinned.contains(&i) {
                        labs.rail.pinned.retain(|x| *x != i);
                    } else {
                        labs.rail.pinned.push(i);
                    }
                    cx.notify();
                })
                .ok();
            }),
    )
    .item(
        PopupMenuItem::new("Work labelled issues")
            .icon(IconName::Bot)
            .checked(unattended),
    )
    .item(PopupMenuItem::new("Work an issue…").icon(IconName::Inbox))
    .item(
        PopupMenuItem::new("New session")
            .icon(IconName::Plus)
            .on_click(move |_, _, cx| {
                new.update(cx, |labs, cx| labs.new_session(i, AGENTS[0], cx))
                    .ok();
            }),
    )
    .item(PopupMenuItem::new("New worktree…").icon(Icon::empty().path(crate::assets::GIT_BRANCH)))
    .item(
        PopupMenuItem::new("Open terminal")
            .icon(IconName::SquareTerminal)
            .on_click(move |_, _, cx| {
                terminal
                    .update(cx, |labs, cx| {
                        if !labs.term.open {
                            labs.toggle_terminal(cx);
                        }
                    })
                    .ok();
            }),
    )
    .item(
        PopupMenuItem::new("Copy project path")
            .icon(IconName::Copy)
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(format!("~/work/{name}")));
            }),
    )
    .item(PopupMenuItem::new("Refresh Git status").icon(IconName::Redo))
    .separator()
    .item(
        danger(p, "Remove from workspace", IconName::Delete).on_click(move |_, window, cx| {
            remove
                .update(cx, |labs, cx| labs.confirm_delete(name, window, cx))
                .ok();
        }),
    )
}

pub(super) fn session_menu(
    menu: PopupMenu,
    i: usize,
    this: WeakEntity<Labs>,
    p: Palette,
) -> PopupMenu {
    menu.item(PopupMenuItem::new("Rename…").icon(Icon::empty().path(crate::assets::SQUARE_PEN)))
        .item(PopupMenuItem::new("Restart the agent").icon(IconName::Redo))
        .item(PopupMenuItem::new("Export as Markdown…").icon(IconName::ExternalLink))
        .separator()
        .item(
            danger(p, "Close session", IconName::Close).on_click(move |_, _, cx| {
                this.update(cx, |labs, cx| {
                    labs.drop_sessions(|at, _| at == i);
                    cx.notify();
                })
                .ok();
            }),
        )
}
