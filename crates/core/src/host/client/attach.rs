//! Reaching a host: start `attach` there over ssh, say hello, and settle the
//! build both ends run, shutting down a service on another build when nothing
//! is mid-turn there or the person agreed to stop what is.

use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncReadExt as _;
use tokio::process::Command;
use tokio::sync::oneshot;

use super::{Build, Call, HostConn, HostError, HostSession};

/// How to reach a host.
pub struct HostAddr {
    /// The `ssh` destination, which also names the host in what is said of it.
    pub ssh: String,
    /// Run the relay directly through `sh` rather than over ssh: how the
    /// smoke run and the tests reach a host on this machine.
    pub local: bool,
}

/// `BatchMode` so a password prompt fails at once instead of waiting on a
/// person who cannot see it; the keepalives notice a host that went away.
pub(crate) const SSH_OPTS: [&str; 7] = [
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
];

/// How long a service told to shut down gets to close its side.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

/// How long a failed attach gets to finish saying why on stderr.
const STDERR_WAIT: Duration = Duration::from_secs(2);

/// How much of what an attach says on stderr is kept; the rest is read and dropped.
const STDERR_KEEP: u64 = 8 * 1024;

/// `cmd` run on the host, or here through `sh` when `local`; the same text
/// either way, so the two cannot drift apart in their quoting.
pub(super) fn command(addr: &HostAddr, cmd: &str) -> Command {
    let mut command = if addr.local {
        let mut command = Command::new("sh");
        command.arg("-c");
        command
    } else {
        let mut command = Command::new("ssh");
        // `--` so a destination cannot be read as an option.
        command.args(SSH_OPTS).arg("--").arg(&addr.ssh);
        command
    };
    command.arg(cmd).kill_on_drop(true);
    command
}

/// Attach, say hello, and settle the build: same build → connected; other build with nothing busy
/// → `Shutdown` it and attach again once; other build busy → `Busy`. Without
/// `take`, an attach made by itself: another machine's attach holding the
/// host is left alone, as `Held`.
pub async fn attach(
    addr: &HostAddr,
    remote: &str,
    build: &Build,
    take: bool,
) -> Result<HostConn, HostError> {
    let (conn, said) = start_attach(addr, remote)?;
    match reached(settle(&conn, build, take).await, said).await? {
        Settled::Same => return Ok(conn),
        Settled::Restarted => {
            // The old service closes the stream as it exits; the next attach
            // must find it gone rather than half there.
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, conn.cmds.closed()).await;
        }
    }
    let (conn, said) = start_attach(addr, remote)?;
    reached(settle_again(&conn, build, take).await, said).await?;
    Ok(conn)
}

/// Attach and, unless the host already runs `build`, shut its service down
/// whatever is mid-turn there, then attach again: what to do once the person
/// has agreed to stop the sessions a `Busy` named.
pub async fn restart(addr: &HostAddr, remote: &str, build: &Build) -> Result<HostConn, HostError> {
    restart_over(|| start_attach(addr, remote), build).await
}

/// `restart`, attaching with `attach`.
pub(super) async fn restart_over(
    mut attach: impl FnMut() -> Result<(HostConn, oneshot::Receiver<String>), HostError>,
    build: &Build,
) -> Result<HostConn, HostError> {
    let (conn, said) = attach()?;
    let stop = async {
        let (theirs, _, _) = hello(&conn, build, true).await?;
        if theirs == *build {
            return Ok(false);
        }
        conn.ask_for(Call::Shutdown, |a| a.done).await?;
        Ok(true)
    };
    if !reached(stop.await, said).await? {
        return Ok(conn);
    }
    let _ = tokio::time::timeout(SHUTDOWN_WAIT, conn.cmds.closed()).await;
    let (conn, said) = attach()?;
    let again = async {
        let (theirs, _, _) = hello(&conn, build, true).await?;
        if theirs == *build {
            Ok(())
        } else {
            Err(HostError::Protocol(format!(
                "the host came back on build {theirs}, not {build}"
            )))
        }
    };
    reached(again.await, said).await?;
    Ok(conn)
}

