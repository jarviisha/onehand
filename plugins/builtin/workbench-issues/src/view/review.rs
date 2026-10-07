//! The review block on the Issues page: what a run waiting for approval is
//! judged on, below where its work stands, and the two answers to it.
//!
//! Opened only by a person pressing *Review…*, never by a run reaching an
//! approval, and closed only by a person: closing it, or picking another
//! issue. It draws what was read when it was opened, not what the run says
//! now, so the answer never changes under the reader; a press the run no
//! longer waits for reloads it and says so, rather than approving what was
//! not read. Every press carries the visit it was drawn from, which the run
//! checks again.

use super::IssuesView;
use super::full::{Full, Side, files_view};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::text::TextView;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::template::section;
use onehand_core::issues::{IssueKey, LocalIssue};
use onehand_core::task::Approval;
use onehand_core::task::marks::{self, Change};
use onehand_core::task::work::{
    ANSWER_CHANGED, ANSWER_LINES, Checked, IssueWork, Reading, UnderReview, Work, last_lines,
};
use onehand_plugin_host::{Request, action, status_ink};
use std::path::{Path, PathBuf};

/// How many lines of what a check printed are drawn: its last ones, where a
/// build or a test run says how it ended.
const CHECK_LINES: usize = 20;

/// How tall the block grows before it scrolls, in rems: the body stays in
/// reach below it.
const BLOCK_MAX_H: f32 = 32.;

/// What a press sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
    Continue,
    Revise,
}

/// The review block's state on the page.
#[derive(Default)]
pub(super) struct ReviewState {
    /// The issue it is open on; `None` while closed.
    open: Option<IssueKey>,
    /// The task whose run it reviews.
    task: Option<String>,
    /// The folder its work is read in.
    dir: Option<PathBuf>,
    /// What was read when it was opened, or reloaded by a refused press.
    read: Option<UnderReview>,
    /// A press found the run no longer waiting where it was read.
    changed: bool,
    /// What a press sent, once it went.
    sent: Option<Sent>,
    /// The whole answer is drawn, not only its last lines.
    whole: bool,
    /// The issue's acceptance is open.
    acceptance: bool,
    /// The note for *Revise…*, while it is written; `refused` while it was
    /// sent empty.
    note: Option<Entity<TextareaState>>,
    refused: bool,
    /// What the step under review changed, as read off git.
    files: Reading<(String, String), Vec<Change>>,
}

impl ReviewState {
    /// Whether it is open on the issue `key`.
    pub(super) fn open_on(&self, key: Option<&IssueKey>) -> bool {
        self.open.is_some() && self.open.as_ref() == key
    }
}

impl IssuesView {
    /// Open the review of issue `number` of `root` from outside the page: the
    /// issue is put on the page first.
    pub(crate) fn review_on_page(&mut self, root: &Path, number: u64, cx: &mut Context<Self>) {
        let key = self.key(root, number);
        let work = key
            .as_ref()
            .and_then(|key| self.works.iter().find(|work| work.key == *key))
            .cloned();
        if let (Some(key), Some(work)) = (key, work) {
            self.open_review(key, &work.work, cx);
        }
    }

    /// *Review…* on issue `number` of the project on screen: the block on the
    /// page; from the tab, the issue on the page with its block open, since
    /// the review is never drawn at the dock's width.
    pub(super) fn review(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_none() {
            self.ask_later(window, cx, move |ask, root, window, cx| {
                ask(&Request::ReviewInIssues { root, number }, window, cx)
            });
            return;
        }
        if let Some(root) = self.root.clone() {
            self.review_on_page(&root, number, cx);
        }
    }

    /// Open the review of `work`, the work of issue `key`, as it waits now.
    pub(super) fn open_review(&mut self, key: IssueKey, work: &Work, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        page.review = ReviewState {
            open: Some(key),
            task: Some(work.task.clone()),
            dir: Some(work.dir.clone()),
            read: work.review.clone(),
            ..ReviewState::default()
        };
        self.read_review_files(cx);
        cx.notify();
    }

