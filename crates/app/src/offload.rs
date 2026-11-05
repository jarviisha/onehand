//! Offloaded projects, as the app drives them: who holds each one's code, the
//! code's way up before a prompt and back once the host is done with it, and
//! the lock on this machine's writers meanwhile.
//!
//! **One entry per folder, for the whole process.** Two windows holding the
//! same project hold one copy of its code, so they share whether it is locked
//! and must never copy it back twice at once.
//!
//! **The host is the key's authority.** What is kept here (`offload.json`,
//! [`OffloadState`]) is a cache of its last word: every move asks the host
//! first, so a cache gone stale can only make a move wait, never send old
//! code up over the host's.
//!
//! Every copy and every git read is a blocking call in core, run on the
//! background executor; every call to the host rides its one attach.

use crate::hosts::{self, HostFailure};
use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};
use onehand_core::host::offload::{self, Back, Manifest, OffloadState};
use onehand_core::host::wire::{Holder, MirrorState};
use onehand_core::workspace::{HostName, stem_for, storage_for};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often an offloaded project whose host holds the code is checked for
/// being done there, while the host is attached: a session no window shows
/// sends no turn end here.
pub(crate) const OFFLOAD_POLL: Duration = Duration::from_secs(30);

/// The offloaded projects of every window, by their folder here.
#[derive(Default)]
pub(crate) struct Offloads {
    roots: HashMap<PathBuf, Offload>,
    /// The poll, started with the first entry.
    _poll: Option<Task<()>>,
}

impl Global for Offloads {}

/// One offloaded folder.
pub(crate) struct Offload {
    pub(crate) host: HostName,
    /// As last kept; `None` until read.
    pub(crate) state: Option<OffloadState>,
    /// The code on its way now, if it is.
    pub(crate) moving: Option<Moving>,
    /// Why the last move did not go, in a person's words, until one does.
    pub(crate) failed: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Moving {
    Up,
    Back,
}

impl Offload {
    /// Whether this machine's writers are off: the host holds the code, it is
    /// on its way up, or what was kept has not been read yet.
    pub(crate) fn locked(&self) -> bool {
        self.moving == Some(Moving::Up) || self.state.as_ref().is_none_or(|s| s.key == Holder::Host)
    }

    /// Whether the code is this machine's, settled: what *Run here again*
    /// needs.
    pub(crate) fn here(&self) -> bool {
        self.moving.is_none() && self.state.as_ref().is_some_and(|s| s.key == Holder::Laptop)
    }
}

/// What stands under a locked project's editor and terminals.
pub(crate) fn lock_line(host: &HostName) -> String {
    format!("{host} has this project's code until its sessions finish; edits are off meanwhile")
}

/// What a project's rail mark says while its host holds the code.
pub(crate) fn mark_line(host: &HostName) -> String {
    format!("{host} has the code until its sessions finish")
}

/// `path`'s entry, if it is offloaded.
pub(crate) fn get<'a>(cx: &'a App, path: &Path) -> Option<&'a Offload> {
    cx.try_global::<Offloads>()?.roots.get(path)
}

/// Every locked folder, with its line.
pub(crate) fn lock_lines(cx: &App) -> HashMap<PathBuf, String> {
    cx.try_global::<Offloads>()
        .map(|all| {
            all.roots
                .iter()
                .filter(|(_, o)| o.locked())
                .map(|(path, o)| (path.clone(), lock_line(&o.host)))
                .collect()
        })
        .unwrap_or_default()
}

/// The prompt's wait while `path`'s code goes up to its host.
pub(crate) fn sending(cx: &App, path: &Path) -> Option<String> {
    let o = get(cx, path).filter(|o| o.moving == Some(Moving::Up))?;
    Some(sending_line(&o.host))
}

fn sending_line(host: &HostName) -> String {
    format!("Sending the code to {host}…")
}

