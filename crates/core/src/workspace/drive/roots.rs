//! A project's place in the workspace: added, kept for a task, removed, and
//! the host its code was offloaded to.

use super::{Effect, Signal, WorkspaceDrive};
use crate::workspace::{HostName, Offloaded};
use std::path::{Path, PathBuf};

/// What removing a project takes with it: its sessions, closed, and its
/// unsaved files, discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Losses {
    pub sessions: usize,
    pub unsaved: usize,
}

impl WorkspaceDrive {
    /// Add `dir`, a folder here, as a project and select it, or select it if
    /// it is one already: what the folder picker, a second launch and a
    /// worktree split off do.
    #[must_use = "its effects are to be carried"]
    pub fn open_root(&mut self, dir: PathBuf) -> Vec<Effect> {
        self.workspace.add_root(dir);
        self.opened()
    }

    /// [`Self::open_root`] for `path` on `host`.
    #[must_use = "its effects are to be carried"]
    pub fn open_host_root(&mut self, host: HostName, path: PathBuf) -> Vec<Effect> {
        self.workspace.add_host_root(host, path);
        self.opened()
    }

    fn opened(&mut self) -> Vec<Effect> {
        let mut fx = vec![Effect::Show, Effect::Resweep];
        fx.extend(self.save());
        fx.push(Effect::Changed);
        fx
    }

    /// Keep `dir`, a folder here, as a project for work about to start in it,
    /// selected but not yet shown: added when the window does not have it.
    /// Answers its index.
    #[must_use = "its effects are to be carried"]
    pub fn keep_root(&mut self, dir: PathBuf) -> (usize, Vec<Effect>) {
        if let Some(idx) = self.local_index(&dir) {
            return (idx, Vec::new());
        }
        let idx = self.workspace.add_root(dir);
        (idx, self.kept())
    }

    /// [`Self::keep_root`], selecting nothing: a worktree cut for a task
    /// nobody here started must not move what the person has selected.
    #[must_use = "its effects are to be carried"]
    pub fn keep_root_quietly(&mut self, dir: PathBuf) -> Vec<Effect> {
        if self.local_index(&dir).is_some() {
            return Vec::new();
        }
        self.workspace.add_root_quietly(dir);
        self.kept()
    }

    fn kept(&mut self) -> Vec<Effect> {
        let mut fx = vec![Effect::Resweep];
        fx.extend(self.save());
        fx.push(Effect::Changed);
        fx
    }

    /// What removing project `idx` takes with it, `unsaved` being how many of
    /// its files the editor holds unsaved; `None` when it takes nothing, and
    /// so goes without a question.
    pub fn losses(&self, idx: usize, unsaved: usize) -> Option<Losses> {
        let sessions = self.workspace.roots.get(idx)?.sessions.len();
        (sessions > 0 || unsaved > 0).then_some(Losses { sessions, unsaved })
    }

    /// Remove the project at `path` on `host`, the question already answered:
    /// by host and path rather than by index, since the list can be reordered
    /// while the question is open. Its sessions close, it is said, what is
    /// left shows, and the workspace is written.
    #[must_use = "its effects are to be carried"]
    pub fn remove(&mut self, host: Option<&HostName>, path: &Path) -> Vec<Effect> {
        let Some(idx) = self
            .workspace
            .roots
            .iter()
            .position(|r| r.on_host() == host && r.path == path)
        else {
            return Vec::new();
        };
        let label = self.workspace.roots[idx].label.clone();
        let mut fx = self.forget(idx);
        fx.push(Effect::Signal(Signal::Removed(label)));
        fx.push(Effect::Show);
        fx.extend(self.save());
        fx
    }

    /// Say where the sessions of the project at `dir`, a folder here, run:
    /// on `on`, offloaded, or here again with none. Written once things are
    /// still, as the offload's other writes are.
    #[must_use = "its effects are to be carried"]
    pub fn set_offload(&mut self, dir: &Path, on: Option<Offloaded>) -> Vec<Effect> {
        let Some(idx) = self.local_index(dir) else {
            return Vec::new();
        };
        self.workspace.roots[idx].offload = on;
        self.save_soon()
    }
}
