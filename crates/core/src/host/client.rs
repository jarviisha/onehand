//! The laptop's side of a host: put the service's binary there, attach to it
//! over SSH, make sure both ends run the same build, and read a host session
//! back as the same stream of events a local agent gives.
//!
//! One task owns the attach's stdin and stdout. It hands each answer to the
//! caller that asked by the ask's id, and each journal entry to whoever
//! watches that session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use futures::channel::mpsc as events;
use futures::{Stream, StreamExt as _};
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use super::wire::{build_id, Ask, Call, Entry, Line, Reply, SessionInfo, PROTOCOL};
use crate::acp::{AcpEvent, AcpRequest, Opening};
use crate::config::AgentSpec;

#[derive(Debug)]
pub enum HostError {
    /// ssh could not run, or the host refused us.
    Unreachable(String),
    /// The host is not one this build can run on (`uname -sm` said this).
    Unsupported(String),
    /// The host runs another build, with these sessions mid-turn; it was left alone.
    Busy { build: String, busy: Vec<String> },
    /// The stream broke or the host answered something unexpected.
    Protocol(String),
}

/// How to reach a host: the `ssh` destination, and `local` for running the relay directly (tests, smoke).
pub struct HostAddr {
    pub ssh: String,
    pub local: bool,
}

/// `BatchMode` so a password prompt fails at once instead of waiting on a
/// person who cannot see it; the keepalives notice a host that went away.
const SSH_OPTS: [&str; 7] = [
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
];

/// The only build shipped.
const SUPPORTED: &str = "Linux x86_64";

/// How long a service told to shut down gets to close its side.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

/// How long a failed attach gets to finish saying why on stderr.
const STDERR_WAIT: Duration = Duration::from_secs(2);

/// `cmd` run on the host, or here through `sh` when `local`; the same text
/// either way, so the two cannot drift apart in their quoting.
fn command(addr: &HostAddr, cmd: &str) -> Command {
    let mut command = if addr.local {
        let mut command = Command::new("sh");
        command.arg("-c");
        command
    } else {
        let mut command = Command::new("ssh");
        command.args(SSH_OPTS).arg(&addr.ssh);
        command
    };
    command.arg(cmd).kill_on_drop(true);
    command
}

