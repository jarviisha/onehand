//! The process's attaches, one per host, and the waits on them: the carrier
//! of [`HostDrive`], which decides which ask joins which attempt, what is
//! known of each host, and when a wait's clock attaches again.
//!
//! Every project and session on a host shares the one attach to it, held in
//! [`crate::state::Shared`]. The attach itself is tokio I/O, run on the agent
//! runtime (`onehand_io::host`); what it lands crosses back on a oneshot. One
//! shared future per attempt is what every ask that joins it awaits, and the
//! attempt's landing is told the drive once, whoever awaits it.
//!
//! A wait is listened to through the future [`idle`] or [`back`] hands out:
//! it listens for as long as it lives, and the last one dropped ends the
//! wait and its clock.

use crate::state::Shared;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, LocalBoxFuture};
use gpui::{App, AsyncApp};
use onehand_core::disk::{Disk, HostDisk};
use onehand_core::host::client::{HostConn, HostError};
use onehand_core::host::drive::{Asker, Effect, Got, HostDrive, Input, Kind};
use onehand_core::host::wire::HostSession;
use onehand_core::tree::TreeEntry;
use onehand_core::workspace::HostName;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) use onehand_core::host::drive::{HostState, WaitEnd};
pub(crate) use onehand_core::host::{HostFailure, waiting_line};

type Attempt = future::Shared<LocalBoxFuture<'static, Result<HostConn, HostFailure>>>;

type Ended = future::Shared<oneshot::Receiver<WaitEnd>>;

/// The process's attaches, one per host.
pub(crate) struct Hosts {
    /// Read and written on the UI thread alone, never across an await.
    held: Rc<RefCell<Held>>,
}

#[derive(Default)]
struct Held {
    drive: HostDrive,
    /// Each host's connection, by the attempt that made it.
    conns: BTreeMap<HostName, (u64, HostConn)>,
    /// The attempts under way, by their place in the order.
    attempts: HashMap<u64, Attempt>,
    /// Each wait's end, for its listeners.
    ends: HashMap<u64, (Option<oneshot::Sender<WaitEnd>>, Ended)>,
}

impl Hosts {
    pub(crate) fn new() -> Self {
        Self {
            held: Rc::default(),
        }
    }
}

impl Held {
    /// `host`'s connection, if the drive holds the attempt that made it as
    /// the host's, whether or not it is still up.
    fn ready(&self, host: &HostName) -> Option<&HostConn> {
        let made = self.drive.ready(host)?;
        self.conns
            .get(host)
            .filter(|(m, _)| *m == made)
            .map(|(_, conn)| conn)
    }

    /// The end of wait `id`, made with its first listener.
    fn ended(&mut self, id: u64) -> Ended {
        self.ends
            .entry(id)
            .or_insert_with(|| {
                let (tx, rx) = oneshot::channel();
                (Some(tx), rx.shared())
            })
            .1
            .clone()
    }
}

fn held(cx: &App) -> Rc<RefCell<Held>> {
    Shared::global(cx).hosts.held.clone()
}

/// Carry out what the drive asked for.
fn perform(cx: &mut App, fx: Vec<Effect>) {
    for effect in fx {
        match effect {
            Effect::Arm {
                host,
                kind,
                id,
                after,
            } => cx
                .spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| {
                        let elapsed = Input::Elapsed { host, kind, id };
                        let fx = held(cx).borrow_mut().drive.handle(elapsed);
                        perform(cx, fx);
                    });
                })
                .detach(),
            Effect::Attach { host, kind, id } => {
                // What it comes to is heard by the attach itself.
                let attaching = attach_with(cx, &host, false, true);
                cx.spawn(async move |cx| {
                    let _ = attaching.await;
                    cx.update(|cx| {
                        let polled = Input::Polled { host, kind, id };
                        let fx = held(cx).borrow_mut().drive.handle(polled);
                        perform(cx, fx);
                    });
                })
                .detach();
            }
            Effect::Ended { id, how } => {
                let tx = held(cx)
                    .borrow_mut()
                    .ends
                    .remove(&id)
                    .and_then(|(tx, _)| tx);
                if let Some(tx) = tx {
                    let _ = tx.send(how);
                }
            }
            Effect::Refresh => cx.refresh_windows(),
        }
    }
}

