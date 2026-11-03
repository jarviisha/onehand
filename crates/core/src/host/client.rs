//! The laptop's side of a host: put the service's binary there, attach to it
//! over SSH, make sure both ends run the same build, and read a host session
//! back as the same stream of events a local agent gives.
//!
//! One task owns the attach's stdin and stdout. It hands each answer to the
//! caller that asked by the ask's id, and each journal entry to whoever
//! watches that session.
//!
//! Like `acp::connect`, everything here is polled on a tokio runtime: the
//! attach is a tokio child process and its task is spawned with
//! `tokio::spawn`. The caller owns that runtime; this module starts none.

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

use super::wire::{build_id, Call, Entry, FromHost, Question, Readiness, Reply, SessionInfo};
use crate::acp::{AcpEvent, AcpRequest, Opening};
use crate::config::AgentSpec;

#[derive(Debug)]
pub enum HostError {
    /// ssh could not run, the host refused us, or the connection to it was
    /// lost; with what ssh said on stderr when an attach ended before it answered.
    Unreachable(String),
    /// The host is not one this build can run on (`uname -sm` said this).
    Unsupported(String),
    /// The host runs another build, with these sessions mid-turn; it was left alone.
    Busy { build: String, busy: Vec<String> },
    /// The host refused a call or answered something unexpected, or the
    /// binary to install could not be read here.
    Protocol(String),
}

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

/// How much of what an attach says on stderr is kept; the rest is read and dropped.
const STDERR_KEEP: u64 = 8 * 1024;

