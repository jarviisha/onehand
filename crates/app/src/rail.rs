//! The navigation rail.
//!
//! Top to bottom: the workspace and its menu; a search over sessions and
//! projects beside *New*; the pages; the sessions under a filter; and a
//! one-row foot with Settings. Only the list scrolls. It hides completely,
//! never to an icon column.
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

use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement, IntoElement, ParentElement, Rems, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, Sizable as _, StyledExt};
use model::{Filter, Item, Row};
use std::path::PathBuf;

mod model;
mod project;
mod row;
mod search;
mod session;
mod workspace;
pub use project::{pick_item, unattended_item};
pub(crate) use session::{signal_mark, signal_word};
pub(crate) use workspace::ellipsize_front;

/// The workspace bar and the foot: the height of the conversation's header
/// beside them, so the bars line up across the window.
const BAR_H: Rems = rems(2.75);

/// A single-line rail row: a page, a project, a note.
const ROW_H: Rems = rems(1.875);

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
    /// Groups shown in full past the cap: a project, or `None` for a flat list.
    uncapped: Vec<Option<PathBuf>>,
    search: Entity<InputState>,
    /// The list's own focus, so Enter and the arrows reach it and not a field.
    focus: FocusHandle,
    _search: Subscription,
    /// Says every session's age again while the rail shows.
    _tick: Task<()>,
}

impl RailState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
        let _search = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
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
            uncapped: Vec::new(),
            search,
            focus: cx.focus_handle(),
            _search,
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
    }

    fn searching(&self, cx: &App) -> bool {
        !self.search.read(cx).value().trim().is_empty()
    }
}

/// The sessions and the rows the list draws from them, worked out once per
/// frame and once per key.
struct List {
    items: Vec<Item>,
    rows: Vec<Row>,
}

impl List {
    fn of(shell: &Shell, cx: &App) -> Self {
        let workspace = &shell.workspace_window().workspace;
        let roots = &workspace.roots;
        let items: Vec<Item> = roots
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
                    since: row.since,
                    age: row.since.elapsed().as_secs() / 60,
                }
            })
            .collect();
        let names: Vec<&str> = roots.iter().map(|root| root.label.as_str()).collect();
        let order = workspace.display_order();
        let state = shell.rail_state();
        let query = state.search.read(cx).value().to_string();
        let rows = model::rows(
            &items,
            &model::Shape {
                projects: &names,
                order: &order,
                filter: state.filter,
                query: &query,
                open: &|root| shell.project_unfolded(&roots[root].path),
                uncapped: &|group| {
                    let path = group.map(|root| roots[root].path.clone());
                    state.uncapped.contains(&path)
                },
            },
        );
        Self { items, rows }
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
    let list = List::of(shell, cx);
    let window_state = shell.workspace_window();
    let workspace = &window_state.workspace;
    let target = shell.rail_target(&list, focused);
    let page = shell.page_shown(cx);
    let shown = workspace
        .active_root()
        .map(|root| (workspace.active_root, root.active_session))
        .filter(|_| !page);
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
                    cursor: focused
                        && state.cursor
                            == Some(Cursor::Project(workspace.roots[root].path.clone())),
                    badge: model::badge(&list.items, root),
                },
                cx,
            ),
            Row::Session { item, flat } => {
                let it = &list.items[item];
                let who = match flat {
                    true => workspace.roots[it.root].label.as_str(),
                    false => it.agent.as_str(),
                };
                session::session_row(
                    it,
                    who,
                    flat,
                    session::Mark {
                        shown: shown == Some((it.root, it.session)),
                        at: focused && state.cursor == Some(Cursor::Session(it.uid)),
                        resend: shell.can_resend(it.uid, cx),
                    },
                    cx,
                )
            }
            Row::Empty => note("Empty", cx),
            Row::More { group, hidden } => more(
                group.map(|root| workspace.roots[root].path.clone()),
                hidden,
                cx,
            ),
            Row::NoMatch => note("No sessions match", cx),
            Row::AddProject => add_project(cx),
        });
    }

    div()
        .v_flex()
        .size_full()
        .bg(cx.theme().muted)
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(workspace::workspace_bar(
            window_state.workspace.name.clone().into(),
            window_state.workspace.storage_dir.clone(),
            shell.recents(cx),
            cx,
        ))
        .child(
            div()
                .h_flex()
                .items_center()
                .flex_none()
                .px_3()
                .pt_1()
                .pb_3()
                .gap_1p5()
                .child(search::search_field(state, cx))
                .child(workspace::new_button(shell, window_state, target, cx)),
        )
        .child(pages(shell, cx))
        .child(search::sessions_header(
            state,
            model::attention_count(&list.items),
            cx,
        ))
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
                .v_flex()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_2()
                .pt_2()
                .pb_3()
                .gap_0p5()
                .children(rows),
        )
        .child(foot(shell, cx))
}