    /// Close the block.
    pub(super) fn close_review(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            page.review = ReviewState::default();
            cx.notify();
        }
    }

    /// Read what the step under review changed, when it is pinned at both
    /// ends.
    fn read_review_files(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let review = &mut page.review;
        let (Some(span), Some(dir)) = (
            review.read.as_ref().and_then(|read| read.span.clone()),
            review.dir.clone(),
        ) else {
            return;
        };
        let generation = review.files.ask(span.clone());
        cx.spawn(async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { marks::changes_blocking(&dir, &span.0, &span.1) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if let Some(page) = view.page.as_mut()
                    && page
                        .review
                        .files
                        .land(generation, read, onehand_core::issues::now())
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// What the run of the block's issue waits for approval on now.
    fn waits_now(&self) -> Option<UnderReview> {
        let review = &self.page.as_ref()?.review;
        let key = review.open.as_ref()?;
        let work = self.works.iter().find(|work| work.key == *key)?;
        (Some(&work.work.task) == review.task.as_ref())
            .then(|| work.work.review.clone())
            .flatten()
    }

    /// Whether the run still waits where the block read it. When it waits
    /// somewhere else, the block reloads to what it waits on now and says
    /// so; a run no longer waiting is said by the status line.
    fn still_where_read(&mut self, cx: &mut Context<Self>) -> bool {
        let now = self.waits_now();
        let Some(page) = self.page.as_mut() else {
            return false;
        };
        let review = &mut page.review;
        let read = review.read.as_ref().map(|read| &read.at);
        match now {
            Some(now) if Some(&now.at) == read => true,
            Some(now) => {
                review.read = Some(now);
                review.changed = true;
                review.whole = false;
                self.read_review_files(cx);
                cx.notify();
                false
            }
            None => {
                review.changed = true;
                cx.notify();
                false
            }
        }
    }

    /// *Continue*, on what was read.
    fn press_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.still_where_read(cx) {
            return;
        }
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let review = &mut page.review;
        let (Some(task), Some(read)) = (review.task.clone(), review.read.clone()) else {
            return;
        };
        review.sent = Some(Sent::Continue);
        review.note = None;
        cx.notify();
        let approval = Approval { task, at: read.at };
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&Request::ApproveTask(&approval), window, cx)
        });
    }

    /// *Revise…*: the note is written in the block.
    fn start_revise(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let note = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("What should be done differently?")
        });
        note.update(cx, |input, cx| input.focus(window, cx));
        if let Some(page) = self.page.as_mut() {
            page.review.note = Some(note);
            page.review.refused = false;
            cx.notify();
        }
    }

    fn cancel_revise(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            page.review.note = None;
            cx.notify();
        }
    }

    /// Send the note back, on what was read; an empty one is refused, and a
    /// run no longer waiting there keeps the note to send again once read.
    fn send_revise(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let Some(note) = page.review.note.clone() else {
            return;
        };
        let text = note.read(cx).value().trim().to_string();
        if text.is_empty() {
            page.review.refused = true;
            cx.notify();
            return;
        }
        if !self.still_where_read(cx) {
            return;
        }
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let review = &mut page.review;
        let (Some(task), Some(read)) = (review.task.clone(), review.read.clone()) else {
            return;
        };
        review.sent = Some(Sent::Revise);
        review.note = None;
        cx.notify();
        let approval = Approval { task, at: read.at };
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(
                &Request::ReviseTask {
                    approval: &approval,
                    note: &text,
                },
                window,
                cx,
            )
        });
    }

    fn toggle_review(&mut self, flip: fn(&mut ReviewState), cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            flip(&mut page.review);
            cx.notify();
        }
    }

    /// The block, below where `work` stands, while it is open on `issue`.
    pub(super) fn review_block(
        &self,
        issue: &LocalIssue,
        key: Option<&IssueKey>,
        work: Option<&IssueWork>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let page = self.page.as_ref()?;
        let review = &page.review;
        if !review.open_on(key) {
            return None;
        }
        let read = review.read.as_ref()?;
        let muted = cx.theme().muted_foreground;
        let ink = status_ink(cx);
        let full = page.full();
        let work = work.map(|work| &work.work);

        let head = div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .child(format!("Review: {}", read.of)),
            )
            .child(
                action("issue-review-close")
                    .xsmall()
                    .ghost()
                    .label("Close")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.close_review(cx))),
            );

        let mut block = div()
            .id("issue-review")
            .flex_none()
            .max_h(rems(BLOCK_MAX_H))
            .overflow_y_scroll()
            .v_flex()
            .gap_2()
            .mx_3()
            .my_2()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(head);
        if review.changed && review.sent.is_none() {
            block = block.child(div().text_color(ink.warning).child(ANSWER_CHANGED));
        }

        // The answer, its last lines until all of it is asked for.
        let total = read.answer.lines().count();
        let (shown, left_out) = match review.whole {
            true => (read.answer.as_str(), 0),
            false => last_lines(&read.answer, ANSWER_LINES),
        };
        let (shown, cut) = (shown.to_string(), left_out > 0);
        block = block.child(match read.answer.trim().is_empty() {
            true => div()
                .text_color(muted)
                .child("The step kept no answer.")
                .into_any_element(),
            false => div()
                .v_flex()
                .gap_1()
                .min_w_0()
                .when(cut, |column| {
                    column.child(
                        action("issue-review-whole")
                            .xsmall()
                            .ghost()
                            .label(format!("Show all {total} lines"))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.toggle_review(|review| review.whole = true, cx)
                            })),
                    )
                })
                .child(
                    TextView::markdown("issue-review-answer", shown)
                        .selectable(true)
                        .into_any_element(),
                )
                .into_any_element(),
        });

        // What the step under review changed, drawn only when it changed
        // something; a plan that changed no file draws no files and no check.
        let files = review
            .files
            .value
            .as_ref()
            .map(|(files, _)| files)
            .filter(|files| !files.is_empty());
        if let (Some(files), Some(dir), Some(span)) = (files, &review.dir, &read.span) {
            block = block.child(labelled(
                "Changed",
                files_view(&full, Side::Review, files, dir, Some(span.clone()), cx),
                cx,
            ));
        } else if let Some(why) = review.files.failed.as_deref() {
            block = block.child(labelled(
                "Changed",
                div()
                    .text_color(ink.warning)
                    .child(format!("could not be read: {why}"))
                    .into_any_element(),
                cx,
            ));
        }

        // The check that ran since the step under review began, passed or
        // failed; none ran after a plan, and none is drawn.
        if let Some(line) = check_line(&full, &read.check, work, cx) {
            block = block.child(labelled("Check", line, cx));
        }

        // How the work will be judged, one click away.
        if let Some(acceptance) = section(&issue.body, "Acceptance") {
            let open = review.acceptance;
            block = block.child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .id("issue-review-acceptance")
                            .h_flex()
                            .gap_1()
                            .items_center()
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(|row| row.underline())
                            .child(
                                Icon::new(if open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .xsmall(),
                            )
                            .child("Acceptance")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.toggle_review(|r| r.acceptance = !r.acceptance, cx)
                            })),
                    )
                    .when(open, |column| {
                        column.child(TextView::markdown(
                            "issue-review-acceptance-text",
                            acceptance,
                        ))
                    }),
            );
        }

        block = block.child(answers(review, read, work, cut, total, cx));
        Some(block.into_any_element())
    }
}

