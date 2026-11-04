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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::channel::mpsc as events;
use futures::{Stream, StreamExt as _};
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};

use super::wire::{
    Build, Call, Entry, FromHost, HostSession, HostTerm, Question, Readiness, Reply, SessionInfo,
    TermInfo,
};
use crate::acp::{AcpEvent, AcpRequest, Opening};
use crate::config::AgentSpec;
use crate::editor::SaveOutcome;
use crate::gitstat::GitStatus;
use crate::tree::TreeEntry;

mod attach;
mod files;
mod install;
mod terms;

use attach::command;
pub use attach::{attach, restart, HostAddr};
#[cfg(test)]
use attach::{hello, restart_over, settle, settle_again, Settled};
pub use install::install;
pub use terms::{Recorded, TermReader, TermWriter};

#[derive(Debug)]
pub enum HostError {
    /// ssh could not run, the host refused us, or the connection to it was
    /// lost; with what ssh said on stderr when an attach ended before it answered.
    Unreachable(String),
    /// The host is not one this build can run on (`uname -sm` said this).
    Unsupported(String),
    /// The host runs another build, with these sessions mid-turn; it was left alone.
    Busy {
        build: Build,
        busy: Vec<HostSession>,
    },
    /// The host refused a call or answered something unexpected, or the
    /// binary to install could not be read here.
    Protocol(String),
}

/// The sentence each failure reads as, after whatever names what failed: an
/// unsupported host's is what `uname -sm` said there.
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) | Self::Protocol(why) | Self::Unsupported(why) => {
                f.write_str(why)
            }
            Self::Busy { build, .. } => write!(f, "the host runs another build ({build})"),
        }
    }
}

/// `e`, from a call to `host`, with its words naming the host: what an agent
/// missing there says would otherwise read as missing here. A busy host is
/// said of where it is attached, in words of its own.
fn on_host(host: &str, e: HostError) -> HostError {
    let on = |why| format!("On {host}: {why}");
    match e {
        HostError::Unreachable(why) => HostError::Unreachable(on(why)),
        HostError::Protocol(why) => HostError::Protocol(on(why)),
        HostError::Unsupported(why) => HostError::Unsupported(on(why)),
        e @ HostError::Busy { .. } => e,
    }
}

/// How long a call waits on its answer. A watch's answer waits as long as its
/// replay runs, and has no deadline.
const ASK_WAIT: Duration = Duration::from_secs(30);

/// What an answer carries, in the slot of its kind; every other slot is empty.
/// A call takes the slot it expects, and finds any other answer unexpected.
struct Answer {
    welcome: Option<(Build, Vec<HostSession>)>,
    sessions: Option<Vec<SessionInfo>>,
    opened: Option<HostSession>,
    done: Option<()>,
    probed: Option<bool>,
    listing: Option<(Vec<TreeEntry>, usize)>,
    file: Option<(String, Option<SystemTime>)>,
    saved: Option<SaveOutcome>,
    git: Option<Option<GitStatus>>,
    home: Option<PathBuf>,
    term_opened: Option<HostTerm>,
    terms: Option<Vec<TermInfo>>,
}

impl Answer {
    fn of(reply: Reply) -> Self {
        let mut answer = Answer {
            welcome: None,
            sessions: None,
            opened: None,
            done: None,
            probed: None,
            listing: None,
            file: None,
            saved: None,
            git: None,
            home: None,
            term_opened: None,
            terms: None,
        };
        match reply {
            Reply::Welcome { build, busy } => answer.welcome = Some((build, busy)),
            Reply::Sessions(sessions) => answer.sessions = Some(sessions),
            Reply::Opened { session } => answer.opened = Some(session),
            Reply::Done => answer.done = Some(()),
            Reply::Probed { is_dir } => answer.probed = Some(is_dir),
            Reply::Listing { entries, cut } => answer.listing = Some((entries, cut)),
            Reply::File { text, mtime } => answer.file = Some((text, mtime)),
            Reply::Saved(outcome) => answer.saved = Some(outcome),
            Reply::Git(status) => answer.git = Some(status),
            Reply::Home(home) => answer.home = Some(home),
            Reply::TermOpened { term } => answer.term_opened = Some(term),
            Reply::Terms(terms) => answer.terms = Some(terms),
        }
        answer
    }
}

