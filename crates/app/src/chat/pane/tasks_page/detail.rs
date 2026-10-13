//! The task detail: one task in place of the Tasks page's cards, its head,
//! what it waits on approval for, its last run's visits with what each kept
//! and changed, and its earlier runs.

use super::super::page_parts::page_section;
use super::super::step_strip::{open_review, open_revise, press_continue};
use super::super::{ChatPane, ChatPaneEvent, rel_time};
use super::{Page, Press, TasksPage, open_session, row_said, task_acts};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::task::Row;
use crate::tokens::LEADING_TIGHT;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::now_secs;
use onehand_core::diff::Row as DiffRow;
use onehand_core::task::Approval;
use onehand_core::task::Group;
use onehand_core::task::marks::{self, Change};
use onehand_core::task::work::{
    ANSWER_LINES, Act, Around, PrSeen, UnderReview, Work, last_lines, next_action,
};
use onehand_core::workflow::{Run, Visit};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How many visits a run's timeline draws, its newest ones.
const VISITS_SHOWN: usize = 100;

/// How many changed files a visit lists.
const FILES_SHOWN: usize = 100;

/// How many lines of a visit's output are drawn: their last ones, where an
/// output says how it ended.
const OUTPUT_LINES: usize = 60;

/// How many lines of one file's diff are drawn.
const DIFF_LINES: usize = 400;

/// How many earlier runs a task's detail lists, its newest ones.
const EARLIER_SHOWN: usize = 20;

/// A fold row's chevron, at the dense icon size.
const CHEVRON_W: gpui::Rems = gpui::rems(0.75);

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
pub(in crate::chat::pane) struct TaskDetail {
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

impl ChatPane {
    fn detail_mut(&mut self) -> Option<&mut TaskDetail> {
        match self.page.as_mut() {
            Some(Page::Tasks(page)) => page.open.as_mut(),
            _ => None,
        }
    }

