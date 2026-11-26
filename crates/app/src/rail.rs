//! The navigation rail.
//!
//! Top to bottom: *New session*; the pages; the sessions under a filter.
//! Only the list scrolls. It hides completely,
//! never to an icon column, and the title bar's toggle brings it back. The
//! search over sessions, projects and commands is the title bar's palette,
//! in reach whether the rail is shown or not.
//!
//! The list is the projects with their sessions, or under a flat filter the
//! sessions alone with the project on each row. Every group lists a capped
//! number of sessions and says how many more there are. **The tree's order is
//! the person's**: a project row and a session row are each dragged to
//! another place in it, projects within their pin group, sessions within
//! their project. A flat list is in an order this app keeps nowhere, so it is
//! not dragged.
//!
//! What a row *is* — its state, its words, its age — is decided in
//! [`model`], without drawing, and the same rows are what the keyboard walks.

use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::shell::Shell;
use crate::tokens::ROW_H;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, div,
};
use gpui::{Rems, rems};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use model::{Filter, Item, Row};
use onehand_ui::mark_slot;
use std::path::PathBuf;

mod model;
mod pages;
mod project;
mod row;
mod search;
mod session;
mod workspace;
pub(crate) use model::needs_attention;
pub(crate) use row::page_fill;
pub(crate) use search::workspace_control;
#[cfg(test)]
pub(crate) use search::workspace_hint;
pub(crate) use session::{session_label, signal_mark, status_icon, word_ink};
pub(crate) use workspace::ellipsize_front;

/// How far a session row is set in under its project: one mark's slot, the
/// fold's, and the gap after it. With the inset every row keeps, a session's
/// mark stands under its project's folder and its title under the project's
/// name.
const INDENT: Rems = rems(1. + 0.5);

/// How often a session's age is said again: the finest unit it is said in.
const AGE_TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// The row the keyboard is on in the list, by what it is.
#[derive(Clone, PartialEq)]
enum Cursor {
    Project(PathBuf),
    Session(u64),
}

/// What the rail keeps between frames. None of it is saved: it is where the
/// person is looking right now.
pub(crate) struct RailState {
    filter: Filter,
    /// The filter the attention chip replaced, which its second click restores.
    filter_before: Filter,
    cursor: Option<Cursor>,
    /// The keys put the cursor where it is, rather than a click: only then is
    /// its ring drawn, as the keyboard's mark is everywhere else.
    keyed: bool,
    /// Groups shown in full past the cap: a project, or `None` for a flat list.
    uncapped: Vec<Option<PathBuf>>,
    /// The list's own focus, so Enter and the arrows reach it and not a field.
    focus: FocusHandle,
    /// *New session*'s focus: a tab stop, so the keyboard reaches the
    /// chevron that opens its menu.
    new_focus: FocusHandle,
    /// Says every session's age again while the rail shows.
    _tick: Task<()>,
}

impl RailState {
    pub(crate) fn new(cx: &mut Context<Shell>) -> Self {
        let _tick = cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(AGE_TICK).await;
                let alive = shell.update(cx, |shell: &mut Shell, cx| {
                    if shell.rail_shown() {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            filter: Filter::ByProject,
            filter_before: Filter::ByProject,
            cursor: None,
            keyed: false,
            uncapped: Vec::new(),
            focus: cx.focus_handle(),
            new_focus: cx.focus_handle().tab_stop(true),
            _tick,
        }
    }

    /// Shows `filter`, remembering the one it replaced for the attention
    /// chip to come back to, unless that was the chip's own.
    fn set_filter(&mut self, filter: Filter) {
        if self.filter != Filter::NeedsAttention {
            self.filter_before = self.filter;
        }
        if filter != Filter::NeedsAttention {
            self.filter_before = filter;
        }
        self.filter = filter;
        // A list shown in full stays so for the filter it was asked under;
        // the next one starts at the cap again.
        self.uncapped.clear();
    }

    /// Whether the caret is in the rail: its list or *New session*.
    /// Whoever takes the rail off screen has to move it first, or the window
    /// is left pointing at an element no frame draws and no shortcut reaches.
    pub(crate) fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx) || self.new_focus.contains_focused(window, cx)
    }
}

