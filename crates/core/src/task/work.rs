//! What an issue's work stands at, and what to do about it next.
//!
//! An issue's view reads one summary per issue, [`IssueWork`]: its newest
//! task as a [`Work`], and how many runs and tasks came before. What it
//! waits on, and the one thing to press, is [`next_action`], decided here so
//! the Issues tab and anything else showing an issue cannot decide it twice.
//! Nothing is stored: every line is worked out from the task, its runs and
//! the issue.

use super::{Group, Task, Working};
use crate::connector::{PrState, PullRequest};
use crate::issues::IssueKey;
use crate::workflow::{Outcome, Run, StepKind, Stop};

pub mod left;
pub mod list;

/// What a task waiting on a person waits for, in the words every view of
/// it uses: an approval, or an answer to a card.
pub fn waiting_said(approval: bool) -> &'static str {
    match approval {
        true => "Waiting for approval",
        false => "Waiting for an answer",
    }
}

/// A step, as where it stands in its run's workflow: *Verify · step 3 of 6*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepAt {
    pub label: String,
    /// From 1.
    pub at: usize,
    pub of: usize,
}

/// How a task stands, as the next action is decided from: the task groups
/// and every [`Outcome`], with waiting told apart into its two kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stand {
    Running,
    /// Running, at the step that waits for the forge's status checks.
    StatusChecks,
    /// Waiting for its place, behind the task named, when it is known.
    Queued {
        behind: Option<String>,
    },
    /// Waiting at an approval step, which would start the step named next.
    Approval {
        next: Option<String>,
    },
    /// Waiting on a card the agent parked.
    Card,
    /// Its agent stopped, its session went, or a restart cut it off.
    Resumable(String),
    /// Ended where it could not get past a step: too many misses, or out of
    /// time. What happened and what the step's last visit ended on.
    Exhausted {
        said: String,
        last: Option<String>,
    },
    Failed(String),
    Done,
    /// Stopped, taken over or let go by a person.
    ByPerson,
}

/// Where a run is about its pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrStep {
    /// Its workflow has no pull request step.
    Absent,
    /// It has not reached it yet.
    NotYet,
    Reached,
}

/// An issue's newest task, as its view draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub task: String,
    /// The latest run's id; `None` for a task with no run.
    pub run: Option<String>,
    pub group: Group,
    /// Its group or how it ended, in the Tasks page's words.
    pub said: String,
    /// The step it is at, or where it stopped; `None` once done.
    pub step: Option<StepAt>,
    /// When its open step visit started, in seconds past the epoch.
    since: Option<u64>,
    pub branch: Option<String>,
    /// Where its work stands: the worktree, or the project itself.
    pub dir: std::path::PathBuf,
    /// The connector the branch goes to; `None` where no forge serves the
    /// project, whose work stays on the branch.
    pub forge: Option<String>,
    pull_request: PrStep,
    stand: Stand,
    /// The steps after the one it is at, by label; empty once it is over.
    pub rest: Vec<String>,
    /// The commit a command last passed on, in this run.
    pub(crate) verified_at: Option<String>,
    /// The work as this run found it and as it last left it, as two marks;
    /// `None` until both are pinned.
    pub span: Option<(String, String)>,
    /// The work as the task's first run found it, as a mark: what the branch
    /// is measured from.
    pub base: Option<String>,
}

