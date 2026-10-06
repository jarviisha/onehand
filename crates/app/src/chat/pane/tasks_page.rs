use super::workspace_page::{ALL_PROJECTS, PAGE_WIDE, PageProject, card_box, page_card};
use super::{ChatPane, ChatPaneEvent, Page, rel_time};
use crate::task::Row;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::now_secs;
use onehand_core::diff::Row as DiffRow;
use onehand_core::task::Group;
use onehand_core::task::marks::{self, Change};
use onehand_core::workflow::{Run, Visit};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How many rows each card of the Tasks page draws. Finished tasks are kept
/// by the hundred, and nobody reads that far down a card.
const TASKS_SHOWN: usize = 50;

/// How many visits a run's timeline draws, its newest ones.
const VISITS_SHOWN: usize = 100;

/// How many changed files a visit lists.
const FILES_SHOWN: usize = 100;

/// How many lines of a visit's output, or of an answer awaiting approval,
/// are drawn: their last ones, where an output says how it ended.
const OUTPUT_LINES: usize = 60;

/// How many lines of one file's diff are drawn.
const DIFF_LINES: usize = 400;

/// How many earlier runs a task's detail lists, its newest ones.
const EARLIER_SHOWN: usize = 20;

/// Something read off the UI thread: `None` while it is being read.
type Loaded<T> = Option<Result<T, String>>;

/// Where a run's marks are read: its worktree, then its project.
type Dirs = (PathBuf, PathBuf);

/// The task a detail shows, and what of it is opened.
pub(super) struct TaskDetail {
    id: String,
    /// Visits opened, by run id and visit id.
    expanded: HashSet<(String, u32)>,
    /// Earlier runs opened, by id.
    earlier_open: HashSet<String>,
    /// What changed between two marks, once asked for.
    changes: HashMap<(String, String), Loaded<Vec<Change>>>,
    /// Files opened, by the two marks and the path, with their diffs.
    files: HashMap<(String, String, String), Loaded<Vec<DiffRow>>>,
}

/// The page that lists every task of the window's projects, by what each
/// needs: a person, its turn, its place, or nothing any more.
pub(super) struct TasksPage {
    /// Every project in rail order.
    pub(super) projects: Vec<PageProject>,
    /// The project the page is narrowed to, or `None` for all of them.
    pub(super) filter: Option<PathBuf>,
    /// The task shown in place of the cards, if one is opened.
    pub(super) open: Option<TaskDetail>,
}

impl TasksPage {
    /// What `root` is called here, or its folder's name.
    fn label_of(&self, root: &Path) -> String {
        self.projects
            .iter()
            .find(|project| project.root == root)
            .map(|project| project.label.to_string())
            .or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    }
}

