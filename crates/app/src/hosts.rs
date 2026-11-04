//! One attach per host, held for the whole process.
//!
//! Every project and session on a host shares the one attach to it: the
//! host service takes a single attach at a time, and a second one from this
//! machine would only kick the first. So the attach lives in
//! [`crate::state::Shared`], made on first use and made again only once its
//! stream has ended.
//!
//! The attach is tokio I/O (an ssh child and the task that owns its stream),
//! so it runs on the ACP runtime and its result crosses back on a oneshot,
//! which the caller awaits in `cx.spawn`.
//!
//! A host found running another build with sessions mid-turn is waited on,
//! once per host: asked again every [`HOST_IDLE_POLL`] until an attach lands,
//! which the service's own restart once nothing is busy makes the first
//! time it can. Whatever waited on the host then carries on.
//!
//! A connection that goes without another machine taking the host is lost:
//! it is attached again by itself, with a backoff from [`waits::RECONNECT_FIRST`] to
//! [`waits::RECONNECT_MAX`], for as long as anything listens. One taken by another
//! machine is that machine's, and only a person attaches to it again.

use crate::state::Shared;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, Either, LocalBoxFuture};
use gpui::{App, AsyncApp};
use onehand_core::disk::{Disk, HostDisk};
use onehand_core::host::client::{self, HostConn, HostError};
use onehand_core::host::wire::{HostSession, build_id};
use onehand_core::tree::TreeEntry;
use onehand_core::workspace::HostName;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

mod waits;

pub(crate) use waits::HostState;
use waits::{Waits, next_delay};

/// How often a host kept waiting by its busy sessions is attached again.
pub(crate) const HOST_IDLE_POLL: Duration = Duration::from_secs(15);

/// Why a host could not be attached.
#[derive(Clone, Debug)]
pub(crate) enum HostFailure {
    /// The host runs another build, with these sessions mid-turn; it is
    /// left alone and waited on until they are done, or the person agrees to
    /// stop them.
    Busy {
        host: HostName,
        busy: Vec<HostSession>,
    },
    /// A sentence a person reads, naming the host.
    Other(String),
    /// The host is not one this build can attach to (a machine it does not
    /// run on), or this build cannot attach anywhere (its service binary
    /// missing): a sentence naming why. Never tried again by itself.
    Refused(String),
}

/// The process's attaches, one per host.
pub(crate) struct Hosts {
    slots: Slots<HostConn>,
    /// Read and written on the UI thread alone, never across an await.
    waits: Rc<RefCell<Waits>>,
}

impl Hosts {
    pub(crate) fn new() -> Self {
        Self {
            slots: Slots::new(|conn: &HostConn| !conn.is_closed()),
            waits: Rc::default(),
        }
    }
}

/// How a host's wait ended: its service on this build, or why it could not
/// be reached.
pub(crate) type WaitEnd = Result<(), String>;

type Ended = future::Shared<oneshot::Receiver<WaitEnd>>;

/// The end of the wait on `host`, its busy sessions gone idle, its service
/// restarted by a yes, or the host out of reach; `None` when nothing keeps it
/// waiting. The future listens for as long as it lives: the wait and its
/// poll, started with the first listener, end with the last.
pub(crate) fn idle(cx: &App, host: &HostName) -> Option<impl Future<Output = WaitEnd> + use<>> {
    let waits = Shared::global(cx).hosts.waits.clone();
    let ended = waits.borrow_mut().listen(host)?;
    let listening = Listening {
        waits: waits.clone(),
        host: host.clone(),
        ended: ended.clone(),
    };
    if waits.borrow_mut().poll(host) {
        let (host, ended) = (host.clone(), ended.clone());
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(HOST_IDLE_POLL).await;
                if !waits.borrow().polling(&host, &ended) {
                    return;
                }
                // What it comes to is heard by the attach itself.
                let attaching = cx.update(|cx| attach_with(cx, &host, false, true));
                let _ = attaching.await;
            }
        })
        .detach();
    }
    Some(async move {
        let _listening = listening;
        // Dropped unsent only with its last listener, and this one is here.
        ended.await.unwrap_or(Ok(()))
    })
}