/// The sessions and the rows the list draws from them, worked out once per
/// frame and once per key.
struct List {
    items: Vec<Item>,
    rows: Vec<Row>,
    /// Sessions waiting on the person, the workers left out of `items`
    /// counted too.
    waiting: usize,
}

impl List {
    fn of(shell: &Shell, cx: &App) -> Self {
        let workspace = &shell.workspace_window().workspace;
        let roots = &workspace.roots;
        let mut items: Vec<Item> = roots
            .iter()
            .enumerate()
            .flat_map(|(root, project)| {
                project
                    .sessions
                    .iter()
                    .enumerate()
                    .map(move |(session, s)| (root, session, s))
            })
            .map(|(root, session, s)| {
                let row = shell.session_row(s.uid, cx);
                Item {
                    root,
                    session,
                    uid: s.uid,
                    title: session::session_label(row.title.as_deref(), s.title()).to_string(),
                    agent: s.title().to_string(),
                    signal: row.signal,
                    workers: None,
                    since: row.since,
                    age: row.since.elapsed().as_secs() / 60,
                    background: row.background,
                    subagents: row.subagents,
                    in_turn: row.in_turn,
                    lead: crate::control::lead_of(s.uid),
                    waiting: 0,
                    under: None,
                    host: roots[root]
                        .host_of(&s.folder)
                        .map(|(host, _)| host.to_string()),
                    branch: model::branch_mark(&roots[root], &s.folder),
                }
            })
            .collect();
        let waiting = model::attention_count(&items);
        model::nest_workers(&mut items);
        // A project row brought in for workers alone, all drawn under their
        // lead, is theirs, not the person's.
        let emptied = model::emptied(&items);
        let order: Vec<usize> = workspace
            .display_order()
            .into_iter()
            .filter(|root| !(roots[*root].transient && emptied.contains(root)))
            .collect();
        let state = shell.rail_state();
        let rows = model::rows(
            &items,
            &model::Shape {
                order: &order,
                filter: state.filter,
                open: &|root| shell.project_unfolded(&roots[root].path),
                uncapped: &|group| {
                    let path = group.map(|root| roots[root].path.clone());
                    state.uncapped.contains(&path)
                },
            },
        );
        Self {
            items,
            rows,
            waiting,
        }
    }

    /// The sessions in the order the list draws them, as project and place.
    fn order(&self) -> Vec<(usize, usize, u64)> {
        model::order(&self.rows)
            .into_iter()
            .map(|i| (self.items[i].root, self.items[i].session, self.items[i].uid))
            .collect()
    }
}

