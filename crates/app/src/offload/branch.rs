//! An offloaded project on a branch, as the app drives it: no key and no
//! lock. Its host's sessions work on `onehand/<host>` in a mirror of its
//! own; the branch is fetched here into a remote-tracking ref when it moved,
//! and counted against `HEAD` for the person to merge. The code goes up only
//! when the person asks, as a commit merged into the host's branch.
//!
//! Every git call is a blocking call in core, run on the background
//! executor; every call to the host rides its one attach.

use super::{Moving, Offload, attach, keep, update};
use crate::hosts;
use gpui::{App, AsyncApp};
use onehand_core::host::client::HostConn;
use onehand_core::host::offload::branch as git;
use onehand_core::host::wire::MirrorState;
use onehand_core::workspace::{HostName, stem_for};
use std::path::{Path, PathBuf};

/// What is known here of an offloaded project's branch.
#[derive(Clone, Debug)]
pub(crate) struct BranchState {
    /// `onehand/<host>`.
    pub(crate) name: String,
    /// The commits the branch, as fetched, has that `HEAD` does not; `None`
    /// until first counted.
    pub(crate) ahead: Option<usize>,
    /// Why the last *Update host from laptop* did not land, until one does:
    /// a fetch after it says nothing of it.
    pub(crate) refused: Option<String>,
}

/// The branch mirror's name on the host, beside the mirror a project with a
/// key would have: switching modes never touches the other's code.
fn mirror_name(path: &Path) -> String {
    format!("{}.branch", stem_for(path))
}

/// Start keeping `path`, offloaded to `host` on `branch`, and count what it
/// has to merge. A folder already kept is left as it is.
pub(crate) fn track_branch(cx: &mut App, path: PathBuf, host: HostName, branch: String) {
    if super::get(cx, &path).is_some() {
        return;
    }
    let o = Offload {
        host,
        state: None,
        moving: None,
        failed: None,
        branch: Some(BranchState {
            name: branch,
            ahead: None,
            refused: None,
        }),
    };
    keep(cx, path.clone(), o);
    cx.spawn(async move |cx| refresh(cx, path).await).detach();
}

/// The branch mirror of `path` on the host over `conn`, as the host has it.
async fn mirror(
    cx: &mut AsyncApp,
    conn: &HostConn,
    path: &Path,
    branch: &str,
) -> Result<MirrorState, String> {
    let (conn, name, branch) = (conn.clone(), mirror_name(path), branch.to_string());
    let host = conn.host().to_string();
    cx.update(|cx| hosts::on_runtime(cx, async move { conn.mirror(&name, Some(&branch)).await }))
        .await
        .map_err(|e| format!("Could not ask {host} about its copy: {e}"))
}

