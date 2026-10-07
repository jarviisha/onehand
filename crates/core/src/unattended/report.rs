//! What a run leaves behind it: the pull request it opens, and the report
//! its issue is told once it ends.

use super::{IssueSource, TrackerRef};
use crate::connector::PrState;
use crate::workflow::{Brief, Outcome, Stop};
use serde::{Deserialize, Serialize};

/// The title and body of the pull request a run of `brief` opens, on the
/// issue `issue` when it works one. The body closes the issue only where the
/// forge knows it: an issue kept in onehand alone has a number the forge
/// would read as one of its own.
pub fn pull_request_text(brief: &Brief, issue: Option<&IssueSource>) -> (String, String) {
    let closes = issue.and_then(|issue| match (&issue.tracker, &issue.forge_ref) {
        (TrackerRef::Forge { .. }, _) => Some(format!("#{}", issue.number)),
        (TrackerRef::Synced { .. }, Some(reference)) => Some(reference.clone()),
        (TrackerRef::Local { .. } | TrackerRef::Synced { .. }, _) => None,
    });
    let mut body = String::new();
    if let Some(closes) = closes {
        body += &format!("Closes {closes}.\n\n");
    }
    body += "Opened by onehand. What was pushed passed the project's check first.";
    (brief.title.clone(), body)
}

/// What the run answering a review on `pr` is told, with `how` the words for
/// reading it on the forge.
pub fn review_note(pr: &str, how: &str) -> String {
    format!(
        "The pull request {pr} was asked for again: a reviewer wants changes. Read the review \
         with {how}, and address what is still open."
    )
}

/// Why a review on the open pull request `pr` is not answered: the workflow
/// its task ran has no step its status checks send back to.
pub fn review_unanswerable(pr: &str) -> String {
    format!(
        "its pull request {pr} is open, and the workflow its task ran has no status checks \
         step to answer a review from"
    )
}

/// Why a review on the pull request `pr` is not answered: a person closed it
/// unmerged, and a second beside it would ask again.
pub fn review_closed(pr: &str) -> String {
    format!(
        "its pull request {pr} was closed without being merged, and onehand does not open \
         another; reopen it to have its review answered"
    )
}

/// Why a review is not answered: the branch on the forge, which a reviewer
/// may have pushed to, went its own way from the task's worktree.
pub fn review_diverged() -> String {
    "the branch on the forge went its own way from the task's worktree, and onehand does \
     not push over it"
        .to_string()
}

/// How one run of an issue's task ended, kept until the issue is told. What
/// the run left on its branch is looked up when it is sent, so keeping it
/// asks nothing of the network and lands before anything can be lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReport {
    /// The run it is about.
    pub run: String,
    /// How the run ended; `None` for one cut off and then let go.
    pub outcome: Option<Outcome>,
    /// Whether the run got as far as asking its agent anything.
    pub started: bool,
    /// What its last step answered or printed.
    pub ended_on: Option<String>,
    /// What a card still waiting asked when the run ended.
    pub asked: Option<String>,
    /// What the start found, said last: an earlier task left needing
    /// attention, what the issue's text lacks.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// What a run left behind: a pull request on its branch, or how many commits
/// it has past where it was cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The pull request on the branch, as the forge has it now.
    PullRequest {
        url: String,
        state: PrState,
        draft: bool,
    },
    Commits(u64),
}

/// The comment `pending` leaves on the issue, given what looking for the
/// run's work on `branch` found: the work first, then how the run ended, then
/// a card it left waiting and what its last step ended on.
///
/// **The work leads, whatever the ending.** A run that timed out may still
/// have left commits, and a comment about the timeout alone would hide them.
/// A lookup that *failed* is said as a failure and never as "nothing": that is
/// a claim, and one nobody checked. A run that failed before it asked its
/// agent anything says only that it could not start: it left nothing to look
/// for. What the start noted is said last either way.
pub fn report(pending: &PendingReport, found: &Result<Verdict, String>, branch: &str) -> String {
    let mut said = said_of(pending, found, branch);
    for note in &pending.notes {
        said += &format!("\n\n{note}");
    }
    said
}

/// [`report`] before what the start noted.
fn said_of(pending: &PendingReport, found: &Result<Verdict, String>, branch: &str) -> String {
    if let (Some(Outcome::Failed(why)), false) = (&pending.outcome, pending.started) {
        return could_not_start(why);
    }
    let head = match found {
        Ok(Verdict::PullRequest { url, state, draft }) => match (state, draft) {
            (PrState::Open, false) => format!("onehand opened {url}. It is ready for review."),
            (PrState::Open, true) => format!("onehand opened {url}."),
            (PrState::Merged, _) => format!("onehand opened {url}. It was merged."),
            (PrState::Closed, _) => format!("onehand opened {url}. It was closed unmerged."),
        },
        Ok(Verdict::Commits(0)) => format!("onehand left no commit on `{branch}`."),
        Ok(Verdict::Commits(n)) => format!(
            "onehand left {n} commit{} on `{branch}`.",
            if *n == 1 { "" } else { "s" }
        ),
        Err(err) => format!("onehand could not tell what the run left on `{branch}`: {err}"),
    };
    let mut said = format!("{head}\n\n{}", ended(pending.outcome.as_ref()));
    if let Some(asked) = pending.asked.as_deref().filter(|q| !q.trim().is_empty()) {
        said += &format!(
            "\n\nIt ended waiting on a decision nobody answered:\n\n{}",
            quoted(asked.trim())
        );
    }
    let done = pending.outcome == Some(Outcome::Done);
    if let Some(tail) = pending
        .ended_on
        .as_deref()
        .map(str::trim)
        .filter(|tail| !tail.is_empty() && !done)
    {
        said += &format!("\n\nIts last step ended on:\n\n{}", quoted(tail));
    }
    said
}

/// How a run ended, as a sentence for the issue.
pub(super) fn ended(outcome: Option<&Outcome>) -> String {
    let Some(outcome) = outcome else {
        return "The run was cut off, and let go rather than resumed.".to_string();
    };
    match outcome {
        Outcome::Done => "Every step of the workflow passed.".to_string(),
        Outcome::Stopped(Stop::ByPerson) => "The run was stopped by hand.".to_string(),
        Outcome::Stopped(Stop::TakenOver) => {
            "The run was taken over by hand, and stopped watching.".to_string()
        }
        Outcome::Stopped(Stop::TimedOut) => {
            "The run hit its timeout and was cancelled.".to_string()
        }
        Outcome::Stopped(Stop::LinkLost) => "The agent stopped answering.".to_string(),
        Outcome::Stopped(Stop::Closed) => {
            "The run's session was closed before it finished.".to_string()
        }
        Outcome::Exhausted { step } => {
            format!("The run stopped after too many misses at the {step} step.")
        }
        Outcome::Failed(why) => format!("The run failed: {why}"),
    }
}

/// What the issue is told when a run was claimed and then could not start.
pub fn could_not_start(why: &str) -> String {
    format!("onehand could not start the run: {why}")
}

/// `text` as a Markdown quote, every line of it.
fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