/// Build the rail for a window.
pub fn rail(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let state = shell.rail_state();
    let focused = state.focus.is_focused(window);
    // The keyboard's row is ringed only while the list holds the keys and the
    // keys put it there; a click marks what it picked by what then shows.
    let keyed = focused && state.keyed;
    let list = List::of(shell, cx);
    let window_state = shell.workspace_window();
    let workspace = &window_state.workspace;
    let target = shell.rail_target(&list, focused);
    let page = shell.page_shown(cx);
    let shown = workspace
        .active_root()
        .and_then(|root| root.active_session.map(|s| (workspace.active_root, s)))
        .filter(|_| !page);
    // A project's page, with or without sessions open, is marked on its row.
    let project_page = workspace
        .active_root()
        .filter(|root| !page && root.active_session().is_none())
        .map(|_| workspace.active_root);
    let order = workspace.display_order();
    let mut rows: Vec<AnyElement> = Vec::with_capacity(list.rows.len());
    for row in &list.rows {
        rows.push(match *row {
            // Air between projects, so each reads as its own group.
            Row::Gap => div().h_2().into_any_element(),
            Row::Project { root, open } => project::project_row(
                window_state,
                root,
                project::Place {
                    at: order.iter().position(|r| *r == root).unwrap_or(root),
                    open,
                    chosen: project_page == Some(root),
                    ring: keyed
                        && state.cursor
                            == Some(Cursor::Project(workspace.roots[root].path.clone())),
                    badge: model::badge(&list.items, root),
                    asks: !shell.removal_losses(root, cx).is_empty(),
                },
                cx,
            ),
            Row::Session { item, place } => {
                let it = &list.items[item];
                let who = match place.names_project() {
                    true => workspace.roots[it.root].label.as_str(),
                    false => it.agent.as_str(),
                };
                session::session_row(
                    it,
                    who,
                    place,
                    session::Mark {
                        shown: shown == Some((it.root, it.session)),
                        at: keyed && state.cursor == Some(Cursor::Session(it.uid)),
                        resend: shell.can_resend(it.uid, cx),
                    },
                    cx,
                )
            }
            Row::Empty => note("No open sessions", true, cx),
            Row::More { group, hidden } => more(
                group.map(|root| workspace.roots[root].path.clone()),
                hidden,
                cx,
            ),
            Row::NoMatch => note("No sessions match", false, cx),
            Row::AddProject => add_project(false, cx),
            Row::AddHostProject => add_project(true, cx),
        });
    }

    div()
        .v_flex()
        .size_full()
        .bg(cx.theme().sidebar)
        .text_size(crate::tokens::CHROME_TEXT)
        .text_color(cx.theme().sidebar_foreground)
        // The first of the rail's rows, a breath under the title bar's
        // workspace so the two do not read as one block.
        .child(
            div()
                .flex_none()
                .px_1p5()
                .pt_3()
                .pb_0p5()
                .child(workspace::new_session_row(
                    shell,
                    window_state,
                    target,
                    &state.new_focus,
                    window,
                    cx,
                )),
        )
        .child(pages::pages(shell, cx))
        .child(search::sessions_header(state, list.waiting, cx))
        .child(
            div()
                .id("rail-list")
                .track_focus(&state.focus)
                .key_context("Rail")
                .on_action(cx.listener(|shell: &mut Shell, _: &RailOpen, window, cx| {
                    shell.rail_open_cursor(window, cx);
                }))
                .on_action(cx.listener(|shell: &mut Shell, _: &RailUp, _, cx| {
                    shell.rail_move(false, cx);
                }))
                .on_action(cx.listener(|shell: &mut Shell, _: &RailDown, _, cx| {
                    shell.rail_move(true, cx);
                }))
                .on_action(cx.listener(|shell: &mut Shell, _: &RailFold, _, cx| {
                    shell.rail_fold_cursor(true, cx);
                }))
                .on_action(cx.listener(|shell: &mut Shell, _: &RailUnfold, _, cx| {
                    shell.rail_fold_cursor(false, cx);
                }))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                // With a row's own inset, a mark's slot is centred on the
                // line the pages' marks stand on.
                .px_1p5()
                .pt_2()
                .pb_3()
                .child(div().v_flex().gap_0p5().children(rows)),
        )
}

/// A muted line in the list where rows would be, on the axis their titles
/// take: under a project's sessions when it stands for them, at the list's
/// edge otherwise.
fn note(text: &'static str, in_project: bool, cx: &App) -> AnyElement {
    div()
        .h_flex()
        .items_center()
        .h(ROW_H)
        .when(in_project, |d| d.ml(INDENT))
        .px_2()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(mark_slot(div()))
        .child(text)
        .into_any_element()
}

/// The row a cap leaves: how many it left out, and the way to show them.
fn more(group: Option<PathBuf>, hidden: usize, cx: &mut Context<Shell>) -> AnyElement {
    let id = SharedString::from(match &group {
        Some(path) => format!("rail-more-{}", path.display()),
        None => "rail-more".into(),
    });
    div()
        .h_flex()
        .items_center()
        .h(ROW_H)
        .when(group.is_some(), |d| d.ml(INDENT))
        .px_2()
        .gap_2()
        .child(mark_slot(div()))
        .child(
            crate::controls::action(gpui::ElementId::Name(id))
                .quiet(cx)
                .short()
                .label(format!("{hidden} more"))
                .text_color(cx.theme().muted_foreground)
                .on_click(cx.listener(move |shell: &mut Shell, _, _, cx| {
                    shell.rail_state_mut().uncapped.push(group.clone());
                    cx.notify();
                })),
        )
        .into_any_element()
}

