//! The host's sessions: each one's agent, driven on a task of its own, and
//! its journal, which every agent event and every request goes into before
//! anything else sees it. A laptop that watches a session gets the journal
//! from the start, then what comes after, so a laptop that went away mid-turn
//! loses nothing.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::stream::BoxStream;
use futures::StreamExt as _;
use onehand_core::acp::{AcpEvent, AcpRequest, Opening, ReqTx, SessionOpts};
use onehand_core::config::{AgentSpec, Launch};
use onehand_core::host::journal::{self, Journal};
use onehand_core::host::wire::{busy_after, Entry, Line, SessionInfo};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::serve::log;

/// How a session's agent is reached: `acp::connect` in production, a fake in tests.
pub(crate) type Connector =
    Arc<dyn Fn(Launch, SessionOpts) -> BoxStream<'static, AcpEvent> + Send + Sync>;

const ATTACHMENTS_DROPPED: &str =
    "Attachments are not sent to a host yet; the prompt went without them";

/// What a session's stream ending says when the agent did not say why.
const CLOSED: &str = "The agent's connection closed";

/// What ends a session a service before this one left running.
const STOPPED: &str = "The host service stopped while this session was running";

/// `dir/sessions/<id>/session.json`: what a service started again needs to list the session.
const RECORD: &str = "session.json";

#[derive(Serialize, Deserialize)]
struct Record {
    cwd: PathBuf,
    agent: String,
}

struct Session {
    dir: PathBuf,
    journal: Journal,
    cwd: PathBuf,
    agent: String,
    /// The agent's request channel, once it is up and until it ends.
    tx: Option<ReqTx>,
    /// The laptop watching this session, if one is.
    watcher: Option<UnboundedSender<Line>>,
    busy: bool,
    ended: bool,
}

pub(crate) struct Hub {
    dir: PathBuf,
    /// `PATH` as the user's login shell has it, which agents are found and run with.
    path: Option<OsString>,
    connector: Connector,
    sessions: BTreeMap<String, Session>,
    counter: u64,
}

impl Hub {
    pub(crate) fn new(dir: PathBuf, path: Option<OsString>, connector: Connector) -> Hub {
        Hub {
            dir,
            path,
            connector,
            sessions: BTreeMap::new(),
            counter: 0,
        }
    }

