//! A project's board: its Backlog, its To do, a column per step of its
//! workflow and what is done, side by side, scrolling sideways when they do
//! not fit. What stands in each column is `onehand_core::board`'s to say;
//! this draws it and carries the person's To do changes to the projects'
//! drive. An entity of the window's, drawn by the chat pane as the Tasks
//! page is.

use crate::controls::Short as _;
use crate::drag::DragGhost;
use crate::page_parts::{page_card, page_head, page_top, ruled};
use crate::state::Shared;
use crate::task::Row;
use gpui::{
    App, AppContext as _, BorrowAppContext as _, Context, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Rems, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::board::BOARD_SHOWN;
use onehand_core::project::ProjectId;
use onehand_core::task::Section;
use onehand_ui::RowAct;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

/// How wide each column stands: a task's title and its line of facts with
/// room for its `⋯`, so a board of four steps fits a wide pane and a longer
/// one scrolls sideways rather than squeezing every title.
const COLUMN_W: Rems = rems(17.);

/// The page of one project's board.
pub(crate) struct BoardPage {
    /// The project whose board it shows; none before it is first shown.
    project: Option<ProjectId>,
    /// The project's issues, as every view of them holds them, so an issue
    /// an agent writes reaches the Backlog as it lands; with the file they
    /// are kept in.
    issues: Option<(PathBuf, onehand_issues::Held)>,
    _issues_seen: Option<Subscription>,
    /// One tab stop per row drawn, so the keyboard reaches each row's `⋯`.
    rows: HashMap<RowKey, FocusHandle>,
    /// Focused when the page shows, so its keys have somewhere to land.
    focus: FocusHandle,
}

/// Which row a focus handle is kept for.
#[derive(Clone, PartialEq, Eq, Hash)]
enum RowKey {
    Backlog(u64),
    Todo(u64),
    Task(String),
}

/// An issue row under the pointer, and the column it was taken from. A type
/// of the board's own, as gpui hands a drop only to a target of its type.
#[derive(Clone, Copy)]
struct IssueDrag {
    issue: u64,
    from: Column,
}

/// The columns an issue is dragged between.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Backlog,
    Todo,
}

/// What the board page asks of the window.
pub(crate) enum BoardPageEvent {
    /// Show task `id` in full, on the Tasks page.
    OpenTask(String),
    /// Do to a task what the Tasks page would.
    Task(crate::tasks_page::TasksPageEvent),
    /// Open the settings of the project at this folder, where its board is
    /// bound.
    ProjectSettings(PathBuf),
    /// Open the board's workflow on the Workflows page, its step of this id
    /// open when one is named.
    EditWorkflow { step: Option<String> },
}

impl EventEmitter<BoardPageEvent> for BoardPage {}