/// What a caller asks of the task that owns the stream.
enum Cmd {
    Ask(Call, oneshot::Sender<Result<Reply, HostError>>),
    Watch(
        HostSession,
        events::UnboundedSender<AcpEvent>,
        Option<super::copy::Copy>,
    ),
    Request(HostSession, AcpRequest),
    /// A call whose answer nobody waits on.
    Fire(Call),
    TermWatch(HostTerm, std::sync::mpsc::Sender<Vec<u8>>),
    /// Let go of every call whose caller stopped waiting.
    Forget,
}

/// Who waits on an answer.
enum Pending {
    Caller(oneshot::Sender<Result<Reply, HostError>>),
    /// A refused watch ends that watch; an answered one ends its replay.
    Watch(HostSession, events::UnboundedSender<AcpEvent>),
    /// A refused request is said in the session it was for.
    Request(HostSession),
    /// A refused terminal watch ends its reader.
    TermWatch(HostTerm),
}

/// Who reads a session, and how far its replay has got.
struct Watcher {
    tx: events::UnboundedSender<AcpEvent>,
    /// Whether this watch's `Replay` has come. Until it does, what comes for
    /// the session is left over from an earlier watch of it.
    started: bool,
    /// The seq last handed on, none before the replay's first entry, which is
    /// taken whatever its seq: the journal's first lines may be unreadable.
    last: Option<u64>,
    /// Until the host answers the watch, what comes is the replay. Its `Ready`
    /// gives the `Chat` no channel, so nothing the `Chat` does while it
    /// applies the replay reaches the host, which settled all of it long ago.
    replaying: bool,
    /// The last `Ready` the replay brought, given with a live channel once it ends.
    ready: Option<Readiness>,
    /// The time this watcher last said with `At`.
    said_at: Option<u64>,
    /// The laptop's copy of the journal, which every entry taken is added to.
    copy: Option<super::copy::Copy>,
    /// The seq the watch asked the replay to start after: the copy's last.
    after: Option<u64>,
}

impl Watcher {
    fn new(tx: events::UnboundedSender<AcpEvent>, copy: Option<super::copy::Copy>) -> Self {
        let mut copy = copy;
        // ponytail: the copy is read on the runtime's thread, holding up the
        // pump for as long as a long copy takes; move to spawn_blocking if it shows.
        if let Some(copy) = copy.as_mut().filter(|copy| copy.last().is_none()) {
            // Empty, or broken and so of no use: the host's whole journal fills it.
            let _ = copy.clear();
        }
        let after = copy.as_ref().and_then(|copy| copy.last());
        Watcher {
            tx,
            started: false,
            last: None,
            replaying: true,
            ready: None,
            said_at: None,
            copy,
            after,
        }
    }

    /// Whether `seq` is the next entry this watcher should see.
    fn takes(&mut self, seq: u64) -> bool {
        let next = self.started && self.last.is_none_or(|last| seq > last);
        if next {
            self.last = Some(seq);
        }
        next
    }

    /// Hand on the entry at `seq`, if it is the next one, and keep it in the
    /// copy; false once the watch has ended.
    fn take(
        &mut self,
        forwarders: &mut HashMap<HostSession, mpsc::WeakUnboundedSender<AcpRequest>>,
        me: &mpsc::WeakUnboundedSender<Cmd>,
        session: &HostSession,
        (seq, at, entry): (u64, u64, Entry),
    ) -> bool {
        if !self.takes(seq) {
            return true;
        }
        if let Some(copy) = &mut self.copy {
            // ponytail: a small blocking append on the runtime's thread;
            // move to spawn_blocking if a copy lives on a slow disk. One that
            // fails drops the copy, and the next watch fills it again.
            let _ = copy.append(seq, at, &entry);
        }
        let event = match entry {
            Entry::Ready(ready) if self.replaying => {
                self.ready = Some(ready);
                return true;
            }
            Entry::Ready(ready) => connected(live(forwarders, me, session), ready),
            entry @ (Entry::Event(_) | Entry::Request(_)) => {
                // Only `Ready` has no event; it is handled above.
                let Some(event) = super::copy::as_event(entry) else {
                    return true;
                };
                event
            }
        };
        let ended = matches!(event, AcpEvent::Disconnected(_));
        // 0 is a line that kept no time: the chat stamps it now.
        if at != 0 && self.said_at != Some(at) {
            self.said_at = Some(at);
            let _ = self.tx.unbounded_send(AcpEvent::At(at));
        }
        // A session that ended says so once, and its watch ends with it.
        self.tx.unbounded_send(event).is_ok() && !ended
    }
}

