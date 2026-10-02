//! A run past its first turn: what onehand reads of the work after each turn,
//! what it does next, what a pull request's checks decide, and the one small
//! file a run keeps so a restart can carry on.
//!
//! **Facts, never the agent's word.** Whether there are commits, whether they
//! are pushed and what the checks say are read from git and the forge; what
//! the agent wrote about them is not consulted.
//!
//! **The file holds only what nothing else records**: which phase the run is
//! in and what it has used of its budget. The branch, the commits, the pull
//! request and its checks are read again whenever they are needed, so the
//! file can never disagree with them.

use super::{spoken, Issue, Tracker};
use crate::connector::{CheckState, Connector, PrState, PullRequest};
use crate::worktree;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a run's work amounts to after a turn, as git and the forge say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Commits on the branch past where this attempt is measured from.
    pub commits: u64,
    /// Changes in the worktree that no commit holds.
    pub dirty: bool,
    /// The commit the worktree is at.
    pub head: String,
    /// The branch's pull request; always `None` on a project with no forge.
    pub pr: Option<PullRequest>,
}

impl Facts {
    /// Read them for the worktree at `dir`, counting commits past `since`.
    /// Blocking.
    pub fn read_blocking(
        dir: &Path,
        since: &str,
        repo: &Path,
        branch: &str,
        forge: Option<&dyn Connector>,
    ) -> Result<Self, String> {
        Ok(Self {
            commits: worktree::commits_since_blocking(dir, since)?,
            dirty: worktree::dirty_blocking(dir)?,
            head: worktree::head_blocking(dir)?,
            pr: match forge {
                Some(forge) => forge.pull_request_for_blocking(repo, branch)?,
                None => None,
            },
        })
    }
}

/// A named stretch of an attempt, with its own prompt and a condition onehand
/// checks itself before the run moves on.
///
/// **The order is fixed**: a plan, then the change, then the project's check,
/// then the pull request. Planning first is what keeps an agent from changing
/// code it has not read; the check is run by onehand, so "the checks pass" is
/// never the agent's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Step {
    /// Read the code and say what will be done, changing nothing.
    Plan,
    /// Make the change and commit it. A record written before steps existed
    /// reads as this, which is what its session was doing.
    #[default]
    Implement,
    /// The project's check command failed: fix what it reports.
    Verify,
    /// Push the branch and open the draft pull request.
    OpenPr,
}

impl Step {
    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Implement => "Implement",
            Self::Verify => "Verify",
            Self::OpenPr => "Open PR",
        }
    }
}

/// What a turn left undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// Work sits in the worktree that no commit holds.
    Uncommitted,
    /// Nothing has been committed.
    NoCommits,
    /// Commits, but no pull request for them.
    NoPullRequest,
    /// The pull request is behind the branch.
    Unpushed,
    /// A plan turn ended with no answer to read as the plan.
    NoPlan,
    /// A plan turn changed the code, which is the next step's to do.
    PlanTouchedCode,
    /// The project's check command failed, ending on this.
    CheckFailed(String),
}

/// What a run does once a turn has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Prompt the same session again, saying what is missing.
    CarryOn(Missing),
    /// The step is done: go on to this one in the same session.
    Advance(Step),
    /// The change is committed: run the project's check command.
    RunCheck,
    /// The plan is written and the project wants a person to approve it:
    /// post it, close the session and wait.
    AwaitApproval,
    /// Everything is pushed to an open pull request: close the session and
    /// wait for its checks.
    AwaitChecks,
    /// The run is over as it stands: work left on a branch with no forge, or
    /// a pull request somebody already closed or merged.
    Settle,
    /// Something is still missing and the attempt has no turns left.
    Exhausted,
}

/// What a step's gate needs to know beyond the branch.
#[derive(Debug, Clone, Copy)]
pub struct Gate<'a> {
    /// The project has a forge the work goes to.
    pub forge: bool,
    /// The project wants a person to approve a plan before work starts.
    pub approve_plans: bool,
    /// The project has a check command for onehand to run.
    pub check: bool,
    /// How many more turns may fail their gate before the attempt is spent.
    pub turns_left: u32,
    /// What the turn answered, which is the plan in the plan step.
    pub answer: &'a str,
}