impl BoardPage {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            project: None,
            issues: None,
            _issues_seen: None,
            rows: HashMap::new(),
            focus: cx.focus_handle(),
        }
    }

    /// The project whose board it shows.
    pub(crate) fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }

    /// Come on screen on `project`'s board.
    pub(crate) fn show(&mut self, project: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.as_ref() != Some(&project) {
            self.rows.clear();
        }
        let local = (Shared::global(cx).projects.get(&project))
            .and_then(|p| onehand_core::board::local(p).ok());
        match local {
            Some((root, file)) => {
                if self.issues.as_ref().is_none_or(|(at, _)| *at != file) {
                    let held = onehand_issues::Held::new(
                        &root,
                        file.clone(),
                        crate::features::connectors(),
                        cx,
                    );
                    self._issues_seen = Some(held.observe(|_: &mut Self, cx| cx.notify(), cx));
                    self.issues = Some((file, held));
                }
            }
            None => {
                self.issues = None;
                self._issues_seen = None;
            }
        }
        self.project = Some(project);
        self.focus.clone().focus(window, cx);
        cx.notify();
    }

    fn handle(&mut self, key: RowKey, cx: &mut Context<Self>) -> FocusHandle {
        // ponytail: kept for every row the page has drawn on this project, a
        // few hundred at most; let go of when another project is shown.
        self.rows
            .entry(key)
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    /// The page, read per frame: a task moving already redraws every window,
    /// and a write to the issues wakes it through the file it holds.
    fn page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(id) = self.project.clone() else {
            return self.frame(
                head("Board", String::new(), None, cx),
                onehand_ui::hint("No board shown", cx),
            );
        };
        let Some(project) = Shared::global(cx).projects.get(&id).cloned() else {
            return self.frame(
                head("Board", String::new(), None, cx),
                onehand_ui::hint("This project is no longer kept", cx),
            );
        };
        let Some(board) = project.board().cloned() else {
            return self.frame(
                head("Board", project.name.clone(), None, cx),
                onehand_ui::hint("This project has no board", cx),
            );
        };
        let template = crate::workflow::templates(cx)
            .into_iter()
            .find_map(|entry| entry.template.ok().filter(|t| t.id == board.workflow));
        let refused = onehand_core::board::local(&project)
            .and(onehand_core::board::usable(template.as_ref()).map(|_| ()))
            .err();
        let line = match &template {
            Some(t) => format!("{} · {} · team of {}", project.name, t.name, board.team),
            None => project.name.clone(),
        };
        // A workflow that is gone has nothing to open; its refusal says so.
        let edit = template.is_some().then(|| edit_workflow(cx));
        let head = head("Board", line, edit, cx);
        let (Some(template), None) = (template, refused.clone()) else {
            let body = refusal(
                refused.unwrap_or_default(),
                project.path.clone(),
                cx.entity().downgrade(),
                cx,
            );
            return self.frame(head, body);
        };

        let tasks = crate::task::board(&id, &project.path, &template, cx);
        let mut taken = tasks.taken;
        taken.extend(Shared::global(cx).projects.starting(&id));
        let columns = self.columns(&id, &board, tasks.steps, tasks.done, &taken, cx);
        let body = div()
            .id("board-columns")
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_6()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .children(columns),
            )
            .into_any_element();
        self.frame(head, body)
    }

    /// Every column, left to right.
    fn columns(
        &mut self,
        id: &ProjectId,
        board: &onehand_core::project::Board,
        steps: Vec<(String, String, Vec<Row>, Option<String>)>,
        done: Vec<Row>,
        taken: &HashSet<u64>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        // What the rows need of the issues, taken out first so the rows can
        // be drawn while the page is borrowed to make their focus handles.
        let kept = self.issues.as_ref().map(|(_, held)| held.kept(cx));
        let (backlog_rows, backlog_note, todo_titles) = match kept {
            Some(onehand_core::issues::drive::Kept {
                issues: Some(issues),
                forge,
                ..
            }) => {
                let listed: Vec<(u64, String)> =
                    onehand_core::board::backlog(issues, *forge, board, taken)
                        .into_iter()
                        .map(|issue| (issue.number(), issue.title().to_string()))
                        .collect();
                let note = listed.is_empty().then(|| {
                    "No open issues. Talk one through with an agent; it can write it here \
                     with create_issue."
                        .to_string()
                });
                let titles: Vec<Option<String>> = (board.todo.iter().take(BOARD_SHOWN))
                    .map(|&n| issues.get(n).map(|issue| issue.title().to_string()))
                    .collect();
                (listed, note, titles)
            }
            unread => (
                Vec::new(),
                Some(match unread.and_then(|kept| kept.failed.as_ref()) {
                    Some(why) => format!("The issues could not be read: {why}"),
                    None => "Reading the issues…".to_string(),
                }),
                Vec::new(),
            ),
        };
        let title_of = |at: usize, n: u64| {
            todo_titles
                .get(at)
                .cloned()
                .flatten()
                .unwrap_or_else(|| format!("#{n}"))
        };
        let count = backlog_rows.len();
        let drawn = backlog_rows
            .into_iter()
            .take(BOARD_SHOWN)
            .map(|(n, title)| {
                let acts = acts([("Move to To do", todo_press(id, Todo::Add(n)))]);
                self.issue_row(RowKey::Backlog(n), n, title, acts, None, cx)
            })
            .collect();
        // Taking an issue out of To do is the one drop the Backlog takes.
        let backlog = {
            let id = id.clone();
            column("Backlog", count, drawn, backlog_note, None, cx)
                .drag_over::<IssueDrag>(|style, drag, _, cx| match drag.from {
                    Column::Todo => style.bg(cx.theme().list_hover),
                    Column::Backlog => style,
                })
                .on_drop(move |drag: &IssueDrag, _, cx| {
                    if drag.from == Column::Todo {
                        change_todo(&id, Todo::Drop(drag.issue), cx);
                    }
                })
                .into_any_element()
        };

        // To do, in the order the team pulls it.
        let last = board.todo.len().saturating_sub(1);
        let order: Rc<[u64]> = board.todo.iter().copied().collect();
        let drawn = board
            .todo
            .iter()
            .take(BOARD_SHOWN)
            .enumerate()
            .map(|(at, &n)| {
                let mut moves = Vec::new();
                if at > 0 {
                    moves.push(("Move up", todo_press(id, Todo::Move(n, true))));
                }
                if at < last {
                    moves.push(("Move down", todo_press(id, Todo::Move(n, false))));
                }
                moves.push(("Back to Backlog", todo_press(id, Todo::Drop(n))));
                let place = (id.clone(), order.clone());
                self.issue_row(
                    RowKey::Todo(n),
                    n,
                    title_of(at, n),
                    acts(moves),
                    Some(place),
                    cx,
                )
            })
            .collect();
        // A drop off its rows puts the issue last. Its rows fill under a
        // drag, so the column fills only when it has none, or the two fills
        // would hide the row the issue lands before.
        let empty = board.todo.is_empty();
        let todo = {
            let id = id.clone();
            column(
                "To do",
                board.todo.len(),
                drawn,
                empty.then(|| "Nothing to do; move an issue here from the Backlog".to_string()),
                None,
                cx,
            )
            .drag_over::<IssueDrag>(move |style, _, _, cx| match empty {
                true => style.bg(cx.theme().list_hover),
                false => style,
            })
            .on_drop(move |drag: &IssueDrag, _, cx| {
                change_todo(&id, Todo::Place(drag.issue, usize::MAX), cx);
            })
            .into_any_element()
        };

        let mut columns = vec![backlog, todo];
        for (label, head, rows, step) in steps {
            let col = self.task_column(&label, head, rows, step, "Nothing at this step", cx);
            columns.push(col);
        }
        columns.push(self.task_column("Done", "Done".into(), done, None, "Nothing done yet", cx));
        columns
    }

    /// A column of tasks under `head`, its rows told apart from `label`;
    /// with the id of the workflow's `step` it stands for, a `⋯` in its head
    /// opens that step.
    fn task_column(
        &mut self,
        label: &str,
        head: String,
        rows: Vec<Row>,
        step: Option<String>,
        empty: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = rows.len();
        let drawn = rows
            .into_iter()
            .take(BOARD_SHOWN)
            .map(|row| self.task_row(label, row, cx))
            .collect();
        let more = step.map(|step| {
            let page = cx.entity().downgrade();
            let key = format!("board-step-{step}-more");
            let press: onehand_ui::Press = Rc::new(move |_, _, cx| {
                let step = Some(step.clone());
                let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::EditWorkflow { step }));
            });
            onehand_ui::row_more(key, "Step actions", acts([("Edit step", press)]), cx)
                .into_any_element()
        });
        column(
            head,
            count,
            drawn,
            (count == 0).then(|| empty.to_string()),
            more,
            cx,
        )
        .into_any_element()
    }

    /// A row of an issue, in the Backlog or To do: its title over its
    /// number, its acts in its `⋯` and on a right-click, dragged by its
    /// title. A To do row, given its project and the whole of To do, takes
    /// a dragged issue in just before itself.
    fn issue_row(
        &mut self,
        key: RowKey,
        number: u64,
        title: String,
        acts: Rc<[RowAct]>,
        todo: Option<(ProjectId, Rc<[u64]>)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let line = (format!("#{number}"), crate::theme::meta_ink(cx));
        let from = match todo {
            Some(_) => Column::Todo,
            None => Column::Backlog,
        };
        let ghost = SharedString::from(title.clone());
        let drags = move |row: Stateful<gpui::Div>| {
            let row = row.on_drag(
                IssueDrag {
                    issue: number,
                    from,
                },
                move |_, _, _, cx| {
                    let ghost = ghost.clone();
                    cx.new(|_| DragGhost(ghost))
                },
            );
            let Some((id, order)) = todo else {
                return row;
            };
            // The row under a drag fills, the issue landing just before it.
            row.drag_over::<IssueDrag>(|style, _, _, cx| style.bg(cx.theme().list_hover))
                .on_drop(move |drag: &IssueDrag, _, cx| {
                    if let Some(at) = place_before(&order, drag.issue, number) {
                        change_todo(&id, Todo::Place(drag.issue, at), cx);
                    }
                })
        };
        self.row(key, title, line, acts, None, drags, cx)
    }

    /// A row of a task: its title over what it waits on, as the Tasks page
    /// sorts it and its detail says it, and where it is when that is not the
    /// column it stands in. Pressed, it opens the task; its `⋯` and
    /// right-click offer the Tasks page's acts beside.
    fn task_row(&mut self, column: &str, row: Row, cx: &mut Context<Self>) -> gpui::AnyElement {
        let said = task_line(&row, column);
        let ink = match row.group.section() == Section::NeedsAttention {
            true => crate::theme::status_ink(cx).warning,
            false => cx.theme().muted_foreground,
        };
        let open = open_task(&row.id, cx);
        let page = cx.entity().downgrade();
        let acts: Rc<[RowAct]> = task_offers(&row)
            .into_iter()
            .map(|act| match act {
                None => RowAct::new("Show task", open.clone()),
                Some(act) => {
                    let (event, page) = (act.event(&row), page.clone());
                    let press: onehand_ui::Press = Rc::new(move |_, _, cx| {
                        if let Some(event) = event.clone() {
                            let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::Task(event)));
                        }
                    });
                    RowAct::new(act.label(), press)
                }
            })
            .collect();
        self.row(
            RowKey::Task(row.id.clone()),
            row.title,
            (said, ink),
            acts,
            Some(open),
            |row| row,
            cx,
        )
    }

    /// A row in a column: `title` over `line`, a tab stop whose `⋯` and
    /// right-click offer `acts`; pressing its text runs `open` if it has one.
    /// `drags` adds what the row does under a drag.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        key: RowKey,
        title: String,
        (line, ink): (String, gpui::Hsla),
        acts: Rc<[RowAct]>,
        open: Option<onehand_ui::Press>,
        drags: impl FnOnce(Stateful<gpui::Div>) -> Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focus = self.handle(key.clone(), cx);
        let (name, what) = match &key {
            RowKey::Backlog(n) => (format!("board-backlog-{n}"), "Issue actions"),
            RowKey::Todo(n) => (format!("board-todo-{n}"), "Issue actions"),
            RowKey::Task(id) => (format!("board-task-{id}"), "Task actions"),
        };
        let group = SharedString::from(format!("{name}-row"));
        let ring = cx.theme().ring;
        let text = div()
            .id(SharedString::from(format!("{name}-text")))
            .v_flex()
            .gap_0p5()
            .flex_1()
            .min_w_0()
            .child(div().truncate().child(title))
            .child(div().text_xs().text_color(ink).truncate().child(line));
        let text = match open {
            Some(open) => text
                .cursor_pointer()
                .on_click(move |event, window, cx| open(event, window, cx)),
            None => text,
        };
        let menu_acts = acts.clone();
        let row = onehand_ui::row(false, cx).id(SharedString::from(name.clone()));
        drags(row)
            .group(group.clone())
            .track_focus(&focus)
            .rounded_none()
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(move |row| row.border_color(ring))
            .h_flex()
            .items_center()
            .gap_2()
            .py_1p5()
            .child(text)
            .child(
                onehand_ui::hover_slot(
                    group,
                    false,
                    onehand_ui::row_more(format!("{name}-more"), what, acts, cx),
                )
                .focus(&focus),
            )
            .context_menu(move |menu, _, cx| onehand_ui::row_menu_items(menu, &menu_acts, cx))
            .into_any_element()
    }

    /// The page around `body`: its head kept in place over it.
    fn frame(&self, head: gpui::AnyElement, body: gpui::AnyElement) -> gpui::AnyElement {
        div()
            .key_context("BoardPage")
            .track_focus(&self.focus)
            .size_full()
            .v_flex()
            .child(page_top(None, [head]))
            .child(body)
            .into_any_element()
    }
}