/// `cmd` run on the host, or here through `sh` when `local`; the same text
/// either way, so the two cannot drift apart in their quoting.
fn command(addr: &HostAddr, cmd: &str) -> Command {
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

/// Run `cmd` to its end with `input` on its stdin; what it printed.
async fn run(addr: &HostAddr, cmd: &str, input: &[u8]) -> Result<String, HostError> {
    let mut child = command(addr, cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HostError::Unreachable(format!("ssh: {e}")))?;
    let written = match child.stdin.take() {
        // Dropped once written, so the command reads its end.
        Some(mut stdin) => stdin.write_all(input).await,
        None => Ok(()),
    };
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
    if let Err(e) = written {
        return Err(HostError::Unreachable(format!(
            "the copy to {} was cut short: {e}",
            addr.ssh
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Copy `binary` to the host unless that build is already there; return its remote path
/// (`~/.onehand-host/<build>/onehand-host`). Refuses a host whose `uname -sm` is not `Linux x86_64`
/// (the only build shipped).
pub async fn install(addr: &HostAddr, binary: &Path) -> Result<String, HostError> {
    let bytes = tokio::fs::read(binary)
        .await
        .map_err(|e| HostError::Protocol(format!("{}: {e}", binary.display())))?;
    let build = build_id(&bytes);
    let dir = format!("~/.onehand-host/{build}");
    let path = format!("{dir}/onehand-host");
    let probe = run(
        addr,
        &format!("echo {PROBE} && uname -sm && if test -x {path}; then echo present; fi"),
        &[],
    )
    .await?;
    let (system, present) = probed(&probe);
    if system != SUPPORTED {
        return Err(HostError::Unsupported(system.to_string()));
    }
    if present {
        return Ok(path);
    }
    run(addr, &copy_command(&dir, &build), &bytes).await?;
    Ok(path)
}

/// The line the probe prints first, so whatever the host's startup files print
/// before it cannot be read as the answer.
const PROBE: &str = "__ONEHAND_PROBE__";

/// The probe's answer: `uname -sm` as the host said it, and whether that build is there.
fn probed(out: &str) -> (&str, bool) {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let Some(at) = lines.iter().rposition(|l| *l == PROBE) else {
        return ("", false);
    };
    let system = lines.get(at + 1).copied().unwrap_or("");
    (system, lines.get(at + 2) == Some(&"present"))
}

/// What writes the binary on stdin into `dir`. It is written beside its place
/// and moved there only once it names itself `build`: ssh dying mid-copy
/// leaves `cat` a short file and a clean exit, and a short file left in place
/// would pass for that build from then on. A copy that does not run at all
/// (a C library older than the one it was built against) says what the host
/// said of it instead.
fn copy_command(dir: &str, build: &str) -> String {
    let path = format!("{dir}/onehand-host");
    let tmp = format!("{path}.tmp");
    format!(
        "mkdir -p {dir} && cat > {tmp} && chmod 755 {tmp} && \
         if said=$({tmp} --build-id 2>&1); then \
           if test \"$said\" = {build}; then mv {tmp} {path}; \
           else rm -f {tmp}; echo 'the copy arrived cut short' >&2; exit 1; fi; \
         else ended=$?; rm -f {tmp}; \
           echo \"onehand-host does not run on this host (exit $ended): $said\" >&2; exit 1; fi"
    )
}

/// Attach, say hello, and settle the build: same build → connected; other build with nothing busy
/// → `Shutdown` it and attach again once; other build busy → `Busy`.
pub async fn connect(addr: &HostAddr, remote: &str, build: &str) -> Result<HostConn, HostError> {
    let (conn, said) = attach(addr, remote)?;
    match reached(settle(&conn, build).await, said).await? {
        Settled::Same => return Ok(conn),
        Settled::Restarted => {
            // The old service closes the stream as it exits; the next attach
            // must find it gone rather than half there.
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, conn.cmds.closed()).await;
        }
    }
    let (conn, said) = attach(addr, remote)?;
    reached(settle_again(&conn, build).await, said).await?;
    Ok(conn)
}

/// Start `<remote> attach`, and what it says on stderr, as far as it has once
/// it ends or has said `STDERR_KEEP` bytes.
fn attach(
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

enum Settled {
    Same,
    Restarted,
}

/// Say hello as `build`; a service on another build with nothing mid-turn is
/// told to shut down, so the next attach starts this one.
async fn settle(conn: &HostConn, build: &str) -> Result<Settled, HostError> {
    let (theirs, busy) = hello(conn, build).await?;
    if theirs == build {
        return Ok(Settled::Same);
    }
    if !busy.is_empty() {
        return Err(busy_host(theirs, busy));
    }
    conn.ask_for(Call::Shutdown, |a| a.done).await?;
    Ok(Settled::Restarted)
}

/// Say hello once more after a restart: another build now is a fault, and is
/// never shut down a second time.
async fn settle_again(conn: &HostConn, build: &str) -> Result<(), HostError> {
    let (theirs, busy) = hello(conn, build).await?;
    if theirs == build {
        Ok(())
    } else if !busy.is_empty() {
        Err(busy_host(theirs, busy))
    } else {
        Err(HostError::Protocol(format!(
            "the host came back on build {theirs}, not {build}"
        )))
    }
}

/// Say hello as `build`; the host's build, and its sessions mid-turn.
async fn hello(conn: &HostConn, build: &str) -> Result<(String, Vec<String>), HostError> {
    let hello = Call::Hello {
        build: build.to_string(),
    };
    conn.ask_for(hello, |a| a.welcome).await
}

/// A host on build `build` that was left alone for the sessions in `busy`.
fn busy_host(build: String, busy: Vec<String>) -> HostError {
    HostError::Busy { build, busy }
}

/// What an answer carries, in the slot of its kind; every other slot is empty.
/// A call takes the slot it expects, and finds any other answer unexpected.
struct Answer {
    welcome: Option<(String, Vec<String>)>,
    sessions: Option<Vec<SessionInfo>>,
    opened: Option<String>,
    done: Option<()>,
}

impl Answer {
    fn of(reply: Reply) -> Self {
        let mut answer = Answer {
            welcome: None,
            sessions: None,
            opened: None,
            done: None,
        };
        match reply {
            Reply::Welcome { build, busy } => answer.welcome = Some((build, busy)),
            Reply::Sessions(sessions) => answer.sessions = Some(sessions),
            Reply::Opened { session } => answer.opened = Some(session),
            Reply::Done => answer.done = Some(()),
        }
        answer
    }
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
    /// A refused watch ends that watch; an answered one ends its replay.
    Watch(String, events::UnboundedSender<AcpEvent>),
    /// A refused request is said in the session it was for.
    Request(String),
}

/// Who reads a session, and how far its replay has got.
struct Watcher {
    tx: events::UnboundedSender<AcpEvent>,
    /// The seq last handed on: none until the replay's first entry, seq 0.
    /// Anything else is left over from an earlier watch of the session.
    last: Option<u64>,
    /// Until the host answers the watch, what comes is the replay, whose
    /// `Ready` hands the `Chat` a closed channel: what it sends while it applies
    /// the replay answers things long settled.
    replaying: bool,
    /// The `Ready` the replay brought, given again with a live channel once it ends.
    ready: Option<Readiness>,
}

impl Watcher {
    /// Whether `seq` is the next entry this watcher should see.
    fn takes(&mut self, seq: u64) -> bool {
        let next = match self.last {
            None => seq == 0,
            Some(last) => seq > last,
        };
        if next {
            self.last = Some(seq);
        }
        next
    }
}

/// A request channel whose requests go nowhere: what a replayed `Ready` hands
/// a `Chat`, so the replay cannot make it answer the host again.
fn dead_channel() -> crate::acp::ReqTx {
    mpsc::unbounded_channel().0
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
        tokio::spawn(pump(host, read, write, child, cmds.downgrade(), rx));
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

    /// Ask `call`, and take from its answer the slot `take` names.
    async fn ask_for<T>(&self, call: Call, take: fn(Answer) -> Option<T>) -> Result<T, HostError> {
        let reply = self.ask(call).await?;
        let unexpected = HostError::Protocol(format!("unexpected answer: {reply:?}"));
        take(Answer::of(reply)).ok_or(unexpected)
    }

    pub async fn list(&self) -> Result<Vec<SessionInfo>, HostError> {
        self.ask_for(Call::List, |a| a.sessions).await
    }

    pub async fn open(
        &self,
        cwd: PathBuf,
        mut agent: AgentSpec,
        opening: Opening,
    ) -> Result<String, HostError> {
        // The laptop's variables mean nothing on the host and may be secrets.
        agent.env.clear();
        let open = Call::Open {
            cwd,
            agent,
            opening,
        };
        self.ask_for(open, |a| a.opened).await
    }

    /// The session as a local one reads: `Ready` → `Connected` (with a `tx` whose requests go to the
    /// host), `Event(e)` → `e`, `Request(r)` → `Echo(r)`. A `Ready` in the replay comes with a closed
    /// `tx`, and the live one follows the replay's last event. Ends with `Disconnected`: the
    /// session's own, or the stream closing ("lost the connection to <host>"), or the host taken
    /// ("<host> was attached from another machine").
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
async fn pump<R, W>(
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
    let mut watchers: HashMap<String, Watcher> = HashMap::new();
    // Each session's request channel, kept weakly so a replayed `Ready` reuses
    // the one still in use without keeping it alive.
    let mut forwarders: HashMap<String, mpsc::WeakUnboundedSender<AcpRequest>> = HashMap::new();
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
                let Ok(said) = serde_json::from_str::<FromHost>(&text) else { continue };
                match said {
                    FromHost::Kicked => break format!("{host} was attached from another machine"),
                    FromHost::Answer { id, result } => match (pending.remove(&id), result) {
                        (Some(Pending::Caller(tx)), result) => {
                            let _ = tx.send(result.map_err(HostError::Protocol));
                        }
                        (Some(Pending::Watch(session, tx)), result) => {
                            // A watch already replaced, or ended, has been told so.
                            let Some(w) = watchers.get_mut(&session).filter(|w| w.tx.same_receiver(&tx))
                            else { continue };
                            match result {
                                Ok(_) => {
                                    w.replaying = false;
                                    if let Some(ready) = w.ready.take() {
                                        let tx = live(&mut forwarders, &me, &session);
                                        let _ = w.tx.unbounded_send(connected(tx, ready));
                                    }
                                }
                                Err(why) => {
                                    let _ = tx.unbounded_send(AcpEvent::Disconnected(why));
                                    watchers.remove(&session);
                                }
                            }
                        }
                        (Some(Pending::Request(session)), Err(why)) => {
                            if let Some(w) = watchers.get(&session) {
                                let _ = w.tx.unbounded_send(AcpEvent::Error {
                                    text: why,
                                    detail: None,
                                });
                            }
                        }
                        (Some(Pending::Request(_)), Ok(_)) | (None, _) => {}
                    },
                    FromHost::Entry { session, seq, entry } => {
                        let Some(w) = watchers.get_mut(&session) else { continue };
                        if !w.takes(seq) {
                            continue;
                        }
                        let mut ended = false;
                        let event = match entry {
                            Entry::Ready(ready) if w.replaying => {
                                w.ready = Some(ready);
                                connected(dead_channel(), ready)
                            }
                            Entry::Ready(ready) => {
                                connected(live(&mut forwarders, &me, &session), ready)
                            }
                            Entry::Event(event) => {
                                ended = matches!(event, AcpEvent::Disconnected(_));
                                event
                            }
                            Entry::Request(request) => AcpEvent::Echo(request),
                        };
                        // A session that ended says so once, and its watch ends with it.
                        if w.tx.unbounded_send(event).is_err() || ended {
                            watchers.remove(&session);
                            forwarders.remove(&session);
                        }
                    }
                }
            }
            cmd = cmds.recv() => {
                let Some(cmd) = cmd else { return };
                let (call, waiter) = match cmd {
                    Cmd::Ask(call, tx) => (call, Pending::Caller(tx)),
                    Cmd::Watch(session, tx) => {
                        let watcher = Watcher { tx: tx.clone(), last: None, replaying: true, ready: None };
                        // A second watch of a session replaces the first.
                        if let Some(old) = watchers.insert(session.clone(), watcher) {
                            let _ = old.tx.unbounded_send(AcpEvent::Disconnected(
                                "this session is now watched elsewhere".into(),
                            ));
                        }
                        (Call::Watch { session: session.clone() }, Pending::Watch(session, tx))
                    }
                    // Only a live channel reaches here, held by someone who meant it:
                    // a replay hands its `Chat` a closed one.
                    Cmd::Request(session, request) => {
                        (Call::Request { session: session.clone(), request }, Pending::Request(session))
                    }
                };
                let id = next;
                next += 1;
                pending.insert(id, waiter);
                let Ok(mut text) = serde_json::to_string(&Question { id, call }) else { continue };
                text.push('\n');
                // A write that fails means the stream is gone, which the read
                // side is about to say.
                let _ = write.write_all(text.as_bytes()).await;
            }
        }
    };
    for (_, w) in watchers {
        let _ = w.tx.unbounded_send(AcpEvent::Disconnected(why.clone()));
    }
    for (_, waiter) in pending {
        match waiter {
            Pending::Caller(tx) => {
                let _ = tx.send(Err(HostError::Unreachable(why.clone())));
            }
            Pending::Watch(..) | Pending::Request(_) => {}
        }
    }
    refuse_queued(&mut cmds, &why);
}

/// A `Ready` as a local agent says it, with `tx` as its request channel.
fn connected(tx: crate::acp::ReqTx, ready: Readiness) -> AcpEvent {
    let Readiness { resumed, forkable } = ready;
    AcpEvent::Connected {
        tx,
        resumed,
        forkable,
    }
}

/// Answer what was queued as the stream ended: no more is read.
fn refuse_queued(cmds: &mut mpsc::UnboundedReceiver<Cmd>, why: &str) {
    cmds.close();
    while let Ok(cmd) = cmds.try_recv() {
        match cmd {
            Cmd::Ask(_, tx) => {
                let _ = tx.send(Err(HostError::Unreachable(why.to_string())));
            }
            Cmd::Watch(_, tx) => {
                let _ = tx.unbounded_send(AcpEvent::Disconnected(why.to_string()));
            }
            Cmd::Request(..) => {}
        }
    }
}

/// The session's request channel to the host: the one a `Chat` still holds, or a new one.
fn live(
    forwarders: &mut HashMap<String, mpsc::WeakUnboundedSender<AcpRequest>>,
    me: &mpsc::WeakUnboundedSender<Cmd>,
    session: &str,
) -> crate::acp::ReqTx {
    if let Some(tx) = forwarders.get(session).and_then(|w| w.upgrade()) {
        return tx;
    }
    let tx = forward(me, session.to_string());
    forwarders.insert(session.to_string(), tx.downgrade());
    tx
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