impl ChatPane {
    /// Show the Tasks page, narrowed to `filter` when it is one of
    /// `projects`. Leaves the shown session the way the workspace page does.
    pub fn show_tasks(
        &mut self,
        projects: Vec<PageProject>,
        filter: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let filter = filter.filter(|only| projects.iter().any(|project| &project.root == only));
        self.page = Some(Page::Tasks(TasksPage {
            projects,
            filter,
            open: None,
        }));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    fn filter_tasks(&mut self, only: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(Page::Tasks(page)) = self.page.as_mut() {
            page.filter = only;
            cx.notify();
        }
    }

    fn detail_mut(&mut self) -> Option<&mut TaskDetail> {
        match self.page.as_mut() {
            Some(Page::Tasks(page)) => page.open.as_mut(),
            _ => None,
        }
    }

    /// Show task `id` in place of the cards, or the cards again for `None`.
    pub(crate) fn open_task(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(Page::Tasks(page)) = self.page.as_mut() {
            page.open = id.map(|id| TaskDetail {
                id,
                expanded: HashSet::new(),
                earlier_open: HashSet::new(),
                changes: HashMap::new(),
                files: HashMap::new(),
            });
            cx.notify();
        }
    }

    /// Open or close a visit; opening one pinned at both ends, `pinned`,
    /// reads what it changed, once.
    fn toggle_visit(
        &mut self,
        key: (String, u32),
        pinned: Option<(String, String)>,
        dirs: Dirs,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.detail_mut() else {
            return;
        };
        if !detail.expanded.remove(&key) {
            detail.expanded.insert(key);
            // Read again unless it is under way or landed: a failed read is
            // tried again on the next opening.
            let held = |pair: &(String, String)| {
                matches!(detail.changes.get(pair), Some(None | Some(Ok(_))))
            };
            if let Some(pair) = pinned.filter(|pair| !held(pair)) {
                detail.changes.insert(pair.clone(), None);
                let (from, to) = pair.clone();
                self.read_marks(
                    dirs,
                    move |dir| marks::changes_blocking(dir, &from, &to),
                    move |detail, read| {
                        detail.changes.insert(pair, Some(read));
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Open a changed file's diff, or close it.
    fn toggle_file(&mut self, key: (String, String, String), dirs: Dirs, cx: &mut Context<Self>) {
        let Some(detail) = self.detail_mut() else {
            return;
        };
        if detail.files.remove(&key).is_none() {
            detail.files.insert(key.clone(), None);
            let (from, to, path) = key.clone();
            self.read_marks(
                dirs,
                move |dir| marks::file_diff_blocking(dir, &from, &to, &path),
                move |detail, read| {
                    // Only into a file still open: one closed meanwhile stays closed.
                    if let Some(slot) = detail.files.get_mut(&key) {
                        *slot = Some(read);
                    }
                },
                cx,
            );
        }
        cx.notify();
    }

    fn toggle_earlier(&mut self, run: String, cx: &mut Context<Self>) {
        if let Some(detail) = self.detail_mut() {
            if !detail.earlier_open.remove(&run) {
                detail.earlier_open.insert(run);
            }
            cx.notify();
        }
    }

    /// Read something of a run's marks off the UI thread, in its worktree,
    /// or in its project once the worktree is gone, and `land` it in the
    /// detail if that is still open.
    fn read_marks<T: Send + 'static>(
        &mut self,
        (dir, repo): Dirs,
        read: impl FnOnce(&Path) -> Result<T, String> + Send + 'static,
        land: impl FnOnce(&mut TaskDetail, Result<T, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |pane, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read(if dir.is_dir() { &dir } else { &repo }) })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                if let Some(detail) = pane.detail_mut() {
                    land(detail, read);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The Tasks page. Rows are read per frame, as the workspace page reads
    /// runs: a task moving already refreshes every window.
    pub(super) fn tasks_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(Page::Tasks(page)) = self.page.as_ref() else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
        let roots: Vec<PathBuf> = match &page.filter {
            Some(only) => vec![only.clone()],
            None => page.projects.iter().map(|p| p.root.clone()).collect(),
        };
        // The rows come in group order, so waiting ones lead ended ones.
        let (mut waiting, mut running, mut queued, mut finished) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for row in crate::task::rows(&roots, cx) {
            match row.group {
                group if group.needs_attention() => waiting.push(row),
                Group::Waiting | Group::Ended | Group::Running => running.push(row),
                Group::Queued => queued.push(row),
                Group::Finished => finished.push(row),
            }
        }
        let removed = crate::task::removed(&roots, cx);
        // An opened task is read per frame too; one let go meanwhile leaves
        // the cards on screen.
        let opened = page.open.as_ref().and_then(|detail| {
            let row = [&waiting, &running, &queued, &finished]
                .into_iter()
                .flatten()
                .find(|row| row.id == detail.id)?;
            Some((detail, crate::task::task(&detail.id, cx)?, row))
        });
        if let Some((detail, task, row)) = opened {
            let body = task_detail(detail, &task, row, page, cx);
            return self.tasks_frame(body, cx);
        }

        let filter_name = page
            .filter
            .as_deref()
            .map_or_else(|| ALL_PROJECTS.to_string(), |root| page.label_of(root));
        let choices: Vec<(SharedString, Option<PathBuf>)> =
            std::iter::once((ALL_PROJECTS.into(), None))
                .chain(
                    page.projects
                        .iter()
                        .map(|project| (project.label.clone(), Some(project.root.clone()))),
                )
                .collect();
        let current = page.filter.clone();
        let this = cx.entity();
        let filter = crate::controls::menu_below(
            "tasks-filter",
            crate::controls::action("tasks-filter-trigger")
                .ghost()
                .small()
                .label(filter_name)
                .icon(Icon::new(IconName::ChevronDown))
                .text_color(muted),
            move |mut menu, _, _| {
                for (label, only) in &choices {
                    let (only, this) = (only.clone(), this.clone());
                    menu = menu.item(
                        crate::controls::menu_item(label.clone())
                            .checked(only == current)
                            .on_click(move |_, _, cx: &mut App| {
                                this.update(cx, |pane: &mut Self, cx| {
                                    pane.filter_tasks(only.clone(), cx)
                                });
                            }),
                    );
                }
                menu
            },
        );

        // Each card says what it holds when it holds nothing, and how many it
        // left out when the cap bites.
        let card = |title: &'static str,
                    key: &'static str,
                    rows: Vec<Row>,
                    empty: &str,
                    control: Option<gpui::AnyElement>,
                    cx: &mut Context<Self>| {
            let (count, hidden) = (rows.len(), rows.len().saturating_sub(TASKS_SHOWN));
            let drawn: Vec<gpui::AnyElement> = rows
                .into_iter()
                .take(TASKS_SHOWN)
                .enumerate()
                .map(|(i, row)| task_row((key, i), row, page, cx))
                .collect();
            page_card(title, Some(count), control, cx)
                .children((count == 0).then(|| muted_line(empty.to_string())))
                .children(drawn)
                .children((hidden > 0).then(|| muted_line(format!("{hidden} more not shown"))))
        };
        let attention = card(
            "Needs attention",
            "tasks-attention",
            waiting,
            "Nothing needs you.",
            Some(filter.into_any_element()),
            cx,
        );
        let running = card(
            "Running",
            "tasks-running",
            running,
            "Nothing is running.",
            None,
            cx,
        );
        let queued = card(
            "Queued",
            "tasks-queued",
            queued,
            "Nothing is queued.",
            None,
            cx,
        );
        let finished = card(
            "Finished",
            "tasks-finished",
            finished,
            "Nothing has finished yet.",
            None,
            cx,
        )
        .children((removed > 0).then(|| {
            muted_line(match removed {
                1 => "1 older task was removed.".to_string(),
                n => format!("{n} older tasks were removed."),
            })
        }));

        let body = vec![
            attention.into_any_element(),
            running.into_any_element(),
            queued.into_any_element(),
            finished.into_any_element(),
        ];
        self.tasks_frame(body, cx)
    }

    /// The page around `body`: its header, then one scrolled column.
    fn tasks_frame(&self, body: Vec<gpui::AnyElement>, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .child(
                div()
                    .id("tasks-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        div().h_flex().w_full().justify_center().child(
                            div()
                                .v_flex()
                                .gap_6()
                                .w_full()
                                .max_w(rems(PAGE_WIDE))
                                .text_sm()
                                .children(body),
                        ),
                    ),
            )
            .into_any_element()
    }
}

/// The muted line under a task's title: its workflow, where it is or how it
/// ended, and its project.
fn row_said(row: &Row, page: &TasksPage) -> String {
    [
        row.name.clone(),
        row.at.clone(),
        page.label_of(&row.project),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// One task: its title, then its workflow, where it is and its project,
/// then what can be done with it from here. Pressing the text opens the
/// task.
fn task_row(
    key: (&'static str, usize),
    row: Row,
    page: &TasksPage,
    cx: &mut Context<ChatPane>,
) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let said = row_said(&row, page);
    let (name, i) = key;
    let actions = row_actions(&format!("{name}-{i}"), &row, false, cx);
    let id = row.id.clone();
    let text = div()
        .id(SharedString::from(format!("{name}-open-{i}")))
        .v_flex()
        .gap_0p5()
        .flex_1()
        .min_w_0()
        .child(div().truncate().child(row.title))
        .child(div().text_xs().text_color(muted).truncate().child(said))
        .cursor_pointer()
        .on_click(
            cx.listener(move |pane: &mut ChatPane, _, _, cx| pane.open_task(Some(id.clone()), cx)),
        );
    card_box(cx)
        .p_2()
        .h_flex()
        .items_center()
        .gap_2()
        .child(text)
        .children(actions)
        .into_any_element()
}

/// What can be done with the task `row` from here, keyed by `key`. A
/// finished task offers nothing on a row, and *Retry* in its detail when
/// `retry_finished`.
fn row_actions(
    key: &str,
    row: &Row,
    retry_finished: bool,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let id = row.id.clone();
    let button = |what: &'static str, label: &'static str| {
        crate::controls::action(SharedString::from(format!("{key}-{what}")))
            .ghost()
            .small()
            .label(label)
    };
    let open = open_session(&format!("{key}-open"), row, cx);
    let retry = |cx: &mut Context<ChatPane>| {
        button("retry", "Retry")
            .tooltip("Run it again in a new run")
            .on_click(emit(&id, cx, ChatPaneEvent::RetryTask))
            .into_any_element()
    };
    let mut actions: Vec<gpui::AnyElement> = Vec::new();
    match row.group {
        Group::Waiting | Group::Running | Group::Queued => {
            actions.extend(open.map(IntoElement::into_any_element));
            if row.stoppable {
                actions.push(
                    button("stop", "Stop")
                        .on_click(emit(&id, cx, ChatPaneEvent::StopTask))
                        .into_any_element(),
                );
            }
        }
        Group::Ended => {
            if row.resumable {
                actions.push(
                    button("resume", "Resume")
                        .tooltip("Carry on from that step in a new session")
                        .on_click(emit(&id, cx, ChatPaneEvent::ResumeTask))
                        .into_any_element(),
                );
            }
            actions.push(retry(cx));
            actions.push(
                button("dismiss", "Dismiss")
                    .tooltip("Let this task go; it is kept as history and its work stays")
                    .on_click(emit(&id, cx, ChatPaneEvent::DismissTask))
                    .into_any_element(),
            );
        }
        Group::Finished => {
            if retry_finished {
                actions.push(retry(cx));
            }
        }
    }
    actions
}

/// Task `task`, listed as `row`: a way back to the cards, its head with what
/// can be done with it, what it waits on approval for, its last run's
/// timeline, then its earlier runs.
fn task_detail(
    detail: &TaskDetail,
    task: &onehand_core::task::Task,
    row: &Row,
    page: &TasksPage,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let muted = cx.theme().muted_foreground;
    let back = crate::controls::action("task-back")
        .ghost()
        .small()
        .icon(Icon::new(IconName::ChevronLeft))
        .label("All tasks")
        .on_click(cx.listener(|pane: &mut ChatPane, _, _, cx| pane.open_task(None, cx)));
    let head = card_box(cx)
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(div().font_semibold().child(row.title.clone()))
                .child(div().text_xs().text_color(muted).child(row_said(row, page))),
        )
        .children(row_actions("task-detail", row, true, cx));
    let mut out = vec![
        div().h_flex().child(back).into_any_element(),
        head.into_any_element(),
    ];
    let Some((last, earlier)) = task.runs.split_last() else {
        return out;
    };
    if let Some((step, answer)) = last.under_review() {
        let open = open_session("task-review-open", row, cx).map(IntoElement::into_any_element);
        let said = match answer.trim().is_empty() {
            true => vec![muted_line("The step kept no answer.", cx)],
            false => mono_well(answer, cx),
        };
        out.push(
            page_card("Awaiting approval", None, open, cx)
                .child(div().text_xs().text_color(muted).child(format!(
                    "{} answered; it is approved in its session.",
                    step.label
                )))
                .children(said)
                .into_any_element(),
        );
    }
    // Only a task at work has a visit under way: an open visit of any other
    // was cut off, by a quit or a lost session, before it could end.
    let live = match row.group {
        Group::Running | Group::Waiting => true,
        Group::Ended | Group::Queued | Group::Finished => false,
    };
    let title = format!("Run {}", task.runs.len());
    out.push(
        page_card(title, Some(last.visits().len()), None, cx)
            .children(timeline(detail, last, live, cx))
            .into_any_element(),
    );
    if !earlier.is_empty() {
        let now = now_secs();
        let mut card = page_card("Earlier runs", Some(earlier.len()), None, cx);
        for (n, run) in earlier.iter().enumerate().rev().take(EARLIER_SHOWN) {
            let open = detail.earlier_open.contains(&run.id);
            let ended = match &run.outcome {
                Some(outcome) => outcome.said(),
                None => "Cut off".to_string(),
            };
            let at = run
                .history
                .last()
                .map_or_else(String::new, |t| rel_time(now, t.at));
            let id = run.id.clone();
            card = card.child(
                div()
                    .id(SharedString::from(format!("task-earlier-{}", run.id)))
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                        pane.toggle_earlier(id.clone(), cx)
                    }))
                    .child(Icon::new(chevron(open)).small().text_color(muted))
                    .child(div().font_semibold().child(format!("Run {}", n + 1)))
                    .child(div().flex_1().min_w_0().truncate().child(ended))
                    .child(div().text_xs().text_color(muted).child(at)),
            );
            if open {
                card = card.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .pl_4()
                        .children(timeline(detail, run, false, cx)),
                );
            }
        }
        let hidden = earlier.len().saturating_sub(EARLIER_SHOWN);
        if hidden > 0 {
            card = card.child(muted_line(&format!("{hidden} older runs not shown"), cx));
        }
        out.push(card.into_any_element());
    }
    out
}

/// `run`'s visits, oldest first, each opening onto what it kept and what
/// it changed. `live` when the run is at work, so its open visit is under
/// way rather than cut off.
fn timeline(
    detail: &TaskDetail,
    run: &Run,
    live: bool,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let muted = cx.theme().muted_foreground;
    let visits = run.visits();
    if visits.is_empty() {
        return vec![muted_line("It has not started.", cx)];
    }
    let hidden = visits.len().saturating_sub(VISITS_SHOWN);
    let now = now_secs();
    let mut out: Vec<gpui::AnyElement> = Vec::new();
    if hidden > 0 {
        out.push(muted_line(
            &format!("{hidden} earlier visits not shown"),
            cx,
        ));
    }
    for visit in &visits[hidden..] {
        let key = (run.id.clone(), visit.id);
        let open = detail.expanded.contains(&key);
        let label = run
            .template
            .steps
            .iter()
            .find(|step| step.id == visit.step)
            .map_or_else(|| visit.step.clone(), |step| step.label.clone());
        let started = rel_time(now, visit.started_at);
        // A cut-off visit has no end to measure to, and its time would grow
        // for ever.
        let until = match (visit.ended_at, live) {
            (Some(at), _) => Some(at),
            (None, true) => Some(now),
            (None, false) => None,
        };
        let when = match until {
            Some(at) => format!(
                "{started} · {}",
                crate::chat::transcript::elapsed(at.saturating_sub(visit.started_at))
            ),
            None => started,
        };
        let why = visit.why.clone().unwrap_or_else(|| {
            match live {
                true => "In progress",
                false => "Cut off",
            }
            .to_string()
        });
        let pinned = visit.start.clone().zip(visit.end.clone());
        let dirs = dirs(run);
        out.push(
            div()
                .id(SharedString::from(format!(
                    "task-visit-{}-{}",
                    run.id, visit.id
                )))
                .h_flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                    pane.toggle_visit(key.clone(), pinned.clone(), dirs.clone(), cx)
                }))
                .child(Icon::new(chevron(open)).small().text_color(muted))
                .child(div().flex_none().child(label))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(why),
                )
                .child(div().flex_none().text_xs().text_color(muted).child(when))
                .into_any_element(),
        );
        if open {
            out.push(
                div()
                    .v_flex()
                    .gap_2()
                    .pl_6()
                    .children(visit_body(detail, run, visit, live, cx))
                    .into_any_element(),
            );
        }
    }
    out
}

