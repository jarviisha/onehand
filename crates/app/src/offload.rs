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

mod branch;
pub(crate) use branch::BranchState;
pub(crate) use branch::{offload_branch_to, track_branch, update_host};

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
    /// Its host refused the poll's attach (no service it can run, say): not
    /// attached by the poll again until the person checks again.
    pub(crate) refused: bool,
    /// On a branch: no key and no lock, and `state` is never kept. `Up` is
    /// the host being updated from here, `Back` its branch being fetched.
    pub(crate) branch: Option<branch::BranchState>,
}

/// What the poll does with one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Due {
    /// Count and fetch its host's branch.
    Branch,
    /// Check its code, the host attached.
    Back,
    /// Attach to its host, then check its code.
    Attach,
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
        self.branch.is_none()
            && (self.moving == Some(Moving::Up)
                || self.state.as_ref().is_none_or(|s| s.key == Holder::Host))
    }

    /// Whether the code is this machine's, settled: what *Run here again*
    /// needs.
    pub(crate) fn here(&self) -> bool {
        self.moving.is_none()
            && (self.branch.is_some()
                || self.state.as_ref().is_some_and(|s| s.key == Holder::Laptop))
    }

    /// Whether a prompt to a session of it sends the code up first: this
    /// machine holds it, or as far as anything here knows, and nothing moves
    /// it now. Never on a branch, whose host works on what it has.
    pub(crate) fn sends_up(&self) -> bool {
        self.branch.is_none() && self.moving.is_none() && !self.locked()
    }

    /// Whether its code is on its way up before a prompt now, which holds
    /// the prompt: never on a branch, whose update sends nothing ahead of
    /// one (the host refuses a prompt while it merges).
    pub(crate) fn going_up(&self) -> bool {
        self.branch.is_none() && self.moving == Some(Moving::Up)
    }

    /// What the poll does with this folder now: `live` is whether its host
    /// is attached, `taken` whether another machine holds it. A branch is
    /// counted either way, since a merge here changes the count with no host;
    /// a mirror only when its host holds the code, attaching first when it
    /// is not attached.
    pub(crate) fn due(&self, live: bool, taken: bool) -> Option<Due> {
        if self.moving.is_some() {
            return None;
        }
        if self.branch.is_some() {
            return Some(Due::Branch);
        }
        let held = self.state.as_ref().is_some_and(|s| s.key == Holder::Host);
        match (held, live) {
            (false, _) => None,
            (true, true) => Some(Due::Back),
            // A refusal waits for the person: trying cannot change it.
            (true, false) if taken || self.refused => None,
            (true, false) => Some(Due::Attach),
        }
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

/// What a project's rail mark says while its host's branch has `ahead`
/// commits to merge.
pub(crate) fn merge_line(branch: &str, host: &HostName, ahead: usize) -> String {
    match ahead {
        1 => format!("{branch} on {host} has 1 commit to merge"),
        n => format!("{branch} on {host} has {n} commits to merge"),
    }
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
    let o = get(cx, path).filter(|o| o.going_up())?;
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
    if !get(cx, path).is_some_and(Offload::sends_up) {
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
    let read = state.is_none();
    keep(
        cx,
        path.clone(),
        Offload {
            host,
            state,
            moving: None,
            failed: None,
            refused: false,
            branch: None,
        },
    );
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
            // Taken for the host's, which keeps it locked and lets the next
            // check ask the host: a copy back held against nothing kept
            // stops on every file rather than writing over one.
            Err(why) => {
                o.state = Some(fresh(Holder::Host));
                o.failed = Some(format!("Could not read what was kept: {why}"));
            }
        });
    })
    .detach();
}