/// Overview, Tasks with how many tasks need a person, Issues and Workflows.
fn pages(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let tasks = attention_pill(crate::task::attention(&shell.page_roots(), cx));
    div()
        .v_flex()
        .flex_none()
        .px_2()
        .pb_3()
        .gap_0p5()
        .child(page_row(
            ("rail-overview", "Overview"),
            Icon::new(IconName::LayoutDashboard),
            shell.workspace_shown(cx),
            None,
            "What is waiting, working and open across every project",
            cx,
            |shell, window, cx| shell.show_workspace(window, cx),
        ))
        .child(page_row(
            ("rail-tasks", "Tasks"),
            Icon::new(IconName::Inbox),
            shell.tasks_shown(cx),
            tasks,
            "Every task, and what each needs",
            cx,
            |shell, window, cx| shell.show_tasks(None, window, cx),
        ))
        .child(page_row(
            ("rail-issues", "Issues"),
            Icon::new(crate::icons::Icon::CircleDot),
            shell.issues_shown(cx),
            None,
            "Every issue of every project, and where its work stands",
            cx,
            |shell, window, cx| shell.show_issues(window, cx),
        ))
        .child(page_row(
            ("rail-workflows", "Workflows"),
            Icon::new(IconName::Play),
            shell.workflows_shown(cx),
            None,
            "Every workflow, to run one or write one",
            cx,
            |shell, window, cx| shell.show_workflows(window, cx),
        ))
}

/// A page's row, on the selected fill in full ink while its page shows.
fn page_row<F: Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static>(
    (id, label): (&'static str, &'static str),
    icon: Icon,
    on: bool,
    count: Option<String>,
    tip: &'static str,
    cx: &mut Context<Shell>,
    go: F,
) -> impl IntoElement + use<F> {
    let hover = row::hover_fill(cx);
    div()
        .id(id)
        .h_flex()
        .items_center()
        .h(ROW_H)
        .px_1p5()
        .gap_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .when(on, |d| {
            d.bg(row::chosen_fill(cx))
                .text_color(cx.theme().accent_foreground)
        })
        .when(!on, |d| {
            d.hover(move |d| d.bg(hover))
                .text_color(cx.theme().muted_foreground)
        })
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| go(shell, window, cx)),
        )
        .child(div().w_4().flex().justify_center().child(icon.small()))
        .child(div().flex_1().child(label))
        .children(count.map(|n| {
            div()
                .px_1()
                .rounded(cx.theme().radius)
                .bg(cx.theme().secondary)
                .text_xs()
                .text_color(cx.theme().secondary_foreground)
                .child(n)
        }))
}

/// What the Tasks row's pill reads for `n` tasks needing a person: nothing
/// at zero, so a pill is always news.
fn attention_pill(n: usize) -> Option<String> {
    (n > 0).then(|| n.to_string())
}

/// One row: Settings on the right, marked while it shows.
fn foot(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .h(BAR_H)
        .px_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().flex_1())
        .child(
            crate::controls::action("open-settings")
                .ghost()
                .small()
                .icon(Icon::new(IconName::Settings).text_color(cx.theme().muted_foreground))
                .tooltip("Appearance, the workspace, agents and connections")
                .selected(shell.settings_shown())
                .on_click(
                    cx.listener(|shell: &mut Shell, _, window, cx| shell.open_settings(window, cx)),
                ),
        )
}

/// A muted line in the list where rows would be.
fn note(text: &'static str, cx: &App) -> AnyElement {
    div()
        .h_flex()
        .items_center()
        .h(ROW_H)
        .pl_7()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
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
        .when(group.is_some(), |d| d.pl_7())
        .when(group.is_none(), |d| d.pl_1p5())
        .child(
            crate::controls::action(gpui::ElementId::Name(id))
                .ghost()
                .xsmall()
                .label(format!("{hidden} more"))
                .text_color(cx.theme().muted_foreground)
                .on_click(cx.listener(move |shell: &mut Shell, _, _, cx| {
                    shell.rail_state_mut().uncapped.push(group.clone());
                    cx.notify();
                })),
        )
        .into_any_element()
}