impl Work {
    /// `task` as its issue's view draws it, given what the app says it is
    /// doing and, when it is queued, the title of the task holding its place.
    pub(crate) fn of(task: &Task, working: Option<Working>, behind: Option<String>) -> Self {
        let run = task.runs.last();
        let stand = stand(task, run, working, behind);
        let said = match (&stand, working) {
            (Stand::Approval { .. }, _) => waiting_said(true).to_string(),
            (Stand::Card, _) => waiting_said(false).to_string(),
            (_, Some(Working::Running)) => "Running".to_string(),
            (_, Some(Working::Queued)) => "Queued".to_string(),
            (_, Some(Working::Waiting) | None) => {
                task.ended_said().unwrap_or_else(|| "Cut off".to_string())
            }
        };
        let step = run.filter(|_| stand != Stand::Done).and_then(|run| {
            let spec = run.current()?;
            Some(StepAt {
                label: spec.label.clone(),
                at: run.step + 1,
                of: run.template.steps.len(),
            })
        });
        let since = run
            .filter(|run| !run.over() && working.is_some())
            .and_then(|run| run.visits().last())
            .filter(|visit| visit.ended_at.is_none())
            .map(|visit| visit.started_at);
        let rest = run
            .filter(|run| !run.over())
            .map(|run| {
                run.template
                    .steps
                    .iter()
                    .skip(run.step + 1)
                    .map(|step| step.label.clone())
                    .collect()
            })
            .unwrap_or_default();
        let first = |run: &Run| run.visits().first().and_then(|visit| visit.start.clone());
        let last = |run: &Run| {
            run.visits()
                .iter()
                .rev()
                .find_map(|visit| visit.end.clone().or_else(|| visit.start.clone()))
        };
        Self {
            rest,
            verified_at: run.and_then(|run| run.marks.verified_at.clone()),
            span: run.and_then(|run| first(run).zip(last(run))),
            base: task.runs.first().and_then(first),
            task: task.id.clone(),
            run: run.map(|run| run.id.clone()),
            group: task.group(working),
            said,
            step,
            since,
            branch: task.setup.branch.clone(),
            dir: task.setup.dir.clone(),
            forge: task.setup.forge.clone(),
            pull_request: run.map_or(PrStep::Absent, pr_step),
            stand,
        }
    }

    /// Whether it should have a pull request to read: a forge serves its
    /// project and the run has reached its pull request step, or waits on
    /// its status checks.
    pub fn has_pull_request(&self) -> bool {
        self.forge.is_some()
            && self.branch.is_some()
            && (self.pull_request == PrStep::Reached || self.stand == Stand::StatusChecks)
    }

    /// Whether it is running, queued or waiting, rather than over.
    pub fn active(&self) -> bool {
        match self.group {
            Group::Running | Group::Queued | Group::Waiting => true,
            Group::Ended | Group::Finished => false,
        }
    }
}

fn stand(
    task: &Task,
    run: Option<&Run>,
    working: Option<Working>,
    behind: Option<String>,
) -> Stand {
    let kind = run.and_then(Run::current).map(|step| &step.kind);
    match working {
        Some(Working::Queued) => Stand::Queued { behind },
        Some(Working::Running) => match kind {
            Some(StepKind::StatusChecks { .. }) => Stand::StatusChecks,
            Some(
                StepKind::Agent { .. }
                | StepKind::Command { .. }
                | StepKind::Approval { .. }
                | StepKind::Push
                | StepKind::PullRequest,
            )
            | None => Stand::Running,
        },
        // An approval step's open visit is an approval; anything else that
        // waits on a person is a card.
        Some(Working::Waiting) => match (run, kind) {
            (Some(run), Some(StepKind::Approval { .. }))
                if run.visits().last().is_some_and(|v| v.ended_at.is_none()) =>
            {
                Stand::Approval {
                    next: run
                        .template
                        .steps
                        .get(run.step + 1)
                        .map(|step| step.label.clone()),
                }
            }
            _ => Stand::Card,
        },
        None if task.dismissed => Stand::ByPerson,
        None => match task.outcome() {
            None => Stand::Resumable("it was cut off".to_string()),
            Some(Outcome::Done) => Stand::Done,
            Some(Outcome::Stopped(stop)) => match stop {
                Stop::ByPerson | Stop::TakenOver => Stand::ByPerson,
                Stop::LinkLost | Stop::Closed => Stand::Resumable(stop.said().to_string()),
                Stop::TimedOut => Stand::Exhausted {
                    said: format!("Timed out at {}", step_label(run, None)),
                    last: last_why(run),
                },
            },
            Some(Outcome::Exhausted { step }) => Stand::Exhausted {
                said: format!("Too many misses at {}", step_label(run, Some(&step))),
                last: last_why(run),
            },
            Some(Outcome::Failed(why)) => Stand::Failed(why),
        },
    }
}

/// The label of step `id` in `run`'s workflow, or of the step it is at.
fn step_label(run: Option<&Run>, id: Option<&str>) -> String {
    let Some(run) = run else {
        return String::new();
    };
    let spec = match id {
        Some(id) => run.template.steps.iter().find(|step| step.id == id),
        None => run.current(),
    };
    spec.map_or_else(|| id.unwrap_or_default().to_string(), |s| s.label.clone())
}