/// The end of the wait for lost `host` to be attached again: `Ok` once an
/// attach lands, `Err(why)` once the host turns out taken, or running another
/// build, which its busy wait then follows. `None` unless the host is lost.
/// As [`idle`], the future listens for as long as it lives, and the loop of
/// attaches, started with the first listener, stops with the last.
pub(crate) fn back(cx: &App, host: &HostName) -> Option<impl Future<Output = WaitEnd> + use<>> {
    settle(cx, host);
    let waits = Shared::global(cx).hosts.waits.clone();
    let ended = waits.borrow_mut().listen_back(host)?;
    let listening = Listening {
        waits: waits.clone(),
        host: host.clone(),
        ended: ended.clone(),
    };
    if waits.borrow_mut().poll_back(host) {
        let (host, ended) = (host.clone(), ended.clone());
        cx.spawn(async move |cx| {
            let mut delay = None;
            loop {
                let wait = next_delay(delay);
                delay = Some(wait);
                cx.background_executor().timer(wait).await;
                if !waits.borrow().polling_back(&host, &ended) {
                    return;
                }
                // What it comes to is heard by the attach itself.
                let attaching = cx.update(|cx| attach_with(cx, &host, false, true));
                let _ = attaching.await;
            }
        })
        .detach();
    }
    Some(async move {
        let _listening = listening;
        ended.await.unwrap_or(Ok(()))
    })
}

/// What this run knows of its attach to `host`.
pub(crate) fn state(cx: &App, host: &HostName) -> HostState {
    settle(cx, host);
    Shared::global(cx).hosts.waits.borrow().state(host)
}

/// A live host whose connection has closed is lost, or taken, from now on,
/// whether or not the watch on its closing has crossed back yet.
fn settle(cx: &App, host: &HostName) {
    let hosts = &Shared::global(cx).hosts;
    let Ok(mut waits) = hosts.waits.try_borrow_mut() else {
        return;
    };
    if waits.state(host) != HostState::Live {
        return;
    }
    if let Some(conn) = hosts.slots.ready(host).filter(HostConn::is_closed) {
        waits.closed(host, conn.taken());
    }
}

/// `host`'s connection closed, `taken` by another machine or not: unless a
/// newer attach is up, the host is lost or taken from now on.
fn closed(cx: &mut App, host: &HostName, taken: bool) {
    if live(cx, host).is_some() {
        return;
    }
    let changed = Shared::global(cx)
        .hosts
        .waits
        .borrow_mut()
        .closed(host, taken);
    if changed {
        cx.refresh_windows();
    }
}

/// Whether lost `host` is being attached again by itself: something listens
/// for it to be back.
pub(crate) fn retrying(cx: &App, host: &HostName) -> bool {
    Shared::global(cx).hosts.waits.borrow().retrying(host)
}

/// What is said of `host` out of reach: that it is tried again only while it
/// is, `retrying`.
pub(crate) fn not_connected(host: &HostName, retrying: bool) -> String {
    match retrying {
        true => format!("Not connected to {host}; trying again"),
        false => format!("Not connected to {host}"),
    }
}

/// Whether `host` was last found running another build with sessions
/// mid-turn.
pub(crate) fn found_busy(cx: &App, host: &HostName) -> bool {
    Shared::global(cx).hosts.waits.borrow().found_busy(host)
}

/// One listener to a host's wait, counted while it lives.
struct Listening {
    waits: Rc<RefCell<Waits>>,
    host: HostName,
    ended: Ended,
}

impl Drop for Listening {
    fn drop(&mut self) {
        // Never borrowed across a drop; were it, the wait would outlive its
        // listeners until the host answers, which is all that is lost.
        if let Ok(mut waits) = self.waits.try_borrow_mut() {
            waits.leave(&self.host, &self.ended);
        }
    }
}

/// The sessions mid-turn on `host`, as it last said, while it is waited on.
pub(crate) fn busy(cx: &App, host: &HostName) -> Vec<HostSession> {
    Shared::global(cx).hosts.waits.borrow().busy(host).to_vec()
}

/// What is said wherever something waits on `host`'s `n` busy sessions.
pub(crate) fn waiting_line(host: &HostName, n: usize) -> String {
    let sessions = match n {
        1 => "session",
        _ => "sessions",
    };
    format!(
        "Waiting for {n} {sessions} on {host} to finish before restarting its host service \
         onto this build"
    )
}

