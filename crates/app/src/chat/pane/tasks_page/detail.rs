//! The task detail: one task in place of the Tasks page's cards, its head,
//! what it waits on approval for, its last run's visits with what each kept
//! and changed, and its earlier runs.

use super::super::step_strip::{open_review, open_revise, press_continue};
use super::super::workspace_page::{card_box, page_card};
use super::super::{ChatPane, ChatPaneEvent, rel_time};
use super::{Page, TasksPage, open_session, row_actions, row_said};
use crate::task::Row;
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
}

/// Task `task`, listed as `row`: a way back to the cards, its head with what
/// can be done with it, what it waits on approval for, its last run's
/// timeline, then its earlier runs.
pub(super) fn task_detail(
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
        .children(row_actions("task-detail", row, true, cx))
        .children(answers_review(task).then(|| {
            crate::controls::action("task-answer-review")
                .ghost()
                .small()
                .label("Answer the pull request review")
                .tooltip(
                    "Run it again from its repair step with the review as its note, as putting \
                     the label back does",
                )
                .on_click(super::emit(&task.id, cx, ChatPaneEvent::AnswerReview))
        }));
    let mut out = vec![
        div().h_flex().child(back).into_any_element(),
        head.into_any_element(),
    ];
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

/// Whether `task` offers to answer a review on its pull request: one it can
/// have (`Task::reviewable`), its last run done having opened a pull request.
/// Whether it is still open, and whether the review can be answered, is the
/// preflight's to say when pressed.
fn answers_review(task: &onehand_core::task::Task) -> bool {
    task.reviewable()
        && task.runs.last().is_some_and(|run| {
            run.outcome == Some(onehand_core::workflow::Outcome::Done) && run.pull_request.is_some()
        })
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
            false => button.ghost(),
        };
        button.on_click(super::emit(&task.id, cx, ChatPaneEvent::RetryTaskCurrent))
    });
    Some(
        page_card("Way out", None, None, cx)
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
        true => vec![muted_line("The step kept no answer.", cx)],
        false => mono_well(shown, cx),
    };
    let lines = review.answer.lines().count();
    let cut = (left_out > 0).then(|| {
        div()
            .text_xs()
            .text_color(crate::theme::status_ink(cx).warning)
            .child(format!(
                "Showing the last {ANSWER_LINES} of {lines} lines; Review… opens all of it."
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
    page_card("Awaiting approval", None, open, cx)
        .children(said)
        .children(cut)
        .child(
            div().h_flex().child(
                crate::controls::action("task-review-read")
                    .ghost()
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
                .ghost()
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
                .icon(Icon::new(IconName::Check))
                .label("Continue")
                .on_click(move |_, window, cx| press_continue(pane.clone(), &pressed, window, cx)),
            review.continue_said(),
        ))
        .into_any_element()
}

/// `text`'s last lines in a mono well, saying how many were left out.
fn mono_well(text: &str, cx: &App) -> Vec<gpui::AnyElement> {
    let (shown, hidden) = last_lines(text, OUTPUT_LINES);
    let well = div()
        .v_flex()
        .w_full()
        .min_w_0()
        .p_2()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
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

/// Where `run`'s marks are read.
fn dirs(run: &Run) -> Dirs {
    (run.setup.dir.clone(), run.setup.repo.clone())
}