/// A line of the block: what it is, then what it says.
fn labelled(name: &'static str, value: AnyElement, cx: &Context<IssuesView>) -> AnyElement {
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .w(rems(5.))
                .text_color(cx.theme().muted_foreground)
                .child(name),
        )
        .child(div().flex_1().min_w_0().child(value))
        .into_any_element()
}

/// The check that ran since the step under review began: a pass as the
/// full form last read it against the work now, a failure as it exited, and
/// either way the last lines it printed. One that ran before results were
/// kept is judged by the commit alone, as the work's own check says it
/// (*not recorded*, or *cannot tell*); none ran, and nothing is drawn.
fn check_line(
    full: &Full<'_>,
    check: &Checked,
    work: Option<&Work>,
    cx: &Context<IssuesView>,
) -> Option<AnyElement> {
    let muted = cx.theme().muted_foreground;
    let against_now = |work: Option<&Work>| match work.and_then(|work| full.check_for(work)) {
        Some(Ok(check)) => check.said(),
        Some(Err(why)) => format!("could not be read against the work now: {why}"),
        None => "reading it against the work now…".to_string(),
    };
    let (said, printed) = match check {
        Checked::NotRun => return None,
        Checked::NotKept => (against_now(work), ""),
        Checked::Ran(ran) if ran.passed => (against_now(work), ran.tail.trim_end()),
        Checked::Ran(ran) => (ran.said(), ran.tail.trim_end()),
    };
    let (printed, left_out) = last_lines(printed, CHECK_LINES);
    Some(
        div()
            .v_flex()
            .gap_1()
            .min_w_0()
            .child(div().truncate().child(said))
            .when(left_out > 0, |column| {
                column.child(
                    div()
                        .text_color(muted)
                        .child(format!("{left_out} earlier lines not shown")),
                )
            })
            .when(!printed.is_empty(), |column| {
                column.child(
                    div()
                        .p_1()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .children(printed.lines().map(|line| div().child(line.to_string()))),
                )
            })
            .into_any_element(),
    )
}