/// What a prompt to a session of `path` says while the code goes up.
pub(crate) fn sending_line_for(cx: &App, path: &Path) -> String {
    match get(cx, path) {
        Some(o) => sending_line(&o.host),
        None => "Sending the code to the host…".into(),
    }
}

/// Claim the send of `path`'s code before a prompt to a session of it: `true`
/// when it is offloaded and this machine holds the code, or holds it as far
/// as anything here knows, and nothing else moves it now. Claimed at once, so
/// a second press waits on this send rather than starting one of its own.
pub(crate) fn claim_send(cx: &mut App, path: &Path) -> bool {
    if !get(cx, path).is_some_and(|o| o.moving.is_none() && !o.locked()) {
        return false;
    }
    cx.update_global::<Offloads, _>(|all, _| {
        if let Some(o) = all.roots.get_mut(path) {
            o.moving = Some(Moving::Up);
        }
    });
    true
}

/// Start keeping `path`, offloaded to `host`: with `state` when
/// it was just offloaded, else reading what was kept of it. A folder already
/// kept is left as it is, unless `state` says it anew.
pub(crate) fn track(cx: &mut App, path: PathBuf, host: HostName, state: Option<OffloadState>) {
    if get(cx, &path).is_some() && state.is_none() {
        return;
    }
    let first = cx.default_global::<Offloads>()._poll.is_none();
    let polling = first.then(|| cx.spawn(async move |cx| poll(cx).await));
    let read = state.is_none();
    let offloads = cx.global_mut::<Offloads>();
    offloads.roots.insert(
        path.clone(),
        Offload {
            host,
            state,
            moving: None,
            failed: None,
        },
    );
    if polling.is_some() {
        offloads._poll = polling;
    }
    if !read {
        return;
    }
    cx.spawn(async move |cx| {
        let dir = storage_for(&path);
        let read = cx
            .background_executor()
            .spawn(async move { OffloadState::load(&dir) })
            .await;
        update(cx, &path, |o| match read {
            // Nothing kept: the host is asked before any move, so taking the
            // key for this machine's can only make the first prompt ask.
            Ok(state) => o.state = Some(state.unwrap_or_else(|| fresh(Holder::Laptop))),
            // Kept locked: what holds the code is not known here.
            Err(why) => o.failed = Some(format!("Could not read what was kept: {why}")),
        });
    })
    .detach();
}

/// Stop keeping `path`: it runs here again, or left the workspace.
/// What was kept stays on disk, so a later offload sends only what changed.
pub(crate) fn forget(cx: &mut App, path: &Path) {
    if cx.try_global::<Offloads>().is_some() {
        cx.update_global::<Offloads, _>(|all, _| {
            all.roots.remove(path);
        });
    }
}

fn fresh(key: Holder) -> OffloadState {
    OffloadState {
        key,
        manifest: Manifest::default(),
        moved: 0,
        stopped: None,
    }
}

/// Change `path`'s entry, if it is still kept, telling every observer.
fn update<R>(cx: &mut AsyncApp, path: &Path, f: impl FnOnce(&mut Offload) -> R) -> Option<R> {
    cx.update(|cx| {
        cx.try_global::<Offloads>()?;
        cx.update_global::<Offloads, _>(|all, _| all.roots.get_mut(path).map(f))
    })
}

/// The attach to `host`, its failure in a person's words.
async fn attach(
    cx: &mut AsyncApp,
    host: &HostName,
) -> Result<onehand_core::host::client::HostConn, String> {
    match cx.update(|cx| hosts::conn(cx, host)).await {
        Ok(conn) => Ok(conn),
        Err(HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why)) => {
            Err(why)
        }
        Err(HostFailure::Busy { host, .. }) => {
            Err(format!("{host} is waiting for its sessions to finish"))
        }
    }
}

/// This machine's mirror of `path` on the host over `conn`, as the host has
/// it now.
async fn mirror_of(
    cx: &mut AsyncApp,
    conn: &onehand_core::host::client::HostConn,
    path: &Path,
) -> Result<MirrorState, String> {
    let (conn, name, host) = (conn.clone(), stem_for(path), conn.host().to_string());
    cx.update(|cx| hosts::on_runtime(cx, async move { conn.mirror(&name).await }))
        .await
        .map_err(|e| format!("Could not ask {host} about its copy: {e}"))
}

