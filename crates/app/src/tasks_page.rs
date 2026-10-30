//! The Tasks page: every task of the window's projects, by what each needs,
//! and one task opened in place of the cards. An entity of the window's,
//! drawn by the chat pane, so what it is narrowed to outlives its leaving
//! the screen.

use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::page_parts::PageProject;
use crate::page_parts::{page_card, page_head, pair, ruled};
use crate::task::Row;
use crate::tokens::PAGE_WIDE;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::task::{Group, Section};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How many rows each card of the Tasks page draws. Finished tasks are kept
/// by the hundred, and nobody reads that far down a card.
const TASKS_SHOWN: usize = 50;

mod detail;
use detail::TaskDetail;
use detail::task_detail;

/// The page that lists every task of the window's projects, by what each
/// needs: a person, its turn, its place, or nothing any more.
pub(crate) struct TasksPage {
    /// Every project in rail order.
    projects: Vec<PageProject>,
    /// The project the page is narrowed to, or `None` for all of them. Kept
    /// while the page is away, for its next showing.
    filter: Option<PathBuf>,
    /// The task shown in place of the cards, if one is opened.
    open: Option<TaskDetail>,
    /// Focused when the page shows, so Esc reaches the task detail: clicking
    /// through the cards moves no focus, and a page that held none would hear
    /// no key at all.
    focus: FocusHandle,
}

/// What the Tasks page asks of the window, which owns tasks' sessions,
/// retries and worktrees.
pub(crate) enum TasksPageEvent {
    /// Show session `uid`, in whichever window holds it.
    ShowSession {
        uid: u64,
        window: gpui::AnyWindowHandle,
    },
    /// Resume the interrupted task `id`.
    ResumeTask(String),
    /// Let the ended task `id` go; it is kept as history.
    DismissTask(String),
    /// Run task `id` again, asking first where from.
    RetryTask(String),
    /// Run task `id` again with what Settings say now, asking first.
    RetryTaskCurrent(String),
    /// Answer the review on issue task `id`'s open pull request.
    AnswerReview(String),
    /// Remove task `id`'s worktree and branch after its pull request merged,
    /// asking first.
    RemoveWorktree(String),
    /// Stop task `id`, queued or running.
    StopTask(String),
}

impl EventEmitter<TasksPageEvent> for TasksPage {}

// The Tasks page's key: the task open in place of the cards closed, the
// cards back.
gpui::actions!(tasks_page, [CloseTaskDetail]);

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