/// Keep `o` for `path`, starting the poll with the first one kept.
fn keep(cx: &mut App, path: PathBuf, o: Offload) {
    let first = cx.default_global::<Offloads>()._poll.is_none();
    let polling = first.then(|| cx.spawn(async move |cx| poll(cx).await));
    let offloads = cx.global_mut::<Offloads>();
    offloads.roots.insert(path, o);
    if polling.is_some() {
        offloads._poll = polling;
    }
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
        Err(HostFailure::Busy { host, busy, terms }) => {
            Err(crate::hosts::waiting_line(&host, busy.len(), terms))
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
    cx.update(|cx| hosts::on_runtime(cx, async move { conn.mirror(&name, None).await }))
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

/// Keep `state` for `path`, written on the background executor: the
/// manifest holds a line per file.
async fn save(cx: &mut AsyncApp, path: &Path, state: &OffloadState) -> Result<(), String> {
    let (dir, state) = (storage_for(path), state.clone());
    cx.background_executor()
        .spawn(async move { state.save(&dir) })
        .await
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
        save(cx, &path, &state).await?;
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
    save(cx, &path, &state).await?;
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
                    o.state = Some((**state).clone());
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
) -> Result<OffloadState, (String, Option<Box<OffloadState>>)> {
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
        let kept = save(cx, path, &state).await;
        return kept
            .map(|()| state.clone())
            .map_err(|why| (why, Some(Box::new(state))));
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
    let kept = save(cx, path, &state).await;
    match (handed, kept) {
        (Ok(_), Ok(())) => Ok(state),
        (Err(why), _) => Err((
            format!("{host} did not take the code: {why}"),
            Some(Box::new(state)),
        )),
        (Ok(_), Err(why)) => Err((why, Some(Box::new(state)))),
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
        Some(conn) => bring_back(cx, &conn, &path, &host, sent, None).await,
        None => Ok(None),
    };
    update(cx, &path, |o| landed(o, came));
}

/// What a copy back's end leaves: the code settled, and why it failed if it did.
type Came = Result<Option<OffloadState>, (String, Option<Box<OffloadState>>)>;

/// Keep what a copy back `came` to, the code no longer moving.
fn landed(o: &mut Offload, came: Came) {
    o.moving = None;
    match came {
        // Still stopped: nothing moved, and why the last try failed stands.
        Ok(Some(mut state)) if state.stopped.is_some() => {
            if let Some(was) = &o.state {
                state.moved = was.moved;
            }
            o.state = Some(state);
        }
        Ok(Some(state)) => {
            o.state = Some(state);
            o.failed = None;
            o.refused = false;
        }
        Ok(None) => {}
        Err((why, state)) => {
            if let Some(state) = state {
                o.state = Some(*state);
            }
            o.failed = Some(why);
        }
    }
}

/// Check `path` again because the person asked, attaching to its host if
/// it is not attached: why not, on its page, when that fails.
pub(crate) async fn check_again(cx: &mut AsyncApp, path: PathBuf) {
    let Some(host) = update(cx, &path, |o| o.host.clone()) else {
        return;
    };
    match attach(cx, &host).await {
        Ok(_) => {
            // Attached: whatever stood in the way of reaching the host is gone.
            update(cx, &path, |o| {
                o.failed = None;
                o.refused = false;
            });
            check_back(cx, path).await
        }
        Err(why) => {
            update(cx, &path, |o| o.failed = Some(why));
        }
    }
}

/// Where this machine's versions of what changed go before the host's code
/// is taken over them: a folder of their own in the project's storage, named
/// for when.
pub(crate) fn backup_dir(path: &Path) -> PathBuf {
    let when = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
    storage_for(path).join("backups").join(when)
}

/// Take the host's code over what changed here since it went, once the
/// person said so: what changed is copied into `into` first, then the code
/// comes and the key comes back as on any copy back. `Ok(false)` when the
/// host said this machine held the code already: nothing was taken, and
/// what changed here stays. `Err` why not, said on the project's page too,
/// nothing here written over.
pub(crate) async fn take_over(
    cx: &mut AsyncApp,
    path: PathBuf,
    into: PathBuf,
) -> Result<bool, String> {
    let claimed = update(cx, &path, |o| {
        let state = o.state.as_ref().filter(|s| s.key == Holder::Host)?;
        if o.moving.is_some() {
            return None;
        }
        let sent = state.manifest.clone();
        o.moving = Some(Moving::Back);
        Some((o.host.clone(), sent))
    });
    let Some(Some((host, sent))) = claimed else {
        let why = "The code was moving; check again, then take it".to_string();
        update(cx, &path, |o| o.failed = Some(why.clone()));
        return Err(why);
    };
    // Asked first, so a host that hands the code back without a copy is
    // told apart from one whose code came.
    let (came, here) = match attach(cx, &host).await {
        Ok(conn) => match mirror_of(cx, &conn, &path).await {
            Ok(mirror) => (
                bring_back(cx, &conn, &path, &host, sent, Some(&into)).await,
                mirror.key == Holder::Laptop,
            ),
            Err(why) => (Err((why, None)), false),
        },
        Err(why) => (Err((why, None)), false),
    };
    let said = match &came {
        Ok(Some(state)) if state.key == Holder::Laptop => Ok(!here),
        // Files changed here after the backup was made: kept as they are.
        Ok(Some(state)) if state.stopped.is_some() => Err(
            "Files changed here while the code came back; they stay as this machine has them"
                .to_string(),
        ),
        Ok(Some(_)) => Err(format!(
            "{host} kept the code; the next check brings it back"
        )),
        Ok(None) => Err(format!(
            "{host} is working on the code again; wait for it to finish"
        )),
        Err((why, _)) => Err(why.clone()),
    };
    // Once the backup is made, a failure after it still says where it is:
    // this machine's versions may now be only there.
    let at = into.clone();
    let backed = cx
        .background_executor()
        .spawn(async move { at.is_dir() })
        .await;
    let said = said.map_err(|why| match backed {
        true => format!("{why}; this machine's versions are in {}", into.display()),
        false => why,
    });
    // Said once, on the project's page where it was pressed.
    update(cx, &path, |o| {
        landed(o, came);
        if let Err(why) = &said {
            o.failed = Some(why.clone());
        }
    });
    said
}

/// The copy back: the state to keep after it, `None` when the host still
/// works on the code; or why it failed, with what to keep then.
///
/// With `backup`, the person said to take the host's code over what changed
/// here: what changed is copied there first, and the code comes over it.
async fn bring_back(
    cx: &mut AsyncApp,
    conn: &onehand_core::host::client::HostConn,
    path: &Path,
    host: &HostName,
    sent: Manifest,
    backup: Option<&Path>,
) -> Came {
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
        save(cx, path, &state)
            .await
            .map_err(|why| (why, Some(Box::new(state.clone()))))?;
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
    let backup = backup.map(Path::to_path_buf);
    // What the copy writes over waits here until it is known not to be an
    // edit made meanwhile.
    let keep = storage_for(path).join("coming-back");
    let back = cx
        .background_executor()
        .spawn(async move {
            match (
                offload::bring_back_blocking(&addr, &folder, &at, &listed, &manifest, &keep)?,
                backup,
            ) {
                (Back::Stopped(changes), Some(into)) => offload::take_back_blocking(
                    &addr, &folder, &at, &listed, &manifest, &changes, &into, &keep,
                ),
                (back, _) => Ok(back),
            }
        })
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
        // The host's code is here but for what changed here as it came: held
        // against that code, those stop it as any change here does.
        Back::Raced(manifest, changes) => OffloadState {
            key: Holder::Host,
            manifest,
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
            save(cx, path, &came)
                .await
                .map_err(|why| (why, Some(Box::new(came.clone()))))?;
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
    save(cx, path, &state)
        .await
        .map_err(|why| (why, Some(Box::new(state.clone()))))?;
    Ok(Some(state))
}

/// Check `path` now, as a turn of a session on it ends.
pub(crate) fn check_soon(cx: &mut App, path: PathBuf) {
    let Some(o) = get(cx, &path) else {
        return;
    };
    match o.branch.is_some() {
        true => cx.spawn(async move |cx| branch::refresh(cx, path).await),
        false => cx.spawn(async move |cx| check_back(cx, path).await),
    }
    .detach();
}

/// Every [`OFFLOAD_POLL`], each folder whose host holds the code is checked,
/// attaching to the host by itself when it is not attached, so the code
/// comes back with nothing of the project on screen; each one on a branch is
/// counted, and its branch fetched if it moved while the host is attached.
///
/// The attach made here never takes a host another machine holds, and one
/// the host refuses stands on the project's page, not tried again until the
/// person checks again: trying cannot change it.
async fn poll(cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(OFFLOAD_POLL).await;
        let due: Vec<(PathBuf, HostName, Due)> = cx.update(|cx| {
            cx.try_global::<Offloads>().map_or_else(Vec::new, |all| {
                all.roots
                    .iter()
                    .filter_map(|(path, o)| {
                        let live = hosts::live(cx, &o.host).is_some();
                        let taken = match hosts::state(cx, &o.host) {
                            hosts::HostState::Taken => true,
                            hosts::HostState::Unknown
                            | hosts::HostState::Attaching
                            | hosts::HostState::Live
                            | hosts::HostState::Lost(_) => false,
                        };
                        Some((path.clone(), o.host.clone(), o.due(live, taken)?))
                    })
                    .collect()
            })
        });
        // A host not reached once this pass is not tried again for its other
        // folders: each try could wait out ssh's connect.
        let mut unreached = std::collections::HashSet::new();
        for (path, host, due) in due {
            if due == Due::Attach && unreached.contains(&host) {
                continue;
            }
            match due {
                Due::Branch => branch::refresh(cx, path).await,
                Due::Back => check_back(cx, path).await,
                Due::Attach => match cx.update(|cx| hosts::conn_quietly(cx, &host)).await {
                    Ok(_) => check_back(cx, path).await,
                    Err(HostFailure::Refused(why)) => {
                        update(cx, &path, |o| {
                            o.failed = Some(why);
                            o.refused = true;
                        });
                    }
                    // Out of reach, busy or taken: the next poll tries again,
                    // or finds it taken and leaves it.
                    Err(
                        HostFailure::Other(_) | HostFailure::Busy { .. } | HostFailure::Taken(_),
                    ) => {
                        unreached.insert(host);
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Due, Moving, Offload, fresh};
    use onehand_core::host::wire::Holder;
    use onehand_core::workspace::HostName;

    fn kept(key: Option<Holder>, moving: Option<Moving>) -> Offload {
        Offload {
            host: HostName("box".into()),
            state: key.map(fresh),
            moving,
            failed: None,
            refused: false,
            branch: None,
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

    /// On a branch nothing here is ever locked or sent up before a prompt,
    /// whatever was kept; it runs here again whenever nothing moves it.
    #[test]
    fn a_branch_is_never_locked() {
        let on_branch = |moving| Offload {
            branch: Some(super::branch::BranchState {
                name: "onehand/box".into(),
                ahead: None,
                refused: None,
            }),
            ..kept(None, moving)
        };
        assert!(!on_branch(None).locked());
        assert!(!on_branch(Some(Moving::Up)).locked());
        assert!(on_branch(None).here());
        assert!(!on_branch(Some(Moving::Back)).here());
        assert!(!on_branch(None).sends_up());
        // Updating the host from here sends nothing before a prompt.
        assert!(!on_branch(Some(Moving::Up)).going_up());
        assert!(kept(Some(Holder::Laptop), Some(Moving::Up)).going_up());
        // Counted with or without its host: a merge here shows at once.
        assert_eq!(on_branch(None).due(false, false), Some(Due::Branch));
        assert_eq!(on_branch(Some(Moving::Back)).due(true, false), None);
        assert_eq!(
            kept(Some(Holder::Host), None).due(true, false),
            Some(Due::Back)
        );
        assert_eq!(kept(Some(Holder::Laptop), None).due(true, false), None);
    }

    /// A host holding the code is attached by the poll itself, unless another
    /// machine holds it or it refused the poll's attach.
    #[test]
    fn the_poll_attaches_for_code_its_host_holds() {
        let held = kept(Some(Holder::Host), None);
        assert_eq!(held.due(false, false), Some(Due::Attach));
        assert_eq!(held.due(false, true), None);
        let refused = Offload {
            refused: true,
            ..kept(Some(Holder::Host), None)
        };
        assert_eq!(refused.due(false, false), None);
        assert_eq!(refused.due(true, false), Some(Due::Back));
        // A failure that may pass, the network gone, say, is tried again.
        let failed = Offload {
            failed: Some("Could not ask box about its copy".into()),
            ..kept(Some(Holder::Host), None)
        };
        assert_eq!(failed.due(false, false), Some(Due::Attach));
        assert_eq!(
            kept(Some(Holder::Host), Some(Moving::Back)).due(false, false),
            None
        );
    }

    /// An update's failure stands until the next update, whatever a fetch
    /// says meanwhile; a fetch's own failure goes with the next fetch.
    #[test]
    fn an_update_failure_outlives_a_fetch() {
        use super::branch::{BranchState, landed_on};
        let mut o = Offload {
            branch: Some(BranchState {
                name: "onehand/box".into(),
                ahead: None,
                refused: None,
            }),
            ..kept(None, Some(Moving::Up))
        };
        landed_on(&mut o, Moving::Up, Err("conflict in a.rs".into()));
        o.moving = Some(Moving::Back);
        landed_on(&mut o, Moving::Back, Ok(2));
        let branch = o.branch.as_ref().unwrap();
        assert_eq!(branch.refused.as_deref(), Some("conflict in a.rs"));
        assert_eq!(branch.ahead, Some(2));
        o.moving = Some(Moving::Back);
        landed_on(&mut o, Moving::Back, Err("fetch failed".into()));
        assert_eq!(o.failed.as_deref(), Some("fetch failed"));
        o.moving = Some(Moving::Back);
        landed_on(&mut o, Moving::Back, Ok(2));
        assert_eq!(o.failed, None);
        o.moving = Some(Moving::Up);
        landed_on(&mut o, Moving::Up, Ok(0));
        assert_eq!(o.branch.as_ref().unwrap().refused, None);
        assert_eq!(o.moving, None);
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
