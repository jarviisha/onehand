//! The host's sessions: each one's agent, driven on a task of its own, and
//! its journal, which every agent event and every request goes into before
//! anything else sees it. A laptop that watches a session gets the journal
//! from the start, then what comes after, so a laptop that went away mid-turn
//! loses nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use futures::stream::BoxStream;
use futures::StreamExt as _;
use onehand_core::acp::{
    AcpEvent, AcpRequest, Elicitation, Opening, PermissionRequest, ReqTx, SessionOpts,
};
use onehand_core::config::{AgentSpec, Launch};
use onehand_core::host::journal::{self, Journal, Mark};
use onehand_core::host::wire::{
    aside_after, busy_after, Entry, FromHost, HostSession, Readiness, SessionInfo,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::serve::{log, unix_secs};

mod branch;
pub(crate) use branch::update_blocking;
mod mirrors;

/// How a session's agent is reached: `acp::connect` in production, a fake in tests.
pub(crate) type Connector =
    Arc<dyn Fn(Launch, SessionOpts) -> BoxStream<'static, AcpEvent> + Send + Sync>;

const ATTACHMENTS_DROPPED: &str =
    "Attachments are not sent to a host yet; the prompt went without them";

/// What a session's stream ending says when the agent did not say why.
const CLOSED: &str = "The agent's connection closed";

/// What ends a session the laptop ended.
const ENDED_HERE: &str = "the session was ended from the laptop";

/// What ends a session a service before this one left running.
const STOPPED: &str = "The host service stopped while this session was running";

/// `dir/sessions/<id>/session.json`: what a service started again needs to list the session.
const RECORD: &str = "session.json";

/// `dir/bypass.json`: the folders whose cards wait for a person. Every other
/// folder's are answered by the host, so a folder never seen is answered.
const BYPASS: &str = "bypass.json";

#[derive(Default, Serialize, Deserialize)]
struct Bypass {
    off: BTreeSet<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    cwd: PathBuf,
    agent: String,
}

struct Session {
    dir: PathBuf,
    /// Open until the session ends, and none after: a host keeps every session
    /// it ever ran, and a handle each would run it out of files.
    journal: Option<Journal>,
    cwd: PathBuf,
    agent: String,
    /// The agent's request channel, once it is up and until it ends.
    tx: Option<ReqTx>,
    /// The laptop watching this session, if one is.
    watcher: Option<UnboundedSender<FromHost>>,
    /// Cards journaled that nobody has answered yet.
    parked: Vec<Parked>,
    /// The rpc ids of the cards the host answered itself.
    answered: Vec<serde_json::Value>,
    busy: bool,
    /// The background work and subagents it has running, by id.
    aside: BTreeSet<String>,
    ended: bool,
    /// The task driving its agent, which owns the agent's stream: aborting it
    /// drops the stream, and with it the agent's process.
    drive: Option<tokio::task::AbortHandle>,
}

pub(crate) struct Hub {
    dir: PathBuf,
    /// `PATH` as the user's login shell has it, which agents are found and run with.
    path: Option<OsString>,
    connector: Connector,
    sessions: BTreeMap<HostSession, Session>,
    counter: u64,
    /// The folders whose cards wait for a person, or why they could not be
    /// read, in which case every folder's cards wait: a choice kept on disk
    /// and lost is not taken for a folder never switched.
    off: Result<BTreeSet<PathBuf>, String>,
    mirrors: mirrors::Mirrors,
    /// The branch mirrors being updated from the laptop now.
    updating: BTreeSet<PathBuf>,
    /// The laptop the attach being served named, which its mirrors are kept for.
    pub(crate) laptop: Option<String>,
}

impl Hub {
    pub(crate) fn new(dir: PathBuf, path: Option<OsString>, connector: Connector) -> Hub {
        Hub {
            dir,
            path,
            connector,
            sessions: BTreeMap::new(),
            counter: 0,
            off: Ok(BTreeSet::new()),
            mirrors: Ok(BTreeMap::new()),
            updating: BTreeSet::new(),
            laptop: None,
        }
    }

    /// Take back every session a service before this one left under
    /// `dir/sessions/`, each ended: its agent went with that service. One the
    /// journal does not show ending is ended here, so each watch of it ends once.
    pub(crate) fn load(&mut self) {
        // None kept is every folder answered, the default; one that cannot
        // be read answers none, and is left for a person to look at.
        self.off = match fs::read(self.dir.join(BYPASS)) {
            Ok(bytes) => serde_json::from_slice::<Bypass>(&bytes)
                .map(|bypass| bypass.off)
                .map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(e) => Err(e.to_string()),
        }
        .map_err(|e| {
            let why = format!("{BYPASS} could not be read, so no card is answered: {e}");
            log(&why);
            why
        });
        self.mirrors = mirrors::load(&self.dir);
        let dirs = match fs::read_dir(self.dir.join("sessions")) {
            Ok(dirs) => dirs,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => return log(&format!("earlier sessions not read: {e}")),
        };
        for dir in dirs.flatten().map(|d| d.path()) {
            let Some(id) = dir.file_name().and_then(|n| n.to_str()) else {
                log(&format!("{} skipped: its name is not UTF-8", dir.display()));
                continue;
            };
            match load(id, &dir) {
                Ok(session) => {
                    self.sessions.insert(HostSession(id.to_owned()), session);
                }
                Err(e) => log(&format!("session {id} skipped: {e}")),
            }
        }
    }

    /// Start a session: journal under `dir/sessions/<id>/`, agent driven on a spawned task.
    pub(crate) fn open(
        hub: &Arc<Mutex<Hub>>,
        cwd: PathBuf,
        mut agent: AgentSpec,
        opening: Opening,
    ) -> Result<HostSession, String> {
        lock(hub).may_use(&cwd)?;
        // The laptop clears it already; its variables name the laptop's paths.
        agent.env.clear();
        // Only the id is taken with the hub locked: the launch and the files
        // touch the disk, which would hold up every other session.
        let (path, connector) = {
            let this = lock(hub);
            (this.path.clone(), this.connector.clone())
        };
        let mut launch = agent.launch(path.as_deref())?;
        if let Some(path) = &path {
            // A declared agent's command, and whatever it runs in turn, is found
            // on this PATH too, not only a preset's binary.
            launch
                .env
                .insert(0, ("PATH".into(), path.to_string_lossy().into_owned()));
        }
        let (id, dir) = lock(hub).fresh_id();
        let journal = Journal::open(&dir)
            .map_err(|e| format!("Could not start this session's journal: {e}"))?;
        // Before the agent starts, so a service started again can list every session it ran.
        let record = Record {
            cwd: cwd.clone(),
            agent: agent.name.clone(),
        };
        serde_json::to_vec(&record)
            .map_err(std::io::Error::other)
            .and_then(|bytes| fs::write(dir.join(RECORD), bytes))
            .map_err(|e| format!("Could not record this session: {e}"))?;
        let opts = SessionOpts {
            cwd: cwd.clone(),
            opening,
            mcp_servers: vec![],
            dirs: vec![],
        };
        let events = connector(launch, opts);
        lock(hub).sessions.insert(
            id.clone(),
            Session {
                dir,
                journal: Some(journal),
                cwd,
                agent: agent.name,
                tx: None,
                watcher: None,
                parked: Vec::new(),
                answered: Vec::new(),
                busy: false,
                aside: BTreeSet::new(),
                ended: false,
                drive: None,
            },
        );
        let task = tokio::spawn(drive(hub.clone(), id.clone(), events));
        if let Some(s) = lock(hub).sessions.get_mut(&id) {
            s.drive = Some(task.abort_handle());
        }
        Ok(id)
    }

    /// `<unix secs>-<counter>`, skipping one whose directory a run before this one left.
    fn fresh_id(&mut self) -> (HostSession, PathBuf) {
        let secs = unix_secs();
        loop {
            self.counter += 1;
            let id = format!("{secs}-{}", self.counter);
            let dir = self.dir.join("sessions").join(&id);
            if !dir.exists() {
                return (HostSession(id), dir);
            }
        }
    }

    /// Send `Replay` and every entry so far to `sink` (only those past
    /// `after` when the journal reaches it), then live entries except
    /// `Request`. The journal is read with the hub unlocked, so a long replay
    /// holds up no other session; what lands meanwhile is read after.
    pub(crate) fn watch(
        hub: &Mutex<Hub>,
        session: &HostSession,
        after: Option<u64>,
        sink: UnboundedSender<FromHost>,
    ) -> Result<(), String> {
        let dir = lock(hub).unwatch(session)?;
        let read = || journal::read_from(&dir, Mark::default());
        // Off the runtime's own threads where it has others to give, so a long
        // read stalls no other task; a single-threaded runtime has none.
        let read = match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
            Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(read),
            Ok(_) | Err(_) => read(),
        };
        let (entries, mark) = read.map_err(unreadable)?;
        lock(hub).replay(session, after, sink, entries, mark)
    }

    /// Stop passing `session` live entries on, for a watch that will replay
    /// them; where its journal is.
    fn unwatch(&mut self, session: &HostSession) -> Result<PathBuf, String> {
        let s = self
            .sessions
            .get_mut(session)
            .ok_or_else(|| unknown(session))?;
        let (dir, cwd) = (s.dir.clone(), s.cwd.clone());
        self.may_use(&cwd)?;
        if let Some(s) = self.sessions.get_mut(session) {
            s.watcher = None;
        }
        Ok(dir)
    }

    /// Send `Replay`, `entries` (the journal up to `mark`) and whatever was
    /// journaled after `mark` to `sink`, then make it the session's watcher.
    /// Past `after` alone when the journal has a line of that seq; a journal
    /// that does not reach it is not the one the watcher holds a copy of, and
    /// is sent whole, saying so with no `after`. Called with the hub locked,
    /// so no entry lands between the two.
    fn replay(
        &mut self,
        session: &HostSession,
        after: Option<u64>,
        sink: UnboundedSender<FromHost>,
        entries: journal::Entries,
        mark: Mark,
    ) -> Result<(), String> {
        let s = self
            .sessions
            .get_mut(session)
            .ok_or_else(|| unknown(session))?;
        let (since, end) = journal::read_from(&s.dir, mark).map_err(unreadable)?;
        // A journal's seq is its line number, so it reaches `after` when it has more lines.
        let after = after.filter(|after| end.lines() > *after);
        let replay = FromHost::Replay {
            session: session.clone(),
            after,
        };
        if sink.send(replay).is_err() {
            return Ok(());
        }
        // ponytail: the whole journal is read, then the copy's part dropped;
        // read from a byte offset if long journals make watches slow.
        let later = entries
            .into_iter()
            .chain(since)
            .filter(|(seq, _, _)| after.is_none_or(|after| *seq > after));
        for (seq, at, entry) in later {
            let line = FromHost::Entry {
                session: session.clone(),
                seq,
                at,
                entry,
            };
            if sink.send(line).is_err() {
                return Ok(());
            }
        }
        s.watcher = Some(sink);
        Ok(())
    }

    /// Hand `request` to the agent, then journal it.
    pub(crate) fn request(
        &mut self,
        session: &HostSession,
        request: AcpRequest,
    ) -> Result<(), String> {
        let s = self
            .sessions
            .get_mut(session)
            .ok_or_else(|| unknown(session))?;
        if s.ended {
            return Err(ENDED.into());
        }
        let cwd = s.cwd.clone();
        self.may_use(&cwd)?;
        let s = self.sessions.get_mut(session).expect("found above");
        // A card has one answer: one the host gave already wins, and the
        // person's goes nowhere rather than reach the agent a second time.
        let rpc = answers(&request);
        if rpc.is_some_and(|rpc| s.answered.contains(rpc)) {
            return Ok(());
        }
        let Some(tx) = s.tx.clone() else {
            return Err("the agent is still starting".into());
        };
        if matches!(request, AcpRequest::Prompt { .. }) {
            self.prompt_to(&cwd)?;
        }
        let s = self.sessions.get_mut(session).expect("found above");
        if let Some(rpc) = rpc {
            s.parked.retain(|card| card.rpc() != rpc);
        }
        let (request, dropped) = match request {
            AcpRequest::Prompt { text, attachments } if !attachments.is_empty() => (
                AcpRequest::Prompt {
                    text,
                    attachments: vec![],
                },
                true,
            ),
            request @ (AcpRequest::Prompt { .. }
            | AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. }) => (request, false),
        };
        // Journaled only once the agent has it. The hub is locked throughout,
        // so nothing the agent says of it is journaled ahead of it.
        tx.send(request.clone()).map_err(|_| ENDED.to_string())?;
        // The request first, then what is said of it, the order the laptop sees live.
        self.record(session, Entry::Request(request));
        if dropped {
            self.record(
                session,
                Entry::Event(AcpEvent::Notice(ATTACHMENTS_DROPPED.into())),
            );
        }
        Ok(())
    }

    /// Whether the host answers the cards of sessions in `cwd` by itself.
    pub(crate) fn bypass(&self, cwd: &Path) -> bool {
        self.off.as_ref().is_ok_and(|off| !off.contains(cwd))
    }

    /// Set whether the host answers `cwd`'s cards by itself, keeping it on
    /// disk first; switched on, the cards already waiting there are answered.
    pub(crate) fn set_bypass(&mut self, cwd: PathBuf, on: bool) -> Result<bool, String> {
        let mut off = self.off.clone()?;
        match on {
            true => off.remove(&cwd),
            false => off.insert(cwd.clone()),
        };
        write_bypass(&self.dir, &Bypass { off })
            .map_err(|e| format!("Could not keep the switch: {e}"))
            .map(|off| self.off = Ok(off))?;
        if on {
            let waiting: Vec<HostSession> = self
                .sessions
                .iter()
                .filter(|(_, s)| s.cwd == cwd && !s.parked.is_empty())
                .map(|(id, _)| id.clone())
                .collect();
            for id in waiting {
                let parked =
                    std::mem::take(&mut self.sessions.get_mut(&id).expect("listed").parked);
                for card in parked {
                    match self.answer(&id, &card) {
                        Some(answer) => self.record(&id, Entry::Answered { card: None, answer }),
                        None => {
                            if let Some(s) = self.sessions.get_mut(&id) {
                                s.parked.push(card);
                            }
                        }
                    }
                }
            }
        }
        Ok(self.bypass(&cwd))
    }

    /// Journal a card the agent parked: answered at once while the switch
    /// is on, the two in one line; else left for a person.
    fn card(&mut self, session: &HostSession, card: Parked) {
        match self.answer(session, &card) {
            Some(answer) => self.record(
                session,
                Entry::Answered {
                    card: Some(Box::new(card.event())),
                    answer,
                },
            ),
            None => {
                if let Some(s) = self.sessions.get_mut(session) {
                    s.parked.push(card.clone());
                }
                self.record(session, Entry::Event(card.event()));
            }
        }
    }

    /// The answer the host sent the agent for `card`, when its folder's
    /// switch is on, the card has one and the agent took it.
    fn answer(&self, session: &HostSession, card: &Parked) -> Option<AcpRequest> {
        let s = self.sessions.get(session)?;
        if !self.bypass(&s.cwd) {
            return None;
        }
        let answer = match card {
            Parked::Permission(p) => p.bypass_answer()?,
            Parked::Question(e) => e.bypass_answer(),
        };
        s.tx.as_ref()?.send(answer.clone()).ok()?;
        Some(answer)
    }

    pub(crate) fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .map(|(id, s)| SessionInfo {
                session: id.clone(),
                cwd: s.cwd.clone(),
                agent: s.agent.clone(),
                busy: s.busy,
                ended: s.ended,
            })
            .collect()
    }

    /// Drop every watcher (the attach went away or was replaced). Sessions run on.
    pub(crate) fn unwatch_all(&mut self) {
        for s in self.sessions.values_mut() {
            s.watcher = None;
        }
    }

    /// Journal `entry` at this second, then pass it to the watcher unless it
    /// is a request, which the one attached laptop applied itself when it sent it.
    fn record(&mut self, session: &HostSession, entry: Entry) {
        let Some(s) = self.sessions.get_mut(session) else {
            return;
        };
        s.busy = busy_after(s.busy, &entry);
        aside_after(&mut s.aside, &entry);
        let cwd = s.cwd.clone();
        if matches!(entry, Entry::Event(AcpEvent::TurnEnded { .. })) {
            self.commit_left_soon(session, &cwd);
        }
        let Some(s) = self.sessions.get_mut(session) else {
            return;
        };
        // A card answered by the host is never answered again.
        match &entry {
            Entry::Answered { answer, .. } => s.answered.extend(answers(answer).cloned()),
            Entry::Ready(_) | Entry::Event(_) | Entry::Request(_) => {}
        }
        let Some(journal) = &mut s.journal else {
            return log(&format!(
                "session {session}: an entry came after the session ended"
            ));
        };
        let at = unix_secs();
        let seq = match journal.append(at, &entry) {
            Ok(seq) => seq,
            Err(e) => {
                log(&format!(
                    "session {session}: an entry was not journaled: {e}"
                ));
                return;
            }
        };
        // An answer the host gave is news to the laptop, as the agent's events are.
        let live = match &entry {
            Entry::Ready(_) | Entry::Event(_) | Entry::Answered { .. } => true,
            Entry::Request(_) => false,
        };
        if !live {
            return;
        }
        if let Some(watcher) = &s.watcher {
            let line = FromHost::Entry {
                session: session.clone(),
                seq,
                at,
                entry,
            };
            if watcher.send(line).is_err() {
                s.watcher = None;
            }
        }
    }

    /// At a turn's end in a branch mirror, commit what the turn left onto
    /// its branch, off the hub's lock: nothing the agent did is lost before
    /// the laptop fetches it.
    fn commit_left_soon(&self, session: &HostSession, cwd: &Path) {
        let Some((mirror, branch)) = self.branch_of(cwd) else {
            return;
        };
        let session = session.clone();
        std::thread::spawn(
            move || match branch::commit_left_blocking(&mirror, &branch) {
                Ok(true) => {}
                Ok(false) if !branch::on_branch(&mirror, &branch) => log(&format!(
                    "session {session}: its mirror is not on {branch}, so its turn's end was not \
                 committed"
                )),
                Ok(false) if branch::mid_operation(&mirror) => log(&format!(
                    "session {session}: its mirror is in the middle of a merge or a \
                     cherry-pick, so its turn's end was not committed"
                )),
                Ok(false) => {}
                Err(e) => log(&format!(
                    "session {session}: what its turn left was not committed: {e}"
                )),
            },
        );
    }

    /// End `session` because the laptop asked: its agent is stopped and the
    /// journal says so. Refused for a session in another laptop's mirror, as
    /// a request to it is; one already ended is left as it is.
    pub(crate) fn end_session(&mut self, session: &HostSession) -> Result<(), String> {
        let s = self.sessions.get(session).ok_or_else(|| unknown(session))?;
        if s.ended {
            return Ok(());
        }
        let cwd = s.cwd.clone();
        self.may_use(&cwd)?;
        if let Some(drive) = self.sessions.get_mut(session).and_then(|s| s.drive.take()) {
            drive.abort();
        }
        self.record(
            session,
            Entry::Event(AcpEvent::Disconnected(ENDED_HERE.into())),
        );
        self.end(session);
        Ok(())
    }

    fn end(&mut self, session: &HostSession) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.ended = true;
            s.drive = None;
            s.busy = false;
            s.aside.clear();
            s.tx = None;
            s.journal = None;
            s.parked.clear();
        }
    }
}