/// The answers, each with what it starts; once one is sent, what came of it.
fn answers(
    review: &ReviewState,
    read: &UnderReview,
    work: Option<&Work>,
    cut: bool,
    lines: usize,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = status_ink(cx);
    let moved_on = work
        .filter(|work| work.review.as_ref().map(|now| &now.at) != Some(&read.at))
        .map(|work| match &work.step {
            Some(step) => format!("The run moved on to {}.", step.label),
            // Ended: how, in its own words (*Workflow done: every step
            // passed.*).
            None => format!("{}.", work.said),
        });
    if review.sent.is_some() || (review.changed && moved_on.is_some()) {
        let said = match (review.sent, moved_on) {
            (_, Some(moved)) => moved,
            (Some(Sent::Continue), None) => "Approved; sent to the run.".to_string(),
            (Some(Sent::Revise), None) => "Sent back; sent to the run.".to_string(),
            (None, None) => String::new(),
        };
        return div().text_color(muted).child(said).into_any_element();
    }
    if let Some(note) = &review.note {
        return div()
            .v_flex()
            .gap_1()
            .child(div().text_color(muted).child(read.revise_said()))
            .child(Textarea::new(note).h(rems(6.)))
            .when(review.refused, |column| {
                column.child(div().text_color(ink.warning).child("Say what to change."))
            })
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .justify_end()
                    .child(
                        action("issue-review-cancel")
                            .xsmall()
                            .ghost()
                            .label("Cancel")
                            .on_click(
                                cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_revise(cx)),
                            ),
                    )
                    .child(
                        action("issue-review-send")
                            .xsmall()
                            .primary()
                            .label("Send back")
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                view.send_revise(window, cx)
                            })),
                    ),
            )
            .into_any_element();
    }
    div()
        .v_flex()
        .gap_1()
        .when(cut, |column| {
            column.child(
                div()
                    .text_color(ink.warning)
                    .child(format!("Showing the last {ANSWER_LINES} of {lines} lines.")),
            )
        })
        .child(
            div()
                .h_flex()
                .gap_2()
                .items_center()
                .child(
                    action("issue-review-revise")
                        .xsmall()
                        .ghost()
                        .label("Revise…")
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                            view.start_revise(window, cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(read.revise_said()),
                ),
        )
        .child(
            div()
                .h_flex()
                .gap_2()
                .items_center()
                .child(
                    action("issue-review-continue")
                        .xsmall()
                        .primary()
                        .icon(Icon::new(IconName::Check))
                        .label("Continue")
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                            view.press_continue(window, cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(read.continue_said()),
                ),
        )
        .into_any_element()
}
