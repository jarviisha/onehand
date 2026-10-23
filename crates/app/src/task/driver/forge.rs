//! A run's forge steps as the app carries them out: the push, the pull
//! request and the look at its status checks, each done off the UI thread
//! and reported to the run's drive.

use super::{feed, read, with};
use gpui::{App, Task};
use onehand_core::connector::{self, CheckState, Connector, PullRequest};
use onehand_core::task::drive::Input;
use onehand_core::workflow::{PrOpened, Seen, judge, pull_request_step, waited_on, with_logs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often a run waiting on its pull request's status checks looks at
/// them.
// ponytail: the session stays open while the status checks run, and a
// restart waits afresh; park the run with no session once idle adapters cost
// something.
const STATUS_CHECKS_EVERY: Duration = Duration::from_secs(60);

/// How many failing status checks' logs a repair is handed.
const LOGS_MAX: usize = 3;

/// Where a run's forge steps go.
struct Forge {
    /// The run's work.
    dir: std::path::PathBuf,
    branch: String,
    connector: &'static dyn Connector,
}

/// Where the run on session `uid` goes on the forge: its setup's connector,
/// or why there is none.
fn forge_of(uid: u64, cx: &App) -> Option<Result<Forge, String>> {
    read(uid, cx, |d| {
        let setup = &d.run().setup;
        let branch = setup
            .branch
            .clone()
            .ok_or_else(|| "the run has no branch of its own".to_string())?;
        let name = setup.forge.as_deref().unwrap_or_default();
        let connector = connector::named(crate::features::connectors(), name)
            .ok_or_else(|| format!("no connector called {name} is built into this onehand"))?;
        Ok(Forge {
            dir: setup.dir.clone(),
            branch,
            connector,
        })
    })
}

/// Push `commit` as the run's branch, or, with none, open its pull request
/// unless one is open on the branch already; then report how it went, with
/// the pull request opened or taken up. One closed without being merged is
/// refused: a person turned it down, and a second beside it would ask again.
///
/// Held as a step's command is: its flag is what a Stop sets, and no pull
/// request is opened once it is.
pub(super) fn on_forge(uid: u64, commit: Option<String>, cx: &mut App) {
    let Some(forge) = forge_of(uid, cx) else {
        return;
    };
    let text = read(uid, cx, |d| {
        let task = crate::task::task(d.task(), cx);
        onehand_core::unattended::pull_request_text(
            &d.run().brief,
            task.as_ref().and_then(|task| task.issue()),
        )
    });
    let cancel = Arc::new(AtomicBool::new(false));
    with(uid, cx, |d| d.command = Some(cancel.clone()));
    cx.spawn(async move |cx| {
        let done = cx
            .background_executor()
            .spawn(async move {
                let Forge {
                    dir,
                    branch,
                    connector,
                } = forge?;
                if let Some(commit) = commit {
                    return connector
                        .push_blocking(&dir, &commit, &branch)
                        .map(|()| None);
                }
                let opened = |pr: PullRequest| PrOpened {
                    number: pr.number,
                    url: pr.url,
                };
                if let Some(pr) =
                    pull_request_step(connector.pull_request_for_blocking(&dir, &branch)?)?
                {
                    return Ok(Some(opened(pr)));
                }
                if cancel.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                let (title, body) = text.unwrap_or_default();
                connector.open_pull_request_blocking(&dir, &branch, &title, &body)?;
                // Read back for what to keep; one that cannot be read yet is
                // still found by its branch.
                Ok(connector
                    .pull_request_for_blocking(&dir, &branch)
                    .ok()
                    .flatten()
                    .map(opened))
            })
            .await;
        cx.update(|cx| {
            with(uid, cx, |d| d.command = None);
            feed(uid, Input::ForgeDone(done), cx);
        });
    })
    .detach();
}

/// Look at the pull request's status checks on `pushed` every
/// [`STATUS_CHECKS_EVERY`] until they say something other than pending,
/// waiting at most `wait` from `since`. Failing ones carry their logs; all
/// passing takes a draft out of draft first, and one that could not be is
/// waited on like a forge that could not be read.
pub(super) fn watch_status_checks(
    uid: u64,
    pushed: Option<String>,
    since: Instant,
    wait: Duration,
    cx: &mut App,
) -> Task<()> {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(STATUS_CHECKS_EVERY).await;
            let Some(forge) = cx.update(|cx| forge_of(uid, cx)) else {
                return;
            };
            let pushed = pushed.clone();
            let seen = cx
                .background_executor()
                .spawn(async move {
                    let Forge {
                        dir,
                        branch,
                        connector,
                    } = match forge {
                        Ok(forge) => forge,
                        Err(why) => return Seen::Fail(why),
                    };
                    let read = connector.pull_request_for_blocking(&dir, &branch);
                    let pr = read.as_ref().ok().cloned().flatten();
                    let seen = judge(
                        read.as_ref().map(Option::as_ref).map_err(Clone::clone),
                        pushed.as_deref(),
                        since.elapsed(),
                        wait,
                    );
                    match (seen, pr) {
                        (Seen::Repair(said), Some(pr)) => {
                            let logs: Vec<(String, String)> = pr
                                .checks
                                .iter()
                                .filter(|c| c.state == CheckState::Failed)
                                .take(LOGS_MAX)
                                .map(|check| {
                                    let log = connector
                                        .check_log_blocking(&dir, check)
                                        .unwrap_or_else(|why| format!("(no log: {why})"));
                                    (check.name.clone(), log)
                                })
                                .collect();
                            Seen::Repair(with_logs(said, &logs))
                        }
                        (Seen::Passed, Some(pr)) if pr.draft => {
                            match connector.mark_ready_blocking(&dir, pr.number) {
                                Ok(()) => Seen::Passed,
                                Err(why) => waited_on(
                                    format!("{} could not be taken out of draft: {why}", pr.url),
                                    since.elapsed(),
                                    wait,
                                ),
                            }
                        }
                        (seen, _) => seen,
                    }
                })
                .await;
            if seen == Seen::Pending {
                continue;
            }
            cx.update(|cx| feed(uid, Input::ChecksSeen(seen), cx));
            return;
        }
    })
}