#[derive(Clone)]
pub struct HostConn {
    cmds: mpsc::UnboundedSender<Cmd>,
    /// The `ssh` destination, which names the host in what is said of it.
    host: String,
    /// Set when the host closed this connection because another machine
    /// attached: that machine holds the host now, and only a person should
    /// take it back.
    taken: Arc<AtomicBool>,
}

impl HostConn {
    /// Serve a stream already open, with the attach process that holds it.
    fn start<R, W>(host: String, read: R, write: W, child: Option<Child>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (cmds, rx) = mpsc::unbounded_channel();
        let taken = Arc::new(AtomicBool::new(false));
        let pumped = pump(
            host.clone(),
            read,
            write,
            child,
            cmds.downgrade(),
            rx,
            taken.clone(),
        );
        tokio::spawn(pumped);
        Self { cmds, host, taken }
    }

    /// A connection to `host` that is already gone: everything asked of it
    /// fails at once as unreachable. Building one needs no runtime.
    pub fn closed(host: &str) -> Self {
        let (cmds, _) = mpsc::unbounded_channel();
        Self {
            cmds,
            host: host.into(),
            taken: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A connection to `host` that the host closed because another machine
    /// attached. Building one needs no runtime.
    pub fn taken_from(host: &str) -> Self {
        let conn = Self::closed(host);
        conn.taken.store(true, Ordering::Relaxed);
        conn
    }

    /// A connection over a pipe, with no ssh behind it, to a host named `vps`.
    #[cfg(test)]
    fn over<R, W>(read: R, write: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::start("vps".into(), read, write, None)
    }

    /// Ask `call`, giving up on its answer after `wait`.
    async fn ask_within(&self, call: Call, wait: Duration) -> Result<Reply, HostError> {
        let (tx, rx) = oneshot::channel();
        let lost = || HostError::Unreachable("the connection to the host is closed".into());
        self.cmds.send(Cmd::Ask(call, tx)).map_err(|_| lost())?;
        match tokio::time::timeout(wait, rx).await {
            Ok(answer) => answer.map_err(|_| lost())?,
            Err(_) => {
                // Dropping `rx` closed its slot; the task lets it go.
                let _ = self.cmds.send(Cmd::Forget);
                Err(HostError::Protocol(format!(
                    "the host did not answer within {} s",
                    wait.as_secs()
                )))
            }
        }
    }

    /// Ask `call`, and take from its answer the slot `take` names.
    async fn ask_for<T>(&self, call: Call, take: fn(Answer) -> Option<T>) -> Result<T, HostError> {
        self.ask_for_within(call, ASK_WAIT, take).await
    }

    /// [`Self::ask_for`], giving up on the answer after `wait`.
    async fn ask_for_within<T>(
        &self,
        call: Call,
        wait: Duration,
        take: fn(Answer) -> Option<T>,
    ) -> Result<T, HostError> {
        let reply = self.ask_within(call, wait).await?;
        let unexpected = HostError::Protocol(format!("unexpected answer: {reply:?}"));
        take(Answer::of(reply)).ok_or(unexpected)
    }

    /// Whether the task that owns the stream has ended: nothing asked of this
    /// connection will be answered again, and a new attach is needed.
    pub fn is_closed(&self) -> bool {
        self.cmds.is_closed()
    }

    /// Resolves once the task that owns the stream has ended, as
    /// [`Self::is_closed`] then says; to be awaited on the runtime.
    pub fn wait_closed(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let cmds = self.cmds.clone();
        async move { cmds.closed().await }
    }

    /// Whether the host closed this connection because another machine
    /// attached, rather than the link going away.
    pub fn taken(&self) -> bool {
        self.taken.load(Ordering::Relaxed)
    }

    pub async fn list(&self) -> Result<Vec<SessionInfo>, HostError> {
        self.ask_for(Call::List, |a| a.sessions).await
    }

    /// Whether `path` is a folder on the host.
    pub async fn is_dir(&self, path: &Path) -> Result<bool, HostError> {
        let probe = Call::Probe {
            path: path.to_path_buf(),
        };
        self.ask_for(probe, |a| a.probed).await
    }

    /// Start a session on the host. What goes wrong names the host, as an
    /// agent missing there would otherwise read as missing here.
    pub async fn open(
        &self,
        cwd: PathBuf,
        mut agent: AgentSpec,
        opening: Opening,
    ) -> Result<HostSession, HostError> {
        // The laptop's variables mean nothing on the host and may be secrets.
        agent.env.clear();
        let open = Call::Open {
            cwd,
            agent,
            opening,
        };
        self.ask_for(open, |a| a.opened)
            .await
            .map_err(|e| on_host(&self.host, e))
    }

    /// The session as a local one reads: `Ready` → `Connected` (with a `tx` whose requests go to the
    /// host), `Event(e)` → `e`, `Request(r)` → `Echo(r)`, each after an `At` when its journal
    /// second is known and not the one last said. A `Ready` in the replay is held back, and
    /// its one `Connected` follows the replay's last event; a replay that ended the session gives
    /// none. Ends with `Disconnected`: the
    /// session's own, or the stream closing ("lost the connection to <host>"), or the host taken
    /// ("<host> was attached from another machine").
    ///
    /// With `copy`, the host replays only what comes after the copy's last
    /// entry, and the copy's entries come first, as if replayed; when the
    /// host's journal does not reach that far, the copy is dropped and the
    /// whole replay taken. Every entry taken is added to the copy.
    pub fn watch(
        &self,
        session: &HostSession,
        copy: Option<super::copy::Copy>,
    ) -> impl Stream<Item = AcpEvent> + Send + 'static {
        let (tx, rx) = events::unbounded();
        if let Err(e) = self.cmds.send(Cmd::Watch(session.clone(), tx, copy)) {
            if let Cmd::Watch(_, tx, _) = e.0 {
                own(
                    &tx,
                    AcpEvent::Disconnected("the connection to the host is closed".into()),
                );
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
    taken: Arc<AtomicBool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    // Held so the attach process lives exactly as long as this task.
    let _child = child;
    let mut lines = BufReader::new(read).lines();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut watchers: HashMap<HostSession, Watcher> = HashMap::new();
    // Each session's request channel, kept weakly so a replayed `Ready` reuses
    // the one still in use without keeping it alive.
    let mut forwarders: HashMap<HostSession, mpsc::WeakUnboundedSender<AcpRequest>> =
        HashMap::new();
    let mut terms = terms::Terms::default();
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
                    FromHost::Kicked => {
                        // Said before the channel closes, so whoever sees it
                        // closed also sees why.
                        taken.store(true, Ordering::Relaxed);
                        break format!("{host} was attached from another machine");
                    }
                    FromHost::TermOutput { term, data } => terms.output(term, data.0),
                    FromHost::TermExit { term } => terms.exited(term),
                    FromHost::Replay { session, after } => {
                        let Some(w) = watchers.get_mut(&session) else { continue };
                        // The host answers watches in order, each after its
                        // replay: while an earlier watch of the session is
                        // unanswered, this replay is that watch's.
                        let earlier = pending.values().any(|p| matches!(
                            p,
                            Pending::Watch(s, tx) if *s == session && !w.tx.same_receiver(tx)
                        ));
                        if earlier {
                            continue;
                        }
                        w.started = true;
                        // The host goes on from the copy's end only when it
                        // says so; otherwise its replay is the whole journal.
                        let held = match &mut w.copy {
                            // ponytail: the whole copy is read on the
                            // runtime's thread, holding up the pump; move to
                            // spawn_blocking if a long copy shows.
                            Some(copy) if after.is_some() && after == w.after => copy.read(),
                            Some(copy) => {
                                let _ = copy.clear();
                                Vec::new()
                            }
                            None => Vec::new(),
                        };
                        for entry in held {
                            if !w.take(&mut forwarders, &me, &session, entry) {
                                watchers.remove(&session);
                                forwarders.remove(&session);
                                break;
                            }
                        }
                    }
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
                                    own(&tx, AcpEvent::Disconnected(why));
                                    watchers.remove(&session);
                                    forwarders.remove(&session);
                                }
                            }
                        }
                        (Some(Pending::Request(session)), Err(why)) => {
                            if let Some(w) = watchers.get_mut(&session) {
                                own(&w.tx, AcpEvent::Error { text: why, detail: None });
                                // The next entry says its own time again.
                                w.said_at = None;
                            }
                        }
                        (Some(Pending::TermWatch(term)), Err(_)) => terms.refused(id, term),
                        (Some(Pending::Request(_) | Pending::TermWatch(_)), Ok(_)) | (None, _) => {}
                    },
                    FromHost::Entry { session, seq, at, entry } => {
                        let Some(w) = watchers.get_mut(&session) else { continue };
                        if !w.take(&mut forwarders, &me, &session, (seq, at, entry)) {
                            watchers.remove(&session);
                            forwarders.remove(&session);
                        }
                    }
                }
            }
            cmd = cmds.recv() => {
                let Some(cmd) = cmd else { return };
                let id = next;
                let (call, waiter) = match cmd {
                    Cmd::Forget => {
                        pending.retain(|_, p| match p {
                            Pending::Caller(tx) => !tx.is_closed(),
                            Pending::Watch(..) | Pending::Request(_) | Pending::TermWatch(_) => true,
                        });
                        continue;
                    }
                    Cmd::Ask(call, tx) => (call, Some(Pending::Caller(tx))),
                    Cmd::Watch(session, tx, copy) => {
                        let watcher = Watcher::new(tx.clone(), copy);
                        let after = watcher.after;
                        // A second watch of a session replaces the first.
                        if let Some(old) = watchers.insert(session.clone(), watcher) {
                            let elsewhere = "this session is now watched elsewhere";
                            own(&old.tx, AcpEvent::Disconnected(elsewhere.into()));
                        }
                        (Call::Watch { session: session.clone(), after }, Some(Pending::Watch(session, tx)))
                    }
                    // Only a live channel reaches here: a replay hands its `Chat` none.
                    Cmd::Request(session, request) => {
                        (Call::Request { session: session.clone(), request }, Some(Pending::Request(session)))
                    }
                    Cmd::Fire(call) => (call, None),
                    Cmd::TermWatch(term, tx) => {
                        terms.watch(id, term, tx);
                        (Call::TermWatch { term }, Some(Pending::TermWatch(term)))
                    }
                };
                next += 1;
                if let Some(waiter) = waiter {
                    pending.insert(id, waiter);
                }
                let Ok(mut text) = serde_json::to_string(&Question { id, call }) else { continue };
                text.push('\n');
                // A write that fails means the stream is gone, which the read
                // side is about to say.
                let _ = write.write_all(text.as_bytes()).await;
            }
        }
    };
    // Closed before anyone is told, so whoever reads the end of a watch
    // finds the connection closed, and why from `taken`.
    cmds.close();
    for (_, w) in watchers {
        own(&w.tx, AcpEvent::Disconnected(why.clone()));
    }
    for (_, waiter) in pending {
        match waiter {
            Pending::Caller(tx) => {
                let _ = tx.send(Err(HostError::Unreachable(why.clone())));
            }
            Pending::Watch(..) | Pending::Request(_) | Pending::TermWatch(_) => {}
        }
    }
    refuse_queued(&mut cmds, &why);
}

