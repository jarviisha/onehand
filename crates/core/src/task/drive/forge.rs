//! A run's forge steps, one call to the forge at a time: the push; the pull
//! request, taken up when one is open on the branch and opened otherwise;
//! and the look at its status checks, again on a clock until they say
//! something, with the logs of the failing ones read for a repair and a draft
//! taken out of draft once they all pass.
//!
//! **A forge call that leaves something on the forge is awaited**: a push, an
//! open and taking a draft out of draft each hold a Stop until they land, so
//! a run said to be stopped has nothing of it still going up. A look leaves
//! nothing, and is dropped when the run moved on.

use super::{Chat, Effect, RunDrive, Timer};
use crate::connector::{CheckState, PullRequest};
use crate::workflow::{judge, pull_request_step, waited_on, with_logs, PrOpened, Seen};
use crate::Now;
use std::time::{Duration, Instant};

/// How often a run waiting on its pull request's status checks looks at
/// them.
// ponytail: the session stays open while the status checks run, and a
// restart waits afresh; park the run with no session once idle adapters cost
// something.
pub(super) const STATUS_CHECKS_EVERY: Duration = Duration::from_secs(60);

/// How many failing status checks' logs a repair is handed.
const LOGS_MAX: usize = 3;

/// The status checks being waited on: on the commit `pushed`, since `since`,
/// at most `wait`.
#[derive(Debug)]
pub(super) struct Watch {
    pushed: Option<String>,
    since: Instant,
    wait: Duration,
}

/// The forge call a run is in, and what its answer is for.
#[derive(Debug)]
pub(super) enum Forging {
    /// Pushing the commit the check passed on.
    Pushing,
    /// Taking up the pull request open on the branch, or opening one.
    Opening,
    /// Waiting for the status checks, looking again on the clock.
    Watching(Watch),
    /// Reading the logs of the failing checks, for the repair `said`.
    Logs(Watch, String),
    /// Taking the pull request at `url` out of draft, all its checks passed.
    Readying(Watch, String),
}

impl RunDrive {
    /// Push `commit` as the run's branch.
    pub(super) fn push(&mut self, commit: String, fx: &mut Vec<Effect>) {
        self.command = true;
        self.forging = Some(Forging::Pushing);
        fx.push(Effect::Push { commit });
    }

    /// Take up the pull request open on the run's branch, or open one.
    pub(super) fn open_pull_request(&mut self, fx: &mut Vec<Effect>) {
        self.command = true;
        self.forging = Some(Forging::Opening);
        fx.push(Effect::FindPullRequest);
    }

    /// Wait for the pull request's status checks on `pushed`, at most
    /// `wait` from `now`.
    pub(super) fn watch_checks(
        &mut self,
        pushed: Option<String>,
        wait: Duration,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        self.forging = Some(Forging::Watching(Watch {
            pushed,
            since: now.at,
            wait,
        }));
        fx.push(Effect::Arm {
            timer: Timer::Checks,
            after: STATUS_CHECKS_EVERY,
        });
    }

    /// The clock for the next look ran out: look, while the run still waits.
    pub(super) fn look_again(&mut self, fx: &mut Vec<Effect>) {
        if matches!(self.forging, Some(Forging::Watching(_))) {
            fx.push(Effect::FindPullRequest);
        }
    }

    /// The pushed commit landed, or did not.
    pub(super) fn pushed(
        &mut self,
        pushed: Result<(), String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        if matches!(self.forging, Some(Forging::Pushing)) {
            self.forge_done(pushed.map(|()| None), chat, now, fx);
        }
    }

    /// The pull request on the run's branch, as the forge read it.
    pub(super) fn found(
        &mut self,
        found: Result<Option<PullRequest>, String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        match self.forging.take() {
            Some(Forging::Opening) => match found.and_then(pull_request_step) {
                Err(why) => self.forge_done(Err(why), chat, now, fx),
                Ok(Some(pr)) => self.forge_done(Ok(Some(opened(pr))), chat, now, fx),
                // A Stop pressed while it was read opens nothing.
                Ok(None) if self.stopping.is_some() => self.forge_done(Ok(None), chat, now, fx),
                Ok(None) => {
                    self.forging = Some(Forging::Opening);
                    fx.push(Effect::OpenPullRequest);
                }
            },
            Some(Forging::Watching(watch)) => {
                let seen = judge(
                    found.as_ref().map(Option::as_ref).map_err(Clone::clone),
                    watch.pushed.as_deref(),
                    now.at.saturating_duration_since(watch.since),
                    watch.wait,
                );
                match (seen, found.ok().flatten()) {
                    (Seen::Repair(said), Some(pr)) => {
                        let checks: Vec<_> = pr
                            .checks
                            .into_iter()
                            .filter(|c| c.state == CheckState::Failed)
                            .take(LOGS_MAX)
                            .collect();
                        match checks.is_empty() {
                            true => self.seen(watch, Seen::Repair(said), chat, now, fx),
                            false => {
                                self.forging = Some(Forging::Logs(watch, said));
                                fx.push(Effect::ReadLogs { checks });
                            }
                        }
                    }
                    (Seen::Passed, Some(pr)) if pr.draft => {
                        self.command = true;
                        self.forging = Some(Forging::Readying(watch, pr.url));
                        fx.push(Effect::MarkReady { number: pr.number });
                    }
                    (seen, _) => self.seen(watch, seen, chat, now, fx),
                }
            }
            // Not what the run asked about now.
            other @ (None | Some(Forging::Pushing | Forging::Logs(..) | Forging::Readying(..))) => {
                self.forging = other
            }
        }
    }

