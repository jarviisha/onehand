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

/// What a `Hello` from a laptop too old to say `take` means: it takes the host.
fn yes() -> bool {
    true
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
    /// With `take`, a person's attach: it takes the host from whatever attach
    /// holds it. Without, an attach made by itself, refused with `Held` while
    /// another laptop's attach is live there. `laptop` names this laptop's
    /// run, so an attach it lost but the host has yet to see go is its own.
    Hello {
        build: Build,
        #[serde(default = "yes")]
        take: bool,
        #[serde(default)]
        laptop: Option<String>,
    },
    Shutdown,
    List,
    Open {
        cwd: PathBuf,
        agent: AgentSpec,
        opening: Opening,
    },
    /// With `after`, the laptop already holds the journal up to that seq.
    Watch {
        session: HostSession,
        #[serde(default)]
        after: Option<u64>,
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
    /// The host user's home folder.
    Home,
    /// `git clone` `url` into `into`, an absolute path that does not exist yet.
    Clone {
        url: String,
        into: PathBuf,
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
    /// Whether the host answers the cards of sessions in `cwd` by itself;
    /// with `on`, set it first.
    Bypass {
        cwd: PathBuf,
        on: Option<bool>,
    },
    /// This laptop's mirror called `name`, made on first use; refused when
    /// another laptop owns it. With `branch`, one made now works on that
    /// branch and has no key; an older laptop sends none.
    Mirror {
        name: String,
        #[serde(default)]
        branch: Option<String>,
    },
    /// Hand the mirror at `path`'s key to `to`. To the laptop only while
    /// nothing works there and no prompt went to it since `generation`.
    MirrorKey {
        path: PathBuf,
        to: Holder,
        generation: u64,
    },
    /// What the mirror at `path` holds, as its copy is listed: the files git
    /// does not ignore, relative to it.
    MirrorFiles {
        path: PathBuf,
    },
    /// Bring `commit`, pushed into the branch mirror at `path`, onto its
    /// branch: the branch starts from it, or it is merged in. Refused while
    /// something works there.
    MirrorUpdate {
        path: PathBuf,
        commit: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// `busy` holds the ids of the sessions that are mid-turn, `terms` how
    /// many terminals are open: a restart ends both.
    Welcome {
        build: Build,
        busy: Vec<HostSession>,
        #[serde(default)]
        terms: usize,
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
    Home(PathBuf),
    TermOpened {
        term: HostTerm,
    },
    Terms(Vec<TermInfo>),
    /// A `Hello` without `take`, refused: another laptop's attach holds the host.
    Held,
    /// Whether the host answers a folder's cards by itself, as it is now.
    Bypass {
        on: bool,
    },
    Mirror(MirrorState),
    MirrorFiles(Vec<PathBuf>),
}

/// Which side may change an offloaded project's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Holder {
    Laptop,
    Host,
}

/// A mirror as the host has it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorState {
    /// Absolute, on the host.
    pub path: PathBuf,
    pub key: Holder,
    /// Moved on by every prompt sent to a session there.
    pub generation: u64,
    /// Whether a session there is mid-turn or has work running aside.
    pub working: bool,
    /// A branch mirror's branch, as the commit it is now; none for a mirror
    /// with a key, or a branch not started yet.
    #[serde(default)]
    pub head: Option<String>,
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
    /// first readable entry, or past `after` when the watch asked for that and
    /// the journal reaches it; it runs until that watch's answer.
    Replay {
        session: HostSession,
        #[serde(default)]
        after: Option<u64>,
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
    /// A card the host answered by itself, with that answer. The card is
    /// here when it is journaled in the same line, so a laptop takes the two
    /// at once; none when it was journaled earlier, while it waited.
    Answered {
        card: Option<Box<AcpEvent>>,
        answer: AcpRequest,
    },
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
        Entry::Ready(_) | Entry::Answered { .. } => busy,
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

/// Update `aside`, the ids of the background work and subagents a session
/// has running, after `entry`; work that outlives its turn still writes files.
pub fn aside_after(aside: &mut std::collections::BTreeSet<String>, entry: &Entry) {
    let Entry::Event(event) = entry else { return };
    match event {
        AcpEvent::Background(crate::acp::BackgroundReport::Spawned { id, .. }) => {
            aside.insert(format!("bg:{id}"));
        }
        AcpEvent::Background(crate::acp::BackgroundReport::Ended { id, .. }) => {
            aside.remove(&format!("bg:{id}"));
        }
        AcpEvent::SubagentSpawned { id, .. } => {
            aside.insert(format!("sub:{id}"));
        }
        AcpEvent::SubagentEnded { id, .. } => {
            aside.remove(&format!("sub:{id}"));
        }
        AcpEvent::Disconnected(_) => aside.clear(),
        AcpEvent::Connected { .. }
        | AcpEvent::SessionId(_)
        | AcpEvent::AgentChunk { .. }
        | AcpEvent::ThoughtChunk(_)
        | AcpEvent::UserChunk(_)
        | AcpEvent::ToolCall(_)
        | AcpEvent::ToolUpdate(_)
        | AcpEvent::Plan(_)
        | AcpEvent::Usage(_)
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
        | AcpEvent::Notice(_)
        | AcpEvent::Echo(_)
        | AcpEvent::At(_) => {}
    }
}

/// The folder a clone of `url` is named by: its last segment after `/` or
/// `:`, without a trailing `/` or `.git`. None when that leaves no name, as
/// for a URL that names only a server.
pub fn clone_name(url: &str) -> Option<String> {
    // The server of `scheme://server/path` is never the name.
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => url,
    };
    let path = path.trim_end_matches('/');
    let last = path.rsplit(['/', ':']).next().unwrap_or(path);
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!matches!(name, "" | "." | "..")).then(|| name.to_string())
}

/// The build id of a host binary: FNV-1a 64 over its bytes, as 16 hex digits.
pub fn build_id(bytes: &[u8]) -> Build {
    Build(format!("{:016x}", crate::chat::store::fnv1a(bytes)))
}

#[cfg(test)]
mod tests;