async fn hand_key(
    cx: &mut AsyncApp,
    conn: &onehand_core::host::client::HostConn,
    mirror: &Path,
    to: Holder,
    generation: u64,
) -> Result<MirrorState, String> {
    let (conn, at) = (conn.clone(), mirror.to_path_buf());
    cx.update(|cx| {
        hosts::on_runtime(
            cx,
            async move { conn.mirror_key(&at, to, generation).await },
        )
    })
    .await
    .map_err(|e| e.to_string())
}

fn save(path: &Path, state: &OffloadState) -> Result<(), String> {
    state.save(&storage_for(path))
}

/// Offload `path` to `host`: `rsync` checked on both ends, the mirror asked
/// for, the code sent up and what went kept. Answers the mirror, and what
/// to keep. The key stays here: the first prompt hands it over. A mirror
/// whose code the host holds already (offloaded before, never brought back)
/// is not sent over: its code is newer, and it comes back as any does.
pub(crate) async fn offload_to(
    cx: &mut AsyncApp,
    path: PathBuf,
    host: HostName,
) -> Result<(PathBuf, OffloadState), String> {
    let addr = host.addr();
    let check = cx
        .background_executor()
        .spawn(async move { offload::rsync_present_blocking(&addr) });
    check.await?;
    let conn = attach(cx, &host).await?;
    let mirror = mirror_of(cx, &conn, &path).await?;
    let dir = storage_for(&path);
    let kept = {
        let dir = dir.clone();
        cx.background_executor()
            .spawn(async move { OffloadState::load(&dir) })
            .await
    }?;
    if mirror.key == Holder::Host {
        let state = OffloadState {
            key: Holder::Host,
            ..kept.unwrap_or_else(|| fresh(Holder::Host))
        };
        save(&path, &state)?;
        return Ok((mirror.path, state));
    }
    let sent = kept.map(|kept| kept.manifest).unwrap_or_default();
    let (addr, folder, at) = (host.addr(), path.clone(), mirror.path.clone());
    let manifest = cx
        .background_executor()
        .spawn(async move { offload::send_up_blocking(&addr, &folder, &at, &sent) })
        .await?;
    let state = OffloadState {
        key: Holder::Laptop,
        manifest,
        moved: onehand_core::chat::now_secs(),
        stopped: None,
    };
    save(&path, &state)?;
    Ok((mirror.path, state))
}

/// Before a prompt to a session of `path`, once [`claim_send`] claimed it:
/// send the code up and hand the key to the host. `Ok` once the host holds it, which it may have already;
/// `Err` why not, the key left here.
pub(crate) async fn send_up(cx: &mut AsyncApp, path: PathBuf) -> Result<(), String> {
    let Some((host, sent)) = update(cx, &path, |o| {
        let sent = o.state.as_ref().map(|s| s.manifest.clone());
        (o.host.clone(), sent.unwrap_or_default())
    }) else {
        return Err("The project is no longer offloaded".into());
    };
    let went = send_up_with(cx, &path, &host, sent).await;
    update(cx, &path, |o| {
        o.moving = None;
        match &went {
            Ok(state) => {
                o.state = Some(state.clone());
                o.failed = None;
            }
            Err((why, state)) => {
                if let Some(state) = state {
                    o.state = Some(state.clone());
                }
                o.failed = Some(why.clone());
            }
        }
    });
    went.map(drop).map_err(|(why, _)| why)
}