/// What the run's last visit ended on: the last line of what it kept, a
/// failed command's output or an answer. Not why it ended, which is the
/// outcome already said.
fn last_why(run: Option<&Run>) -> Option<String> {
    let output = run?.visits().last()?.output.as_deref()?;
    let line = output
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    Some(line.to_string())
}

fn pr_step(run: &Run) -> PrStep {
    let Some(step) = run
        .template
        .steps
        .iter()
        .find(|step| step.kind == StepKind::PullRequest)
    else {
        return PrStep::Absent;
    };
    let reached = run.outcome == Some(Outcome::Done)
        || run.visits().iter().any(|visit| visit.step == step.id);
    match reached {
        true => PrStep::Reached,
        false => PrStep::NotYet,
    }
}

/// An earlier task of an issue, as one line under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Earlier {
    pub task: String,
    /// Its workflow and how it stands.
    pub line: String,
    pub attention: bool,
    /// Running, queued or waiting: a retry put it back to work.
    pub active: bool,
}

/// One issue's work: its newest task, and what came before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueWork {
    pub key: IssueKey,
    pub work: Work,
    /// Runs of the newest task before its latest one.
    pub earlier_runs: usize,
    /// Every older task, newest first.
    pub earlier: Vec<Earlier>,
}

/// `key`'s work from the tasks working it, each with what the app says it is
/// doing and who holds its place: the newest by when it last moved is the
/// issue's work, the rest are earlier. `None` with no task.
pub fn issue_work<'a>(
    key: IssueKey,
    tasks: impl IntoIterator<Item = (&'a Task, Option<Working>, Option<String>)>,
) -> Option<IssueWork> {
    let mut tasks: Vec<_> = tasks.into_iter().collect();
    tasks.sort_by_key(|(task, _, _)| std::cmp::Reverse(task.recency()));
    let mut tasks = tasks.into_iter();
    let (newest, working, behind) = tasks.next()?;
    let earlier = tasks
        .map(|(task, working, _)| {
            let workflow = task
                .runs
                .last()
                .map_or_else(String::new, |run| run.template.name.clone());
            let said = Work::of(task, working, None).said;
            Earlier {
                task: task.id.clone(),
                line: format!("{workflow} · {said}"),
                attention: task.group(working).needs_attention(),
                active: Work::of(task, working, None).active(),
            }
        })
        .collect();
    Some(IssueWork {
        key,
        work: Work::of(newest, working, behind),
        earlier_runs: newest.runs.len().saturating_sub(1),
        earlier,
    })
}

/// Something a person can do from an issue's view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    RunWorkflow,
    WorkHere,
    /// Show the session *Work here* started, still working the issue: a
    /// second one there would be two agents editing one checkout.
    OpenWorkingSession,
    Edit,
    OpenSession,
    AnswerInSession,
    Review,
    Stop,
    ShowTask,
    Resume,
    Retry,
    OpenPullRequest,
    OpenBranch,
    Refresh,
    ReopenIssue,
}

impl Act {
    /// What its button says.
    pub fn label(self) -> &'static str {
        match self {
            Self::RunWorkflow => "Run workflow…",
            Self::WorkHere => "Work here",
            Self::OpenWorkingSession => "Open session",
            Self::Edit => "Edit",
            Self::OpenSession => "Open session",
            Self::AnswerInSession => "Open session to answer",
            Self::Review => "Review…",
            Self::Stop => "Stop",
            Self::ShowTask => "Show task",
            Self::Resume => "Resume",
            Self::Retry => "Retry…",
            Self::OpenPullRequest => "Open pull request",
            Self::OpenBranch => "Open branch",
            Self::Refresh => "Refresh",
            Self::ReopenIssue => "Reopen issue",
        }
    }
}

