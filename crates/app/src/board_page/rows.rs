//! A board's rows: an issue's, a task's, and the row both are drawn in,
//! with what a task's row offers.

use super::{BoardPage, BoardPageEvent, RowKey};
use crate::task::Row;
use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::task::Section;
use onehand_ui::{Refuses as _, RowAct, Short as _};
use std::rc::Rc;

/// A row's one act drawn beside its `⋯`, for a row waiting on a person: its
/// words, its press, and why it refuses when it does.
pub(super) struct Button {
    pub(super) label: String,
    pub(super) press: onehand_ui::Press,
    pub(super) refused: Option<String>,
}

impl BoardPage {
    /// A row of an issue no task works: its title over `line`, its acts in
    /// its `⋯` and on a right-click, `button` drawn beside them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn issue_row(
        &mut self,
        number: u64,
        title: String,
        line: (String, gpui::Hsla),
        acts: Rc<[RowAct]>,
        button: Option<Button>,
        drags: impl FnOnce(Stateful<gpui::Div>) -> Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let drags = move |row: Stateful<gpui::Div>| {
            drags(row.debug_selector(move || format!("board-issue-{number}")))
        };
        self.row(
            RowKey::Issue(number),
            title,
            line,
            acts,
            button,
            None,
            drags,
            cx,
        )
    }

    /// A row of a task: its title over `line`, else what it waits on as the
    /// Tasks page sorts it and its detail says it. Pressed, it opens the
    /// task; its `⋯` and right-click offer the Tasks page's acts, then
    /// `more`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn task_row(
        &mut self,
        row: Row,
        line: Option<String>,
        more: Vec<RowAct>,
        button: Option<Button>,
        drags: impl FnOnce(Stateful<gpui::Div>) -> Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (said, ink) = match line {
            Some(line) => (line, cx.theme().muted_foreground),
            None => {
                let ink = match row.group.section() == Section::NeedsAttention {
                    true => crate::theme::status_ink(cx).warning,
                    false => cx.theme().muted_foreground,
                };
                (task_line(&row, ""), ink)
            }
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
            .chain(more)
            .collect();
        let key = RowKey::Task(row.id.clone());
        self.row(
            key,
            row.title,
            (said, ink),
            acts,
            button,
            Some(open),
            drags,
            cx,
        )
    }

    /// A row in a column: `title` over `line`, a tab stop whose `⋯` and
    /// right-click offer `acts`, `button` drawn beside them; pressing its
    /// text runs `open` if it has one. `drags` adds what the row does under a
    /// drag.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        key: RowKey,
        title: String,
        (line, ink): (String, gpui::Hsla),
        acts: Rc<[RowAct]>,
        button: Option<Button>,
        open: Option<onehand_ui::Press>,
        drags: impl FnOnce(Stateful<gpui::Div>) -> Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focus = self.handle(key.clone(), cx);
        let (name, what) = match &key {
            RowKey::Issue(n) => (format!("board-issue-{n}"), "Issue actions"),
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
        // The act a person is waited on for stays drawn, as the Tasks page's
        // rows needing attention keep theirs.
        let button = button.map(
            |Button {
                 label,
                 press,
                 refused,
             }| {
                let selector = format!("{name}-button");
                div()
                    .flex_none()
                    .debug_selector(move || selector.clone())
                    .child(
                        crate::controls::action(SharedString::from(format!("{name}-act")))
                            .short()
                            .secondary()
                            .label(label)
                            .refuses(refused)
                            .on_click(move |event, window, cx| press(event, window, cx)),
                    )
            },
        );
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
            .children(button)
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

/// A press that opens task `id`.
pub(super) fn open_task(id: &str, cx: &Context<BoardPage>) -> onehand_ui::Press {
    let (id, page) = (id.to_string(), cx.entity().downgrade());
    Rc::new(move |_, _, cx| {
        let _ = page.update(cx, |_, cx| cx.emit(BoardPageEvent::OpenTask(id.clone())));
    })
}