/// The send itself: what to keep, or why it stopped with what to keep then.
async fn send_up_with(
    cx: &mut AsyncApp,
    path: &Path,
    host: &HostName,
    sent: Manifest,
) -> Result<OffloadState, (String, Option<OffloadState>)> {
    let conn = attach(cx, host).await.map_err(|why| (why, None))?;
    let mirror = mirror_of(cx, &conn, path)
        .await
        .map_err(|why| (why, None))?;
    if mirror.key == Holder::Host {
        let state = OffloadState {
            key: Holder::Host,
            manifest: sent,
            moved: onehand_core::chat::now_secs(),
            stopped: None,
        };
        let kept = save(path, &state);
        return kept
            .map(|()| state.clone())
            .map_err(|why| (why, Some(state)));
    }
    let (addr, folder, at) = (host.addr(), path.to_path_buf(), mirror.path.clone());
    let manifest = cx
        .background_executor()
        .spawn(async move { offload::send_up_blocking(&addr, &folder, &at, &sent) })
        .await
        .map_err(|why| (why, None))?;
    // Kept whatever the key's hand-over says: this is what the mirror holds.
    let handed = hand_key(cx, &conn, &mirror.path, Holder::Host, mirror.generation).await;
    let state = OffloadState {
        key: match handed {
            Ok(_) => Holder::Host,
            Err(_) => Holder::Laptop,
        },
        manifest,
        moved: onehand_core::chat::now_secs(),
        stopped: None,
    };
    let kept = save(path, &state);
    match (handed, kept) {
        (Ok(_), Ok(())) => Ok(state),
        (Err(why), _) => Err((format!("{host} did not take the code: {why}"), Some(state))),
        (Ok(_), Err(why)) => Err((why, Some(state))),
    }
}

/// Bring `path`'s code back if its host holds it and nothing works on it
/// there; said nowhere when it comes, kept as stopped when something here
/// changed meanwhile. Nothing while it is already moving, or the host is not
/// attached: a check never attaches by itself.
pub(crate) async fn check_back(cx: &mut AsyncApp, path: PathBuf) {
    let Some(Some((host, sent))) = update(cx, &path, |o| {
        let state = o.state.as_ref().filter(|s| s.key == Holder::Host)?;
        if o.moving.is_some() {
            return None;
        }
        let sent = state.manifest.clone();
        o.moving = Some(Moving::Back);
        Some((o.host.clone(), sent))
    }) else {
        return;
    };
    let came = match cx.update(|cx| hosts::live(cx, &host)) {
        Some(conn) => bring_back(cx, &conn, &path, &host, sent).await,
        None => Ok(None),
    };
    update(cx, &path, |o| {
        o.moving = None;
        match came {
            Ok(Some(state)) => {
                o.state = Some(state);
                o.failed = None;
            }
            Ok(None) => {}
            Err((why, state)) => {
                if let Some(state) = state {
                    o.state = Some(state);
                }
                o.failed = Some(why);
            }
        }
    });
}

