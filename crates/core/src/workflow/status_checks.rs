//! What a pull request's status checks say about the run waiting on them,
//! read from the forge rather than the agent's word.

use crate::connector::{CheckState, PrState, PullRequest};
use std::time::Duration;

/// How long a pull request with no status checks at all is given for some to
/// appear: a forge registers them a little after the push, so none at once
/// is not yet none at all.
pub(crate) const STATUS_CHECKS_GRACE: Duration = Duration::from_secs(600);

/// What a look at the status checks found, for
/// [`super::Run::status_checks_seen`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Still running, none yet within the grace, or not yet on the pushed
    /// commit.
    Pending,
    /// Every one passed on the pushed commit, or none ran past the grace.
    Passed,
    /// Something for the agent to fix, as the step sent back is told it.
    Repair(String),
    /// Nothing an agent can fix: the run fails, for the reason given.
    Fail(String),
    Merged,
}

/// Something that keeps the run waiting, `why`, after `waited` of at most
/// `wait`: pending while there is time left, the run's failure after.
pub fn waited_on(why: String, waited: Duration, wait: Duration) -> Seen {
    match waited >= wait {
        true => Seen::Fail(format!("{why}, still after {}m", wait.as_secs() / 60)),
        false => Seen::Pending,
    }
}

/// What the forge's read of the pull request on the run's branch, `read`,
/// says after waiting `waited` of at most `wait` for the status checks on
/// `pushed`, the commit the run put there.
///
/// - A failure or a conflict wins over a status check still running: there
///   is something to fix already. The names of the failing ones are what
///   [`Seen::Repair`] holds; their logs are added by whoever can read them.
/// - Status checks on any other head say nothing about what was pushed, so
///   they are waited past, as is a forge that cannot be read for now.
/// - Nothing is waited on past `wait`.
pub fn judge(
    read: Result<Option<&PullRequest>, String>,
    pushed: Option<&str>,
    waited: Duration,
    wait: Duration,
) -> Seen {
    let pending = |why: String| waited_on(why, waited, wait);
    let pr = match read {
        Ok(Some(pr)) => pr,
        Ok(None) => return Seen::Fail("there is no pull request on its branch".to_string()),
        Err(why) => return pending(format!("the pull request could not be read: {why}")),
    };
    match pr.state {
        PrState::Open => {}
        PrState::Closed => {
            return Seen::Fail(format!(
                "its pull request {} was closed without being merged",
                pr.url
            ));
        }
        PrState::Merged => return Seen::Merged,
    }
    if pushed.is_some_and(|pushed| pushed != pr.head) {
        return pending(format!(
            "{} is at another commit than the one onehand pushed",
            pr.url
        ));
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
                "{}has status checks failing on its latest commit: {}. Fix what this \
                 branch's change broke. If a failure is not caused by this branch's change, \
                 say so in your answer and leave the code alone.",
                if pr.conflicting { " It also " } else { " " },
                failing.join(", ")
            );
        }
        return Seen::Repair(said);
    }
    // A wait shorter than the grace ends the grace with it, so a pull request
    // with none at all can still pass.
    let running = pr.checks.iter().any(|c| c.state == CheckState::Pending)
        || (pr.checks.is_empty() && waited < STATUS_CHECKS_GRACE.min(wait));
    match running {
        true => pending(format!("the status checks on {} were not done", pr.url)),
        false => Seen::Passed,
    }
}

/// What a repair is told, `said`, with the end of each failing status
/// check's log after it, as `(name, log)`, each fenced on its own.
pub fn with_logs(mut said: String, logs: &[(String, String)]) -> String {
    for (name, log) in logs {
        said.push_str(&format!(
            "\n\n{name}:\n\n{}",
            super::prompt::fenced(log.trim())
        ));
    }
    said
}
