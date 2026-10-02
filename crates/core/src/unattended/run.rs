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

/// What a turn left undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Work sits in the worktree that no commit holds.
    Uncommitted,
    /// Nothing has been committed.
    NoCommits,
    /// Commits, but no pull request for them.
    NoPullRequest,
    /// The pull request is behind the branch.
    Unpushed,
}

/// What a run does once a turn has ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Prompt the same session again, saying what is missing.
    CarryOn(Missing),
    /// Everything is pushed to an open pull request: close the session and
    /// wait for its checks.
    AwaitChecks,
    /// The run is over as it stands: work left on a branch with no forge, or
    /// a pull request somebody already closed or merged.
    Settle,
    /// Something is still missing and the session has no turns left.
    Exhausted,
}

/// The step after a turn, given what was read, whether the project has a
/// forge, and how many more turns the session may take.
pub fn after_turn(facts: &Facts, forge: bool, turns_left: u32) -> Step {
    if facts
        .pr
        .as_ref()
        .is_some_and(|pr| pr.state != PrState::Open)
    {
        return Step::Settle;
    }
    let missing = if facts.dirty {
        Some(Missing::Uncommitted)
    } else if facts.commits == 0 {
        Some(Missing::NoCommits)
    } else if !forge {
        None
    } else {
        match &facts.pr {
            None => Some(Missing::NoPullRequest),
            Some(pr) if pr.head != facts.head => Some(Missing::Unpushed),
            Some(_) => None,
        }
    };
    match missing {
        None if forge => Step::AwaitChecks,
        None => Step::Settle,
        Some(_) if turns_left == 0 => Step::Exhausted,
        Some(missing) => Step::CarryOn(missing),
    }
}

/// The prompt that sends a session back to work, naming what is missing.
pub fn carry_on(missing: Missing, forge: Option<&dyn Connector>) -> String {
    let finish = match forge {
        Some(forge) => format!(
            "commit it, push the branch, and open the draft pull request with {} if it \
             has none yet",
            forge.open_pull_request_with()
        ),
        None => "commit it on this branch".to_string(),
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
    /// The session took this many turns and something was still missing.
    Turns(u32),
    /// The checks still fail after this many repairs.
    Repairs(u32),
    /// These checks failed again right after a repair aimed at them.
    FailedAgain(Vec<String>),
    /// The checks were still running after this long.
    ChecksPending(Duration),
}

impl Spent {
    /// What happened, as a sentence.
    pub fn said(&self) -> String {
        match self {
            Self::Turns(n) => format!("It used up its {n} turns with the work unfinished."),
            Self::Repairs(n) => format!("Its checks still fail after {n} repairs."),
            Self::FailedAgain(names) => format!(
                "The same checks failed again after a repair: {}.",
                names.join(", ")
            ),
            Self::ChecksPending(d) => {
                format!("Its checks were still running after {}.", spoken(*d))
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
}

impl Progress {
    pub fn new(start: Start, since: Option<String>) -> Self {
        Self {
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