/// What comes after a turn of `step`, given what was read.
///
/// **Only a turn that fails its gate costs a turn.** Four steps each take at
/// least one, so counting every turn would spend a budget of three on a run
/// that never put a foot wrong.
pub fn next(step: Step, facts: &Facts, gate: &Gate) -> Next {
    if facts
        .pr
        .as_ref()
        .is_some_and(|pr| pr.state != PrState::Open)
    {
        return Next::Settle;
    }
    let committed = if facts.dirty {
        Err(Missing::Uncommitted)
    } else if facts.commits == 0 {
        Err(Missing::NoCommits)
    } else {
        Ok(())
    };
    let passed = match step {
        Step::Plan if gate.answer.trim().is_empty() => Err(Missing::NoPlan),
        Step::Plan if facts.dirty || facts.commits > 0 => Err(Missing::PlanTouchedCode),
        Step::Plan if gate.approve_plans => Ok(Next::AwaitApproval),
        Step::Plan => Ok(Next::Advance(Step::Implement)),
        Step::Implement | Step::Verify => committed.map(|()| match (gate.check, gate.forge) {
            (true, _) => Next::RunCheck,
            (false, true) => Next::Advance(Step::OpenPr),
            (false, false) => Next::Settle,
        }),
        Step::OpenPr if !gate.forge => committed.map(|()| Next::Settle),
        Step::OpenPr => committed.and_then(|()| match &facts.pr {
            None => Err(Missing::NoPullRequest),
            Some(pr) if pr.head != facts.head => Err(Missing::Unpushed),
            Some(_) => Ok(Next::AwaitChecks),
        }),
    };
    match passed {
        Ok(next) => next,
        Err(_) if gate.turns_left == 0 => Next::Exhausted,
        Err(missing) => Next::CarryOn(missing),
    }
}

/// What comes after the project's check command ran: on to the pull request,
/// or back to work on what it reported.
pub fn after_check(ran: Result<(), String>, turns_left: u32, forge: bool) -> Next {
    match ran {
        Ok(()) if forge => Next::Advance(Step::OpenPr),
        Ok(()) => Next::Settle,
        Err(_) if turns_left == 0 => Next::Exhausted,
        Err(tail) => Next::CarryOn(Missing::CheckFailed(tail)),
    }
}

/// The prompt that sends a session back to work in `step`, naming what is
/// missing.
pub fn carry_on(missing: &Missing, step: Step, forge: Option<&dyn Connector>) -> String {
    let finish = match forge {
        Some(forge) if step == Step::OpenPr => format!(
            "commit it, push the branch, and open the draft pull request with {} if it \
             has none yet",
            forge.open_pull_request_with()
        ),
        _ => "commit it on this branch".to_string(),
    };
    let said = match missing {
        Missing::Uncommitted => {
            format!("the worktree holds changes no commit has. Finish the work, then {finish}.")
        }
        Missing::NoCommits => format!(
            "the branch has no commit yet, so the issue is not done. Carry on with it, \
             then {finish}."
        ),
        Missing::NoPullRequest => format!(
            "the branch has commits but no pull request. Push it and open the draft pull \
             request with {}.",
            forge.map_or("the forge's tool", |f| f.open_pull_request_with())
        ),
        Missing::Unpushed => {
            "the branch has commits its pull request does not. Push them.".to_string()
        }
        Missing::NoPlan => "that turn gave no plan. Answer with the plan itself, and change \
             nothing."
            .to_string(),
        Missing::PlanTouchedCode => "that turn changed the code, and this step is only the \
             plan. Put the worktree and the branch back as they were, then answer with the \
             plan alone."
            .to_string(),
        Missing::CheckFailed(tail) => format!(
            "the project's check failed. Fix what it reports, then commit.\n\n```\n{}\n```",
            tail.trim()
        ),
    };
    format!("onehand checked the branch after that turn: {said}")
}

/// How long a pull request with no checks at all is given for them to appear.
/// A forge registers its checks a little after the push, and a look straight
/// after it would read "no checks" as "nothing to run".
pub const CHECKS_GRACE: Duration = Duration::from_secs(600);

/// What a pull request's checks decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// Still running, or not reported yet.
    Wait,
    /// Every check passed on the head, or there were none to run.
    Ready { ran: bool },
    /// Start a repair for these failing checks, or for a conflict.
    Repair {
        failing: Vec<String>,
        conflicting: bool,
    },
    /// The run cannot get further by itself.
    Exhausted(Spent),
    /// The pull request was closed or merged, or is gone.
    Gone,
}

/// Why a run stopped short of a pull request ready for review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spent {
    /// This many turns failed their gate, the last in this step.
    Turns(u32, Step),
    /// The checks still fail after this many repairs.
    Repairs(u32),
    /// These checks failed again right after a repair aimed at them.
    FailedAgain(Vec<String>),
    /// The checks were still running after this long.
    ChecksPending(Duration),
    /// Its plan waited this long and nobody approved it.
    Unapproved(Duration),
}