impl Render for BoardPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page(cx)
    }
}

/// The page's head: its name over the project, its workflow and its team,
/// and `edit` at its end when there is a workflow to open.
fn head(
    title: &'static str,
    line: String,
    edit: Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let line = (!line.is_empty()).then(|| line.into_any_element());
    page_head(title, line, edit.into_iter().collect(), cx).into_any_element()
}

/// *Edit workflow*, opening the board's workflow on the Workflows page.
fn edit_workflow(cx: &Context<BoardPage>) -> gpui::AnyElement {
    crate::controls::action("board-edit-workflow")
        .short()
        .secondary()
        .label("Edit workflow")
        .on_click(cx.listener(|_, _, _, cx| cx.emit(BoardPageEvent::EditWorkflow { step: None })))
        .into_any_element()
}

/// A column: a box with its name and count, `control` at its head's end,
/// `rows` capped at [`BOARD_SHOWN`] (said under them when it bites), and
/// `note` where there is nothing to list.
fn column(
    title: impl Into<SharedString>,
    count: usize,
    rows: Vec<gpui::AnyElement>,
    note: Option<String>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::Div {
    let muted = cx.theme().muted_foreground;
    let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
    let hidden = count.saturating_sub(BOARD_SHOWN);
    page_card(title.into(), Some(count), control, cx)
        .flex_none()
        .w(COLUMN_W)
        .children(note.map(muted_line))
        .child(ruled(rows, cx))
        .children((hidden > 0).then(|| muted_line(format!("{hidden} more not shown"))))
}

/// Why the board cannot run, in place of its columns, with the way to the
/// settings that bind it.
fn refusal(
    why: String,
    root: PathBuf,
    page: gpui::WeakEntity<BoardPage>,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_3()
        .text_size(crate::tokens::CHROME_TEXT)
        .child(div().text_color(cx.theme().muted_foreground).child(why))
        .child(
            crate::controls::action("board-project-settings")
                .short()
                .secondary()
                .label("Project settings…")
                .on_click(move |_, _, cx| {
                    let root = root.clone();
                    let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::ProjectSettings(root)));
                }),
        )
        .into_any_element()
}

