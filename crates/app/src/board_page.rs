//! A project's board: its Backlog, the board's own columns and what is
//! done, side by side, scrolling sideways when they do
//! not fit, each column scrolling its own rows. What stands in each column is `onehand_core::board`'s to say;
//! this draws it and carries the person's moves to the boards' drive. An entity of the window's, drawn by the chat pane as the Tasks
//! page is.

use crate::controls::Short as _;
use crate::page_parts::{page_head, page_top};
use crate::state::Shared;
use gpui::{
    App, Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollHandle, StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::project::ProjectId;
use std::collections::HashMap;
use std::path::PathBuf;

mod column;
mod entries;
mod rows;

use column::ColumnKey;
#[cfg(test)]
use rows::{task_line, task_offers};

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
    /// Where the columns stand scrolled sideways.
    across: ScrollHandle,
    /// Where each column's rows stand scrolled, by the column's key.
    down: HashMap<ColumnKey, ScrollHandle>,
    /// Focused when the page shows, so its keys have somewhere to land.
    focus: FocusHandle,
}

/// Which row a focus handle is kept for.
#[derive(Clone, PartialEq, Eq, Hash)]
enum RowKey {
    /// An issue no task works, by its number.
    Issue(u64),
    Task(String),
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
    /// Open the workflow of id `workflow`, a column's, on the Workflows page.
    EditWorkflow { workflow: String },
    /// Open the column dialog on the board's column of id `column`.
    EditColumn { column: String },
}

impl EventEmitter<BoardPageEvent> for BoardPage {}

impl BoardPage {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            project: None,
            issues: None,
            _issues_seen: None,
            rows: HashMap::new(),
            across: ScrollHandle::new(),
            down: HashMap::new(),
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
            self.down.clear();
            self.across = ScrollHandle::new();
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

    /// How far the columns can scroll sideways, as last drawn.
    #[cfg(test)]
    pub(crate) fn across_max(&self) -> gpui::Pixels {
        self.across.max_offset().x
    }

    fn handle(&mut self, key: RowKey, cx: &mut Context<Self>) -> FocusHandle {
        // ponytail: kept for every row the page has drawn on this project, a
        // few hundred at most; let go of when another project is shown.
        self.rows
            .entry(key)
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    /// Where the rows of the column `key` stand scrolled.
    fn column_scroll(&mut self, key: &ColumnKey) -> ScrollHandle {
        self.down.entry(key.clone()).or_default().clone()
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
        let boards = &Shared::global(cx).boards;
        let Some(board) = boards.board(&id).cloned() else {
            let head = head("Board", project.name.clone(), None, cx);
            let Some(why) = boards.unreadable(&id).map(str::to_string) else {
                return self.frame(head, onehand_ui::hint("This project has no board", cx));
            };
            let body = refusal(
                format!("This board cannot be read: {why}"),
                project.path.clone(),
                cx.entity().downgrade(),
                cx,
            );
            return self.frame(head, body);
        };
        let line = format!("{} · team of {}", project.name, board.team);
        let head = head(
            "Board",
            line,
            Some(edit_columns(project.path.clone(), cx)),
            cx,
        );
        if let Err(why) = onehand_core::board::local(&project) {
            let body = refusal(why, project.path.clone(), cx.entity().downgrade(), cx);
            return self.frame(head, body);
        }
        let columns = self.columns(&id, &board, cx);
        // Sideways alone: gpui turns a plain wheel into sideways travel only
        // on a box that scrolls on x and not y, so the gaps and the heads
        // move the board while each column moves its own rows. The scroller
        // is the row of columns itself: a row nested inside it is only as
        // wide as the scroller, so the columns past its edge would overflow
        // that row and leave the scroller nothing to scroll.
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("board-columns")
                    .debug_selector(|| "board-columns".into())
                    .size_full()
                    .h_flex()
                    .items_start()
                    .gap_6()
                    .overflow_x_scroll()
                    .track_scroll(&self.across)
                    .px_8()
                    .py_6()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .children(columns),
            )
            .child(onehand_ui::scrollbar_horizontal(&self.across).id("board-columns-bar"))
            .into_any_element();
        self.frame(head, body)
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

/// The page's head: its name over the project and its team, and `edit` at
/// its end.
fn head(
    title: &'static str,
    line: String,
    edit: Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let line = (!line.is_empty()).then(|| line.into_any_element());
    page_head(title, line, edit.into_iter().collect(), cx).into_any_element()
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

/// *Edit columns*, opening the settings of the project at `root`, where its
/// board's columns are defined.
fn edit_columns(root: PathBuf, cx: &Context<BoardPage>) -> gpui::AnyElement {
    crate::controls::action("board-edit-columns")
        .short()
        .secondary()
        .label("Edit columns")
        .on_click(
            cx.listener(move |_, _, _, cx| cx.emit(BoardPageEvent::ProjectSettings(root.clone()))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
