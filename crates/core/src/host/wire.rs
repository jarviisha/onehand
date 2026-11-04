//! The lines a laptop and a host exchange, and the two small rules both ends
//! share: when a session counts as mid-turn, and how a host binary is named.

use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::acp::{AcpEvent, AcpRequest, Opening};
use crate::config::AgentSpec;
use crate::editor::SaveOutcome;
use crate::gitstat::GitStatus;
use crate::tree::TreeEntry;

/// A session on a host, as the host names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostSession(pub String);

impl HostSession {
    /// When the session started, in unix seconds, and its place among those
    /// that started that second: the host names one `<secs>-<counter>`.
    pub fn started(&self) -> (u64, u64) {
        let (secs, n) = self.0.split_once('-').unwrap_or((&self.0, ""));
        (secs.parse().unwrap_or(0), n.parse().unwrap_or(0))
    }
}

/// A host binary's build id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Build(pub String);

/// A terminal on a host, as the host names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostTerm(pub u64);

/// Raw bytes, which cross as one base64 string: a terminal's output is not
/// text, and as a JSON array of numbers it would take four times the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::acp::base64_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        crate::acp::base64_decode(&text)
            .map(Bytes)
            .ok_or_else(|| serde::de::Error::custom("not base64"))
    }
}

impl fmt::Display for HostSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the laptop sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
pub struct Question {
    pub id: u64,
    pub call: Call,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    Hello {
        build: Build,
    },
    Shutdown,
    List,
    Open {
        cwd: PathBuf,
        agent: AgentSpec,
        opening: Opening,
    },
    Watch {
        session: HostSession,
    },
    Request {
        session: HostSession,
        request: AcpRequest,
    },
    /// Whether `path` is a folder on the host.
    Probe {
        path: PathBuf,
    },
    /// A folder's listing, as the file tree reads one here.
    ReadDir {
        path: PathBuf,
    },
    /// A file for the editor, refused when too large or not UTF-8.
    ReadFile {
        path: PathBuf,
    },
    /// Write `text` unless the file changed since `expected`.
    SaveFile {
        path: PathBuf,
        text: String,
        expected: Option<SystemTime>,
    },
    GitStatus {
        root: PathBuf,
    },
    TermOpen {
        cwd: PathBuf,
        cols: u16,
        rows: u16,
    },
    TermList,
    /// Send what the terminal has kept as `TermOutput`, then answer `Done`;
    /// what it prints after follows as it comes.
    TermWatch {
        term: HostTerm,
    },
    TermWrite {
        term: HostTerm,
        data: Bytes,
    },
    TermResize {
        term: HostTerm,
        cols: u16,
        rows: u16,
    },
    TermClose {
        term: HostTerm,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// `busy` holds the ids of the sessions that are mid-turn.
    Welcome {
        build: Build,
        busy: Vec<HostSession>,
    },
    Sessions(Vec<SessionInfo>),
    Opened {
        session: HostSession,
    },
    Done,
    Probed {
        is_dir: bool,
    },
    /// `cut` is how many entries the listing's bound left out.
    Listing {
        entries: Vec<TreeEntry>,
        cut: usize,
    },
    File {
        text: String,
        mtime: Option<SystemTime>,
    },
    Saved(SaveOutcome),
    /// None when the folder is not in a repository.
    Git(Option<GitStatus>),
    TermOpened {
        term: HostTerm,
    },
    Terms(Vec<TermInfo>),
}

/// What the host sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromHost {
    Answer {
        id: u64,
        result: Result<Reply, String>,
    },
    /// What follows for `session` is a watch's replay, from the journal's
    /// first readable entry; it runs until that watch's answer.
    Replay {
        session: HostSession,
    },
    /// `at` is when the host journaled it, in unix seconds; 0 when not known.
    Entry {
        session: HostSession,
        seq: u64,
        at: u64,
        entry: Entry,
    },
    /// Another attach took this host; this stream is about to close.
    Kicked,
    TermOutput {
        term: HostTerm,
        data: Bytes,
    },
    /// The terminal's process ended; nothing more comes for it.
    TermExit {
        term: HostTerm,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// The agent came up: `AcpEvent::Connected` without its channel.
    Ready(Readiness),
    Event(AcpEvent),
    Request(AcpRequest),
}

/// What an agent that came up says of its session.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Readiness {
    pub resumed: bool,
    pub forkable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session: HostSession,
    pub cwd: PathBuf,
    pub agent: String,
    pub busy: bool,
    pub ended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermInfo {
    pub term: HostTerm,
    pub cwd: PathBuf,
}

/// Whether a session is mid-turn after `entry`, given whether it was before.
pub fn busy_after(busy: bool, entry: &Entry) -> bool {
    match entry {
        Entry::Ready(_) => busy,
        Entry::Request(request) => match request {
            AcpRequest::Prompt { .. } => true,
            AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. } => busy,
        },
        Entry::Event(event) => match event {
            AcpEvent::TurnEnded { .. } | AcpEvent::Disconnected(_) => false,
            AcpEvent::Connected { .. }
            | AcpEvent::SessionId(_)
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
            | AcpEvent::Error { .. }
            | AcpEvent::Interrupted
            | AcpEvent::LoginRequired { .. }
            | AcpEvent::Notice(_)
            | AcpEvent::Echo(_)
            | AcpEvent::At(_) => busy,
        },
    }
}

/// The build id of a host binary: FNV-1a 64 over its bytes, as 16 hex digits.
pub fn build_id(bytes: &[u8]) -> Build {
    Build(format!("{:016x}", crate::chat::store::fnv1a(bytes)))
}

#[cfg(test)]
mod tests;