/// The copy back: the state to keep after it, `None` when the host still
/// works on the code; or why it failed, with what to keep then.
async fn bring_back(
    cx: &mut AsyncApp,
    conn: &onehand_core::host::client::HostConn,
    path: &Path,
    host: &HostName,
    sent: Manifest,
) -> Result<Option<OffloadState>, (String, Option<OffloadState>)> {
    let fail = |why: String| (why, None);
    let mirror = mirror_of(cx, conn, path).await.map_err(fail)?;
    // Only this machine hands the key back to itself, after the copy back:
    // a host that says this machine holds it saw that hand-back land, which
    // was not kept here.
    if mirror.key == Holder::Laptop {
        let state = OffloadState {
            key: Holder::Laptop,
            manifest: sent,
            moved: onehand_core::chat::now_secs(),
            stopped: None,
        };
        save(path, &state).map_err(|why| (why, Some(state.clone())))?;
        return Ok(Some(state));
    }
    if mirror.working {
        return Ok(None);
    }
    let listed = {
        let (conn, at) = (conn.clone(), mirror.path.clone());
        cx.update(|cx| hosts::on_runtime(cx, async move { conn.mirror_files(&at).await }))
            .await
            .map_err(|e| fail(format!("Could not list the copy on {host}: {e}")))?
    };
    let (addr, folder, at) = (host.addr(), path.to_path_buf(), mirror.path.clone());
    let manifest = sent.clone();
    let back = cx
        .background_executor()
        .spawn(async move { offload::bring_back_blocking(&addr, &folder, &at, &listed, &manifest) })
        .await
        .map_err(fail)?;
    let now = onehand_core::chat::now_secs();
    let state = match back {
        // The key stays with the host until the person settles it.
        Back::Stopped(changes) => OffloadState {
            key: Holder::Host,
            manifest: sent,
            moved: now,
            stopped: Some(changes),
        },
        Back::Came(manifest) => {
            // The folder holds this code now, so it is kept whatever the
            // hand-back says; a refused one is copied again next time.
            let came = OffloadState {
                key: Holder::Host,
                manifest,
                moved: now,
                stopped: None,
            };
            save(path, &came).map_err(|why| (why, Some(came.clone())))?;
            match hand_key(cx, conn, &mirror.path, Holder::Laptop, mirror.generation).await {
                Ok(_) => OffloadState {
                    key: Holder::Laptop,
                    ..came
                },
                // A prompt went there while the code came: the host keeps it.
                Err(_) => return Ok(Some(came)),
            }
        }
    };
    save(path, &state).map_err(|why| (why, Some(state.clone())))?;
    Ok(Some(state))
}

/// Check `path` now, as a turn of a session on it ends.
pub(crate) fn check_soon(cx: &mut App, path: PathBuf) {
    if get(cx, &path).is_none() {
        return;
    }
    cx.spawn(async move |cx| check_back(cx, path).await)
        .detach();
}

/// Every [`OFFLOAD_POLL`], each folder whose host holds the code is checked,
/// while the host is attached.
async fn poll(cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(OFFLOAD_POLL).await;
        let due: Vec<PathBuf> = cx.update(|cx| {
            cx.try_global::<Offloads>().map_or_else(Vec::new, |all| {
                all.roots
                    .iter()
                    .filter(|(_, o)| {
                        o.moving.is_none()
                            && o.state.as_ref().is_some_and(|s| s.key == Holder::Host)
                            && hosts::live(cx, &o.host).is_some()
                    })
                    .map(|(path, _)| path.clone())
                    .collect()
            })
        });
        for path in due {
            check_back(cx, path).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Moving, Offload, fresh};
    use onehand_core::host::wire::Holder;
    use onehand_core::workspace::HostName;

    fn kept(key: Option<Holder>, moving: Option<Moving>) -> Offload {
        Offload {
            host: HostName("box".into()),
            state: key.map(fresh),
            moving,
            failed: None,
        }
    }

    /// Writers here are off while the host holds the code, while it goes up,
    /// and while nothing kept has been read; the code is here, settled, only
    /// when this machine holds it and nothing moves it.
    #[test]
    fn the_lock_follows_who_holds_the_code() {
        assert!(!kept(Some(Holder::Laptop), None).locked());
        assert!(kept(Some(Holder::Laptop), Some(Moving::Up)).locked());
        assert!(kept(Some(Holder::Host), None).locked());
        assert!(kept(Some(Holder::Host), Some(Moving::Back)).locked());
        assert!(kept(None, None).locked());
        assert!(kept(Some(Holder::Laptop), None).here());
        assert!(!kept(Some(Holder::Laptop), Some(Moving::Up)).here());
        assert!(!kept(Some(Holder::Host), None).here());
    }

    #[test]
    fn offloading_waits_on_a_host_and_on_turns_here() {
        use crate::shell::offload_refusal;
        assert_eq!(offload_refusal(" ", 0).as_deref(), Some("Name the host"));
        assert_eq!(offload_refusal("box", 0), None);
        assert_eq!(
            offload_refusal("box", 1).as_deref(),
            Some("Wait for the session here to finish")
        );
        assert_eq!(
            offload_refusal("box", 3).as_deref(),
            Some("Wait for 3 sessions here to finish")
        );
    }
}
