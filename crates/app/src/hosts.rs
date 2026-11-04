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

use crate::state::Shared;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, Either, LocalBoxFuture};
use gpui::App;
use onehand_core::host::client::{self, HostConn, HostError};
use onehand_core::host::wire::{HostSession, build_id};
use onehand_core::workspace::HostName;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

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

/// The end of the wait on `host`, its busy sessions gone idle or its service
/// restarted by a yes; `None` when nothing keeps it waiting. Starts the
/// wait's poll, once.
pub(crate) fn idle(cx: &App, host: &HostName) -> Option<impl Future<Output = ()> + use<>> {
    let waits = Shared::global(cx).hosts.waits.clone();
    let ended = waits.borrow().ended(host)?;
    if waits.borrow_mut().poll(host) {
        let (host, ended) = (host.clone(), ended.clone());
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(HOST_IDLE_POLL).await;
                if ended.clone().now_or_never().is_some() {
                    return;
                }
                // What it comes to is heard by the attach itself.
                let attaching = cx.update(|cx| conn(cx, &host));
                let _ = attaching.await;
            }
        })
        .detach();
    }
    Some(ended.map(drop))
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

/// Hosts kept waiting by their busy sessions, one wait each.
#[derive(Default)]
struct Waits(BTreeMap<HostName, Wait>);

struct Wait {
    busy: Vec<HostSession>,
    /// Dropped with the wait, which is what ends `ended` for every waiter.
    _end: oneshot::Sender<()>,
    ended: future::Shared<oneshot::Receiver<()>>,
    polling: bool,
}

impl Waits {
    /// What an attach to `host` came to: a busy host waits, with the
    /// sessions it said; one that lands ends the wait. Whether a wait began.
    fn heard<C>(&mut self, host: &HostName, result: &Result<C, HostFailure>) -> bool {
        match result {
            Ok(_) => {
                self.0.remove(host);
                false
            }
            Err(HostFailure::Busy { busy, .. }) => match self.0.get_mut(host) {
                Some(wait) => {
                    wait.busy = busy.clone();
                    false
                }
                None => {
                    let (end, ended) = oneshot::channel();
                    let wait = Wait {
                        busy: busy.clone(),
                        _end: end,
                        ended: ended.shared(),
                        polling: false,
                    };
                    self.0.insert(host.clone(), wait);
                    true
                }
            },
            // Unreachable for now: still waited on.
            Err(HostFailure::Other(_)) => false,
        }
    }

    fn ended(&self, host: &HostName) -> Option<future::Shared<oneshot::Receiver<()>>> {
        self.0.get(host).map(|wait| wait.ended.clone())
    }

    fn busy(&self, host: &HostName) -> &[HostSession] {
        self.0.get(host).map_or(&[], |wait| &wait.busy)
    }

    /// Whether `host`'s wait has yet to be polled; it is from now on.
    fn poll(&mut self, host: &HostName) -> bool {
        self.0
            .get_mut(host)
            .is_some_and(|wait| !std::mem::replace(&mut wait.polling, true))
    }
}

/// The attach to `host`: the one already up, the attempt already under
/// way, or a new attach (installing the service first when it is not there).
pub(crate) fn conn(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, false)
}

/// A new attach to `host` whose service is restarted onto this build
/// whatever is mid-turn there: what follows a person agreeing to stop the
/// sessions a host is waited on for.
pub(crate) fn restart(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, true)
}

/// Whether `path` is a folder on `host`, asked over `conn` on the ACP runtime.
pub(crate) fn is_dir(
    cx: &App,
    host: &HostName,
    conn: HostConn,
    path: PathBuf,
) -> impl Future<Output = Result<bool, HostFailure>> + use<> {
    let (tx, rx) = oneshot::channel();
    Shared::global(cx).acp.spawn(async move {
        let _ = tx.send(conn.is_dir(&path).await);
    });
    let host = host.clone();
    async move {
        match rx.await {
            Ok(found) => found.map_err(|e| {
                HostFailure::Other(format!("Could not ask {host} about the folder: {e}"))
            }),
            Err(_) => Err(HostFailure::Other(format!(
                "Could not ask {host} about the folder: the question was dropped"
            ))),
        }
    }
}

fn attach_with(
    cx: &App,
    host: &HostName,
    fresh: bool,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    let shared = Shared::global(cx);
    let waits = shared.hosts.waits.clone();
    let attempt = shared.hosts.slots.get(host, fresh, || {
        let (tx, rx) = oneshot::channel();
        let attaching = attach(host.clone(), fresh);
        shared.acp.spawn(async move {
            let _ = tx.send(attaching.await);
        });
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
    async move {
        let result = attempt.await;
        waits.borrow_mut().heard(&host, &result);
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
        HostFailure::Other(format!(
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
        HostError::Unreachable(why) | HostError::Protocol(why) => {
            HostFailure::Other(format!("Could not attach to {host}: {why}"))
        }
        HostError::Unsupported(uname) => HostFailure::Other(format!(
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
    let exe = std::env::current_exe().map_err(|e| HostFailure::Other(format!("{MISSING}: {e}")))?;
    bundled_binary_in(exe.parent().unwrap_or(Path::new(".")))
}

fn bundled_binary_in(dir: &Path) -> Result<PathBuf, HostFailure> {
    let path = dir.join("onehand-host");
    if path.is_file() {
        Ok(path)
    } else {
        Err(HostFailure::Other(MISSING.into()))
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