/// The attach to `host`: the one already up, the attempt already under
/// way, or a new attach (installing the service first when it is not there).
pub(crate) fn conn(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, false, false)
}

/// The attach to `host` already up, if one is: what reads a host's files
/// in passing, as a `git status` sweep does, without attaching for it.
pub(crate) fn live(cx: &App, host: &HostName) -> Option<HostConn> {
    match Shared::global(cx).hosts.slots.reuse(host)? {
        Either::Left(conn) => Some(conn),
        Either::Right(_) => None,
    }
}

/// The files of a host reached over `conn`.
pub(crate) fn disk_over(cx: &App, conn: HostConn) -> Disk {
    Disk::Host(HostDisk::new(conn, Shared::global(cx).acp.handle()))
}

/// The attach to `host`, as [`conn`] makes it, waiting out a host found
/// busy: `waiting` is handed the line to say each time it is, and the attach
/// is made again once its sessions are done. `Err` is why it could not be
/// had, in words a person reads.
pub(crate) async fn attach_waiting(
    host: &HostName,
    cx: &mut AsyncApp,
    waiting: impl Fn(String, &mut AsyncApp),
) -> Result<HostConn, String> {
    loop {
        match cx.update(|cx| conn(cx, host)).await {
            Ok(conn) => return Ok(conn),
            Err(HostFailure::Busy { host: on, busy }) => {
                waiting(waiting_line(&on, busy.len()), cx);
                // None: an attach landed since, which the next turn takes.
                if let Some(idle) = cx.update(|cx| idle(cx, host)) {
                    idle.await?;
                }
            }
            Err(HostFailure::Other(why) | HostFailure::Refused(why)) => return Err(why),
        }
    }
}

/// `work`, a call over a host's attach, run on the ACP runtime and awaited
/// from the UI thread.
pub(crate) fn on_runtime<T, F>(
    cx: &App,
    work: F,
) -> impl Future<Output = Result<T, HostError>> + use<T, F>
where
    T: Send + 'static,
    F: Future<Output = Result<T, HostError>> + Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    Shared::global(cx).acp.spawn(async move {
        let _ = tx.send(work.await);
    });
    async move {
        rx.await.unwrap_or_else(|_| {
            Err(HostError::Unreachable(
                "the question to the host was dropped".into(),
            ))
        })
    }
}

/// Whether anything waits on `host`'s busy sessions.
pub(crate) fn waited(cx: &App, host: &HostName) -> bool {
    Shared::global(cx).hosts.waits.borrow().waited(host)
}

/// A new attach to `host` whose service is restarted onto this build
/// whatever is mid-turn there: what follows a person agreeing to stop the
/// sessions a host is waited on for.
pub(crate) fn restart(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, true, false)
}

/// Whether `path` is a folder on `host`, asked over `conn` on the ACP runtime.
pub(crate) fn is_dir(
    cx: &App,
    host: &HostName,
    conn: HostConn,
    path: PathBuf,
) -> impl Future<Output = Result<bool, HostFailure>> + use<> {
    let asking = on_runtime(cx, async move { conn.is_dir(&path).await });
    let host = host.clone();
    async move {
        asking
            .await
            .map_err(|e| HostFailure::Other(format!("Could not ask {host} about the folder: {e}")))
    }
}

/// `dir`'s listing on the host reached over `conn`, and how many entries its
/// bound left out, asked on the ACP runtime.
pub(crate) fn read_dir(
    cx: &App,
    conn: HostConn,
    dir: PathBuf,
) -> impl Future<Output = Result<(Vec<TreeEntry>, usize), String>> + use<> {
    let host = conn.host().to_string();
    let path = dir.clone();
    let asking = on_runtime(cx, async move { conn.read_dir(&path).await });
    async move {
        asking
            .await
            .map_err(|e| format!("Could not read {} on {host}: {e}", dir.display()))
    }
}

/// The home folder of the account signed in over `conn`, asked on the ACP
/// runtime.
pub(crate) fn home(
    cx: &App,
    conn: HostConn,
) -> impl Future<Output = Result<PathBuf, String>> + use<> {
    let host = conn.host().to_string();
    let asking = on_runtime(cx, async move { conn.home().await });
    async move {
        asking
            .await
            .map_err(|e| format!("Could not read the home folder on {host}: {e}"))
    }
}