    /// The pull request was opened, and read back, or why it was not opened.
    /// One that cannot be read back yet is still found by its branch.
    pub(super) fn pull_request_opened(
        &mut self,
        opened_read: Result<Result<Option<PullRequest>, String>, String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        if matches!(self.forging, Some(Forging::Opening)) {
            let done = opened_read.map(|read| read.ok().flatten().map(opened));
            self.forge_done(done, chat, now, fx);
        }
    }

    /// The failing checks' logs, by check name, or why each was not read.
    pub(super) fn logs_read(
        &mut self,
        logs: Vec<(String, Result<String, String>)>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        let (watch, said) = match self.forging.take() {
            Some(Forging::Logs(watch, said)) => (watch, said),
            // Not what the run asked about now.
            other @ (None
            | Some(
                Forging::Pushing | Forging::Opening | Forging::Watching(_) | Forging::Readying(..),
            )) => return self.forging = other,
        };
        let logs: Vec<(String, String)> = logs
            .into_iter()
            .map(|(name, log)| (name, log.unwrap_or_else(|why| format!("(no log: {why})"))))
            .collect();
        self.seen(watch, Seen::Repair(with_logs(said, &logs)), chat, now, fx);
    }

    /// The draft was taken out of draft, or why not: waited on then, as a
    /// forge that could not be read is.
    pub(super) fn marked_ready(
        &mut self,
        ready: Result<(), String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        let (watch, url) = match self.forging.take() {
            Some(Forging::Readying(watch, url)) => (watch, url),
            // Not what the run asked about now.
            other @ (None
            | Some(
                Forging::Pushing | Forging::Opening | Forging::Watching(_) | Forging::Logs(..),
            )) => return self.forging = other,
        };
        self.command = false;
        if let Some(stop) = self.stopping.take() {
            return self.advance(chat, now, fx, move |run| run.stopped(stop, now.secs));
        }
        let seen = match ready {
            Ok(()) => Seen::Passed,
            Err(why) => waited_on(
                format!("{url} could not be taken out of draft: {why}"),
                now.at.saturating_duration_since(watch.since),
                watch.wait,
            ),
        };
        self.seen(watch, seen, chat, now, fx);
    }

    /// The forge cannot be called at all, for `why`: the call under way
    /// fails, and so do status checks waited on, since no look will ever
    /// read them.
    pub(super) fn no_forge(
        &mut self,
        why: String,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        match self.forging.take() {
            Some(Forging::Watching(watch) | Forging::Logs(watch, _)) => {
                self.seen(watch, Seen::Fail(why), chat, now, fx)
            }
            Some(Forging::Readying(watch, _)) => {
                self.command = false;
                match self.stopping.take() {
                    Some(stop) => {
                        self.advance(chat, now, fx, move |run| run.stopped(stop, now.secs))
                    }
                    None => self.seen(watch, Seen::Fail(why), chat, now, fx),
                }
            }
            Some(Forging::Pushing | Forging::Opening) => self.forge_done(Err(why), chat, now, fx),
            None => {}
        }
    }

    /// A look said `seen`: look again on the clock while it is pending,
    /// otherwise the clock goes on and the engine judges it.
    fn seen(
        &mut self,
        watch: Watch,
        seen: Seen,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        if seen == Seen::Pending {
            self.forging = Some(Forging::Watching(watch));
            return fx.push(Effect::Arm {
                timer: Timer::Checks,
                after: STATUS_CHECKS_EVERY,
            });
        }
        self.budget.resume(now.at);
        self.advance(chat, now, fx, move |run| {
            run.status_checks_seen(seen, now.secs)
        });
    }

    /// A push or a pull request landed, or failed: the run ends there once
    /// called off, and otherwise goes on.
    fn forge_done(
        &mut self,
        done: Result<Option<PrOpened>, String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        self.command = false;
        self.forging = None;
        if let Some(stop) = self.stopping.take() {
            return self.advance(chat, now, fx, move |run| run.stopped(stop, now.secs));
        }
        self.advance(chat, now, fx, move |run| run.forge_done(done, now.secs));
    }
}

/// What the run keeps of a pull request it opened or took up.
fn opened(pr: PullRequest) -> PrOpened {
    PrOpened {
        number: pr.number,
        url: pr.url,
    }
}