    /// Show task `id` in place of the cards, or the cards again for `None`.
    pub(crate) fn open_task(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(Page::Tasks(page)) = self.page.as_mut() {
            page.open = id.clone().map(|id| TaskDetail {
                id,
                expanded: HashSet::new(),
                earlier_open: HashSet::new(),
                changes: HashMap::new(),
                files: HashMap::new(),
                pr: None,
            });
            cx.notify();
        }
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
                onehand_core::connector::named(crate::plugins::connectors(), name)
            });
        let (Some(forge), Some(branch)) = (forge, task.setup.branch.clone()) else {
            return;
        };
        let repo = task.setup.repo.clone();
        cx.spawn(async move |pane, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { forge.pull_request_for_blocking(&repo, &branch) })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                if let Some(detail) = pane.detail_mut().filter(|d| d.id == id) {
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
    cx: &mut Context<ChatPane>,
) -> Vec<gpui::AnyElement> {
    let muted = cx.theme().muted_foreground;
    let mut out = vec![trail(detail, task, row, cx).into_any_element()];
    out.push(
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .text_xl()
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
    if let Some(review) = UnderReview::of(last) {
        out.push(review_card(&task.id, row, &review, cx));
    }
    if row.group == Group::Ended {
        out.extend(way_out(task, cx));
    }
    // Only a task at work has a visit under way: an open visit of any other
    // was cut off, by a quit or a lost session, before it could end.
    let live = match row.group {
        Group::Running | Group::Waiting => true,
        Group::Ended | Group::Queued | Group::Finished => false,
    };
    let title = format!("Run {}", task.runs.len());
    out.push(
        page_section(title, Some(last.visits().len()), None, cx)
            .children(timeline(detail, last, live, cx))
            .into_any_element(),
    );
    if !earlier.is_empty() {
        let now = now_secs();
        let mut section = page_section("Earlier runs", Some(earlier.len()), None, cx);
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
            section = section.child(
                fold_row(
                    SharedString::from(format!("task-earlier-{}", run.id)),
                    open,
                    cx,
                )
                .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                    pane.toggle_earlier(id.clone(), cx)
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
                        .children(timeline(detail, run, false, cx)),
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
/// the task, the most fitting a primary, the next a secondary, the rest and
/// the removal of its worktree in ⋯. ⋯ is always drawn, so the removal, which
/// arrives once the pull request is read, shifts nothing.
fn trail(
    detail: &TaskDetail,
    task: &onehand_core::task::Task,
    row: &Row,
    cx: &mut Context<ChatPane>,
) -> gpui::Div {
    let back = onehand_plugin_host::back_link("task-back", "Tasks", cx)
        .on_click(cx.listener(|pane: &mut ChatPane, _, _, cx| pane.open_task(None, cx)));
    let acts = task_acts(row, true);
    let mut acts = acts.into_iter();
    let primary = acts
        .next()
        .map(|act| act.button("task-detail", row, cx).primary());
    let secondary = acts
        .next()
        .map(|act| act.button("task-detail", row, cx).secondary());
    let rest: Vec<(&'static str, Press)> =
        acts.map(|act| (act.label(), act.press(row, cx))).collect();
    let answer = answers_review(task).then(|| {
        (
            "Answer the pull request review…",
            Rc::new(super::emit(&task.id, cx, ChatPaneEvent::AnswerReview)) as Press,
        )
    });
    let remove = removable(task, detail)
        .then(|| Rc::new(super::emit(&task.id, cx, ChatPaneEvent::RemoveWorktree)) as Press);
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
            .tooltip("More actions")
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
    onehand_plugin_host::strip(cx)
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
    onehand_plugin_host::row(false, cx)
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
        return vec![muted_line("It has not started", cx)];
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
                onehand_core::duration(at.saturating_sub(visit.started_at))
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
            fold_row(
                SharedString::from(format!("task-visit-{}-{}", run.id, visit.id)),
                open,
                cx,
            )
            .on_click(cx.listener(move |pane: &mut ChatPane, _, _, cx| {
                pane.toggle_visit(key.clone(), pinned.clone(), dirs.clone(), cx)
            }))
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
            .child(slot(when, cx))
            .into_any_element(),
        );
        if open {
            out.push(
                div()
                    .v_flex()
                    .gap_2()
                    .pl(FOLD_INDENT)
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
    let mut out = Vec::new();
    if let Some(ran) = &visit.command {
        out.push(muted_line(&format!("The command {}", ran.said()), cx));
    }
    // A failed command's output is the visit's own; a passed one's only its
    // result kept.
    let printed = visit.output.as_deref().or_else(|| {
        visit
            .command
            .as_ref()
            .filter(|ran| ran.passed)
            .map(|ran| ran.tail.as_str())
    });
    if let Some(printed) = printed.filter(|text| !text.trim().is_empty()) {
        out.extend(mono_well(printed, cx));
    }
    let (Some(from), Some(to)) = (visit.start.clone(), visit.end.clone()) else {
        let said = match (visit.ended_at, live) {
            (None, true) => "In progress",
            (None, false) => "Cut off before its end was pinned",
            (Some(_), _) => "No marks were pinned for this visit",
        };
        out.push(muted_line(said, cx));
        return out;
    };
    let changes = match detail.changes.get(&(from.clone(), to.clone())) {
        Some(Some(Ok(changes))) => changes,
        Some(Some(Err(why))) => {
            out.push(warning_line(
                &format!("Reading what changed failed: {why}"),
                cx,
            ));
            return out;
        }
        Some(None) | None => {
            out.push(muted_line("Reading what changed…", cx));
            return out;
        }
    };
    if changes.is_empty() {
        out.push(muted_line("No files changed", cx));
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
        let id = SharedString::from(format!("task-file-{}-{}-{i}", run.id, visit.id));
        // A binary file has no lines to diff: it is listed, never opened, its
        // chevron's slot kept so its path starts where the others do.
        let line = match change.lines {
            Some(_) => fold_row(id, opened.is_some(), cx).on_click(cx.listener(
                move |pane: &mut ChatPane, _, _, cx| {
                    pane.toggle_file(key.clone(), dirs.clone(), cx)
                },
            )),
            None => onehand_plugin_host::row(false, cx)
                .id(id)
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().flex_none().w(CHEVRON_W)),
        };
        out.push(
            line.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(crate::tokens::CODE_TEXT)
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
                    onehand_plugin_host::well(cx)
                        .v_flex()
                        .children(drawn)
                        .into_any_element(),
                );
                if cut > 0 {
                    out.push(muted_line(&format!("{cut} more diff lines not shown"), cx));
                }
            }
            Some(Some(Err(why))) => {
                out.push(warning_line(&format!("Reading the diff failed: {why}"), cx))
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
/// that fits first.
fn way_out(
    task: &onehand_core::task::Task,
    cx: &mut Context<ChatPane>,
) -> Option<gpui::AnyElement> {
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
    let buttons = offered.into_iter().map(|act| {
        let button = crate::controls::action(SharedString::from(format!("task-way-{act:?}")))
            .small()
            .label(act.label());
        let button = match Some(act) == next.primary {
            true => button.primary(),
            false => button.quiet(cx),
        };
        button.on_click(super::emit(&task.id, cx, ChatPaneEvent::RetryTaskCurrent))
    });
    Some(
        page_section("Way out", None, None, cx)
            .child(div().text_sm().child(said))
            .child(div().h_flex().gap_2().children(buttons))
            .into_any_element(),
    )
}

/// What the task's run waits for approval on: the answer, what each answer
/// starts, and the two answers, each carrying the visit it was drawn from.
fn review_card(
    task: &str,
    row: &Row,
    review: &UnderReview,
    cx: &mut Context<ChatPane>,
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
    let pane = cx.entity().downgrade();
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
    page_section("Awaiting approval", None, open, cx)
        .children(said)
        .children(cut)
        .child(
            div().h_flex().child(
                crate::controls::action("task-review-read")
                    .quiet(cx)
                    .small()
                    .label("Review…")
                    .on_click({
                        let pane = pane.clone();
                        move |_, window, cx| {
                            open_review(&on_read, pane.clone(), read.clone(), false, window, cx)
                        }
                    }),
            ),
        )
        .child(answer_row(
            crate::controls::action("task-review-revise")
                .quiet(cx)
                .small()
                .label("Revise…")
                .on_click(
                    cx.listener(move |_, _, window, cx| open_revise(revised.clone(), window, cx)),
                ),
            review.revise_said(),
        ))
        .child(answer_row(
            crate::controls::action("task-review-continue")
                .primary()
                .small()
                .label("Continue")
                .on_click(move |_, window, cx| press_continue(pane.clone(), &pressed, window, cx)),
            review.continue_said(),
        ))
        .into_any_element()
}

/// `text`'s last lines in a mono well, saying how many were left out.
fn mono_well(text: &str, cx: &App) -> Vec<gpui::AnyElement> {
    let (shown, hidden) = last_lines(text, OUTPUT_LINES);
    let well = onehand_plugin_host::well(cx)
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

/// A read that failed, said in the warning ink.
fn warning_line(text: &str, cx: &App) -> gpui::AnyElement {
    div()
        .text_xs()
        .text_color(crate::theme::status_ink(cx).warning)
        .child(text.to_string())
        .into_any_element()
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

/// Where `run`'s marks are read.
fn dirs(run: &Run) -> Dirs {
    (run.setup.dir.clone(), run.setup.repo.clone())
}