/// Start `<remote> attach`, and what it says on stderr, as far as it has once
/// it ends or has said `STDERR_KEEP` bytes.
fn start_attach(
    addr: &HostAddr,
    remote: &str,
) -> Result<(HostConn, oneshot::Receiver<String>), HostError> {
    let mut child = command(addr, &format!("{remote} attach"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HostError::Unreachable(format!("ssh: {e}")))?;
    let (Some(write), Some(read), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(HostError::Unreachable("ssh gave no pipes".into()));
    };
    let (tell, said) = oneshot::channel();
    tokio::spawn(async move {
        let mut text = Vec::new();
        let _ = (&mut stderr).take(STDERR_KEEP).read_to_end(&mut text).await;
        let _ = tell.send(String::from_utf8_lossy(&text).trim().to_string());
        // Read on so a talkative attach never blocks on a full pipe.
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
    });
    Ok((
        HostConn::start(addr.ssh.clone(), read, write, Some(child)),
        said,
    ))
}

/// `result`, with a host that could not be reached explained by what ssh
/// said, when it said anything.
async fn reached<T>(
    result: Result<T, HostError>,
    said: oneshot::Receiver<String>,
) -> Result<T, HostError> {
    match result {
        Err(HostError::Unreachable(why)) => {
            let said = match tokio::time::timeout(STDERR_WAIT, said).await {
                Ok(Ok(said)) if !said.is_empty() => said,
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => why,
            };
            Err(HostError::Unreachable(said))
        }
        Ok(_) | Err(_) => result,
    }
}

pub(super) enum Settled {
    Same,
    Restarted,
}

/// Say hello as `build`; a service on another build with nothing mid-turn
/// and no terminal open is told to shut down, so the next attach starts this
/// one: a shutdown ends both, so either leaves it alone.
pub(super) async fn settle(
    conn: &HostConn,
    build: &Build,
    take: bool,
) -> Result<Settled, HostError> {
    let (theirs, busy, terms) = hello(conn, build, take).await?;
    if theirs == *build {
        return Ok(Settled::Same);
    }
    if !busy.is_empty() || terms > 0 {
        return Err(busy_host(theirs, busy, terms));
    }
    conn.ask_for(Call::Shutdown, |a| a.done).await?;
    Ok(Settled::Restarted)
}

/// Say hello once more after a restart: another build now is a fault, and is
/// never shut down a second time.
pub(super) async fn settle_again(
    conn: &HostConn,
    build: &Build,
    take: bool,
) -> Result<(), HostError> {
    let (theirs, busy, terms) = hello(conn, build, take).await?;
    if theirs == *build {
        Ok(())
    } else if !busy.is_empty() || terms > 0 {
        Err(busy_host(theirs, busy, terms))
    } else {
        Err(HostError::Protocol(format!(
            "the host came back on build {theirs}, not {build}"
        )))
    }
}

/// Say hello as `build`, taking the host or not; the host's build, its
/// sessions mid-turn and how many terminals it has open.
pub(super) async fn hello(
    conn: &HostConn,
    build: &Build,
    take: bool,
) -> Result<(Build, Vec<HostSession>, usize), HostError> {
    let hello = Call::Hello {
        build: build.clone(),
        take,
        laptop: Some(laptop().to_string()),
    };
    let welcome = |a: super::Answer| a.welcome.map(Ok).or(a.held.map(|()| Err(HostError::Held)));
    conn.ask_for(hello, welcome).await?
}

/// This laptop, as a host is told it: an id made once and kept in the config
/// directory, so a mirror the host keeps for this laptop stays its own across
/// runs. One that cannot be kept stands for this run alone.
fn laptop() -> &'static str {
    static LAPTOP: OnceLock<String> = OnceLock::new();
    LAPTOP.get_or_init(|| {
        if cfg!(test) {
            return run_id();
        }
        kept_id(&crate::config::config_dir()).unwrap_or_else(|e| {
            eprintln!("onehand: could not keep this laptop's id: {e}");
            run_id()
        })
    })
}

/// The id kept in `dir`, written there first when there is none.
pub(super) fn kept_id(dir: &std::path::Path) -> std::io::Result<String> {
    let path = dir.join("laptop-id");
    match std::fs::read_to_string(&path) {
        Ok(id) if !id.trim().is_empty() => return Ok(id.trim().to_string()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let id = run_id();
    crate::config::write_atomic(&path, &id)?;
    Ok(id)
}

/// The process and when it asked first: unique enough for one run.
fn run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

/// A host on build `build` that was left alone for the sessions in `busy`
/// and its `terms` open terminals.
fn busy_host(build: Build, busy: Vec<HostSession>, terms: usize) -> HostError {
    HostError::Busy { build, busy, terms }
}
