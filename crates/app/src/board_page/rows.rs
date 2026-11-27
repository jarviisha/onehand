//! A board's rows: an issue's, a task's, and the row both are drawn in,
//! with what a task's row offers.

use super::{BoardPage, BoardPageEvent, Column, IssueDrag, RowKey, Todo, change_todo};
use crate::drag::DragGhost;
use crate::task::Row;
use gpui::{
    AppContext as _, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::project::ProjectId;
use onehand_core::task::Section;
use onehand_ui::RowAct;
use std::rc::Rc;

impl BoardPage {
    /// A row of an issue, in the Backlog or To do: its title over its
    /// number, its acts in its `⋯` and on a right-click, dragged by its
    /// title. A To do row, given its project, takes
    /// a dragged issue in just before itself.
    pub(super) fn issue_row(
        &mut self,
        key: RowKey,
        number: u64,
        title: String,
        acts: Rc<[RowAct]>,
        todo: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let line = (format!("#{number}"), crate::theme::meta_ink(cx));
        let from = match todo {
            Some(_) => Column::Todo,
            None => Column::Backlog,
        };
        let ghost = SharedString::from(title.clone());
        let drags = move |row: Stateful<gpui::Div>| {
            let row = row
                .debug_selector(move || format!("board-issue-{number}"))
                .on_drag(
                    IssueDrag {
                        issue: number,
                        from,
                    },
                    move |_, _, _, cx| {
                        let ghost = ghost.clone();
                        cx.new(|_| DragGhost(ghost))
                    },
                );
            let Some(id) = todo else {
                return row;
            };
            // The row under a drag fills, the issue landing just before it.
            row.drag_over::<IssueDrag>(|style, _, _, cx| style.bg(cx.theme().list_hover))
                .on_drop(move |drag: &IssueDrag, _, cx| {
                    change_todo(&id, Todo::Before(drag.issue, number), cx);
                })
        };
        self.row(key, title, line, acts, None, drags, cx)
    }

    /// A row of a task: its title over what it waits on, as the Tasks page
    /// sorts it and its detail says it, and where it is when that is not the
    /// column it stands in. Pressed, it opens the task; its `⋯` and
    /// right-click offer the Tasks page's acts beside.
    pub(super) fn task_row(
        &mut self,
        column: &str,
        row: Row,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let said = task_line(&row, column);
        let ink = match row.group.section() == Section::NeedsAttention {
            true => crate::theme::status_ink(cx).warning,
            false => cx.theme().muted_foreground,
        };
        let open = open_task(&row.id, cx);
        let page = cx.entity().downgrade();
        let remove = removal(&row.id, cx);
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
            .chain(remove)
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
    pub(super) fn row(
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
            .child(
                div()
                    .debug_selector(|| format!("{name}-title"))
                    .truncate()
                    .child(title),
            )
            .child(div().text_xs().text_color(ink).truncate().child(line));
        #[expect(
            clippy::disallowed_methods,
            reason = "a card's words open the task, a hand-made press no shared builder draws"
        )]
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
}

/// The line under a board task's title: needing attention, running or
/// queued as the Tasks page sorts it, then what it waits on a person for or
/// why it ended, or else the step it is at when that is not `column`. One
/// waiting for room says only that, which *Running* would contradict.
pub(super) fn task_line(row: &Row, column: &str) -> String {
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
pub(super) fn task_offers(row: &Row) -> Vec<Option<crate::tasks_page::TaskAct>> {
    use crate::tasks_page::TaskAct;
    // A task the board holds is let go of from the board, never dismissed.
    let kept = |act: &TaskAct| match act {
        TaskAct::Dismiss => false,
        TaskAct::Extend
        | TaskAct::OpenSession
        | TaskAct::Stop
        | TaskAct::Resume
        | TaskAct::Retry => true,
    };
    std::iter::once(None)
        .chain(
            crate::tasks_page::task_acts(row, false)
                .into_iter()
                .filter(kept)
                .map(Some),
        )
        .collect()
}

/// *Remove from board* for task `id`, while it does not work: its issue goes
/// back to the Backlog and the task stays as history.
pub(super) fn removal(id: &str, cx: &Context<BoardPage>) -> Option<RowAct> {
    if crate::task::is_working(id, cx) {
        return None;
    }
    let (project, issue) = crate::task::task(id, cx)?
        .board_issue()
        .map(|(project, issue)| (project.clone(), issue))?;
    let id = id.to_string();
    let press: onehand_ui::Press = Rc::new(move |_, window, cx| {
        crate::board::remove(&project, issue, Some(&id), window, cx);
    });
    Some(RowAct::new("Remove from board", press))
}

/// A row's acts, from their words and presses.
pub(super) fn acts<'a>(
    acts: impl IntoIterator<Item = (&'a str, onehand_ui::Press)>,
) -> Rc<[RowAct]> {
    acts.into_iter()
        .map(|(label, press)| RowAct::new(label.to_string(), press))
        .collect()
}

/// A press that opens task `id`.
pub(super) fn open_task(id: &str, cx: &Context<BoardPage>) -> onehand_ui::Press {
    let (id, page) = (id.to_string(), cx.entity().downgrade());
    Rc::new(move |_, _, cx| {
        let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::OpenTask(id.clone())));
    })
}
