//! What a pull request's checks say about the run waiting on them, read from
//! the forge rather than the agent's word.

use crate::connector::{CheckState, PrState, PullRequest};
use std::time::Duration;

/// How long a pull request with no checks at all is given for some to
/// appear: a forge registers them a little after the push, so none at once
/// is not yet none at all.
pub(crate) const CHECKS_GRACE: Duration = Duration::from_secs(600);

/// How long checks are waited on when the step's wait does not read, which
/// validation refuses, so only a run file edited by hand meets it.
pub(crate) const WAIT_FALLBACK: Duration = Duration::from_secs(3600);

/// What a look at the checks found, for [`super::Run::checks_seen`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Still running, or none yet within the grace.
    Pending,
    /// Every check passed on the head, or none ran past the grace.
    Passed,
    /// What failed, as the step sent back is told it.
    Failing(String),
    /// Not done within the step's wait, for the reason given: nothing an
    /// agent can fix, so the run ends.
    Stalled(String),
    /// Closed without being merged.
    Closed,
    Merged,
}

/// What the pull request on the run's branch, `pr`, says after waiting
/// `waited` of at most `limit`. A failure or a conflict wins over a check
/// still running: there is something to fix already. The names of the
/// failing checks are what [`Seen::Failing`] holds; their logs are added by
/// whoever can read them.
pub fn judge(pr: Option<&PullRequest>, waited: Duration, limit: Duration) -> Seen {
    let Some(pr) = pr else {
        return Seen::Closed;
    };
    match pr.state {
        PrState::Open => {}
        PrState::Closed => return Seen::Closed,
        PrState::Merged => return Seen::Merged,
    }
    let failing: Vec<&str> = pr
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Failed)
        .map(|c| c.name.as_str())
        .collect();
    if !failing.is_empty() || pr.conflicting {
        let mut said = format!("The pull request {}", pr.url);
        if pr.conflicting {
            said += " conflicts with its base branch: merge the base in and resolve the \
                     conflicts, keeping this branch's work.";
        }
        if !failing.is_empty() {
            said += &format!(
                "{}has checks failing on its latest commit: {}. Fix what this branch's change \
                 broke. If a failure is not caused by this branch's change, say so in your \
                 answer and leave the code alone.",
                if pr.conflicting { " It also " } else { " " },
                failing.join(", ")
            );
        }
        return Seen::Failing(said);
    }
    let pending = pr.checks.iter().any(|c| c.state == CheckState::Pending)
        || (pr.checks.is_empty() && waited < CHECKS_GRACE);
    match pending {
        true if waited >= limit => Seen::Stalled(format!(
            "the checks on {} were still not done after {}m",
            pr.url,
            limit.as_secs() / 60
        )),
        true => Seen::Pending,
        false => Seen::Passed,
    }
}