/// The hub, taken back from a task that panicked holding it, so one
/// session's panic does not stop every other.
pub(crate) fn lock(hub: &Mutex<Hub>) -> MutexGuard<'_, Hub> {
    hub.lock().unwrap_or_else(|e| e.into_inner())
}

const ENDED: &str = "this session has ended";

/// Session `id` read back from `dir`, ended; an error if it has no readable
/// record. A journal that cannot be ended here is said in the log, and the
/// session listed all the same: its entries are still there to watch.
fn load(id: &str, dir: &Path) -> Result<Session, String> {
    let bytes = fs::read(dir.join(RECORD)).map_err(|e| format!("{RECORD}: {e}"))?;
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| format!("{RECORD}: {e}"))?;
    if let Err(e) = end_journal(dir) {
        log(&format!("session {id}: its journal was not ended: {e}"));
    }
    Ok(Session {
        dir: dir.to_path_buf(),
        journal: None,
        cwd: record.cwd,
        agent: record.agent,
        tx: None,
        watcher: None,
        parked: Vec::new(),
        answered: Vec::new(),
        busy: false,
        aside: BTreeSet::new(),
        ended: true,
        drive: None,
    })
}

/// End the journal in `dir` with `STOPPED` unless it already ends in a `Disconnected`.
fn end_journal(dir: &Path) -> std::io::Result<()> {
    let ended = matches!(
        journal::last(dir)?,
        Some(Entry::Event(AcpEvent::Disconnected(_)))
    );
    if !ended {
        let stopped = Entry::Event(AcpEvent::Disconnected(STOPPED.into()));
        Journal::open(dir)?.append(unix_secs(), &stopped)?;
    }
    Ok(())
}