impl TasksPage {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            projects: Vec::new(),
            filter: None,
            open: None,
            focus: cx.focus_handle(),
        }
    }

    /// Come on screen over `projects`, narrowed to `filter` if one is asked
    /// for, else to what was left narrowed last time, so long as it is one of
    /// them; the cards, not a task.
    pub(crate) fn show(
        &mut self,
        projects: Vec<PageProject>,
        filter: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A filter asked for (a project's page's line) wins; else the one left
        // when the page was last shown is kept.
        self.filter = filter
            .or_else(|| self.filter.take())
            .filter(|only| projects.iter().any(|project| &project.root == only));
        self.projects = projects;
        self.open = None;
        self.focus.clone().focus(window, cx);
        cx.notify();
    }

    /// The window's projects changed: a filter on one gone lets go.
    pub(crate) fn set_projects(&mut self, projects: Vec<PageProject>, cx: &mut Context<Self>) {
        if let Some(only) = &self.filter
            && !projects.iter().any(|project| &project.root == only)
        {
            self.filter = None;
        }
        self.projects = projects;
        cx.notify();
    }

    fn filter_tasks(&mut self, only: Option<PathBuf>, cx: &mut Context<Self>) {
        self.filter = only;
        cx.notify();
    }

    /// The Tasks page. Rows are read per frame, as the workspace page reads
    /// runs: a task moving already refreshes every window.
    fn page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let page = self;
        let muted = cx.theme().muted_foreground;
        let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
        let roots: Vec<PathBuf> = match &page.filter {
            Some(only) => vec![only.clone()],
            None => page.projects.iter().map(|p| p.root.clone()).collect(),
        };
        // The rows come in group order, so waiting ones lead ended ones.
        let (mut needs_attention, mut running, mut queued, mut finished) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for row in crate::task::rows(&roots, cx) {
            match row.group.section() {
                Section::NeedsAttention => needs_attention.push(row),
                Section::Running => running.push(row),
                Section::Queued => queued.push(row),
                Section::Finished => finished.push(row),
            }
        }
        let removed = crate::task::removed(&roots, cx);
        // An opened task is read per frame too; one let go meanwhile leaves
        // the cards on screen.
        let opened = page.open.as_ref().and_then(|detail| {
            let row = [&needs_attention, &running, &queued, &finished]
                .into_iter()
                .flatten()
                .find(|row| row.id == detail.id)?;
            Some((detail, crate::task::task(&detail.id, cx)?, row))
        });
        if let Some((detail, task, row)) = opened {
            let body = task_detail(detail, &task, row, page, cx);
            return self.tasks_frame(Vec::new(), body, crate::tokens::PAGE_PROSE, cx);
        }

        let this = cx.entity();
        let filter = crate::page_parts::project_filter(
            "tasks-filter",
            page.filter
                .as_deref()
                .map(|root| page.label_of(root).into()),
            page.projects
                .iter()
                .map(|project| (project.label.clone(), project.root.clone())),
            page.filter.clone(),
            onehand_ui::ControlSize::Tall,
            move |only, cx| {
                this.update(cx, |page: &mut Self, cx| page.filter_tasks(only, cx));
            },
            cx,
        );

        // Each card says what it holds when it holds nothing, and how many it
        // left out when the cap bites.
        let card = |title: &'static str,
                    key: &'static str,
                    rows: Vec<Row>,
                    empty: &str,
                    cx: &mut Context<Self>| {
            let (count, hidden) = (rows.len(), rows.len().saturating_sub(TASKS_SHOWN));
            let drawn: Vec<gpui::AnyElement> = rows
                .into_iter()
                .take(TASKS_SHOWN)
                .enumerate()
                .map(|(i, row)| task_row((key, i), row, page, cx))
                .collect();
            page_card(title, Some(count), None, cx)
                .children((count == 0).then(|| muted_line(empty.to_string())))
                .child(ruled(drawn, cx))
                .children((hidden > 0).then(|| muted_line(format!("{hidden} more not shown"))))
        };
        let attention = card(
            "Needs attention",
            "tasks-attention",
            needs_attention,
            "Nothing needs you",
            cx,
        );
        let running = card(
            "Running",
            "tasks-running",
            running,
            "Nothing is running",
            cx,
        );
        let queued = card("Queued", "tasks-queued", queued, "Nothing is queued", cx);
        let finished = card(
            "Finished",
            "tasks-finished",
            finished,
            "Nothing has finished yet",
            cx,
        )
        .children((removed > 0).then(|| {
            muted_line(match removed {
                1 => "1 older task was removed".to_string(),
                n => format!("{n} older tasks were removed"),
            })
        }));

        // The head names the page and holds no action, the page having none;
        // then the filter at the row's end, the two kept over the cards,
        // which hold the counts. The cards two by two while the column holds
        // two.
        let top = vec![
            page_head("Tasks", None, Vec::new(), cx).into_any_element(),
            div()
                .h_flex()
                .justify_end()
                .child(filter)
                .into_any_element(),
        ];
        let body = vec![
            pair(attention, running).into_any_element(),
            pair(queued, finished).into_any_element(),
        ];
        self.tasks_frame(top, body, PAGE_WIDE, cx)
    }

    /// The page around `body`: its header, `top` kept in place when there is
    /// one, then one scrolled column.
    fn tasks_frame(
        &self,
        top: Vec<gpui::AnyElement>,
        body: Vec<gpui::AnyElement>,
        width: gpui::Rems,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .key_context("TasksPage")
            .track_focus(&self.focus)
            .on_action(cx.listener(|page: &mut Self, _: &CloseTaskDetail, _, cx| {
                page.open_task(None, cx);
            }))
            .size_full()
            .v_flex()
            .when(!top.is_empty(), |page| {
                page.child(crate::page_parts::page_top(Some(width), top))
            })
            .child(
                div()
                    .id("tasks-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(
                        div().h_flex().w_full().justify_center().child(
                            div()
                                .v_flex()
                                .gap_6()
                                .w_full()
                                .max_w(width)
                                .text_size(crate::tokens::CHROME_TEXT)
                                .children(body),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for TasksPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page(cx)
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
    cx: &mut Context<TasksPage>,
) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let said = row_said(&row, page);
    let (name, i) = key;
    // A row needing attention keeps its next action drawn: that attention is
    // earned. Every other row shows its actions while it is pointed at.
    let earned = row.group.section() == Section::NeedsAttention;
    let actions = row_actions(&format!("{name}-{i}"), &row, cx);
    let group = SharedString::from(format!("{name}-row-{i}"));
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
            cx.listener(move |page: &mut TasksPage, _, _, cx| page.open_task(Some(id.clone()), cx)),
        );
    onehand_ui::row(false, cx)
        .group(group.clone())
        .h_flex()
        .items_center()
        .gap_2()
        .py_1p5()
        .child(text)
        .child(onehand_ui::hover_slot(
            group,
            earned,
            div().h_flex().gap_1().children(actions),
        ))
        .into_any_element()
}

/// A press, shared by a button and a menu row that do the same thing.
pub(super) type Press = Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;

/// What can be done with a task from where it is listed.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TaskAct {
    OpenSession,
    Stop,
    Resume,
    Retry,
    Dismiss,
}

