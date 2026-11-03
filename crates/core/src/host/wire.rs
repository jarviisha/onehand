//! The lines a laptop and a host exchange, and the two small rules both ends
//! share: when a session counts as mid-turn, and how a host binary is named.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::acp::{AcpEvent, AcpRequest, Opening};
use crate::config::AgentSpec;

/// The wire's version: a laptop and a host must agree on it to talk.
pub const PROTOCOL: u32 = 1;

/// What the laptop sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ask {
    pub id: u64,
    pub call: Call,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    Hello {
        build: String,
        protocol: u32,
    },
    Shutdown,
    List,
    Open {
        cwd: PathBuf,
        agent: AgentSpec,
        opening: Opening,
    },
    Watch {
        session: String,
    },
    Request {
        session: String,
        request: AcpRequest,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// `busy` holds the ids of the sessions that are mid-turn.
    Welcome {
        build: String,
        protocol: u32,
        busy: Vec<String>,
    },
    Sessions(Vec<SessionInfo>),
    Opened {
        session: String,
    },
    Done,
}

/// What the host sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Answer {
        id: u64,
        result: Result<Reply, String>,
    },
    Entry {
        session: String,
        seq: u64,
        entry: Entry,
    },
    /// Another attach took this host; this stream is about to close.
    Kicked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// The agent came up: `AcpEvent::Connected` without its channel.
    Ready {
        resumed: bool,
        forkable: bool,
    },
    Event(AcpEvent),
    Request(AcpRequest),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session: String,
    pub cwd: PathBuf,
    pub agent: String,
    pub busy: bool,
    pub ended: bool,
}

/// Whether a session is mid-turn after `entry`, given whether it was before.
pub fn busy_after(busy: bool, entry: &Entry) -> bool {
    match entry {
        Entry::Ready { .. } => busy,
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
            | AcpEvent::Echo(_) => busy,
        },
    }
}

/// The build id of a host binary: FNV-1a 64 over its bytes, as 16 hex digits.
pub fn build_id(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests;