/// Keep `bypass` whole, so a service that stops mid-write reads the last one;
/// its set once written.
fn write_bypass(dir: &Path, bypass: &Bypass) -> std::io::Result<BTreeSet<PathBuf>> {
    let text = serde_json::to_string(bypass).map_err(std::io::Error::other)?;
    onehand_core::config::write_atomic(&dir.join(BYPASS), &text)?;
    Ok(bypass.off.clone())
}

/// The rpc id of the card a request answers, if it answers one.
fn answers(request: &AcpRequest) -> Option<&serde_json::Value> {
    match request {
        AcpRequest::PermissionResponse { rpc_id, .. }
        | AcpRequest::ElicitationResponse { rpc_id, .. } => Some(rpc_id),
        AcpRequest::Prompt { .. }
        | AcpRequest::Cancel
        | AcpRequest::SetMode(_)
        | AcpRequest::SetConfigOption { .. }
        | AcpRequest::StopBackground { .. } => None,
    }
}

/// A card the agent parked: a permission or a question.
#[derive(Clone)]
enum Parked {
    Permission(PermissionRequest),
    Question(Elicitation),
}

impl Parked {
    /// The rpc id it is answered by.
    fn rpc(&self) -> &serde_json::Value {
        match self {
            Parked::Permission(p) => &p.rpc_id,
            Parked::Question(e) => &e.rpc_id,
        }
    }