impl TaskAct {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::OpenSession => "Open session",
            Self::Stop => "Stop",
            Self::Resume => "Resume",
            Self::Retry => "Retry…",
            Self::Dismiss => "Dismiss",
        }
    }

    fn tooltip(self) -> Option<&'static str> {
        match self {
            Self::Resume => Some("Carry on from that step in a new session"),
            Self::Retry => Some("Run it again in a new run"),
            Self::Dismiss => Some("Let this task go; it is kept as history and its work stays"),
            Self::OpenSession | Self::Stop => None,
        }
    }

    /// What a press of it does, for a button or a menu row alike.
    pub(super) fn press(self, row: &Row, cx: &Context<TasksPage>) -> Press {
        let id = &row.id;
        match self {
            Self::OpenSession => {
                let session = row.session;
                let page = cx.entity().downgrade();
                Rc::new(move |_, _, cx| {
                    if let Some((uid, window)) = session {
                        let _ = page.update(cx, |_, cx| {
                            cx.emit(TasksPageEvent::ShowSession { uid, window })
                        });
                    }
                })
            }
            Self::Stop => Rc::new(emit(id, cx, TasksPageEvent::StopTask)),
            Self::Resume => Rc::new(emit(id, cx, TasksPageEvent::ResumeTask)),
            Self::Retry => Rc::new(emit(id, cx, TasksPageEvent::RetryTask)),
            Self::Dismiss => Rc::new(emit(id, cx, TasksPageEvent::DismissTask)),
        }
    }

    /// The act as a quiet button keyed by `key`.
    pub(super) fn button(
        self,
        key: &str,
        row: &Row,
        cx: &Context<TasksPage>,
    ) -> gpui_component::button::Button {
        let press = self.press(row, cx);
        let button = crate::controls::action(SharedString::from(format!("{key}-{}", self.label())))
            .short()
            .label(self.label())
            .loading(self == Self::Stop && row.stopping);
        let button = match self.tooltip() {
            Some(tip) => button.tooltip(tip),
            None => button,
        };
        // A Resume the preflight finds blocked refuses, saying why. Asked
        // here, where a Resume is drawn, not for every row listed.
        let button = match self == Self::Resume {
            true => button.refuses(
                crate::task::task(&row.id, cx)
                    .and_then(|task| crate::task::resume_refused(&task, cx)),
            ),
            false => button,
        };
        button.on_click(move |event, window, cx| press(event, window, cx))
    }
}

/// What can be done with the task `row` from here, most fitting first. A
/// finished task offers nothing on a row, and *Retry* in its detail when
/// `retry_finished`.
pub(crate) fn task_acts(row: &Row, retry_finished: bool) -> Vec<TaskAct> {
    let mut acts = Vec::new();
    match row.group {
        Group::Waiting | Group::Running | Group::Queued => {
            if row.session.is_some() {
                acts.push(TaskAct::OpenSession);
            }
            if row.stoppable {
                acts.push(TaskAct::Stop);
            }
        }
        Group::Ended => {
            if row.resumable {
                acts.push(TaskAct::Resume);
            }
            acts.push(TaskAct::Retry);
            acts.push(TaskAct::Dismiss);
        }
        Group::Finished => {
            if retry_finished {
                acts.push(TaskAct::Retry);
            }
        }
    }
    acts
}

/// What can be done with the task `row` from a row, keyed by `key`: each a
/// quiet button.
fn row_actions(key: &str, row: &Row, cx: &mut Context<TasksPage>) -> Vec<gpui::AnyElement> {
    task_acts(row, false)
        .into_iter()
        .map(|act| act.button(key, row, cx).quiet(cx).into_any_element())
        .collect()
}

/// *Open session* on the session `row` runs in, if it has one.
pub(super) fn open_session(
    id: &str,
    row: &Row,
    cx: &mut Context<TasksPage>,
) -> Option<gpui_component::button::Button> {
    row.session?;
    Some(TaskAct::OpenSession.button(id, row, cx).quiet(cx))
}

/// A press that announces `event` about task `id`.
pub(super) fn emit(
    id: &str,
    cx: &Context<TasksPage>,
    event: fn(String) -> TasksPageEvent,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + use<> {
    let (id, page) = (id.to_string(), cx.entity().downgrade());
    move |_, _, cx| {
        let _ = page.update(cx, |_, cx| cx.emit(event(id.clone())));
    }
}