/// Send `event`, one the client makes itself rather than reads from the
/// journal, after an `At` of this second: the last time a replay said is not
/// when this happened.
fn own(tx: &events::UnboundedSender<AcpEvent>, event: AcpEvent) {
    let _ = tx.unbounded_send(AcpEvent::At(crate::chat::store::now_secs()));
    let _ = tx.unbounded_send(event);
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
            Cmd::Watch(_, tx, _) => {
                own(&tx, AcpEvent::Disconnected(why.to_string()));
            }
            // A terminal watch's sender drops here, and its reader ends.
            Cmd::Request(..) | Cmd::Fire(_) | Cmd::TermWatch(..) | Cmd::Forget => {}
        }
    }
}

/// The session's request channel to the host: the one a `Chat` still holds, or a new one.
fn live(
    forwarders: &mut HashMap<HostSession, mpsc::WeakUnboundedSender<AcpRequest>>,
    me: &mpsc::WeakUnboundedSender<Cmd>,
    session: &HostSession,
) -> crate::acp::ReqTx {
    if let Some(tx) = forwarders.get(session).and_then(|w| w.upgrade()) {
        return tx;
    }
    let tx = forward(me, session.clone());
    forwarders.insert(session.clone(), tx.downgrade());
    tx
}

/// A request channel for `session` whose requests go to the host.
fn forward(me: &mpsc::WeakUnboundedSender<Cmd>, session: HostSession) -> crate::acp::ReqTx {
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
