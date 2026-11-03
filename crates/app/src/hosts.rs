//! One attach per host, held for the whole process.
//!
//! Every project and session on a host shares the one connection to it: the
//! host service takes a single attach at a time, and a second one from this
//! machine would only kick the first. So the connection lives in
//! [`crate::state::Shared`], made on first use and made again only once its
//! stream has ended.
//!
//! The attach is tokio I/O (an ssh child and the task that owns its stream),
//! so it runs on the ACP runtime and its result crosses back on a oneshot,
//! which the caller awaits in `cx.spawn`.

use crate::state::Shared;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, LocalBoxFuture};
use gpui::App;
use onehand_core::host::client::{self, HostConn, HostError};
use onehand_core::host::wire::{Build, HostSession, build_id};
use onehand_core::workspace::HostName;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Why a host could not be attached.
#[derive(Clone, Debug)]
pub(crate) enum HostFailure {
    /// The service's binary is not where this build looks for it.
    Missing(String),
    /// The host runs another build, with these sessions mid-turn; it was left
    /// alone until the person agrees to stop them.
    Busy {
        host: HostName,
        build: Build,
        busy: Vec<HostSession>,
    },
    /// A sentence a person reads, naming the host.
    Other(String),
}

/// The process's connections, one per host.
pub(crate) struct Hosts {
    slots: Slots<HostConn>,
}

impl Hosts {
    pub(crate) fn new() -> Self {
        Self {
            slots: Slots::new(|conn: &HostConn| !conn.is_closed()),
        }
    }
}

/// The connection to `host`: the one already up, the attempt already under
/// way, or a new attach (installing the service first when it is not there).
pub(crate) fn conn(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, false)
}

/// A new connection to `host` whose service is restarted onto this build
/// whatever is mid-turn there: what follows a person agreeing to a `Busy`.
pub(crate) fn restart(
    cx: &App,
    host: &HostName,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    attach_with(cx, host, true)
}

fn attach_with(
    cx: &App,
    host: &HostName,
    fresh: bool,
) -> impl Future<Output = Result<HostConn, HostFailure>> + use<> {
    let shared = Shared::global(cx);
    shared.hosts.slots.get(host, fresh, || {
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
    })
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
        client::connect(&addr, &remote, &build).await
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
        HostError::Busy { build, busy } => HostFailure::Busy {
            host: host.clone(),
            build,
            busy,
        },
    }
}

/// The service's binary, which is built beside this one.
fn bundled_binary() -> Result<PathBuf, HostFailure> {
    let exe = std::env::current_exe().map_err(|e| HostFailure::Missing(e.to_string()))?;
    bundled_binary_in(exe.parent().unwrap_or(Path::new(".")))
}

fn bundled_binary_in(dir: &Path) -> Result<PathBuf, HostFailure> {
    let path = dir.join("onehand-host");
    if path.is_file() {
        Ok(path)
    } else {
        Err(HostFailure::Missing(
            "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`"
                .into(),
        ))
    }
}

type Attempt<C> = future::Shared<LocalBoxFuture<'static, Result<C, HostFailure>>>;

enum Slot<C> {
    Ready(C),
    /// Every ask while it runs awaits this one attempt, so two never install at once.
    Connecting(Attempt<C>),
}

/// Connections by host, generic over the connection so the reuse can be
/// tested without one.
///
/// A mutex rather than a `RefCell`: no borrow is ever held across an await,
/// and a lock cannot panic on a reentrant read the way a borrow would.
struct Slots<C> {
    map: Arc<Mutex<BTreeMap<HostName, Slot<C>>>>,
    /// Whether a ready connection still has its stream.
    alive: fn(&C) -> bool,
}

impl<C: Clone + 'static> Slots<C> {
    fn new(alive: fn(&C) -> bool) -> Self {
        Self {
            map: Arc::default(),
            alive,
        }
    }

    /// The connection to `host`, from `make` unless one is up or under way;
    /// with `fresh`, from `make` whatever is there. A failed attempt leaves
    /// nothing behind, so the next ask tries again.
    fn get(
        &self,
        host: &HostName,
        fresh: bool,
        make: impl FnOnce() -> LocalBoxFuture<'static, Result<C, HostFailure>>,
    ) -> LocalBoxFuture<'static, Result<C, HostFailure>> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let attempt = match map.get(host) {
            Some(Slot::Ready(conn)) if !fresh && (self.alive)(conn) => {
                return future::ready(Ok(conn.clone())).boxed_local();
            }
            Some(Slot::Connecting(attempt)) if !fresh => attempt.clone(),
            Some(Slot::Ready(_)) | Some(Slot::Connecting(_)) | None => {
                let attempt = make().shared();
                map.insert(host.clone(), Slot::Connecting(attempt.clone()));
                attempt
            }
        };
        drop(map);
        let map = self.map.clone();
        let host = host.clone();
        async move {
            let result = attempt.clone().await;
            let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
            // Only this attempt's own slot: a restart since has replaced it.
            let ours = match map.get(&host) {
                Some(Slot::Connecting(now)) => now.ptr_eq(&attempt),
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
}

#[cfg(test)]
mod tests;
