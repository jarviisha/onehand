//! A conversation asked for again, from an issue or a task: the live session
//! that holds it, wherever it is, else the saved one resumed on the project
//! it ran in.
//!
//! **That project may have left the workspace**: an unattended run's worktree
//! is dropped from it when the run ends, while the folder stays on disk.
//! Asked to open what was said there, the folder is added back, since a
//! person pressing for that conversation is asking for its project too. Only
//! a conversation never saved, its agent no longer set up, or its folder gone
//! is refused, and each is said.

use super::{Effect, Refusal, Signal, StartsOn, WorkspaceDrive};
use crate::config::AgentSpec;
use std::path::PathBuf;

/// Where a session asked for is live: in this window, in window `W`, or
/// nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live<W> {
    Here(u64),
    There(u64, W),
    Nowhere,
}

/// What a session asked for comes to: effects in this window, or the session
/// shown in window `W`, which holds it.
pub enum Reach<W> {
    Here(Vec<Effect>),
    There(u64, W),
}

/// A conversation as it was saved: the project it ran in, whether that
/// folder is still on disk, its agent's name, and the folder it is kept in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub root: PathBuf,
    pub there: bool,
    pub agent: String,
    pub resume: PathBuf,
}

impl WorkspaceDrive {
    /// Put the conversation the agent named `id` on screen: its live session
    /// where it is (`live`), else read the saved one
    /// ([`Effect::ReadConversation`]).
    #[must_use = "what it comes to is to be carried"]
    pub fn open_conversation<W>(&mut self, id: &str, live: Live<W>) -> Reach<W> {
        match live {
            Live::Here(uid) => Reach::Here(self.select(uid)),
            Live::There(uid, window) => Reach::There(uid, window),
            Live::Nowhere => Reach::Here(vec![Effect::ReadConversation(id.to_string())]),
        }
    }

    /// Put a task's session on screen where it is live, or say it has none.
    #[must_use = "what it comes to is to be carried"]
    pub fn show_task_session<W>(&mut self, live: Live<W>) -> Reach<W> {
        match live {
            Live::Here(uid) => Reach::Here(self.select(uid)),
            Live::There(uid, window) => Reach::There(uid, window),
            Live::Nowhere => Reach::Here(vec![Effect::Signal(Signal::Refused(Refusal::NoSession))]),
        }
    }

    /// What [`Effect::ReadConversation`] found: resumed as session `uid` by
    /// its own agent among `agents`, on its project, kept again when its
    /// folder is there; or said why not. The agent is looked at first, so a
    /// conversation nothing can resume adds no project back.
    pub(super) fn conversation_read(
        &mut self,
        found: Option<Saved>,
        agents: &[AgentSpec],
        uid: u64,
    ) -> Vec<Effect> {
        let Some(saved) = found else {
            return vec![Effect::Signal(Signal::ConversationNotSaved)];
        };
        let Some(spec) = agents.iter().find(|spec| spec.name == saved.agent) else {
            return vec![Effect::Signal(Signal::AgentGone(saved.agent))];
        };
        let (idx, mut fx) = match self.local_index(&saved.root) {
            Some(idx) => (idx, Vec::new()),
            None if saved.there => self.keep_project(saved.root),
            None => return vec![Effect::Signal(Signal::FolderGone(saved.root))],
        };
        fx.extend(self.select_project(idx));
        let starts = StartsOn::Resume(saved.resume);
        let (_, added) = self.add_session_on(Some(spec.clone()), uid, None, starts);
        fx.extend(added);
        fx
    }
}