/// Listen to `host`'s wait of `kind`; `None` when there is nothing to wait
/// for. The future listens for as long as it lives.
fn listen(
    cx: &mut App,
    host: &HostName,
    kind: Kind,
) -> Option<impl Future<Output = WaitEnd> + use<>> {
    let held = held(cx);
    let (id, fx, ended) = {
        let mut h = held.borrow_mut();
        let (id, fx) = h.drive.listen(host, kind)?;
        (id, fx, h.ended(id))
    };
    perform(cx, fx);
    let listening = Listening {
        held,
        host: host.clone(),
        id,
    };
    Some(async move {
        let _listening = listening;
        // Dropped unsent only with its last listener, and this one is here.
        ended.await.unwrap_or(Ok(()))
    })
}

/// The end of the wait on `host`, its busy sessions gone idle, its service
/// restarted by a yes, or the host out of reach; `None` when nothing keeps it
/// waiting.
pub(crate) fn idle(cx: &mut App, host: &HostName) -> Option<impl Future<Output = WaitEnd> + use<>> {
    listen(cx, host, Kind::Busy)
}

/// The end of the wait for lost `host` to be attached again: `Ok` once an
/// attach lands, `Err(why)` once the host turns out taken, or running another
/// build, which its busy wait then follows. `None` unless the host is lost.
pub(crate) fn back(cx: &mut App, host: &HostName) -> Option<impl Future<Output = WaitEnd> + use<>> {
    settle(cx, host);
    listen(cx, host, Kind::Back)
}

/// What this run knows of its attach to `host`.
pub(crate) fn state(cx: &App, host: &HostName) -> HostState {
    settle(cx, host);
    held(cx).borrow().drive.state(host)
}

/// A live host whose connection has closed is lost, or taken, from now on,
/// whether or not the watch on its closing has crossed back yet.
fn settle(cx: &App, host: &HostName) {
    let held = held(cx);
    let Ok(mut h) = held.try_borrow_mut() else {
        return;
    };
    let closed = h.ready(host).filter(|c| c.is_closed()).map(HostConn::taken);
    h.drive.settle(host, closed);
}

/// Whether lost `host` is being attached again by itself: something listens
/// for it to be back.
pub(crate) fn retrying(cx: &App, host: &HostName) -> bool {
    held(cx).borrow().drive.retrying(host)
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
    held(cx).borrow().drive.found_busy(host)
}

/// One listener to a host's wait, counted while it lives.
struct Listening {
    held: Rc<RefCell<Held>>,
    host: HostName,
    id: u64,
}

impl Drop for Listening {
    fn drop(&mut self) {
        // Never borrowed across a drop; were it, the wait would outlive its
        // listeners until the host answers, which is all that is lost.
        if let Ok(mut h) = self.held.try_borrow_mut()
            && h.drive.leave(&self.host, self.id)
        {
            h.ends.remove(&self.id);
        }
    }
}

/// The sessions mid-turn on `host`, and how many terminals it has open, as
/// it last said, while it is waited on.
pub(crate) fn busy(cx: &App, host: &HostName) -> (Vec<HostSession>, usize) {
    let held = held(cx);
    let h = held.borrow();
    let (busy, terms) = h.drive.busy(host);
    (busy.to_vec(), terms)
}

/// An attach to `host` made by itself, for work nobody is watching: as
/// [`conn`], but a host another machine holds is left to it, and a failure
/// to reach it is a blip as a poll's is.
pub(crate) fn conn_quietly(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, false, true)
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
    let held = held(cx);
    let h = held.borrow();
    h.ready(host).filter(|c| !c.is_closed()).cloned()
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
            Err(HostFailure::Busy {
                host: on,
                busy,
                terms,
            }) => {
                waiting(waiting_line(&on, busy.len(), terms), cx);
                // None: an attach landed since, which the next turn takes.
                if let Some(idle) = cx.update(|cx| idle(cx, host)) {
                    idle.await?;
                }
            }
            Err(HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why)) => {
                return Err(why);
            }
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
    onehand_io::host::ask(&Shared::global(cx).acp.handle(), work)
}