/// The line under a board task's title: needing attention, running or
/// queued as the Tasks page sorts it, then what it waits on a person for or
/// why it ended, or else the step it is at when that is not `column`. One
/// waiting for room says only that, which *Running* would contradict.
fn task_line(row: &Row, column: &str) -> String {
    if row.waits_for_room {
        return row.at.clone();
    }
    let word = match row.group.section() {
        Section::NeedsAttention => Some("Needs attention"),
        Section::Running => Some("Running"),
        Section::Queued => Some("Queued"),
        Section::Finished => None,
    };
    let at = match &row.waits {
        Some(waits) => Some(waits.as_str()),
        None => (row.at != column).then_some(row.at.as_str()),
    };
    word.into_iter()
        .chain(at)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What a board task's row offers: the task itself (`None`), then what the
/// Tasks page's row offers it.
fn task_offers(row: &Row) -> Vec<Option<crate::tasks_page::TaskAct>> {
    std::iter::once(None)
        .chain(
            crate::tasks_page::task_acts(row, false)
                .into_iter()
                .map(Some),
        )
        .collect()
}

/// A row's acts, from their words and presses.
fn acts<'a>(acts: impl IntoIterator<Item = (&'a str, onehand_ui::Press)>) -> Rc<[RowAct]> {
    acts.into_iter()
        .map(|(label, press)| RowAct::new(label.to_string(), press))
        .collect()
}