/// The list's last two rows: the way to grow it, with a folder here or, with
/// `on_host`, with one on a host. Set apart from the list above by the first.
fn add_project(on_host: bool, cx: &mut Context<Shell>) -> AnyElement {
    let hover = row::hover_fill(cx);
    let (id, label, tip) = match on_host {
        false => (
            "rail-add-project",
            "Add project…",
            "Add a project to this workspace",
        ),
        true => (
            "rail-add-host-project",
            "Add a project on a host…",
            "Add a project whose folder is on a host",
        ),
    };
    div()
        .id(id)
        .h_flex()
        .items_center()
        .h(ROW_H)
        .when(!on_host, |row| row.mt_3())
        .px_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .hover(move |d| d.bg(hover))
        .text_color(cx.theme().muted_foreground)
        .tooltip(onehand_ui::tip(move |window, cx| {
            Tooltip::new(tip).build(window, cx)
        }))
        .on_click(
            cx.listener(move |shell: &mut Shell, _, window, cx| match on_host {
                false => shell.add_root(cx),
                true => shell.begin_host_project(window, cx),
            }),
        )
        .child(mark_slot(Icon::new(IconName::Plus).small()))
        .child(label)
        .into_any_element()
}

gpui::actions!(
    rail,
    [
        RailUp,
        RailDown,
        FocusRailSearch,
        RailOpen,
        RailFold,
        RailUnfold,
        RailNext,
        RailPrev
    ]
);

impl Shell {
    /// Where *New session* starts: the keyboard's project while the list has the
    /// keyboard, else the one on screen; a worker's is its lead's.
    fn rail_target(&self, list: &List, focused: bool) -> Option<usize> {
        let workspace = &self.workspace_window().workspace;
        match self.rail_state().cursor.as_ref().filter(|_| focused) {
            Some(Cursor::Project(path)) => workspace.roots.iter().position(|r| &r.path == path),
            Some(Cursor::Session(uid)) => list
                .items
                .iter()
                .any(|item| item.uid == *uid)
                .then(|| self.home_of(*uid))
                .flatten(),
            None => None,
        }
        .or_else(|| {
            workspace.active_root()?;
            self.active_session_uid()
                .and_then(|uid| self.home_of(uid))
                .or(Some(workspace.active_root))
        })
    }

    /// Show a session from its row, and put the rail's keyboard row there,
    /// `keyed` when the keys asked for it rather than a click.
    /// The caret goes where showing a session puts it, the composer: whoever
    /// picks a session wants to type to it.
    fn rail_open_session(
        &mut self,
        root: usize,
        session: usize,
        keyed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uid = self.workspace_window().workspace.roots[root].sessions[session].uid;
        self.select_root_session(root, session, window, cx);
        let rail = self.rail_state_mut();
        rail.cursor = Some(Cursor::Session(uid));
        rail.keyed = keyed;
    }

    /// A project row's click: the project's page shows, over whatever
    /// sessions it has open, and the keyboard's row goes there. Going to a
    /// project opens it; only its chevron folds it.
    fn rail_click_project(&mut self, root: usize, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.workspace_window().workspace.roots[root].path.clone();
        self.show_project_page(root, window, cx);
        let rail = self.rail_state_mut();
        rail.cursor = Some(Cursor::Project(path));
        rail.keyed = false;
    }

    /// The person folding a project.
    fn rail_fold(&mut self, path: PathBuf, fold: bool, cx: &mut Context<Self>) {
        if self.project_unfolded(&path) == fold {
            self.toggle_fold(path, cx);
        }
        cx.notify();
    }