/// Whether anything waits on `host`'s busy sessions.
pub(crate) fn waited(cx: &App, host: &HostName) -> bool {
    held(cx).borrow().drive.waited(host)
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

/// The attach to `host` as the drive answers an ask for it: `polled` when a
/// wait's poll asks, whose misses count up before they end it; such an
/// attach is made by itself, and never takes the host from another machine.
/// With `fresh`, a new one whatever is there.
fn attach_with(
    cx: &App,
    host: &HostName,
    fresh: bool,
    polled: bool,
) -> LocalBoxFuture<'static, Result<HostConn, HostFailure>> {
    let held = held(cx);
    let by = match polled {
        true => Asker::Poll,
        false => Asker::Person,
    };
    let got = {
        let mut h = held.borrow_mut();
        let alive = h.ready(host).is_some_and(|c| !c.is_closed());
        h.drive.ask(host, fresh, by, alive)
    };
    let joined = |made: u64| held.borrow().attempts.get(&made).cloned();
    match got {
        Got::Ready => {
            let conn = held.borrow().ready(host).cloned();
            future::ready(conn.ok_or_else(|| dropped(host))).boxed_local()
        }
        Got::Join(made) => match joined(made) {
            Some(attempt) => attempt.boxed_local(),
            None => future::ready(Err(dropped(host))).boxed_local(),
        },
        // The drive's `Wait`: the attempt made by itself is awaited and, as
        // the drive answered, a person's attach taking the host follows
        // should it fail.
        Got::Wait(made) => {
            let attempt = joined(made);
            let (host, cx) = (host.clone(), cx.to_async());
            async move {
                match attempt {
                    Some(attempt) => match attempt.await {
                        Ok(conn) => Ok(conn),
                        Err(_) => again(&cx, &host).await,
                    },
                    None => again(&cx, &host).await,
                }
            }
            .boxed_local()
        }
        Got::Make(made) => make(cx, held, host, made, fresh, by == Asker::Person),
    }
}

fn again(cx: &AsyncApp, host: &HostName) -> LocalBoxFuture<'static, Result<HostConn, HostFailure>> {
    cx.update(|cx| attach_with(cx, host, false, false))
}

fn dropped(host: &HostName) -> HostFailure {
    HostFailure::Other(format!(
        "Could not attach to {host}: the attach was dropped"
    ))
}

/// Make the `made`th attempt for `host`: run on the agent runtime, its
/// landing told the drive once, by whichever awaits it first, and the watch
/// on its connection's closing told it too.
fn make(
    cx: &App,
    held: Rc<RefCell<Held>>,
    host: &HostName,
    made: u64,
    fresh: bool,
    take: bool,
) -> LocalBoxFuture<'static, Result<HostConn, HostFailure>> {
    let rt = Shared::global(cx).acp.handle();
    let onehand_io::host::Attaching { landed, closed } =
        onehand_io::host::attach(&rt, host.clone(), fresh, take);
    let attempt: Attempt = {
        let (host, cx, held) = (host.clone(), cx.to_async(), held.clone());
        async move {
            let result = landed.await.unwrap_or_else(|_| Err(dropped(&host)));
            cx.update(|cx| {
                let fx = {
                    let mut h = held.borrow_mut();
                    h.attempts.remove(&made);
                    let landed = Input::Landed {
                        host: host.clone(),
                        made,
                        result: result.as_ref().map(drop).map_err(Clone::clone),
                    };
                    let fx = h.drive.handle(landed);
                    // Kept only when the drive holds this attempt as the
                    // host's now.
                    if let (Some(true), Ok(conn)) =
                        (h.drive.ready(&host).map(|m| m == made), &result)
                    {
                        h.conns.insert(host.clone(), (made, conn.clone()));
                    }
                    fx
                };
                perform(cx, fx);
            });
            result
        }
        .boxed_local()
        .shared()
    };
    held.borrow_mut().attempts.insert(made, attempt.clone());
    // Awaited here too, so an attempt whose every asker went away still
    // lands, and is heard.
    cx.spawn({
        let attempt = attempt.clone();
        async move |_| {
            let _ = attempt.await;
        }
    })
    .detach();
    cx.spawn({
        let host = host.clone();
        async move |cx| {
            if let Ok(taken) = closed.await {
                cx.update(|cx| {
                    let up = live(cx, &host).is_some();
                    let closing = Input::Closed { host, taken, up };
                    let fx = self::held(cx).borrow_mut().drive.handle(closing);
                    perform(cx, fx);
                });
            }
        }
    })
    .detach();
    attempt.boxed_local()
}

#[cfg(test)]
mod tests;
