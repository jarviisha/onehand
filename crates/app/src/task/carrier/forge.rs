//! A run's forge calls as the app carries them out: each done off the UI
//! thread on the connector the run's setup names, and reported to the run's
//! drive, which decides what each answer leads to.

use super::{feed, read};
use gpui::App;
use onehand_core::connector::{self, Check, Connector};
use onehand_core::task::drive::Input;
use std::path::PathBuf;

// The connectors a run's forge is looked up among: the ones built in, and in
// a test the forge that test watches.
#[cfg(not(test))]
use crate::features::connectors;
#[cfg(test)]
use crate::task::tests::connectors;

/// Where a run's forge calls go.
struct Forge {
    /// The run's work.
    dir: PathBuf,
    branch: String,
    connector: &'static dyn Connector,
}

/// Where task `id`'s run goes on the forge: its setup's connector, or why
/// there is none.
fn forge_of(id: &str, cx: &App) -> Option<Result<Forge, String>> {
    read(id, cx, |d| {
        let setup = &d.run().setup;
        let branch = setup
            .branch
            .clone()
            .ok_or_else(|| "the run has no branch of its own".to_string())?;
        let name = setup.forge.as_deref().unwrap_or_default();
        let connector = connector::named(connectors(), name)
            .ok_or_else(|| format!("no connector called {name} is built into this onehand"))?;
        Ok(Forge {
            dir: setup.dir.clone(),
            branch,
            connector,
        })
    })
}

/// Do `call` on task `id`'s forge off the UI thread, then tell its run what
/// came of it; a run with no forge to call is told why.
fn call(id: &str, cx: &mut App, call: impl FnOnce(Forge) -> Input + Send + 'static) {
    let Some(forge) = forge_of(id, cx) else {
        return;
    };
    let task = id.to_string();
    cx.spawn(async move |cx| {
        let input = cx
            .background_executor()
            .spawn(async move {
                match forge {
                    Ok(forge) => call(forge),
                    Err(why) => Input::NoForge(why),
                }
            })
            .await;
        cx.update(|cx| feed(&task, input, cx));
    })
    .detach();
}

/// Push `commit` as the run's branch.
pub(super) fn push(id: &str, commit: String, cx: &mut App) {
    call(id, cx, move |f| {
        Input::Pushed(f.connector.push_blocking(&f.dir, &commit, &f.branch))
    });
}

/// Read the pull request on the run's branch.
pub(super) fn find(id: &str, cx: &mut App) {
    call(id, cx, |f| {
        Input::PullRequestFound(f.connector.pull_request_for_blocking(&f.dir, &f.branch))
    });
}

/// Open the run's pull request as a draft, written from the commits its
/// branch adds and its brief, then read it back. Commits that cannot be read
/// leave the brief to name it.
pub(super) fn open(id: &str, cx: &mut App) {
    let Some(brief) = read(id, cx, |d| d.run().brief.clone()) else {
        return;
    };
    call(id, cx, move |f| {
        let commits = onehand_core::worktree::branch_commits_blocking(&f.dir, &f.branch, &f.branch)
            .unwrap_or_default();
        let (title, body) = brief.pull_request_text(&commits);
        let opened = f
            .connector
            .open_pull_request_blocking(&f.dir, &f.branch, &title, &body)
            .map(|()| f.connector.pull_request_for_blocking(&f.dir, &f.branch));
        Input::PullRequestOpened(opened)
    });
}

/// Read the logs of `checks`.
pub(super) fn read_logs(id: &str, checks: Vec<Check>, cx: &mut App) {
    call(id, cx, move |f| {
        let logs = checks
            .iter()
            .map(|check| {
                (
                    check.name.clone(),
                    f.connector.check_log_blocking(&f.dir, check),
                )
            })
            .collect();
        Input::LogsRead(logs)
    });
}

/// Take pull request `number` out of draft.
pub(super) fn mark_ready(id: &str, number: u64, cx: &mut App) {
    call(id, cx, move |f| {
        Input::MarkedReady(f.connector.mark_ready_blocking(&f.dir, number))
    });
}