/// The pull request of an issue's work, as last read.
#[derive(Debug, Clone, Copy)]
pub enum PrSeen<'a> {
    /// Not read yet.
    Unread,
    /// Read: the one opened from the branch, if there is one.
    Read(Option<&'a PullRequest>),
    /// The last read failed, and why.
    Failed(&'a str),
}

/// What the issue around the work allows.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    /// The issue is open.
    pub open: bool,
    /// A run may be started on its project.
    pub can_start: bool,
    /// A session *Work here* started still works the issue.
    pub session: bool,
    pub pr: PrSeen<'a>,
    /// Seconds past the epoch, for how long a step has worked.
    pub now: u64,
}

/// What an issue waits on, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    /// The one sentence; `None` when there is nothing to say.
    pub said: Option<String>,
    pub primary: Option<Act>,
    /// In the order drawn, after the primary one.
    pub secondary: Vec<Act>,
    /// The work is over and the issue closed: drawn quieter.
    pub muted: bool,
    /// The issue is closed while its work is still active, which is said.
    pub still_active: bool,
}

/// What `work` waits on, given what is around it: the sentence, the one
/// primary action and the others, in their fixed order. `None` for an
/// issue no run was recorded for.
///
/// **The issue's state only gates a new start.** A closed issue offers no
/// *Run workflow…*, and never hides what an active run needs.
pub fn next_action(work: Option<&Work>, around: Around<'_>) -> Next {
    let Some(work) = work else {
        let here = match around.session {
            true => Act::OpenWorkingSession,
            false => Act::WorkHere,
        };
        // The progress line already says no run is recorded.
        let mut next = row(None, None, &[here]);
        if around.open && around.can_start {
            next.primary = Some(Act::RunWorkflow);
        }
        if !around.open {
            next.secondary = vec![Act::ReopenIssue];
        }
        next.secondary.push(Act::Edit);
        return next;
    };
    let mut next = by_stand(work, around);
    if !around.open {
        next.secondary.retain(|act| *act != Act::RunWorkflow);
        if work.active() {
            next.still_active = true;
            next.secondary.push(Act::ReopenIssue);
        } else {
            next.primary = None;
            next.secondary = vec![Act::ReopenIssue, Act::ShowTask];
            next.muted = true;
            // Closing it was what the row asked for.
            if work.stand == Stand::Done && work.forge.is_none() {
                next.said = Some("Look at the branch".into());
            }
        }
    }
    if !around.can_start {
        next.secondary.retain(|act| *act != Act::RunWorkflow);
    }
    next.secondary.push(Act::Edit);
    next
}

fn row(said: Option<String>, primary: Option<Act>, secondary: &[Act]) -> Next {
    Next {
        said,
        primary,
        secondary: secondary.to_vec(),
        muted: false,
        still_active: false,
    }
}

fn by_stand(work: &Work, around: Around<'_>) -> Next {
    let step = work
        .step
        .as_ref()
        .map(|step| step.label.as_str())
        .unwrap_or("");
    match &work.stand {
        Stand::Running => {
            let mut said = format!("Working on {step}");
            if let Some(since) = work.since {
                said.push_str(&format!(", started {}", crate::rel_time(around.now, since)));
            }
            row(Some(said), None, &[Act::OpenSession, Act::Stop])
        }
        Stand::StatusChecks => row(
            Some("Waiting for the forge's checks, not for you".into()),
            None,
            &[Act::OpenPullRequest, Act::Stop],
        ),
        Stand::Queued { behind } => row(
            Some(match behind {
                Some(title) => format!("Waiting for its place, held by \u{201c}{title}\u{201d}"),
                None => "Waiting for its place".into(),
            }),
            None,
            &[Act::ShowTask],
        ),
        Stand::Approval { next } => row(
            Some(match next {
                Some(next) => format!("Approving starts {next}"),
                None => "Approving ends the run".into(),
            }),
            Some(Act::Review),
            &[Act::OpenSession],
        ),
        Stand::Card => row(
            Some("The agent asks something in its session".into()),
            Some(Act::AnswerInSession),
            &[Act::Stop],
        ),
        Stand::Resumable(why) => row(
            Some(format!("It can go on where it was: {why}")),
            Some(Act::Resume),
            &[Act::Retry],
        ),
        Stand::Exhausted { said, last } => row(
            Some(match last {
                Some(last) => format!("{said}: {last}"),
                None => said.clone(),
            }),
            Some(Act::Retry),
            &[Act::ShowTask],
        ),
        Stand::Failed(why) => row(Some(why.clone()), Some(Act::Retry), &[Act::ShowTask]),
        Stand::ByPerson => row(None, None, &[Act::RunWorkflow]),
        Stand::Done => done(work, around.pr),
    }
}