/// Have the host reached over `conn` `git clone` `url` into `into`, on the
/// ACP runtime.
pub(crate) fn clone_repo(
    cx: &App,
    conn: HostConn,
    url: String,
    into: PathBuf,
) -> impl Future<Output = Result<(), String>> + use<> {
    let host = conn.host().to_string();
    let asking = on_runtime(cx, async move { conn.clone_repo(&url, &into).await });
    async move {
        asking
            .await
            .map_err(|e| format!("Could not clone on {host}: {e}"))
    }
}

/// `polled` when a wait's poll asks, whose misses count up before they end it.
fn attach_with(
    cx: &App,
    host: &HostName,
    fresh: bool,
    polled: bool,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    let shared = Shared::global(cx);
    let waits = shared.hosts.waits.clone();
    let attempt = shared.hosts.slots.get(host, fresh, || {
        // Before the first frame that could show it, so nothing to redraw.
        waits.borrow_mut().attaching(host);
        let (tx, rx) = oneshot::channel();
        // Sent once a connection that landed closes, with whether it was taken.
        let (closed_tx, closed_rx) = oneshot::channel();
        let attaching = attach(host.clone(), fresh);
        shared.acp.spawn(async move {
            let result = attaching.await;
            let conn = result.as_ref().ok().cloned();
            let _ = tx.send(result);
            // This task holds a clone of the connection for as long as it is
            // up, so the connection's stream never ends for want of holders;
            // the slot holds one for as long anyway.
            if let Some(conn) = conn {
                conn.wait_closed().await;
                let _ = closed_tx.send(conn.taken());
            }
        });
        cx.spawn({
            let host = host.clone();
            async move |cx| {
                if let Ok(taken) = closed_rx.await {
                    cx.update(|cx| closed(cx, &host, taken));
                }
            }
        })
        .detach();
        let host = host.clone();
        async move {
            rx.await.unwrap_or_else(|_| {
                Err(HostFailure::Other(format!(
                    "Could not attach to {host}: the attach was dropped"
                )))
            })
        }
        .boxed_local()
    });
    let host = host.clone();
    let cx = cx.to_async();
    async move {
        let result = attempt.await;
        if waits.borrow_mut().heard(&host, &result, polled) {
            cx.update(|cx| cx.refresh_windows());
        }
        result
    }
}

/// Install the service on `host` if it is not there, then attach to it; with
/// `fresh`, restart it onto this build whatever is busy there.
async fn attach(host: HostName, fresh: bool) -> Result<HostConn, HostFailure> {
    let binary = bundled_binary()?;
    let addr = host.addr();
    let failed = |e| failure(&host, e);
    let remote = client::install(&addr, &binary).await.map_err(failed)?;
    // ponytail: read a second time after `install` read it; a few MB on a runtime thread.
    let bytes = std::fs::read(&binary).map_err(|e| {
        HostFailure::Refused(format!(
            "Could not attach to {host}: {}: {e}",
            binary.display()
        ))
    })?;
    let build = build_id(&bytes);
    let conn = if fresh {
        client::restart(&addr, &remote, &build).await
    } else {
        client::attach(&addr, &remote, &build).await
    };
    conn.map_err(failed)
}

/// What a person reads of `e`, an attach to `host` that failed.
fn failure(host: &HostName, e: HostError) -> HostFailure {
    match e {
        e @ (HostError::Unreachable(_) | HostError::Protocol(_)) => {
            HostFailure::Other(format!("Could not attach to {host}: {e}"))
        }
        HostError::Unsupported(uname) => HostFailure::Refused(format!(
            "{host} is not a machine this build runs on: {uname}"
        )),
        HostError::Busy { busy, .. } => HostFailure::Busy {
            host: host.clone(),
            busy,
        },
    }
}

/// What is said when the service's binary is not beside this one.
const MISSING: &str =
    "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`";

/// The service's binary, which is built beside this one.
fn bundled_binary() -> Result<PathBuf, HostFailure> {
    let exe =
        std::env::current_exe().map_err(|e| HostFailure::Refused(format!("{MISSING}: {e}")))?;
    bundled_binary_in(exe.parent().unwrap_or(Path::new(".")))
}