/// Push this folder's `HEAD` into the mirror and have the host bring it
/// onto its branch: started from it, or merged in.
async fn send_commit(
    cx: &mut AsyncApp,
    conn: &HostConn,
    path: &Path,
    host: &HostName,
    mirror: &Path,
) -> Result<(), String> {
    let (addr, folder, at) = (host.addr(), path.to_path_buf(), mirror.to_path_buf());
    let commit = cx
        .background_executor()
        .spawn(async move { git::push_head_blocking(&addr, &folder, &at) })
        .await?;
    let (conn, at) = (conn.clone(), mirror.to_path_buf());
    cx.update(|cx| hosts::on_runtime(cx, async move { conn.mirror_update(&at, &commit).await }))
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Fetch the host's branch when it is not where it was last fetched, then
/// count what it has to merge.
async fn fetch_and_count(
    cx: &mut AsyncApp,
    path: &Path,
    host: &HostName,
    branch: &str,
    state: Option<&MirrorState>,
) -> Result<usize, String> {
    let (addr, folder, branch) = (host.addr(), path.to_path_buf(), branch.to_string());
    let (head, mirror) = (
        state.and_then(|s| s.head.clone()),
        state.map(|s| s.path.clone()),
    );
    cx.background_executor()
        .spawn(async move {
            if let (Some(head), Some(mirror)) = (head, mirror)
                && git::local_head_blocking(&folder, &branch).as_ref() != Some(&head)
            {
                git::fetch_blocking(&addr, &folder, &mirror, &branch)?;
            }
            git::ahead_blocking(&folder, &branch)
        })
        .await
}

/// Offload `path` to `host` on a branch: the mirror asked for, and, unless
/// its branch started already (offloaded before), this folder's commit sent
/// to start it. Answers the mirror and the branch.
pub(crate) async fn offload_branch_to(
    cx: &mut AsyncApp,
    path: PathBuf,
    host: HostName,
) -> Result<(PathBuf, String), String> {
    let branch = git::branch_for(&host);
    let conn = attach(cx, &host).await?;
    let mut state = mirror(cx, &conn, &path, &branch).await?;
    if state.head.is_none() {
        send_commit(cx, &conn, &path, &host, &state.path).await?;
        state = mirror(cx, &conn, &path, &branch).await?;
    }
    fetch_and_count(cx, &path, &host, &branch, Some(&state)).await?;
    Ok((state.path, branch))
}

/// Claim `path`'s branch for `moving`: its host and branch, when it is on
/// one and nothing moves it now.
fn claim(cx: &mut AsyncApp, path: &Path, moving: Moving) -> Option<(HostName, String)> {
    update(cx, path, |o| {
        let name = o.branch.as_ref()?.name.clone();
        if o.moving.is_some() {
            return None;
        }
        o.moving = Some(moving);
        Some((o.host.clone(), name))
    })
    .flatten()
}

/// Keep what a fetch (`Back`) or an update (`Up`) came to, the branch no
/// longer moving. Each failure goes only with its own kind's next success:
/// an update's conflict stands through the fetches the poll makes after it.
pub(super) fn landed_on(o: &mut Offload, by: Moving, counted: Result<usize, String>) {
    o.moving = None;
    let Some(branch) = o.branch.as_mut() else {
        return;
    };
    match (by, counted) {
        (Moving::Up, Ok(ahead)) => {
            branch.ahead = Some(ahead);
            branch.refused = None;
            o.failed = None;
        }
        (Moving::Up, Err(why)) => branch.refused = Some(why),
        (Moving::Back, Ok(ahead)) => {
            branch.ahead = Some(ahead);
            o.failed = None;
        }
        (Moving::Back, Err(why)) => o.failed = Some(why),
    }
}

fn land(cx: &mut AsyncApp, path: &Path, by: Moving, counted: Result<usize, String>) {
    update(cx, path, |o| landed_on(o, by, counted));
}

/// Fetch `path`'s branch if its host moved it, then count what it has to
/// merge. Never attaches: with the host away it counts what was fetched.
pub(crate) async fn refresh(cx: &mut AsyncApp, path: PathBuf) {
    let Some((host, branch)) = claim(cx, &path, Moving::Back) else {
        return;
    };
    let state = match cx.update(|cx| hosts::live(cx, &host)) {
        Some(conn) => match mirror(cx, &conn, &path, &branch).await {
            Ok(state) => Some(state),
            Err(why) => return land(cx, &path, Moving::Back, Err(why)),
        },
        None => None,
    };
    let counted = fetch_and_count(cx, &path, &host, &branch, state.as_ref()).await;
    land(cx, &path, Moving::Back, counted);
}

/// *Update host from laptop*: this folder's commit merged into the host's
/// branch, once nothing works there, then the branch fetched back. Why not,
/// on the project's page.
pub(crate) async fn update_host(cx: &mut AsyncApp, path: PathBuf) {
    let Some((host, branch)) = claim(cx, &path, Moving::Up) else {
        return;
    };
    let counted = async {
        let conn = attach(cx, &host).await?;
        let state = mirror(cx, &conn, &path, &branch).await?;
        if state.working {
            return Err(format!("Wait for the sessions on {host} to finish"));
        }
        send_commit(cx, &conn, &path, &host, &state.path).await?;
        let state = mirror(cx, &conn, &path, &branch).await?;
        fetch_and_count(cx, &path, &host, &branch, Some(&state)).await
    }
    .await;
    land(cx, &path, Moving::Up, counted);
}
