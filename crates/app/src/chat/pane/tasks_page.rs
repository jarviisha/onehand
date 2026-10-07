use super::workspace_page::{ALL_PROJECTS, PAGE_WIDE, PageProject, card_box, page_card};
use super::{ChatPane, ChatPaneEvent, Page};
use crate::task::Row;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::task::{Group, Section};
use std::path::{Path, PathBuf};

/// How many rows each card of the Tasks page draws. Finished tasks are kept
/// by the hundred, and nobody reads that far down a card.
const TASKS_SHOWN: usize = 50;

mod detail;
pub(super) use detail::TaskDetail;
use detail::task_detail;

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
            needs_attention,
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

/// A press that announces `event` about task `id`.
pub(super) fn emit(
    id: &str,
    cx: &Context<ChatPane>,
    event: fn(String) -> ChatPaneEvent,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + use<> {
    let (id, pane) = (id.to_string(), cx.entity().downgrade());
    move |_, _, cx| {
        let _ = pane.update(cx, |_, cx| cx.emit(event(id.clone())));
    }
}
