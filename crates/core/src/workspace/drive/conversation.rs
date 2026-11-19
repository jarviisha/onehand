//! A conversation asked for again, from an issue or a task: the live session
//! that holds it, wherever it is, else the saved one resumed on the project
//! it ran in.
//!
//! **That project may have left the workspace**: a worker's worktree is
//! dropped from it when the worker closes, while the folder stays on disk.
//! Asked to open what was said there, the folder is added back, since a
//! person pressing for that conversation is asking for its project too. Only
//! a conversation never saved, its agent no longer set up, or its folder gone
//! is refused, and each is said.

use super::{Effect, Signal, StartsOn, WorkspaceDrive};
use crate::config::AgentSpec;
use crate::project::Project;
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
        let idx = match self.local_index(&saved.root) {
            Some(idx) => idx,
            None if saved.there => {
                let spec = spec.clone();
                return vec![Effect::Place { saved, spec, uid }];
            }
            None => return vec![Effect::Signal(Signal::FolderGone(saved.root))],
        };
        self.resume_at(idx, saved, spec.clone(), uid, Vec::new())
    }

    /// What [`Effect::Place`] found: `saved`'s folder is of `project`. The
    /// folder is kept as a project of the workspace, and the conversation
    /// resumed there as session `uid` by `spec`.
    #[must_use = "its effects are to be carried"]
    pub fn resume_placed(
        &mut self,
        saved: Saved,
        spec: AgentSpec,
        uid: u64,
        project: &Project,
    ) -> Vec<Effect> {
        let (idx, fx) = self.keep_project(saved.root.clone(), project);
        self.resume_at(idx, saved, spec, uid, fx)
    }

    fn resume_at(
        &mut self,
        idx: usize,
        saved: Saved,
        spec: AgentSpec,
        uid: u64,
        mut fx: Vec<Effect>,
    ) -> Vec<Effect> {
        fx.extend(self.select_project(idx));
        let starts = StartsOn::Resume(saved.resume);
        let (_, added) = self.add_session_on(Some(spec), uid, None, starts);
        fx.extend(added);
        fx
    }
}