impl Spent {
    /// What happened, as a sentence.
    pub fn said(&self) -> String {
        match self {
            Self::Turns(n, step) => format!(
                "It used up its {n} turns with the work unfinished, stuck at its {} step.",
                step.label()
            ),
            Self::Repairs(n) => format!("Its checks still fail after {n} repairs."),
            Self::FailedAgain(names) => format!(
                "The same checks failed again after a repair: {}.",
                names.join(", ")
            ),
            Self::ChecksPending(d) => {
                format!("Its checks were still running after {}.", spoken(*d))
            }
            Self::Unapproved(d) => {
                format!("Its plan waited {} and nobody approved it.", spoken(*d))
            }
        }
    }
}

/// What a pull request's checks decide, given how long the run has waited on
/// them, how long it may, how many repairs it has left, and which checks were
/// failing when the last repair started.
///
/// **Only the head counts**: the forge reports checks for the commit the pull
/// request is at, so a check that passed on an earlier commit is not here.
/// A failure wins over a check still running, so a repair starts as soon as
/// there is something to repair.
pub fn after_checks(
    pr: Option<&PullRequest>,
    waited: Duration,
    limit: Duration,
    repairs_left: u32,
    repairs_used: u32,
    failed_before: &[String],
) -> Checked {
    let Some(pr) = pr.filter(|pr| pr.state == PrState::Open) else {
        return Checked::Gone;
    };
    let failing: Vec<String> = pr
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Failed)
        .map(|c| c.name.clone())
        .collect();
    if !failing.is_empty() || pr.conflicting {
        let again: Vec<String> = failing
            .iter()
            .filter(|name| failed_before.contains(name))
            .cloned()
            .collect();
        return if !again.is_empty() {
            Checked::Exhausted(Spent::FailedAgain(again))
        } else if repairs_left == 0 {
            Checked::Exhausted(Spent::Repairs(repairs_used))
        } else {
            Checked::Repair {
                failing,
                conflicting: pr.conflicting,
            }
        };
    }
    let pending = pr.checks.iter().any(|c| c.state == CheckState::Pending)
        || (pr.checks.is_empty() && waited < CHECKS_GRACE);
    match pending {
        true if waited >= limit => Checked::Exhausted(Spent::ChecksPending(limit)),
        true => Checked::Wait,
        false => Checked::Ready {
            ran: !pr.checks.is_empty(),
        },
    }
}

/// What a session of a run is started to do, which is what its first prompt
/// says beyond the issue itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Start {
    /// A new branch, nothing on it yet.
    Fresh,
    /// The branch carries work from an earlier attempt, with no pull request.
    Earlier,
    /// The pull request is open and the issue was asked for again: a reviewer
    /// wants changes.
    Review { pr: String, number: u64 },
    /// The pull request's checks failed, or it conflicts with its base.
    Repair {
        pr: String,
        conflicting: bool,
        failing: Vec<Failure>,
    },
}

/// A failed check and the end of its log, or why there is no log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub check: String,
    pub log: String,
}

impl Start {
    /// What the first prompt says about this start, if anything.
    pub(super) fn said(&self, forge: Option<&dyn Connector>) -> Option<String> {
        match self {
            Self::Fresh => None,
            Self::Earlier => Some(
                "This branch already carries work from an earlier attempt at this issue, \
                 perhaps uncommitted. Look at it first and build on it rather than starting \
                 over."
                    .to_string(),
            ),
            Self::Review { pr, number } => Some(format!(
                "This issue's pull request is {pr}, and it was asked for again: a reviewer \
                 wants changes. Read the review with {}, and address what is still open.",
                forge.map_or_else(
                    || "the forge's tool".to_string(),
                    |f| f.read_review_with(*number)
                )
            )),
            Self::Repair {
                pr,
                conflicting,
                failing,
            } => {
                let mut said = format!("This issue's pull request is {pr}.");
                if *conflicting {
                    said.push_str(
                        " It conflicts with its base branch: bring the base in and resolve \
                         the conflicts, keeping this branch's work.",
                    );
                }
                if !failing.is_empty() {
                    said.push_str(
                        " These checks failed on its latest commit. Fix what this branch's \
                         change broke. If a failure is not caused by this branch's change, say \
                         so in your answer and leave the code alone.",
                    );
                    for f in failing {
                        said.push_str(&format!("\n\n{}:\n\n```\n{}\n```", f.check, f.log.trim()));
                    }
                }
                Some(said)
            }
        }
    }