/// A press that opens task `id`.
fn open_task(id: &str, cx: &Context<BoardPage>) -> onehand_ui::Press {
    let (id, page) = (id.to_string(), cx.entity().downgrade());
    Rc::new(move |_, _, cx| {
        let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::OpenTask(id.clone())));
    })
}

/// A change a person makes to To do.
#[derive(Clone, Copy)]
enum Todo {
    /// Put the issue last in To do.
    Add(u64),
    /// Move the issue one place up, or down.
    Move(u64, bool),
    /// Put the issue back in the Backlog.
    Drop(u64),
    /// Put the issue at this place in To do, counted without it; past the
    /// end, last.
    Place(u64, usize),
}

/// A press making `change` to project `id`'s To do.
fn todo_press(id: &ProjectId, change: Todo) -> onehand_ui::Press {
    let id = id.clone();
    Rc::new(move |_, _, cx| change_todo(&id, change, cx))
}

/// Make `change` to project `id`'s To do, a press's or a drop's: through the
/// projects' drive, and To do looked at again at once, since the team may
/// have room.
fn change_todo(id: &ProjectId, change: Todo, cx: &mut App) {
    let fx = cx.update_global::<Shared, _>(|shared, _| match change {
        Todo::Add(n) => shared.projects.add_todo(id, n),
        Todo::Move(n, up) => shared.projects.move_todo(id, n, up),
        Todo::Drop(n) => shared.projects.drop_todo(id, n),
        Todo::Place(n, at) => shared.projects.place_todo(id, n, at),
    });
    crate::projects::carry(fx, cx);
    crate::board::pull_now(id, cx);
    cx.refresh_windows();
}

