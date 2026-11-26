//! What a task's work stands at, and what to do about it next.
//!
//! A task is read as a [`Work`]; what it waits on, and the one thing to
//! press, is [`next_action`], decided here so the Tasks page and an issue's
//! view cannot decide it twice. Nothing is stored: every line is worked out
//! from the task and its runs.

use super::{Group, Task, Working};
use crate::connector::{PrState, PullRequest};
use crate::workflow::{Failure, Outcome, Run, StepKind, Stop};

pub mod list;
mod review;

pub use review::{last_lines, Checked, UnderReview, Waits, ANSWER_CHANGED, ANSWER_LINES};

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
    /// Ended where it could not get past a step: too many misses. What
    /// happened and what the step's last visit ended on.
    Exhausted {
        said: String,
        last: Option<String>,
    },
    /// Out of time: where, and how long it worked against which timeout.
    TimedOut(String),
    Failed {
        why: String,
        kind: Failure,
    },
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

/// A task, as what to do about it is decided from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub group: Group,
    /// The step it is at, or where it stopped; `None` once done.
    pub step: Option<StepAt>,
    /// When its open step visit started, in seconds past the epoch.
    since: Option<u64>,
    pub branch: Option<String>,
    /// Its latest run works on a worktree of its own, still on disk, which a
    /// person may remove once the work is merged.
    pub worktree: bool,
    /// The connector the branch goes to; `None` where no forge serves the
    /// project, whose work stays on the branch.
    pub forge: Option<String>,
    pull_request: PrStep,
    stand: Stand,
    /// The timeout a retry with current settings would run with differs
    /// from the one its last run ran with. Told by the app, which knows
    /// Settings; `false` until it does.
    pub timeout_moved: bool,
}

impl Work {
    /// `task`, given what the app says it is doing and, when it is queued,
    /// the title of the task holding its place.
    pub fn of(task: &Task, working: Option<Working>, behind: Option<String>) -> Self {
        let run = task.runs.last();
        let stand = stand(task, run, working, behind);
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
        Self {
            timeout_moved: false,
            group: task.group(working),
            step,
            since,
            branch: task.setup.branch.clone(),
            worktree: !task.worktree_removed
                && run.is_some_and(|run| run.template.place == crate::workflow::Place::Worktree),
            forge: task.setup.forge.clone(),
            pull_request: run.map_or(PrStep::Absent, pr_step),
            stand,
        }
    }

    /// Whether *Remove worktree…* is offered for it, given its pull request
    /// as read: its worktree is left on disk until a person asks, and is
    /// offered only once the work it holds is merged and it is done.
    pub fn removal_offered(&self, pr: Option<&PullRequest>) -> bool {
        self.worktree
            && self.stand == Stand::Done
            && self.has_pull_request()
            && pr.is_some_and(|pr| pr.state == PrState::Merged)
    }

    /// Whether it should have a pull request to read: a forge serves its
    /// project and the run has reached its pull request step, or waits on
    /// its status checks.
    pub(crate) fn has_pull_request(&self) -> bool {
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
                | StepKind::Review { .. }
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
                Stop::LinkLost | Stop::Closed => {
                    Stand::Resumable(stop.said().unwrap_or_default().to_string())
                }
                Stop::TimedOut => Stand::TimedOut(format!(
                    "Timed out at {} after {}, against its timeout of {}",
                    step_label(run, None),
                    worked(run.map_or(0, |run| run.spent_secs)),
                    run.map_or("", |run| run.template.timeout.as_str()),
                )),
            },
            Some(Outcome::Exhausted { step }) => Stand::Exhausted {
                said: format!("Too many misses at {}", step_label(run, Some(&step))),
                last: last_why(run),
            },
            Some(Outcome::Failed(why)) => Stand::Failed {
                why,
                kind: run.and_then(Run::failed_on).unwrap_or(Failure::Other),
            },
        },
    }
}

/// How long a run worked, as a timeout is written: *45m*, *1h 30m*.
pub(crate) fn worked(secs: u64) -> String {
    match (secs / 3600, secs % 3600 / 60) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
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

/// Something a person can do from an issue's view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
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
    /// Retry with what Settings say now, rather than what the last run ran
    /// with.
    RetryCurrent,
    /// Remove the worktree and its branch, once its pull request merged:
    /// judged first, and asked in a modal.
    RemoveWorktree,
    OpenPullRequest,
    OpenBranch,
    Refresh,
    ReopenIssue,
}

impl Act {
    /// What its button says.
    pub fn label(self) -> &'static str {
        match self {
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
            Self::RetryCurrent => "Retry with current settings…",
            Self::RemoveWorktree => "Remove worktree…",
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
/// issue, which no task works: a session on it, and what is done to the
/// issue itself.
///
/// **The issue's state never hides what an active run needs.**
pub fn next_action(work: Option<&Work>, around: Around<'_>) -> Next {
    let Some(work) = work else {
        let here = match around.session {
            true => Act::OpenWorkingSession,
            false => Act::WorkHere,
        };
        let mut next = row(None, None, &[here]);
        if !around.open {
            next.secondary = vec![Act::ReopenIssue];
        }
        next.secondary.push(Act::Edit);
        return next;
    };
    let mut next = by_stand(work, around);
    if !around.open {
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
            Some(format!("It can go on where it was, with its marks: {why}")),
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
        Stand::TimedOut(said) => {
            // Retry keeps the timeout; a changed one is the other way out.
            let others: &[Act] = match work.timeout_moved {
                true => &[Act::RetryCurrent, Act::ShowTask],
                false => &[Act::ShowTask],
            };
            row(Some(said.clone()), Some(Act::Retry), others)
        }
        Stand::Failed {
            why,
            kind: Failure::Configuration,
        } => row(
            Some(why.clone()),
            Some(Act::RetryCurrent),
            &[Act::Retry, Act::ShowTask],
        ),
        Stand::Failed {
            why,
            kind: Failure::Forge | Failure::Other,
        } => row(Some(why.clone()), Some(Act::Retry), &[Act::ShowTask]),
        Stand::ByPerson => row(None, None, &[]),
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
            PrState::Merged if work.removal_offered(Some(pr)) => {
                row(None, None, &[Act::RemoveWorktree])
            }
            PrState::Merged => row(None, None, &[]),
            PrState::Closed => row(
                Some(
                    "The pull request was closed unmerged: reopen the pull request, not the issue"
                        .into(),
                ),
                Some(Act::OpenPullRequest),
                &[Act::ShowTask],
            ),
        },
    }
}

#[cfg(test)]
mod tests;