/// Run `cmd` to its end with `input` on its stdin; what it printed.
async fn run(addr: &HostAddr, cmd: &str, input: &[u8]) -> Result<String, HostError> {
    let mut child = command(addr, cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HostError::Unreachable(format!("ssh: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A write that fails means the command already ended; its status says why.
        let _ = stdin.write_all(input).await;
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| HostError::Unreachable(e.to_string()))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(HostError::Unreachable(if why.is_empty() {
            format!("{} ended with {}", addr.ssh, out.status)
        } else {
            why
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Copy `binary` to the host unless that build is already there; return its remote path
/// (`~/.onehand-host/<build>/onehand-host`). Refuses a host whose `uname -sm` is not `Linux x86_64`
/// (the only build shipped).
pub async fn install(addr: &HostAddr, binary: &Path) -> Result<String, HostError> {
    let bytes = tokio::fs::read(binary)
        .await
        .map_err(|e| HostError::Unreachable(format!("{}: {e}", binary.display())))?;
    let dir = format!("~/.onehand-host/{}", build_id(&bytes));
    let path = format!("{dir}/onehand-host");
    let probe = run(
        addr,
        &format!("uname -sm && if test -x {path}; then echo present; fi"),
        &[],
    )
    .await?;
    let mut lines = probe.lines();
    let system = lines.next().unwrap_or("").trim();
    if system != SUPPORTED {
        return Err(HostError::Unsupported(system.to_string()));
    }
    if lines.next() == Some("present") {
        return Ok(path);
    }
    // Written beside its place and moved there whole, so a copy cut short is
    // never the one an attach runs.
    run(
        addr,
        &format!(
            "mkdir -p {dir} && cat > {path}.tmp && chmod 755 {path}.tmp && mv {path}.tmp {path}"
        ),
        &bytes,
    )
    .await?;
    Ok(path)
}

/// Attach, say hello, and settle the build: same build → connected; other build with nothing busy
/// → `Shutdown` it and attach again once; other build busy → `Busy`.
pub async fn connect(addr: &HostAddr, remote: &str, build: &str) -> Result<HostConn, HostError> {
    let (conn, settled) = attached(addr, remote, build).await?;
    match settled {
        Settled::Same => return Ok(conn),
        Settled::Restarted => {
            // The old service closes the stream as it exits; the next attach
            // must find it gone rather than half there.
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, conn.cmds.closed()).await;
        }
    }
    match attached(addr, remote, build).await? {
        (conn, Settled::Same) => Ok(conn),
        (_, Settled::Restarted) => Err(HostError::Protocol(format!(
            "{} did not come back on build {build}",
            addr.ssh
        ))),
    }
}

/// One attach, settled; a failure to reach the host says what ssh said.
async fn attached(
    addr: &HostAddr,
    remote: &str,
    build: &str,
) -> Result<(HostConn, Settled), HostError> {
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
    let said = tokio::spawn(async move {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text).await;
        String::from_utf8_lossy(&text).trim().to_string()
    });
    let conn = HostConn::start(addr.ssh.clone(), read, write, Some(child));
    match settle(&conn, build).await {
        Ok(settled) => Ok((conn, settled)),
        Err(HostError::Unreachable(why)) => {
            let said = match tokio::time::timeout(STDERR_WAIT, said).await {
                Ok(Ok(said)) if !said.is_empty() => said,
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => why,
            };
            Err(HostError::Unreachable(said))
        }
        Err(e) => Err(e),
    }
}

pub(crate) enum Settled {
    Same,
    Restarted,
}

/// Say hello as `build`; a service on another build with nothing mid-turn is
/// told to shut down, so the next attach starts this one.
async fn settle(conn: &HostConn, build: &str) -> Result<Settled, HostError> {
    let hello = Call::Hello {
        build: build.to_string(),
        protocol: PROTOCOL,
    };
    match conn.ask(hello).await? {
        Reply::Welcome {
            build: theirs,
            protocol: _,
            busy,
        } => {
            if theirs == build {
                return Ok(Settled::Same);
            }
            if !busy.is_empty() {
                return Err(HostError::Busy {
                    build: theirs,
                    busy,
                });
            }
        }
        other @ (Reply::Sessions(_) | Reply::Opened { .. } | Reply::Done) => {
            return Err(unexpected(&other))
        }
    }
    match conn.ask(Call::Shutdown).await? {
        Reply::Done => Ok(Settled::Restarted),
        other @ (Reply::Welcome { .. } | Reply::Sessions(_) | Reply::Opened { .. }) => {
            Err(unexpected(&other))
        }
    }
}

fn unexpected(reply: &Reply) -> HostError {
    HostError::Protocol(format!("unexpected answer: {reply:?}"))
}

/// What a caller asks of the task that owns the stream.
enum Cmd {
    Ask(Call, oneshot::Sender<Result<Reply, HostError>>),
    Watch(String, events::UnboundedSender<AcpEvent>),
    Request(String, AcpRequest),
}

/// Who waits on an answer.
enum Pending {
    Caller(oneshot::Sender<Result<Reply, HostError>>),
    /// A refused watch ends that watch.
    Watch(events::UnboundedSender<AcpEvent>),
    /// A refused request is said in the session it was for.
    Request(String),
}

#[derive(Clone)]
pub struct HostConn {
    cmds: mpsc::UnboundedSender<Cmd>,
}

impl HostConn {
    /// Serve a stream already open, with the attach process that holds it.
    fn start<R, W>(host: String, read: R, write: W, child: Option<Child>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (cmds, rx) = mpsc::unbounded_channel();
        tokio::spawn(serve(host, read, write, child, cmds.downgrade(), rx));
        Self { cmds }
    }

    /// A connection over a pipe, with no ssh behind it.
    #[cfg(test)]
    fn over<R, W>(read: R, write: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::start("test".into(), read, write, None)
    }

    async fn ask(&self, call: Call) -> Result<Reply, HostError> {
        let (tx, rx) = oneshot::channel();
        let lost = || HostError::Unreachable("the connection to the host is closed".into());
        self.cmds.send(Cmd::Ask(call, tx)).map_err(|_| lost())?;
        rx.await.map_err(|_| lost())?
    }

    pub async fn list(&self) -> Result<Vec<SessionInfo>, HostError> {
        match self.ask(Call::List).await? {
            Reply::Sessions(sessions) => Ok(sessions),
            other @ (Reply::Welcome { .. } | Reply::Opened { .. } | Reply::Done) => {
                Err(unexpected(&other))
            }
        }
    }

    pub async fn open(
        &self,
        cwd: PathBuf,
        mut agent: AgentSpec,
        opening: Opening,
    ) -> Result<String, HostError> {
        // The laptop's variables mean nothing on the host and may be secrets.
        agent.env.clear();
        match self
            .ask(Call::Open {
                cwd,
                agent,
                opening,
            })
            .await?
        {
            Reply::Opened { session } => Ok(session),
            other @ (Reply::Welcome { .. } | Reply::Sessions(_) | Reply::Done) => {
                Err(unexpected(&other))
            }
        }
    }

    /// The session as a local one reads: `Ready` → `Connected` (with a `tx` whose requests go to the
    /// host), `Event(e)` → `e`, `Request(r)` → `Echo(r)`. Ends with `Disconnected` when the stream
    /// closes ("lost the connection to <host>") or the host was taken ("<host> was attached from
    /// another machine").
    pub fn watch(&self, session: &str) -> impl Stream<Item = AcpEvent> + Send + 'static {
        let (tx, rx) = events::unbounded();
        if let Err(e) = self.cmds.send(Cmd::Watch(session.to_string(), tx)) {
            if let Cmd::Watch(_, tx) = e.0 {
                let _ = tx.unbounded_send(AcpEvent::Disconnected(
                    "the connection to the host is closed".into(),
                ));
            }
        }
        // The stream keeps the connection up while it is read.
        let keep = self.cmds.clone();
        rx.map(move |event| {
            let _ = &keep;
            event
        })
    }
}

/// The task that owns the stream. It ends when the stream does, or when
/// nothing is left that could ask or read anything of it.
async fn serve<R, W>(
    host: String,
    read: R,
    mut write: W,
    child: Option<Child>,
    me: mpsc::WeakUnboundedSender<Cmd>,
    mut cmds: mpsc::UnboundedReceiver<Cmd>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    // Held so the attach process lives exactly as long as this task.
    let _child = child;
    let mut lines = BufReader::new(read).lines();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut watchers: HashMap<String, events::UnboundedSender<AcpEvent>> = HashMap::new();
    // Ids start at 1: the host answers a line it cannot read with id 0.
    let mut next = 1_u64;
    let why = loop {
        tokio::select! {
            line = lines.next_line() => {
                let text = match line {
                    Ok(Some(text)) => text,
                    Ok(None) | Err(_) => break format!("lost the connection to {host}"),
                };
                // A line this build cannot read is skipped rather than fatal.
                let Ok(line) = serde_json::from_str::<Line>(&text) else { continue };
                match line {
                    Line::Kicked => break format!("{host} was attached from another machine"),
                    Line::Answer { id, result } => match (pending.remove(&id), result) {
                        (Some(Pending::Caller(tx)), result) => {
                            let _ = tx.send(result.map_err(HostError::Protocol));
                        }
                        (Some(Pending::Watch(tx)), Err(why)) => {
                            let _ = tx.unbounded_send(AcpEvent::Disconnected(why));
                            watchers.retain(|_, w| !w.same_receiver(&tx));
                        }
                        (Some(Pending::Request(session)), Err(why)) => {
                            if let Some(w) = watchers.get(&session) {
                                let _ = w.unbounded_send(AcpEvent::Error {
                                    text: why,
                                    detail: None,
                                });
                            }
                        }
                        (Some(Pending::Watch(_) | Pending::Request(_)), Ok(_)) | (None, _) => {}
                    },
                    Line::Entry { session, seq: _, entry } => {
                        let Some(w) = watchers.get(&session) else { continue };
                        let event = match entry {
                            Entry::Ready { resumed, forkable } => AcpEvent::Connected {
                                tx: forward(&me, session.clone()),
                                resumed,
                                forkable,
                            },
                            Entry::Event(event) => event,
                            Entry::Request(request) => AcpEvent::Echo(request),
                        };
                        if w.unbounded_send(event).is_err() {
                            watchers.remove(&session);
                        }
                    }
                }
            }
            cmd = cmds.recv() => {
                let Some(cmd) = cmd else { return };
                let (call, waiter) = match cmd {
                    Cmd::Ask(call, tx) => (call, Pending::Caller(tx)),
                    Cmd::Watch(session, tx) => {
                        // A second watch of a session replaces the first.
                        watchers.insert(session.clone(), tx.clone());
                        (Call::Watch { session }, Pending::Watch(tx))
                    }
                    Cmd::Request(session, request) => (
                        Call::Request { session: session.clone(), request },
                        Pending::Request(session),
                    ),
                };
                let id = next;
                next += 1;
                pending.insert(id, waiter);
                let Ok(mut text) = serde_json::to_string(&Ask { id, call }) else { continue };
                text.push('\n');
                // A write that fails means the stream is gone, which the read
                // side is about to say.
                let _ = write.write_all(text.as_bytes()).await;
            }
        }
    };
    for (_, w) in watchers {
        let _ = w.unbounded_send(AcpEvent::Disconnected(why.clone()));
    }
    for (_, waiter) in pending {
        match waiter {
            Pending::Caller(tx) => {
                let _ = tx.send(Err(HostError::Unreachable(why.clone())));
            }
            Pending::Watch(_) | Pending::Request(_) => {}
        }
    }
}

/// A request channel for `session` whose requests go to the host.
fn forward(me: &mpsc::WeakUnboundedSender<Cmd>, session: String) -> crate::acp::ReqTx {
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(cmds) = me.upgrade() {
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                if cmds.send(Cmd::Request(session.clone(), request)).is_err() {
                    return;
                }
            }
        });
    }
    tx
}

#[cfg(test)]
mod tests;
