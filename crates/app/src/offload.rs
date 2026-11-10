//! Offloaded projects' carrier: the process's [`OffloadDrive`], held in a
//! global, its errands run off the UI loop over the attach each asks for, and
//! its answers handed back. What happens next is the drive's.
//!
//! **One entry per folder, for the whole process.** Two windows holding the
//! same project hold one copy of its code, so the drive is held once, and
//! every window reads who holds the code from it.
//!
//! Whoever asked for a send, an offload or a take awaits its answer here: the
//! drive says it as a signal, and the carrier hands it to the oldest asker
//! for that folder.

use crate::hosts;
use crate::state::Shared;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, LocalBoxFuture};
use gpui::{App, BorrowAppContext as _, Global, Task};
use onehand_core::host::offload::drive::{
    Effect, Errand, HostSeen, Input, Kept, OffloadDrive, Reach, Reached, Signal,
};
use onehand_core::workspace::HostName;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};

pub(crate) use onehand_core::host::offload::drive::{BranchState, Moving, Offload};

type Asked<T> = HashMap<PathBuf, VecDeque<oneshot::Sender<T>>>;

/// The offloaded projects of every window, and whoever awaits an answer.
#[derive(Default)]
pub(crate) struct Offloads {
    drive: OffloadDrive,
    sends: Asked<Result<(), String>>,
    offloads: Asked<Result<(PathBuf, Kept), String>>,
    takes: Asked<Result<bool, String>>,
    /// The poll's clock, while it is armed.
    _poll: Option<Task<()>>,
}

impl Global for Offloads {}

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
    cx.try_global::<Offloads>()?.drive.get(path)
}

/// Every locked folder, with its line.
pub(crate) fn lock_lines(cx: &App) -> HashMap<PathBuf, String> {
    cx.try_global::<Offloads>()
        .map(|all| {
            all.drive
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

/// Whether `path` is offloaded and this machine holds its code, as far as
/// anything here knows: a prompt to one of its sessions sends the code first.
pub(crate) fn holds_code(cx: &App, path: &Path) -> bool {
    get(cx, path).is_some_and(Offload::sends_up)
}

/// Where this machine's versions of what changed go before the host's code
/// is taken over them, named for now.
pub(crate) fn backup_dir(path: &Path) -> PathBuf {
    let when = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
    onehand_core::host::offload::drive::backup_dir(path, &when)
}

/// Ask the drive, telling every observer, and carry out what it answers.
fn drive(cx: &mut App, ask: impl FnOnce(&mut OffloadDrive) -> Vec<Effect>) {
    let fx = cx.update_global::<Offloads, _>(|all, _| ask(&mut all.drive));
    perform(cx, fx);
}

/// Ask the drive for something whose answer is awaited: `command` says
/// whether it asked for one, and `asked` files the asker's half where the
/// signal that answers it finds it, before anything is carried out.
fn ask<T: 'static>(
    cx: &mut App,
    asked: fn(&mut Offloads) -> &mut Asked<T>,
    path: &Path,
    command: impl FnOnce(&mut OffloadDrive) -> (bool, Vec<Effect>),
) -> Option<oneshot::Receiver<T>> {
    cx.default_global::<Offloads>();
    let (rx, fx) = cx.update_global::<Offloads, _>(|all, _| {
        let (waits, fx) = command(&mut all.drive);
        let rx = waits.then(|| {
            let (tx, rx) = oneshot::channel();
            asked(all)
                .entry(path.to_path_buf())
                .or_default()
                .push_back(tx);
            rx
        });
        (rx, fx)
    });
    perform(cx, fx);
    rx
}

/// What an asker hears: the answer, or why there is none.
async fn heard<T>(rx: Option<oneshot::Receiver<T>>, dropped: &'static str) -> Result<T, String> {
    match rx {
        Some(rx) => rx.await.map_err(|_| dropped.to_string()),
        None => Err(dropped.to_string()),
    }
}

/// Start keeping `path`, offloaded to `host`: with `state` when it was just
/// offloaded, else reading what was kept of it.
pub(crate) fn track(
    cx: &mut App,
    path: PathBuf,
    host: HostName,
    state: Option<onehand_core::host::offload::OffloadState>,
) {
    if kept_as_is(cx, &path, state.is_some()) {
        return;
    }
    cx.default_global::<Offloads>();
    drive(cx, |d| d.track(path, host, state));
}

/// Start keeping `path`, offloaded to `host` on `branch`.
pub(crate) fn track_branch(cx: &mut App, path: PathBuf, host: HostName, branch: String) {
    if kept_as_is(cx, &path, false) {
        return;
    }
    cx.default_global::<Offloads>();
    drive(cx, |d| d.track_branch(path, host, branch));
}

/// Whether keeping `path` again asks nothing of the drive, as it says:
/// asked first, since every ask tells the observers, and the window's own
/// observer keeps every offloaded folder again.
fn kept_as_is(cx: &App, path: &Path, anew: bool) -> bool {
    cx.try_global::<Offloads>()
        .is_some_and(|all| all.drive.kept_as_is(path, anew))
}

/// Stop keeping `path`: it runs here again, or left the workspace.
pub(crate) fn forget(cx: &mut App, path: &Path) {
    if kept_as_is(cx, path, false) {
        drive(cx, |d| {
            d.forget(path);
            Vec::new()
        });
    }
}

/// Before a prompt to a session of `path`: the code sent up and the key
/// handed over, claimed at once so a second press waits on this send.
/// `None` when the code is not this machine's to send now.
pub(crate) fn send_up(
    cx: &mut App,
    path: &Path,
) -> Option<impl Future<Output = Result<(), String>> + use<>> {
    let rx = ask(cx, |all| &mut all.sends, path, |d| d.claim_send(path))?;
    Some(heard(Some(rx), "The send was dropped").map(Result::flatten))
}

/// Offload `path` to `host`, into a mirror or `on_branch`: the mirror and
/// what to keep of it, or why not.
pub(crate) fn offload(
    cx: &mut App,
    path: PathBuf,
    host: HostName,
    on_branch: bool,
) -> impl Future<Output = Result<(PathBuf, Kept), String>> + use<> {
    let at = path.clone();
    let rx = ask(
        cx,
        |all| &mut all.offloads,
        &at,
        move |d| (true, d.offload(path, host, on_branch)),
    );
    heard(rx, "The offload was dropped").map(Result::flatten)
}

/// Take the host's code over what changed here, once the person said so,
/// backing this machine's versions up into `into` first.
pub(crate) fn take_over(
    cx: &mut App,
    path: PathBuf,
    into: PathBuf,
) -> impl Future<Output = Result<bool, String>> + use<> {
    let at = path.clone();
    let rx = ask(
        cx,
        |all| &mut all.takes,
        &at,
        move |d| (true, d.take_over(&path, into)),
    );
    heard(rx, "Taking the code was dropped").map(Result::flatten)
}

/// Check `path` again because the person asked.
pub(crate) fn check_again(cx: &mut App, path: &Path) {
    cx.default_global::<Offloads>();
    drive(cx, |d| d.check_again(path));
}

/// *Update host from laptop* on `path`, offloaded on a branch.
pub(crate) fn update_host(cx: &mut App, path: &Path) {
    cx.default_global::<Offloads>();
    drive(cx, |d| d.update_host(path));
}

/// A session on a host in `root` gave `signal`.
pub(crate) fn session_said(
    cx: &mut App,
    root: &Path,
    on_host: bool,
    signal: &onehand_core::chat::drive::Signal,
) {
    if cx.try_global::<Offloads>().is_some() {
        drive(cx, |d| d.session_said(root, on_host, signal));
    }
}

fn perform(cx: &mut App, fx: Vec<Effect>) {
    for effect in fx {
        match effect {
            Effect::Arm(after) => {
                let clock = cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(elapsed);
                });
                cx.update_global::<Offloads, _>(|all, _| all._poll = Some(clock));
            }
            Effect::Run(errand) => run(cx, errand),
            Effect::Signal(signal) => tell(cx, signal),
        }
    }
}