    /// Take back every session a service before this one left under
    /// `dir/sessions/`, each ended: its agent went with that service. One the
    /// journal does not show ending is ended here, so each watch of it ends once.
    pub(crate) fn load(&mut self) {
        let dirs = match fs::read_dir(self.dir.join("sessions")) {
            Ok(dirs) => dirs,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => return log(&format!("earlier sessions not read: {e}")),
        };
        for dir in dirs.flatten().map(|d| d.path()) {
            let Some(id) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                continue;
            };
            match load(&dir) {
                Ok(session) => {
                    self.sessions.insert(id, session);
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
    ) -> Result<String, String> {
        // The laptop clears it already; its variables name the laptop's paths.
        agent.env.clear();
        let mut this = hub.lock().unwrap_or_else(|e| e.into_inner());
        let mut launch = agent.launch(this.path.as_deref())?;
        if let Some(path) = &this.path {
            // A declared agent's command, and whatever it runs in turn, is found
            // on this PATH too, not only a preset's binary.
            launch
                .env
                .insert(0, ("PATH".into(), path.to_string_lossy().into_owned()));
        }
        let (id, dir) = this.fresh_id();
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
        let events = (this.connector)(launch, opts);
        this.sessions.insert(
            id.clone(),
            Session {
                dir,
                journal,
                cwd,
                agent: agent.name,
                tx: None,
                watcher: None,
                busy: false,
                ended: false,
            },
        );
        drop(this);
        tokio::spawn(drive(hub.clone(), id.clone(), events));
        Ok(id)
    }

    /// `<unix secs>-<counter>`, skipping one whose directory a run before this one left.
    fn fresh_id(&mut self) -> (String, PathBuf) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        loop {
            self.counter += 1;
            let id = format!("{secs}-{}", self.counter);
            let dir = self.dir.join("sessions").join(&id);
            if !dir.exists() {
                return (id, dir);
            }
        }
    }

    /// Send every entry so far to `sink`, then live entries except `Request`.
    pub(crate) fn watch(
        &mut self,
        session: &str,
        sink: UnboundedSender<Line>,
    ) -> Result<(), String> {
        let s = self
            .sessions
            .get_mut(session)
            .ok_or_else(|| unknown(session))?;
        let entries = journal::read(&s.dir)
            .map_err(|e| format!("Could not read this session's journal: {e}"))?;
        for (seq, entry) in entries {
            let line = Line::Entry {
                session: session.to_string(),
                seq,
                entry,
            };
            if sink.send(line).is_err() {
                return Ok(());
            }
        }
        s.watcher = Some(sink);
        Ok(())
    }

    /// Journal `request`, then hand it to the agent.
    pub(crate) fn request(&mut self, session: &str, request: AcpRequest) -> Result<(), String> {
        let s = self.sessions.get(session).ok_or_else(|| unknown(session))?;
        if s.ended {
            return Err(ENDED.into());
        }
        let Some(tx) = s.tx.clone() else {
            return Err("the agent is still starting".into());
        };
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
        // The request first, then what is said of it, the order the laptop sees live.
        self.record(session, Entry::Request(request.clone()));
        if dropped {
            self.record(
                session,
                Entry::Event(AcpEvent::Error {
                    text: ATTACHMENTS_DROPPED.into(),
                    detail: None,
                }),
            );
        }
        tx.send(request).map_err(|_| ENDED.to_string())
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

    /// Journal `entry`, then pass it to the watcher unless it is a request,
    /// which the one attached laptop applied itself when it sent it.
    fn record(&mut self, session: &str, entry: Entry) {
        let Some(s) = self.sessions.get_mut(session) else {
            return;
        };
        s.busy = busy_after(s.busy, &entry);
        let seq = match s.journal.append(&entry) {
            Ok(seq) => seq,
            Err(e) => {
                log(&format!(
                    "session {session}: an entry was not journaled: {e}"
                ));
                return;
            }
        };
        let live = match &entry {
            Entry::Ready { .. } | Entry::Event(_) => true,
            Entry::Request(_) => false,
        };
        if !live {
            return;
        }
        if let Some(watcher) = &s.watcher {
            let line = Line::Entry {
                session: session.to_string(),
                seq,
                entry,
            };
            if watcher.send(line).is_err() {
                s.watcher = None;
            }
        }
    }

    fn end(&mut self, session: &str) {
        if let Some(s) = self.sessions.get_mut(session) {
            s.ended = true;
            s.busy = false;
            s.tx = None;
        }
    }
}

const ENDED: &str = "this session has ended";

/// A session read back from `dir`, ended; an error if it has no readable record.
fn load(dir: &Path) -> Result<Session, String> {
    let bytes = fs::read(dir.join(RECORD)).map_err(|e| format!("{RECORD}: {e}"))?;
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| format!("{RECORD}: {e}"))?;
    let entries = journal::read(dir).map_err(|e| format!("journal: {e}"))?;
    let mut journal = Journal::open(dir).map_err(|e| format!("journal: {e}"))?;
    let ended = matches!(
        entries.last(),
        Some((_, Entry::Event(AcpEvent::Disconnected(_))))
    );
    if !ended {
        journal
            .append(&Entry::Event(AcpEvent::Disconnected(STOPPED.into())))
            .map_err(|e| format!("journal: {e}"))?;
    }
    Ok(Session {
        dir: dir.to_path_buf(),
        journal,
        cwd: record.cwd,
        agent: record.agent,
        tx: None,
        watcher: None,
        busy: false,
        ended: true,
    })
}

fn unknown(session: &str) -> String {
    format!("there is no session {session} on this host")
}

/// Journal each of the agent's events until its stream ends, then mark the session ended.
async fn drive(hub: Arc<Mutex<Hub>>, id: String, mut events: BoxStream<'static, AcpEvent>) {
    let mut said_why = false;
    while let Some(event) = events.next().await {
        let mut hub = hub.lock().unwrap_or_else(|e| e.into_inner());
        match event {
            AcpEvent::Connected {
                tx,
                resumed,
                forkable,
            } => {
                if let Some(s) = hub.sessions.get_mut(&id) {
                    s.tx = Some(tx);
                }
                hub.record(&id, Entry::Ready { resumed, forkable });
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
            | AcpEvent::Permission(_)
            | AcpEvent::Elicitation(_)
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
            | AcpEvent::Echo(_)) => hub.record(&id, Entry::Event(event)),
        }
    }
    let mut hub = hub.lock().unwrap_or_else(|e| e.into_inner());
    if !said_why {
        hub.record(&id, Entry::Event(AcpEvent::Disconnected(CLOSED.into())));
    }
    hub.end(&id);
}

#[cfg(test)]
mod tests;
