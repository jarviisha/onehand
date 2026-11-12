//! A project's place in the workspace: added, kept for a task, removed, and
//! the host its code was offloaded to.

use super::{Effect, Refusal, Signal, WorkspaceDrive};
use crate::workspace::{HostName, Offloaded};
use std::path::{Path, PathBuf};

/// What removing a project takes with it: its sessions, closed, and its
/// unsaved files, discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Losses {
    pub sessions: usize,
    pub unsaved: usize,
}

/// A refusal, said.
fn refused<T>(why: Refusal) -> (Result<T, Refusal>, Vec<Effect>) {
    (Err(why), vec![Effect::Signal(Signal::Refused(why))])
}

impl WorkspaceDrive {
    /// Where the project at `root`, a folder on this machine, sits among the
    /// workspace's projects; refused, and said, when it is no longer one.
    #[must_use = "a refusal is to be said"]
    pub fn find_project(&self, root: &Path) -> (Result<usize, Refusal>, Vec<Effect>) {
        match self.local_index(root) {
            Some(idx) => (Ok(idx), Vec::new()),
            None => refused(Refusal::ProjectGone),
        }
    }

    /// The folder of project `idx` on this machine; refused, and said, when
    /// there is none: gone, or on a host.
    #[must_use = "a refusal is to be said"]
    pub fn local_project(&self, idx: usize) -> (Result<PathBuf, Refusal>, Vec<Effect>) {
        let local = self
            .workspace
            .roots
            .get(idx)
            .and_then(|root| root.local_path());
        match local {
            Some(path) => (Ok(path.to_path_buf()), Vec::new()),
            None => refused(Refusal::ProjectGone),
        }
    }

    /// What *Run check* runs: the active project on this machine and its
    /// check command; refused, and said, when the project selected is not on
    /// this machine or has no check command.
    #[must_use = "a refusal is to be said"]
    pub fn check_target(&self) -> (Result<(PathBuf, String), Refusal>, Vec<Effect>) {
        let Some(root) = self.workspace.active_root() else {
            return refused(Refusal::NotHere);
        };
        match (root.local_path(), &root.check) {
            (Some(path), Some(check)) => (Ok((path.to_path_buf(), check.clone())), Vec::new()),
            (Some(_), None) => refused(Refusal::NoCheck),
            (None, _) => refused(Refusal::NotHere),
        }
    }

    /// Add `dir`, a folder here, as a project and select it, or select it if
    /// it is one already: what the folder picker, a second launch and a
    /// worktree split off do.
    #[must_use = "its effects are to be carried"]
    pub fn open_project(&mut self, dir: PathBuf) -> Vec<Effect> {
        self.workspace.add_root(dir);
        self.opened()
    }

    /// [`Self::open_project`] for `path` on `host`.
    #[must_use = "its effects are to be carried"]
    pub fn open_host_project(&mut self, host: HostName, path: PathBuf) -> Vec<Effect> {
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
    pub fn keep_project(&mut self, dir: PathBuf) -> (usize, Vec<Effect>) {
        if let Some(idx) = self.local_index(&dir) {
            return (idx, Vec::new());
        }
        let idx = self.workspace.add_root(dir);
        (idx, self.kept())
    }

    /// [`Self::keep_project`], selecting nothing: a worktree cut for a task
    /// nobody here started must not move what the person has selected.
    #[must_use = "its effects are to be carried"]
    pub fn keep_project_quietly(&mut self, dir: PathBuf) -> Vec<Effect> {
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
    /// on `on`, offloaded, or here again with none. The sessions it had close,
    /// since they ran where it no longer does, and it is written once things
    /// are still, as the offload's other writes are. `None` when `dir` is no
    /// longer a project here: nothing moves.
    #[must_use = "its effects are to be carried"]
    pub fn set_offload(&mut self, dir: &Path, on: Option<Offloaded>) -> Option<Vec<Effect>> {
        let idx = self.local_index(dir)?;
        let mut fx = self.drop_sessions_in(dir);
        self.workspace.roots[idx].offload = on;
        fx.extend(self.save_soon());
        Some(fx)
    }
}
