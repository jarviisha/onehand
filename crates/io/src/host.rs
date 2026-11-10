//! The attach to a host and every call over it, on the runtime the agent
//! runtime owns: an ssh child and the task that owns its stream are tokio
//! I/O, so they run here, and what they come to crosses back on a oneshot
//! the front end awaits on its own executor.
//!
//! What follows from an attach (which ask joins which attempt, what is known
//! of a host, the waits on it) is `onehand_core::host::drive`'s.

use futures::channel::oneshot;
use onehand_core::host::HostFailure;
use onehand_core::host::client::{self, HostConn, HostError};
use onehand_core::host::wire::build_id;
use onehand_core::workspace::HostName;
use std::future::Future;
use std::path::{Path, PathBuf};
use tokio::runtime::Handle;

/// An attach under way: what it lands, and, once a connection it landed
/// closes, whether another machine took the host.
pub struct Attaching {
    pub landed: oneshot::Receiver<Result<HostConn, HostFailure>>,
    pub closed: oneshot::Receiver<bool>,
}

/// Attach to `host` on `rt`, installing the service there first when it is
/// not; with `fresh`, restart it onto this build whatever is busy there.
/// Without `take`, a host another machine holds is left to it.
pub fn attach(rt: &Handle, host: HostName, fresh: bool, take: bool) -> Attaching {
    let (tx, landed) = oneshot::channel();
    let (closed_tx, closed) = oneshot::channel();
    rt.spawn(async move {
        let result = attach_now(host, fresh, take).await;
        let conn = result.as_ref().ok().cloned();
        let _ = tx.send(result);
        // This task holds a clone of the connection for as long as it is up,
        // so its stream never ends for want of holders.
        if let Some(conn) = conn {
            conn.wait_closed().await;
            let _ = closed_tx.send(conn.taken());
        }
    });
    Attaching { landed, closed }
}

/// `work`, a call over a host's attach, run on `rt` and awaited wherever.
pub fn ask<T, F>(rt: &Handle, work: F) -> impl Future<Output = Result<T, HostError>> + use<T, F>
where
    T: Send + 'static,
    F: Future<Output = Result<T, HostError>> + Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    rt.spawn(async move {
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

async fn attach_now(host: HostName, fresh: bool, take: bool) -> Result<HostConn, HostFailure> {
    let binary = bundled_binary()?;
    let addr = host.addr();
    let failed = |e| HostFailure::of(&host, e);
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
        client::attach(&addr, &remote, &build, take).await
    };
    conn.map_err(failed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_bundled_binary_is_said_plainly() {
        let dir = std::env::temp_dir().join(format!("onehand-bundled-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let found = bundled_binary_in(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        match found {
            Err(HostFailure::Refused(text)) => assert_eq!(
                text,
                "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`"
            ),
            Ok(_)
            | Err(HostFailure::Busy { .. } | HostFailure::Other(_) | HostFailure::Taken(_)) => {
                panic!("expected the missing binary's words, refused")
            }
        }
    }
}