/// The poll's clock came round: the drive is told how each host stands.
fn elapsed(cx: &mut App) {
    let mut seen: HashMap<HostName, HostSeen> = HashMap::new();
    let hosts: Vec<HostName> = cx
        .global::<Offloads>()
        .drive
        .values()
        .map(|o| o.host.clone())
        .collect();
    for host in hosts {
        let stands = HostSeen {
            live: hosts::live(cx, &host).is_some(),
            taken: hosts::state(cx, &host) == hosts::HostState::Taken,
        };
        seen.insert(host, stands);
    }
    drive(cx, |d| d.handle(Input::Elapsed { seen }));
}

/// Run `errand` off the UI loop over the attach it asks for, then hand its
/// answer to the drive.
fn run(cx: &mut App, errand: Errand) {
    let reaching = reach(cx, &errand.host, errand.reach);
    let rt = Shared::global(cx).acp.handle();
    cx.spawn(async move |cx| {
        let conn = reaching.await;
        let input: Input = cx
            .background_executor()
            .spawn(async move { errand.run(conn, &rt) })
            .await;
        cx.update(|cx| drive(cx, |d| d.handle(input)));
    })
    .detach();
}

/// The attach `reach` names, to `host`.
fn reach(cx: &App, host: &HostName, reach: Reach) -> LocalBoxFuture<'static, Reached> {
    match reach {
        Reach::Nothing => future::ready(Ok(None)).boxed_local(),
        Reach::Live => future::ready(Ok(hosts::live(cx, host))).boxed_local(),
        Reach::Take => hosts::conn(cx, host).map(|got| got.map(Some)).boxed_local(),
        Reach::Quiet => hosts::conn_quietly(cx, host)
            .map(|got| got.map(Some))
            .boxed_local(),
    }
}

/// Hand what the drive said to the oldest asker for its folder.
fn tell(cx: &mut App, signal: Signal) {
    cx.update_global::<Offloads, _>(|all, _| {
        fn answer<T>(asked: &mut Asked<T>, path: &Path, said: T) {
            if let Some(tx) = asked.get_mut(path).and_then(VecDeque::pop_front) {
                let _ = tx.send(said);
            }
            asked.retain(|_, waiting| !waiting.is_empty());
        }
        match signal {
            Signal::Sent { path, result } => answer(&mut all.sends, &path, result),
            Signal::Offloaded { path, result, .. } => answer(&mut all.offloads, &path, result),
            Signal::TookOver { path, result } => answer(&mut all.takes, &path, result),
        }
    });
}

#[cfg(test)]
mod tests;
