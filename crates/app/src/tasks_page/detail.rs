//! The task detail: one task in place of the Tasks page's cards, its head,
//! what it waits on approval for, its last run's visits with what each kept
//! and changed, and its earlier runs.

use super::{Press, TasksPage, TasksPageEvent, open_session, row_said, task_acts};
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::page_parts::page_card;
use crate::task::Row;
use crate::task::approval::{open_review, open_revise, press_continue};
use crate::tokens::LEADING_TIGHT;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::now_secs;
use onehand_core::diff::Row as DiffRow;
use onehand_core::rel_time;
use onehand_core::task::Approval;
use onehand_core::task::Group;
use onehand_core::task::marks::{self, Change};
use onehand_core::task::work::{
    ANSWER_LINES, Act, Around, PrSeen, UnderReview, Work, last_lines, next_action,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod visits;

/// How many lines of a visit's output are drawn: their last ones, where an
/// output says how it ended.
const OUTPUT_LINES: usize = 60;

/// How many earlier runs a task's detail lists, its newest ones.
const EARLIER_SHOWN: usize = 20;

/// Where what a fold row opens onto starts: past the row's inset, its
/// chevron and the gap after it, on the label's axis.
const FOLD_INDENT: gpui::Rems = gpui::rems(0.5 + 0.75 + 0.5);

/// The slot a row's time stands in: *2d · 14m 3s* at most.
const TIME_W: gpui::Rems = gpui::rems(7.);

/// Something read off the UI thread: `None` while it is being read.
type Loaded<T> = Option<Result<T, String>>;

/// Where a run's marks are read: its worktree, then its project.
type Dirs = (PathBuf, PathBuf);

/// The task a detail shows, and what of it is opened.
pub(super) struct TaskDetail {
    pub(super) id: String,
    /// Visits opened, by run id and visit id.
    expanded: HashSet<(String, u32)>,
    /// Earlier runs opened, by id.
    earlier_open: HashSet<String>,
    /// What changed between two marks, once asked for.
    changes: HashMap<(String, String), Loaded<Vec<Change>>>,
    /// Files opened, by the two marks and the path, with their diffs.
    files: HashMap<(String, String, String), Loaded<Vec<DiffRow>>>,
    /// The pull request on the task's branch, read off the UI thread when
    /// the detail opens on a task that opened one: what its issue's view
    /// judges the ways out by.
    pr: Loaded<Option<onehand_core::connector::PullRequest>>,
}

impl TasksPage {
    fn detail_mut(&mut self) -> Option<&mut TaskDetail> {
        self.open.as_mut()
    }

    /// Show task `id` in place of the cards, or the cards again for `None`.
    pub(crate) fn open_task(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.open = id.clone().map(|id| TaskDetail {
            id,
            expanded: HashSet::new(),
            earlier_open: HashSet::new(),
            changes: HashMap::new(),
            files: HashMap::new(),
            pr: None,
        });
        cx.notify();
        if let Some(id) = id {
            self.read_pr(id, cx);
        }
    }

    /// Read the pull request on task `id`'s branch, when it opened one.
    fn read_pr(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(task) = crate::task::task(&id, cx).filter(|t| t.opened_pull_request()) else {
            return;
        };
        let forge =
            task.setup.forge.as_deref().and_then(|name| {
                onehand_core::connector::named(crate::features::connectors(), name)
            });
        let (Some(forge), Some(branch)) = (forge, task.setup.branch.clone()) else {
            return;
        };
        let repo = task.setup.repo.clone();
        cx.spawn(async move |page, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { forge.pull_request_for_blocking(&repo, &branch) })
                .await;
            let _ = page.update(cx, |page: &mut Self, cx| {
                if let Some(detail) = page.detail_mut().filter(|d| d.id == id) {
                    detail.pr = Some(read);
                    cx.notify();
                }
            });
        })
        .detach();
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
        cx.spawn(async move |page, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read(if dir.is_dir() { &dir } else { &repo }) })
                .await;
            let _ = page.update(cx, |page: &mut Self, cx| {
                if let Some(detail) = page.detail_mut() {
                    land(detail, read);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Task `task`, listed as `row`, read as a page: a trail with the way back
/// to the cards and what can be done with it (one primary, one other, the
/// rest in ⋯), its title and one line of facts, what it waits on approval
/// for, its way out, its last run's timeline, then its earlier runs, each a
/// plain section.
pub(super) fn task_detail(
    detail: &TaskDetail,
    task: &onehand_core::task::Task,
    row: &Row,
    page: &TasksPage,
    cx: &mut Context<TasksPage>,
) -> Vec<gpui::AnyElement> {
    let muted = cx.theme().muted_foreground;
    let last = task.runs.last();
    // What waits on the person below holds the view's one primary while it
    // shows, so the trail's first action steps down beside it.
    let review = last
        .and_then(UnderReview::of)
        .map(|review| review_card(&task.id, row, &review, cx));
    let way = match (last, row.group) {
        (Some(_), Group::Ended) => way_out(task, cx),
        _ => None,
    };
    let held = review.is_some() || way.as_ref().is_some_and(|(_, primary)| *primary);
    let mut out = vec![trail(detail, task, row, held, cx).into_any_element()];
    out.push(
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_semibold()
                    .line_height(gpui::relative(LEADING_TIGHT))
                    .child(row.title.clone()),
            )
            .child(div().text_xs().text_color(muted).child(row_said(row, page)))
            .into_any_element(),
    );
    let Some((last, earlier)) = task.runs.split_last() else {
        return out;
    };
    out.extend(review);
    out.extend(way.map(|(section, _)| section));
    // Only a task at work has a visit under way: an open visit of any other
    // was cut off, by a quit or a lost session, before it could end.
    let live = match row.group {
        Group::Running | Group::Waiting => true,
        Group::Ended | Group::Queued | Group::Finished => false,
    };
    let title = format!("Run {}", task.runs.len());
    out.push(
        page_card(title, Some(last.visits().len()), None, cx)
            .children(visits::timeline(detail, last, live, cx))
            .into_any_element(),
    );
    if !earlier.is_empty() {
        let now = now_secs();
        let mut section = page_card("Earlier runs", Some(earlier.len()), None, cx);
        for (n, run) in earlier.iter().enumerate().rev().take(EARLIER_SHOWN) {
            let open = detail.earlier_open.contains(&run.id);
            let ended = match &run.outcome {
                Some(outcome) => outcome.said_in(&run.template),
                None => "Cut off".to_string(),
            };
            let at = run
                .history
                .last()
                .map_or_else(String::new, |t| rel_time(now, t.at));
            let id = run.id.clone();
            section = section.child(
                fold_row(
                    SharedString::from(format!("task-earlier-{}", run.id)),
                    open,
                    cx,
                )
                .on_click(cx.listener(move |page: &mut TasksPage, _, _, cx| {
                    page.toggle_earlier(id.clone(), cx)
                }))
                .child(
                    div()
                        .flex_none()
                        .font_semibold()
                        .child(format!("Run {}", n + 1)),
                )
                .child(div().flex_1().min_w_0().truncate().child(ended))
                .child(slot(at, cx)),
            );
            if open {
                section = section.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .pl(FOLD_INDENT)
                        .children(visits::timeline(detail, run, false, cx)),
                );
            }
        }
        let hidden = earlier.len().saturating_sub(EARLIER_SHOWN);
        if hidden > 0 {
            section = section.child(muted_line(&format!("{hidden} older runs not shown"), cx));
        }
        out.push(section.into_any_element());
    }
    out
}

/// The detail's trail: the way back to the cards, then what can be done with
/// the task, the most fitting a primary (a secondary while `held`: a section
/// below holds the view's one primary), the next a secondary, the rest and
/// the removal of its worktree in ⋯. ⋯ is always drawn, so the removal, which
/// arrives once the pull request is read, shifts nothing.
fn trail(
    detail: &TaskDetail,
    task: &onehand_core::task::Task,
    row: &Row,
    held: bool,
    cx: &mut Context<TasksPage>,
) -> gpui::Div {
    let project = row
        .project
        .file_name()
        .map(|name| SharedString::from(name.to_string_lossy().into_owned()));
    let back = onehand_ui::crumbs(
        onehand_ui::crumb_link("task-back", "Tasks", cx)
            .on_click(cx.listener(|page: &mut TasksPage, _, _, cx| page.open_task(None, cx))),
        project
            .into_iter()
            .chain([SharedString::from(row.name.clone())]),
        cx,
    );
    let acts = task_acts(row, true);
    let mut acts = acts.into_iter();
    let primary = acts.next().map(|act| {
        let button = act.button("task-detail", row, cx);
        match held {
            true => button.secondary(),
            false => button.primary(),
        }
    });
    let secondary = acts
        .next()
        .map(|act| act.button("task-detail", row, cx).secondary());
    let rest: Vec<(&'static str, Press)> =
        acts.map(|act| (act.label(), act.press(row, cx))).collect();
    let answer = answers_review(task).then(|| {
        (
            "Answer the pull request review…",
            Rc::new(super::emit(&task.id, cx, TasksPageEvent::AnswerReview)) as Press,
        )
    });
    let remove = removable(task, detail)
        .then(|| Rc::new(super::emit(&task.id, cx, TasksPageEvent::RemoveWorktree)) as Press);
    let rest: Vec<_> = rest.into_iter().chain(answer).collect();
    let empty = rest.is_empty() && remove.is_none();
    // What ⋯ holds and is still being read or done waits on ⋯ itself.
    let waits = matches!(
        crate::task::pressed(&task.id, cx),
        Some(Act::AnswerReview | Act::RemoveWorktree)
    );
    let danger = crate::theme::status_ink(cx).danger;
    let more = crate::controls::menu_below(
        SharedString::from(format!("task-more-{}", task.id)),
        crate::controls::action("task-more")
            .small()
            .quiet_icon(false, cx)
            .icon(IconName::Ellipsis)
            .named("More actions")
            .loading(waits)
            .refuses(empty.then_some("Nothing more to do with this task")),
        move |mut menu, _, _| {
            for (label, press) in &rest {
                let press = press.clone();
                menu = menu.item(
                    crate::controls::menu_item(*label)
                        .on_click(move |event, window, cx| press(event, window, cx)),
                );
            }
            if let Some(press) = &remove {
                let press = press.clone();
                if !rest.is_empty() {
                    menu = menu.separator();
                }
                menu = menu.item(
                    crate::controls::menu_row(move |_, _| {
                        div().text_color(danger).child(Act::RemoveWorktree.label())
                    })
                    .on_click(move |event, window, cx| press(event, window, cx)),
                );
            }
            menu
        },
    );
    onehand_ui::strip(cx)
        .px_0()
        .child(back)
        .child(div().flex_1())
        .children(primary)
        .children(secondary)
        .child(more)
}

/// A row that opens what is under it: the chevron first, in its own slot, so
/// what it opens onto starts on the label's axis, [`FOLD_INDENT`] in.
fn fold_row(id: SharedString, open: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    onehand_ui::row(false, cx)
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            Icon::new(chevron(open))
                .xsmall()
                .flex_none()
                .text_color(cx.theme().muted_foreground),
        )
}

/// A time or an age at a row's end, in a slot that keeps its width.
fn slot(text: String, cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .w(TIME_W)
        .h_flex()
        .justify_end()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// Whether `task` offers to answer a review on its pull request: one it can
/// have (`Task::reviewable`) that opened one, its last run done. Whether it
/// is still open, and whether the review can be answered, is the
/// preflight's to say when pressed.
fn answers_review(task: &onehand_core::task::Task) -> bool {
    task.reviewable()
        && task.opened_pull_request()
        && task
            .runs
            .last()
            .is_some_and(|run| run.outcome == Some(onehand_core::workflow::Outcome::Done))
}

/// Whether its issue's view would offer removing `task`'s worktree, by
/// core's rule, given the pull request `detail` read: never before it is
/// read and merged.
fn removable(task: &onehand_core::task::Task, detail: &TaskDetail) -> bool {
    let Some(Ok(pr)) = &detail.pr else {
        return false;
    };
    Work::of(task, None, None).removal_offered(pr.as_ref())
}

/// An ended task's way out, as its issue says it (`next_action`): why it
/// ended, and the ways out the head's actions do not already offer, the one
/// that fits first. Answers whether it drew a primary, which the trail then
/// steps down beside.
fn way_out(
    task: &onehand_core::task::Task,
    cx: &mut Context<TasksPage>,
) -> Option<(gpui::AnyElement, bool)> {
    let mut work = Work::of(task, None, None);
    work.timeout_moved = crate::unattended::timeout_moved(task, cx);
    let around = Around {
        open: true,
        can_start: false,
        session: false,
        pr: PrSeen::Unread,
        now: now_secs(),
    };
    let next = next_action(Some(&work), around);
    let said = next.said?;
    let offered: Vec<Act> = next
        .primary
        .into_iter()
        .chain(next.secondary)
        .filter(|act| matches!(act, Act::RetryCurrent))
        .collect();
    let primary = offered.iter().any(|act| Some(*act) == next.primary);
    let buttons = offered.into_iter().map(|act| {
        let button = crate::controls::action(SharedString::from(format!("task-way-{act:?}")))
            .short()
            .label(act.label());
        let button = match Some(act) == next.primary {
            true => button.primary(),
            false => button.quiet(cx),
        };
        button.on_click(super::emit(&task.id, cx, TasksPageEvent::RetryTaskCurrent))
    });
    Some((
        page_card("Way out", None, None, cx)
            .child(div().text_size(crate::tokens::CHROME_TEXT).child(said))
            .child(div().h_flex().gap_2().children(buttons))
            .into_any_element(),
        primary,
    ))
}

/// What the task's run waits for approval on: the answer, what each answer
/// starts, and the two answers, each carrying the visit it was drawn from.
fn review_card(
    task: &str,
    row: &Row,
    review: &UnderReview,
    cx: &mut Context<TasksPage>,
) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let open = open_session("task-review-open", row, cx).map(IntoElement::into_any_element);
    let (shown, left_out) = last_lines(&review.answer, ANSWER_LINES);
    let said = match review.answer.trim().is_empty() {
        true => vec![muted_line("The step kept no answer", cx)],
        false => mono_well(shown, cx),
    };
    let lines = review.answer.lines().count();
    let cut = (left_out > 0).then(|| {
        div()
            .text_xs()
            .text_color(crate::theme::status_ink(cx).warning)
            .child(format!(
                "Showing the last {ANSWER_LINES} of {lines} lines; Review… opens all of it"
            ))
    });
    let approval = Approval {
        task: task.to_string(),
        at: review.at.clone(),
    };
    let (on_read, revised, pressed, read) =
        (task.to_string(), approval.clone(), approval, review.clone());
    let answer_row = |button: gpui_component::button::Button, said: String| {
        div()
            .h_flex()
            .gap_2()
            .items_center()
            .child(button)
            .child(div().text_xs().text_color(muted).child(said))
    };
    page_card("Awaiting approval", None, open, cx)
        .children(said)
        .children(cut)
        .child(
            div().h_flex().child(
                crate::controls::action("task-review-read")
                    .quiet(cx)
                    .short()
                    .label("Review…")
                    .on_click(move |_, window, cx| {
                        open_review(&on_read, read.clone(), false, window, cx)
                    }),
            ),
        )
        .child(answer_row(
            crate::controls::action("task-review-revise")
                .quiet(cx)
                .short()
                .label("Revise…")
                .on_click(move |_, window, cx| open_revise(revised.clone(), window, cx)),
            review.revise_said(),
        ))
        .child(answer_row(
            crate::controls::action("task-review-continue")
                .primary()
                .short()
                .label("Continue")
                .on_click(move |_, window, cx| press_continue(&pressed, window, cx)),
            review.continue_said(),
        ))
        .into_any_element()
}

/// `text`'s last lines in a mono well, saying how many were left out.
fn mono_well(text: &str, cx: &App) -> Vec<gpui::AnyElement> {
    let (shown, hidden) = last_lines(text, OUTPUT_LINES);
    let well = onehand_ui::well(cx)
        .v_flex()
        .p_2()
        .children(shown.lines().map(|line| {
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