/// An opened visit: what it kept or printed, then the files it changed.
fn visit_body(
    detail: &TaskDetail,
    run: &Run,
    visit: &Visit,
    live: bool,
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let mut out = visit
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
        .map(|output| mono_well(output, cx))
        .unwrap_or_default();
    let (Some(from), Some(to)) = (visit.start.clone(), visit.end.clone()) else {
        let said = match (visit.ended_at, live) {
            (None, true) => "In progress",
            (None, false) => "Cut off before its end was pinned.",
            (Some(_), _) => "No marks were pinned for this visit.",
        };
        out.push(muted_line(said, cx));
        return out;
    };
    let changes = match detail.changes.get(&(from.clone(), to.clone())) {
        Some(Some(Ok(changes))) => changes,
        Some(Some(Err(why))) => {
            out.push(muted_line(&format!("What changed was not read: {why}"), cx));
            return out;
        }
        Some(None) | None => {
            out.push(muted_line("Reading what changed…", cx));
            return out;
        }
    };
    if changes.is_empty() {
        out.push(muted_line("No files changed.", cx));
    }
    let muted = cx.theme().muted_foreground;
    for (i, change) in changes.iter().take(FILES_SHOWN).enumerate() {
        let key = (from.clone(), to.clone(), change.path.clone());
        let opened = detail.files.get(&key);
        let counts = match change.lines {
            Some((added, removed)) => {
                crate::chat::transcript::line_counts(added as usize, removed as usize, cx)
            }
            None => Some(div().text_color(muted).child("binary")),
        };
        let dirs = dirs(run);
        let line = div()
            .id(SharedString::from(format!(
                "task-file-{}-{}-{i}",
                run.id, visit.id
            )))
            .h_flex()
            .items_center()
            .gap_2();
        // A binary file has no lines to diff: it is listed, never opened.
        let line = match change.lines {
            Some(_) => line
                .cursor_pointer()
                .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                    pane.toggle_file(key.clone(), dirs.clone(), cx)
                }))
                .child(
                    Icon::new(chevron(opened.is_some()))
                        .small()
                        .text_color(muted),
                ),
            None => line.pl_4(),
        };
        out.push(
            line.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .child(change.path.clone()),
            )
            .children(counts.map(|counts| counts.text_xs()))
            .into_any_element(),
        );
        match opened {
            Some(Some(Ok(rows))) => {
                let mut budget = DIFF_LINES;
                let drawn = crate::chat::transcript::diff_rows(rows, &mut budget, cx);
                let cut = rows.len().saturating_sub(DIFF_LINES);
                out.push(
                    div()
                        .v_flex()
                        .w_full()
                        .min_w_0()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .children(drawn)
                        .into_any_element(),
                );
                if cut > 0 {
                    out.push(muted_line(&format!("{cut} more diff lines not shown"), cx));
                }
            }
            Some(Some(Err(why))) => {
                out.push(muted_line(&format!("The diff was not read: {why}"), cx))
            }
            Some(None) => out.push(muted_line("Reading the diff…", cx)),
            None => {}
        }
    }
    let hidden = changes.len().saturating_sub(FILES_SHOWN);
    if hidden > 0 {
        out.push(muted_line(&format!("{hidden} more files not shown"), cx));
    }
    out
}

