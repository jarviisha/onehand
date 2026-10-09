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

    /// A search is on; it opens every project so no match hides behind a
    /// fold, and folding waits until it is cleared.
    pub(super) fn searching(&self, cx: &App) -> bool {
        !self.rail.search.read(cx).value().trim().is_empty()
    }

    /// The list as it is drawn, before it is drawn: one source for the
    /// elements and for the order the arrows walk, so the two never differ.
    pub(super) fn list_rows(&self, cx: &App) -> Vec<Row> {
        let query = self.rail.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let shown = model::visible(&self.rail.sessions, self.rail.filter, &query);
        let mut rows = Vec::new();
        if self.rail.filter != Filter::ByProject {
            self.capped(&mut rows, None, shown, true);
        } else {
            for i in self.projects() {
                let mine: Vec<usize> = shown
                    .iter()
                    .copied()
                    .filter(|s| self.rail.sessions[*s].project == i)
                    .collect();
                if searching && mine.is_empty() && !PROJECTS[i].name.to_lowercase().contains(&query)
                {
                    continue;
                }
                let open = searching || !self.rail.folded.contains(&i);
                if !rows.is_empty() {
                    rows.push(Row::Gap);
                }
                rows.push(Row::Project { project: i, open });
                if !open {
                    continue;
                }
                if mine.is_empty() {
                    rows.push(Row::Empty);
                }
                self.capped(&mut rows, Some(i), mine, false);
            }
        }
        if rows.is_empty() {
            rows.push(Row::NoMatch);
        }
        rows
    }

    /// A group's sessions up to `SESSION_CAP`, and a row saying how many the
    /// cap left out, until the person asks for all of them.
    fn capped(&self, rows: &mut Vec<Row>, group: Option<usize>, sessions: Vec<usize>, flat: bool) {
        let cap = if self.rail.uncapped.contains(&group) {
            sessions.len()
        } else {
            SESSION_CAP
        };
        let hidden = sessions.len().saturating_sub(cap);
        rows.extend(
            sessions
                .into_iter()
                .take(cap)
                .map(|session| Row::Session { session, flat }),
        );
        if hidden > 0 {
            rows.push(Row::More { group, hidden });
        }
    }

    /// The sessions in the order the list draws them, which the arrows walk.
    pub(super) fn order(&self, cx: &App) -> Vec<usize> {
        self.list_rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                Row::Session { session, .. } => Some(session),
                Row::Gap | Row::Project { .. } | Row::Empty | Row::More { .. } | Row::NoMatch => {
                    None
                }
            })
            .collect()
    }

    pub(super) fn list(
        &self,
        p: &Palette,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        for row in self.list_rows(cx) {
            out.push(match row {
                // Air between projects, so each reads as its own group.
                Row::Gap => div().h_2().into_any_element(),
                Row::Project { project, open } => {
                    self.project_header(p, project, open, focused, cx)
                }
                Row::Session { session, flat } => self.session_row(p, session, flat, focused, cx),
                Row::Empty => note(p, "Empty"),
                Row::More { group, hidden } => more(p, group, hidden, cx),
                Row::NoMatch => note(p, "No sessions match"),
            });
        }
        out.push(self.add_project(p, cx));
        out
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

/// One row of the list, before it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Row {
    Gap,
    Project {
        project: usize,
        open: bool,
    },
    Session {
        session: usize,
        flat: bool,
    },
    /// An open project with no sessions.
    Empty,
    /// What the cap left out of a project's sessions, or of a flat list's.
    More {
        group: Option<usize>,
        hidden: usize,
    },
    NoMatch,
}

/// The row a cap leaves: how many it left out, and the way to show them.
fn more(p: &Palette, group: Option<usize>, hidden: usize, cx: &mut Context<Labs>) -> AnyElement {
    h_flex()
        .id(("more", group.map_or(0, |g| g + 1)))
        .h(rems(ROW_H))
        .when(group.is_some(), |d| d.pl(rems(RAIL_INDENT)))
        .when(group.is_none(), |d| d.pl_1p5())
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .hover(|d| d.bg(p.selected))
        .text_xs()
        .text_color(p.text2)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.rail.uncapped.push(group);
            cx.notify();
        }))
        .child(format!("{hidden} more"))
        .into_any_element()
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
                    labs.close_session(i, cx);
                })
                .ok();
            }),
        )
}