    fn rail_open_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.rail_state().cursor.clone() {
            Some(Cursor::Session(uid)) => {
                if let Some((root, session)) = self.find_session(uid) {
                    self.rail_open_session(root, session, true, window, cx);
                }
            }
            Some(Cursor::Project(path)) => {
                let fold = self.project_unfolded(&path);
                self.rail_fold(path, fold, cx);
                self.rail_state_mut().keyed = true;
            }
            None => {}
        }
    }

    /// Folds or unfolds the cursor's project; folding from a session moves
    /// the cursor up to its project, so it stays on a row that is drawn.
    fn rail_fold_cursor(&mut self, fold: bool, cx: &mut Context<Self>) {
        let roots = &self.workspace_window().workspace.roots;
        let path = match self.rail_state().cursor.clone() {
            Some(Cursor::Project(path)) => path,
            // A worker folds with its lead's project, which it is drawn in.
            Some(Cursor::Session(uid)) => match self.home_of(uid) {
                Some(root) => roots[root].path.clone(),
                None => return,
            },
            None => return,
        };
        self.rail_fold(path.clone(), fold, cx);
        let rail = self.rail_state_mut();
        rail.keyed = true;
        if fold {
            rail.cursor = Some(Cursor::Project(path));
        }
    }

    /// Moves the keyboard's row to the project or session drawn above or
    /// below it, from the first or last when it is on none.
    fn rail_move(&mut self, down: bool, cx: &mut Context<Self>) {
        let list = List::of(self, cx);
        let roots = &self.workspace_window().workspace.roots;
        let rows: Vec<Cursor> = list
            .rows
            .iter()
            .filter_map(|row| match *row {
                Row::Project { root, .. } => Some(Cursor::Project(roots[root].path.clone())),
                Row::Session { item, .. } => Some(Cursor::Session(list.items[item].uid)),
                Row::Gap
                | Row::Empty
                | Row::More { .. }
                | Row::NoMatch
                | Row::AddProject
                | Row::AddHostProject => None,
            })
            .collect();
        let at = self
            .rail_state()
            .cursor
            .as_ref()
            .and_then(|cursor| rows.iter().position(|row| row == cursor));
        let next = match (at, down) {
            (Some(at), true) => rows.get(at + 1).or(rows.last()),
            (Some(at), false) => rows.get(at.saturating_sub(1)),
            (None, true) => rows.first(),
            (None, false) => rows.last(),
        };
        if let Some(next) = next.cloned() {
            let rail = self.rail_state_mut();
            rail.cursor = Some(next);
            rail.keyed = true;
            cx.notify();
        }
    }

    fn find_session(&self, uid: u64) -> Option<(usize, usize)> {
        let roots = &self.workspace_window().workspace.roots;
        roots.iter().enumerate().find_map(|(root, project)| {
            let session = project.sessions.iter().position(|s| s.uid == uid)?;
            Some((root, session))
        })
    }

    /// Shows the session before or after the current one in the rail's order.
    /// The keyboard's row counts only while the list has the keyboard: it
    /// stays where the rail last left it, while Ctrl+Tab and the number keys
    /// move what is on screen.
    pub(crate) fn rail_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let order = List::of(self, cx).order();
        let focused = self.rail_state().focus.is_focused(window);
        let from = match self.rail_state().cursor.as_ref().filter(|_| focused) {
            Some(Cursor::Session(uid)) => Some(*uid),
            Some(Cursor::Project(_)) | None => self.active_session_uid(),
        };
        // From a project's page, the steps start at that project: forward to
        // its first session, back to the one above it.
        let page = self.workspace_window().workspace.active_root;
        let page_at = from
            .is_none()
            .then(|| order.iter().position(|o| o.0 == page))
            .flatten();
        let next = match (
            from.and_then(|uid| order.iter().position(|o| o.2 == uid)),
            page_at,
        ) {
            (Some(at), _) if forward => order.get(at + 1).or(order.last()),
            (Some(at), _) => order.get(at.saturating_sub(1)),
            (None, Some(at)) if forward => order.get(at),
            (None, Some(at)) => order.get(at.saturating_sub(1)),
            (None, None) => order.first(),
        };
        if let Some(&(root, session, _)) = next {
            self.rail_open_session(root, session, true, window, cx);
        }
    }
}

#[cfg(test)]
mod tests;

/// A name as a menu row or the workspace control cuts it at a character.
#[cfg(test)]
pub(crate) fn cut_at_count(name: &str) -> gpui::SharedString {
    row::ellipsize(name, row::MAX_LABEL)
}