/// `text`'s last lines in a mono well, saying how many were left out.
fn mono_well(text: &str, cx: &App) -> Vec<gpui::AnyElement> {
    let lines: Vec<&str> = text.lines().collect();
    let hidden = lines.len().saturating_sub(OUTPUT_LINES);
    let well = div()
        .v_flex()
        .w_full()
        .min_w_0()
        .p_2()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .children(lines[hidden..].iter().map(|line| {
            // An empty line still takes its height.
            div().child(match line.is_empty() {
                true => " ".to_string(),
                false => line.to_string(),
            })
        }));
    let mut out = Vec::new();
    if hidden > 0 {
        out.push(muted_line(&format!("{hidden} earlier lines not shown"), cx));
    }
    out.push(well.into_any_element());
    out
}

fn muted_line(text: &str, cx: &App) -> gpui::AnyElement {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_string())
        .into_any_element()
}

/// The chevron of something that opens: down while it is open.
fn chevron(open: bool) -> IconName {
    match open {
        true => IconName::ChevronDown,
        false => IconName::ChevronRight,
    }
}

/// *Open session* on the session `row` runs in, if it has one.
fn open_session(
    id: &str,
    row: &Row,
    cx: &mut Context<ChatPane>,
) -> Option<gpui_component::button::Button> {
    let (uid, window) = row.session?;
    Some(
        crate::controls::action(SharedString::from(id.to_string()))
            .ghost()
            .small()
            .label("Open session")
            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                cx.emit(ChatPaneEvent::ShowSession { uid, window });
            })),
    )
}

/// Where `run`'s marks are read.
fn dirs(run: &Run) -> Dirs {
    (run.setup.dir.clone(), run.setup.repo.clone())
}

/// A press that announces `event` about task `id`.
fn emit(
    id: &str,
    cx: &Context<ChatPane>,
    event: fn(String) -> ChatPaneEvent,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + use<> {
    let (id, pane) = (id.to_string(), cx.entity().downgrade());
    move |_, _, cx| {
        let _ = pane.update(cx, |_, cx| cx.emit(event(id.clone())));
    }
}