fn bundled_binary_in(dir: &Path) -> Result<PathBuf, HostFailure> {
    let path = dir.join("onehand-host");
    if path.is_file() {
        Ok(path)
    } else {
        Err(HostFailure::Refused(MISSING.into()))
    }
}

type Attempt<C> = future::Shared<LocalBoxFuture<'static, Result<C, HostFailure>>>;

enum Slot<C> {
    Ready(C),
    /// Every ask while it runs awaits this one attempt, so two never install at once.
    Attaching(Attempt<C>),
}

/// Attaches by host, generic over the connection so the reuse can be
/// tested without one.
///
/// `Rc<RefCell>`: the attempts it holds are local futures, read only on the
/// UI thread, and no borrow is held across an await or a call to `make`.
struct Slots<C> {
    map: Rc<RefCell<BTreeMap<HostName, Slot<C>>>>,
    /// Whether a ready connection still has its stream.
    alive: fn(&C) -> bool,
}

impl<C: Clone + 'static> Slots<C> {
    fn new(alive: fn(&C) -> bool) -> Self {
        Self {
            map: Rc::default(),
            alive,
        }
    }

    /// The connection to `host`, from `make` unless one is up or under way;
    /// with `fresh`, from `make` whatever is there. A failed attempt leaves
    /// nothing behind, so the next ask tries again.
    ///
    /// `fresh` assumes no attempt is under way: one that is goes on, but what
    /// it ends in is no longer kept.
    fn get(
        &self,
        host: &HostName,
        fresh: bool,
        make: impl FnOnce() -> LocalBoxFuture<'static, Result<C, HostFailure>>,
    ) -> LocalBoxFuture<'static, Result<C, HostFailure>> {
        let found = if fresh { None } else { self.reuse(host) };
        let attempt = match found {
            Some(Either::Left(conn)) => return future::ready(Ok(conn)).boxed_local(),
            Some(Either::Right(attempt)) => attempt,
            None => {
                let attempt = make().shared();
                let slot = Slot::Attaching(attempt.clone());
                self.map.borrow_mut().insert(host.clone(), slot);
                attempt
            }
        };
        let map = self.map.clone();
        let host = host.clone();
        async move {
            let result = attempt.clone().await;
            let mut map = map.borrow_mut();
            // Only this attempt's own slot: a restart since has replaced it.
            let ours = match map.get(&host) {
                Some(Slot::Attaching(now)) => now.ptr_eq(&attempt),
                Some(Slot::Ready(_)) | None => false,
            };
            if ours {
                match &result {
                    Ok(conn) => {
                        map.insert(host, Slot::Ready(conn.clone()));
                    }
                    Err(_) => {
                        map.remove(&host);
                    }
                }
            }
            result
        }
        .boxed_local()
    }

    /// The connection `host`'s slot holds ready, whether or not it is up.
    fn ready(&self, host: &HostName) -> Option<C> {
        match self.map.borrow().get(host)? {
            Slot::Ready(conn) => Some(conn.clone()),
            Slot::Attaching(_) => None,
        }
    }

    /// What `host`'s slot has worth reusing: a live connection, or an attempt
    /// still under way.
    ///
    /// An attempt is polled once to see whether it has ended: a shared future
    /// settles only when polled, so one whose every waiter went away still
    /// looks under way after its attach is long over. Ended, it is reused only
    /// if it left a connection that is still up; a failure nobody heard is
    /// not handed to the next ask, which tries again.
    fn reuse(&self, host: &HostName) -> Option<Either<C, Attempt<C>>> {
        let mut map = self.map.borrow_mut();
        let conn = match map.get(host)? {
            Slot::Ready(conn) => conn.clone(),
            Slot::Attaching(attempt) => match attempt.clone().now_or_never() {
                None => return Some(Either::Right(attempt.clone())),
                Some(Ok(conn)) => conn,
                Some(Err(_)) => return None,
            },
        };
        if !(self.alive)(&conn) {
            return None;
        }
        map.insert(host.clone(), Slot::Ready(conn.clone()));
        Some(Either::Left(conn))
    }
}

#[cfg(test)]
mod tests;