/// Where in `todo` issue `dragged` goes to stand just before `target`: the
/// target's place once the dragged issue is taken out. None when dropped on
/// itself, which moves nothing.
fn place_before(todo: &[u64], dragged: u64, target: u64) -> Option<usize> {
    if dragged == target {
        return None;
    }
    todo.iter()
        .filter(|&&n| n != dragged)
        .position(|&n| n == target)
}

#[cfg(test)]
mod tests {
    use super::{place_before, task_line, task_offers};
    use crate::task::Row;
    use onehand_core::task::Group;

    fn row(group: Group, at: &str, waits: Option<&str>) -> Row {
        Row {
            id: "t".into(),
            title: "Fix it".into(),
            name: "from the board · Build".into(),
            at: at.into(),
            waits: waits.map(str::to_string),
            waits_for_room: false,
            project: "/p".into(),
            group,
            session: None,
            resumable: false,
            stoppable: false,
            stopping: false,
        }
    }

    #[test]
    fn a_task_says_what_it_waits_on() {
        let approval = row(Group::Waiting, "Build", Some("Approving starts Review"));
        assert_eq!(
            task_line(&approval, "Build"),
            "Needs attention · Approving starts Review"
        );
        let card = row(
            Group::Waiting,
            "Build",
            Some("The agent asks something in its session"),
        );
        assert_eq!(
            task_line(&card, "Build"),
            "Needs attention · The agent asks something in its session"
        );
        let mut room = row(Group::Running, "Waiting for room at Review", None);
        room.waits_for_room = true;
        assert_eq!(task_line(&room, "Review"), "Waiting for room at Review");
        assert_eq!(
            task_line(&row(Group::Running, "Build", None), "Build"),
            "Running"
        );
        assert_eq!(
            task_line(&row(Group::Queued, "Build", None), "Gone"),
            "Queued · Build"
        );
        let failed = row(
            Group::Ended,
            "failed",
            Some("The agent of Build did not start."),
        );
        assert_eq!(
            task_line(&failed, "Build"),
            "Needs attention · The agent of Build did not start."
        );
    }

    #[test]
    fn a_task_offers_the_tasks_pages_acts() {
        let labels = |row: &Row| -> Vec<&'static str> {
            task_offers(row)
                .into_iter()
                .map(|act| act.map_or("Show task", |act| act.label()))
                .collect()
        };
        let mut ended = row(Group::Ended, "failed", Some("It failed"));
        ended.resumable = true;
        assert_eq!(labels(&ended), ["Show task", "Resume", "Retry…", "Dismiss"]);
        let mut running = row(Group::Running, "Build", None);
        running.stoppable = true;
        assert_eq!(labels(&running), ["Show task", "Stop"]);
        assert_eq!(labels(&row(Group::Finished, "done", None)), ["Show task"]);
    }

    #[test]
    fn a_drop_on_a_row_lands_just_before_it() {
        let todo = [1, 2, 3, 4];
        // From below: the target keeps its place.
        assert_eq!(place_before(&todo, 4, 2), Some(1));
        // From above: the target moves up one once the issue is out.
        assert_eq!(place_before(&todo, 1, 3), Some(1));
        // Onto the row after itself: its own place, which writes nothing.
        assert_eq!(place_before(&todo, 2, 3), Some(1));
        // From the Backlog.
        assert_eq!(place_before(&todo, 9, 1), Some(0));
        assert_eq!(place_before(&todo, 3, 3), None);
    }
}