/// The list's last row: the way to grow it.
fn add_project(cx: &mut Context<Shell>) -> AnyElement {
    let hover = row::hover_fill(cx);
    div()
        .id("rail-add-project")
        .h_flex()
        .items_center()
        .h(ROW_H)
        .mt_3()
        .px_1p5()
        .gap_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .hover(move |d| d.bg(hover))
        .text_color(cx.theme().muted_foreground)
        .tooltip(|window, cx| Tooltip::new("Add a project to this workspace").build(window, cx))
        .on_click(cx.listener(|shell: &mut Shell, _, _, cx| shell.add_root(cx)))
        .child(
            div()
                .w_4()
                .flex()
                .justify_center()
                .child(Icon::new(IconName::Plus).small()),
        )
        .child("Add project…")
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
    /// Where *New* starts: the keyboard's project while the list has the
    /// keyboard, else the one on screen.
    fn rail_target(&self, list: &List, focused: bool) -> Option<usize> {
        let workspace = &self.workspace_window().workspace;
        match self.rail_state().cursor.as_ref().filter(|_| focused) {
            Some(Cursor::Project(path)) => workspace.roots.iter().position(|r| &r.path == path),
            Some(Cursor::Session(uid)) => list
                .items
                .iter()
                .find(|item| item.uid == *uid)
                .map(|item| item.root),
            None => None,
        }
        .or_else(|| workspace.active_root().map(|_| workspace.active_root))
    }

    /// Show a session from its row, and put the rail's keyboard row there.
    /// The caret goes where showing a session puts it, the composer: whoever
    /// picks a session wants to type to it.
    fn rail_open_session(
        &mut self,
        root: usize,
        session: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uid = self.workspace_window().workspace.roots[root].sessions[session].uid;
        self.select_root_session(root, session, window, cx);
        self.rail_state_mut().cursor = Some(Cursor::Session(uid));
    }

    /// A project row's click: the project's last session shows, or its page
    /// when it has none, and the keyboard's row goes there. Going to a project
    /// opens it; only its chevron folds it.
    fn rail_click_project(&mut self, root: usize, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.workspace_window().workspace.roots[root].path.clone();
        self.select_root(root, window, cx);
        self.rail_state_mut().cursor = Some(Cursor::Project(path));
    }

    /// The person folding a project. While a search is on every project
    /// stays open, so a fold would change nothing on screen and then land
    /// unasked when the search clears; it waits instead.
    fn rail_fold(&mut self, path: PathBuf, fold: bool, cx: &mut Context<Self>) {
        if !self.rail_state().searching(cx) && self.project_unfolded(&path) == fold {
            self.toggle_fold(path, cx);
        }
        cx.notify();
    }

    fn rail_open_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.rail_state().cursor.clone() {
            Some(Cursor::Session(uid)) => {
                if let Some((root, session)) = self.find_session(uid) {
                    self.rail_open_session(root, session, window, cx);
                }
            }
            Some(Cursor::Project(path)) => {
                let fold = self.project_unfolded(&path);
                self.rail_fold(path, fold, cx);
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
            Some(Cursor::Session(uid)) => match self.find_session(uid) {
                Some((root, _)) => roots[root].path.clone(),
                None => return,
            },
            None => return,
        };
        self.rail_fold(path.clone(), fold, cx);
        if fold {
            self.rail_state_mut().cursor = Some(Cursor::Project(path));
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
                Row::Gap | Row::Empty | Row::More { .. } | Row::NoMatch | Row::AddProject => None,
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
            self.rail_state_mut().cursor = Some(next);
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
    pub(crate) fn rail_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let order = List::of(self, cx).order();
        let from = match &self.rail_state().cursor {
            Some(Cursor::Session(uid)) => Some(*uid),
            Some(Cursor::Project(_)) | None => self.active_session_uid(),
        };
        let next = match from.and_then(|uid| order.iter().position(|o| o.2 == uid)) {
            Some(at) if forward => order.get(at + 1).or(order.last()),
            Some(at) => order.get(at.saturating_sub(1)),
            None => order.first(),
        };
        if let Some(&(root, session, _)) = next {
            self.rail_open_session(root, session, window, cx);
        }
    }

    /// Brings the rail back if it is hidden, and puts the caret in its search.
    pub(crate) fn focus_rail_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_rail(cx);
        let search = self.rail_state().search.clone();
        search.read(cx).focus_handle(cx).focus(window, cx);
    }
}

#[cfg(test)]
mod tests;