    fn event(self) -> AcpEvent {
        match self {
            Parked::Permission(p) => AcpEvent::Permission(p),
            Parked::Question(e) => AcpEvent::Elicitation(e),
        }
    }
}

fn unreadable(e: std::io::Error) -> String {
    format!("Could not read this session's journal: {e}")
}

fn unknown(session: &HostSession) -> String {
    format!("there is no session {session} on this host")
}

/// Journal each of the agent's events until its stream ends, then mark the session ended.
async fn drive(hub: Arc<Mutex<Hub>>, id: HostSession, mut events: BoxStream<'static, AcpEvent>) {
    let mut said_why = false;
    while let Some(event) = events.next().await {
        let mut hub = lock(&hub);
        match event {
            AcpEvent::Connected {
                tx,
                resumed,
                forkable,
            } => {
                if let Some(s) = hub.sessions.get_mut(&id) {
                    s.tx = Some(tx);
                }
                hub.record(&id, Entry::Ready(Readiness { resumed, forkable }));
            }
            event @ AcpEvent::Disconnected(_) => {
                said_why = true;
                hub.record(&id, Entry::Event(event));
                hub.end(&id);
            }
            event @ (AcpEvent::SessionId(_)
            | AcpEvent::AgentChunk { .. }
            | AcpEvent::ThoughtChunk(_)
            | AcpEvent::UserChunk(_)
            | AcpEvent::ToolCall(_)
            | AcpEvent::ToolUpdate(_)
            | AcpEvent::Plan(_)
            | AcpEvent::Usage(_)
            | AcpEvent::Background(_)
            | AcpEvent::SubagentSpawned { .. }
            | AcpEvent::SubagentEnded { .. }
            | AcpEvent::InChild { .. }
            | AcpEvent::StopRefused { .. }
            | AcpEvent::AvailableCommands(_)
            | AcpEvent::Modes { .. }
            | AcpEvent::ModeChanged(_)
            | AcpEvent::PickConfirmed { .. }
            | AcpEvent::PickRefused { .. }
            | AcpEvent::ConfigOptions(_)
            | AcpEvent::TerminalOutput { .. }
            | AcpEvent::TerminalExit { .. }
            | AcpEvent::TurnEnded { .. }
            | AcpEvent::Error { .. }
            | AcpEvent::Interrupted
            | AcpEvent::LoginRequired { .. }
            | AcpEvent::Notice(_)
            | AcpEvent::Echo(_)) => hub.record(&id, Entry::Event(event)),
            AcpEvent::Permission(p) => hub.card(&id, Parked::Permission(p)),
            AcpEvent::Elicitation(e) => hub.card(&id, Parked::Question(e)),
            // A journal line carries its own time.
            AcpEvent::At(_) => {}
        }
    }
    let mut hub = lock(&hub);
    if !said_why {
        hub.record(&id, Entry::Event(AcpEvent::Disconnected(CLOSED.into())));
    }
    hub.end(&id);
}

#[cfg(test)]
mod tests;