    /// Whether the pull request already exists, so the agent pushes to it
    /// rather than opening one.
    pub(super) fn has_pull_request(&self) -> bool {
        matches!(self, Self::Review { .. } | Self::Repair { .. })
    }
}

/// What a run has used, and what it is doing it for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// What the run's current or next session is for.
    pub start: Start,
    /// Repairs started in this attempt.
    pub repairs: u32,
    /// The checks failing when the last repair started.
    pub failed_before: Vec<String>,
    /// Working time spent in this attempt, in seconds.
    pub spent_secs: u64,
    /// The commit this attempt's work is counted from, when that is not the
    /// base: an answer to a review counts only what it adds.
    pub since: Option<String>,
    /// The step the run is in. Each field from here on defaults, because a
    /// record written before it existed must still read.
    #[serde(default)]
    pub step: Step,
    /// The plan last written, carried into a later session of the attempt.
    #[serde(default)]
    pub plan: Option<String>,
    /// What a person asked to change in that plan.
    #[serde(default)]
    pub revise: Option<String>,
}

impl Progress {
    /// A repair starts at the change, since what to do is in the failing
    /// checks; every other attempt starts with a plan.
    pub fn new(start: Start, since: Option<String>) -> Self {
        Self {
            step: match start {
                Start::Repair { .. } => Step::Implement,
                Start::Fresh | Start::Earlier | Start::Review { .. } => Step::Plan,
            },
            plan: None,
            revise: None,
            start,
            repairs: 0,
            failed_before: Vec::new(),
            spent_secs: 0,
            since,
        }
    }
}

/// Where a run stands between sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// A session is working, or is to be started.
    Working,
    /// Everything is pushed and the run waits on checks, with no session,
    /// since this many seconds past the epoch.
    AwaitingChecks { since: u64 },
    /// Its plan is written and waits for a person to approve it, with no
    /// session, since this many seconds past the epoch.
    AwaitingApproval { since: u64 },
}

/// Where an issue lives, as a file can hold it: connectors by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kept {
    Forge(String),
    Local(PathBuf),
    Synced { file: PathBuf, forge: String },
}

impl Tracker {
    /// This tracker as a file holds it.
    pub fn kept(&self) -> Kept {
        match self {
            Self::Forge(c) => Kept::Forge(c.name().to_string()),
            Self::Local(file) => Kept::Local(file.clone()),
            Self::Synced { file, forge } => Kept::Synced {
                file: file.clone(),
                forge: forge.name().to_string(),
            },
        }
    }
}

impl Kept {
    /// The tracker again, finding connectors by name; `None` if one named is
    /// not built in any more.
    pub fn tracker(
        &self,
        named: impl Fn(&str) -> Option<&'static dyn Connector>,
    ) -> Option<Tracker> {
        Some(match self {
            Self::Forge(name) => Tracker::Forge(named(name)?),
            Self::Local(file) => Tracker::Local(file.clone()),
            Self::Synced { file, forge } => Tracker::Synced {
                file: file.clone(),
                forge: named(forge)?,
            },
        })
    }
}

/// One run that has not ended, as its file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The project the issue was found in.
    pub repo: PathBuf,
    pub kept: Kept,
    /// The forge the work goes to, by name.
    pub forge: Option<String>,
    pub issue: Issue,
    pub branch: String,
    pub base: String,
    pub dir: PathBuf,
    /// A person picked the issue, rather than the search finding it.
    pub by_hand: bool,
    pub phase: Phase,
    pub progress: Progress,
}

/// `<config_dir>/onehand/runs/`, one file per run that has not ended.
pub fn runs_dir() -> PathBuf {
    crate::config::config_dir().join("runs")
}

/// A file name no other run has, for a run on issue `number`.
pub fn new_record_file(number: u64) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    runs_dir().join(format!("{nanos}-{number}.json"))
}

/// Write `record` to `file`, whole or not at all. Blocking.
pub fn save_record_blocking(file: &Path, record: &Record) -> Result<(), String> {
    let text = serde_json::to_string_pretty(record).map_err(|err| err.to_string())?;
    crate::config::write_atomic(file, &text).map_err(|err| format!("{}: {err}", file.display()))
}

/// Every run file in `dir`, each read or why it could not be. Blocking.
pub fn load_records_blocking(dir: &Path) -> Vec<(PathBuf, Result<Record, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let read = std::fs::read_to_string(&p)
                .map_err(|err| err.to_string())
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()));
            (p, read)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

#[cfg(test)]
mod tests;