/// A done run: where its work ended up, which on a forge is its pull
/// request.
fn done(work: &Work, pr: PrSeen<'_>) -> Next {
    let Some(forge) = &work.forge else {
        return row(
            Some("Look at the branch; close the issue when satisfied".into()),
            None,
            &[Act::ShowTask],
        );
    };
    // No pull request step, or no branch to have opened one from: the
    // branch, or the work itself, is the result.
    if !work.has_pull_request() {
        return row(
            Some("The branch is the result".into()),
            Some(Act::OpenBranch),
            &[Act::ShowTask],
        );
    }
    match pr {
        PrSeen::Unread => row(
            Some("Reading the pull request…".into()),
            None,
            &[Act::ShowTask],
        ),
        PrSeen::Failed(why) => row(
            Some(format!("The pull request could not be read: {why}")),
            Some(Act::Refresh),
            &[Act::ShowTask],
        ),
        PrSeen::Read(None) => row(
            Some(format!("No pull request on {forge} for the branch")),
            Some(Act::OpenBranch),
            &[Act::ShowTask],
        ),
        PrSeen::Read(Some(pr)) => match pr.state {
            PrState::Open => row(
                Some(format!("Review it on {forge}")),
                Some(Act::OpenPullRequest),
                &[],
            ),
            PrState::Merged => row(None, None, &[Act::RunWorkflow]),
            PrState::Closed => row(
                Some(
                    "The pull request was closed unmerged: reopen the pull request, not the \
                     issue, and put the label back to have it answered"
                        .into(),
                ),
                Some(Act::OpenPullRequest),
                &[Act::ShowTask],
            ),
        },
    }
}

/// A pull request as one line names it: *#7 · open, draft*.
pub fn pr_named(pr: &PullRequest) -> String {
    format!("#{} · {}", pr.number, pr_said(pr))
}

/// A pull request's state in a word or two: *open, draft*, *open*,
/// *merged*, *closed unmerged*.
pub(crate) fn pr_said(pr: &PullRequest) -> &'static str {
    match (pr.state, pr.draft) {
        (PrState::Open, true) => "open, draft",
        (PrState::Open, false) => "open",
        (PrState::Merged, _) => "merged",
        (PrState::Closed, _) => "closed unmerged",
    }
}

/// A read of something slow about an issue's work, kept to what asked for
/// it and to when.
///
/// Every request bumps the generation and names what it is `about`; only an
/// answer carrying the current generation is taken. A switch to another
/// issue, two refreshes answering out of order and a retry that started a
/// new run are all dropped the same way. A failed read keeps the last value,
/// marked with the failure.
#[derive(Debug)]
pub struct Reading<K, T> {
    generation: u64,
    about: Option<K>,
    /// The last answer taken, and when it was read.
    pub value: Option<(T, u64)>,
    /// Why the last read failed, when it did.
    pub failed: Option<String>,
}

impl<K, T> Default for Reading<K, T> {
    fn default() -> Self {
        Self {
            generation: 0,
            about: None,
            value: None,
            failed: None,
        }
    }
}

impl<K: PartialEq, T> Reading<K, T> {
    /// A read about `about` is sent: its generation. What was read about
    /// something else is dropped, so it is never shown for this.
    pub fn ask(&mut self, about: K) -> u64 {
        if self.about.as_ref() != Some(&about) {
            self.value = None;
            self.failed = None;
            self.about = Some(about);
        }
        self.generation += 1;
        self.generation
    }

    /// What the reads are about now.
    pub fn about(&self) -> Option<&K> {
        self.about.as_ref()
    }

    /// The answer to the read of `generation`, read at `at`: whether it was
    /// taken.
    pub fn land(&mut self, generation: u64, answer: Result<T, String>, at: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        match answer {
            Ok(value) => {
                self.value = Some((value, at));
                self.failed = None;
            }
            Err(why) => self.failed = Some(why),
        }
        true
    }
}

#[cfg(test)]
mod tests;
