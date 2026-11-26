//! A project's place in the workspace: added, kept for a task, removed, and
//! the host its code was offloaded to.

use super::{Effect, Refusal, Signal, WorkspaceDrive};
use crate::project::Project;
use crate::workspace::{normalize_root, HostName, ProjectRoot};
use std::path::{Path, PathBuf};

/// The row `dir`, on `host`, comes in as: `project`'s row, at its own folder,
/// when `dir` is one of its folders; or a bare one, of no project, when its
/// project is not known yet or does not hold it, which never draws another
/// folder's facts.
fn row(host: Option<&HostName>, dir: PathBuf, project: Option<&Project>) -> ProjectRoot {
    match project.filter(|p| p.folder(host, &dir).is_some()) {
        Some(project) => ProjectRoot::of(project),
        None => {
            let mut root = ProjectRoot::new(dir);
            root.host = host.cloned();
            root
        }
    }
}

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
    /// there is no such project, or it is on a host.
    #[must_use = "a refusal is to be said"]
    pub fn local_project(&self, idx: usize) -> (Result<PathBuf, Refusal>, Vec<Effect>) {
        let Some(root) = self.workspace.roots.get(idx) else {
            return refused(Refusal::ProjectGone);
        };
        match root.local_path() {
            Some(path) => (Ok(path.to_path_buf()), Vec::new()),
            None => refused(Refusal::NotHere),
        }
    }

    /// What *Run check* runs: the active project on this machine and its
    /// check command; refused, and said, when there is no project, the one
    /// selected is not on this machine, or it has no check command.
    #[must_use = "a refusal is to be said"]
    pub fn check_target(&self) -> (Result<(PathBuf, String), Refusal>, Vec<Effect>) {
        let Some(root) = self.workspace.active_root() else {
            return refused(Refusal::NoProject);
        };
        match (root.local_path(), &root.check) {
            (Some(path), Some(check)) => (Ok((path.to_path_buf(), check.clone())), Vec::new()),
            (Some(_), None) => refused(Refusal::NoCheck),
            (None, _) => refused(Refusal::NotHere),
        }
    }

    /// Add `dir`, a folder here of `project`, and select it, or select it if
    /// it is in the workspace already: what the folder picker, a second
    /// launch and a worktree split off do.
    /// A folder whose project is not known yet comes in at once, of none,
    /// and is said to be placed.
    #[must_use = "its effects are to be carried"]
    pub fn open_project(&mut self, dir: PathBuf, project: Option<&Project>) -> Vec<Effect> {
        let dir = normalize_root(dir);
        let mut fx = self.told_first(project);
        // A folder of a row already here selects that row, which the person
        // keeps from now on though only a worker or a run brought it.
        if let Some(idx) = self.local_index(&dir) {
            self.workspace.select_root(idx);
            self.workspace.roots[idx].transient = false;
            fx.extend(self.opened());
            return fx;
        }
        let root = row(None, dir, project);
        if self.workspace.clashes(&root) {
            fx.push(Effect::Signal(Signal::Refused(Refusal::FolderTaken)));
            return fx;
        }
        let idx = self.workspace.add_placed(root);
        fx.extend(self.unplaced(idx, true));
        fx.extend(self.opened());
        fx
    }

    /// `project` as the caller now has it, told its rows first: a worktree
    /// just recorded is a folder of its row before the row is looked for.
    fn told_first(&mut self, project: Option<&Project>) -> Vec<Effect> {
        project.map_or_else(Vec::new, |project| self.told(project))
    }

    /// [`Self::open_project`] for `project`'s own folder, on its host.
    #[must_use = "its effects are to be carried"]
    pub fn open_host_project(&mut self, project: &Project) -> Vec<Effect> {
        let root = row(project.host.as_ref(), project.path.clone(), Some(project));
        if self.workspace.clashes(&root) {
            return vec![Effect::Signal(Signal::Refused(Refusal::FolderTaken))];
        }
        self.workspace.add_placed(root);
        self.opened()
    }

    /// Say the row at `idx` is to be placed in a project, when it was just
    /// added of none. A worker's folder is not: it leaves with the worker.
    fn unplaced(&self, idx: usize, fresh: bool) -> Vec<Effect> {
        match self.workspace.roots.get(idx) {
            Some(root) if fresh && root.project.is_none() && !root.transient => {
                vec![Effect::Unplaced(root.path.clone())]
            }
            Some(_) | None => Vec::new(),
        }
    }

    fn opened(&mut self) -> Vec<Effect> {
        let mut fx = vec![Effect::Show, Effect::Resweep];
        fx.extend(self.save());
        fx.push(Effect::Changed);
        fx
    }

    /// Keep `dir`, a folder here, as a project for work about to start in it,
    /// selected but not yet shown: added when the window does not have it.
    /// Answers its index; refused, and said, when a project on a host has
    /// the folder.
    #[must_use = "its effects are to be carried"]
    pub fn keep_project(
        &mut self,
        dir: PathBuf,
        project: Option<&Project>,
    ) -> (Result<usize, Refusal>, Vec<Effect>) {
        let dir = normalize_root(dir);
        let mut fx = self.told_first(project);
        if let Some(idx) = self.local_index(&dir) {
            return (Ok(idx), fx);
        }
        let root = row(None, dir, project);
        if self.workspace.clashes(&root) {
            fx.push(Effect::Signal(Signal::Refused(Refusal::FolderTaken)));
            return (Err(Refusal::FolderTaken), fx);
        }
        let idx = self.workspace.add_placed(root);
        fx.extend(self.unplaced(idx, true));
        fx.extend(self.kept());
        (Ok(idx), fx)
    }

    /// [`Self::keep_project`], selecting nothing: a worktree cut for a task
    /// nobody here started must not move what the person has selected.
    #[must_use = "its effects are to be carried"]
    pub fn keep_project_quietly(&mut self, dir: PathBuf, project: Option<&Project>) -> Vec<Effect> {
        let dir = normalize_root(dir);
        let mut fx = self.told_first(project);
        if self.local_index(&dir).is_some() {
            return fx;
        }
        let root = row(None, dir, project);
        if self.workspace.clashes(&root) {
            fx.push(Effect::Signal(Signal::Refused(Refusal::FolderTaken)));
            return fx;
        }
        let idx = self.workspace.add_placed_quietly(root);
        fx.extend(self.unplaced(idx, true));
        fx.extend(self.kept());
        fx
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

    /// The row of `path` on `host`, which named no project, is of `project`:
    /// it takes the project's facts, and the workspace file is written to
    /// name it. A worktree's folder folds into its project's row, when the
    /// window has one, its sessions keeping their folder; else the row
    /// becomes the project's, at its own folder. A row already of a project,
    /// or a folder `project` does not hold, changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn placed(
        &mut self,
        host: Option<&HostName>,
        path: &Path,
        project: &Project,
    ) -> Vec<Effect> {
        if project.folder(host, path).is_none() {
            return Vec::new();
        }
        let ws = &mut self.workspace;
        let Some(bare) = ws.roots.iter().position(|root| {
            root.host.as_ref() == host && root.path == path && root.project.is_none()
        }) else {
            return Vec::new();
        };
        let mut fx = Vec::new();
        let into = ws
            .roots
            .iter()
            .position(|root| root.project.as_ref() == Some(&project.id));
        match into {
            Some(into) => {
                let shown = ws.active_root == bare;
                let gone = &mut ws.roots[bare];
                let (sessions, active, pinned) = (
                    std::mem::take(&mut gone.sessions),
                    gone.active_session,
                    gone.pinned,
                );
                let target = &mut ws.roots[into];
                let first = target.sessions.len();
                let moved = !sessions.is_empty();
                target.sessions.extend(sessions);
                target.pinned |= pinned;
                if shown && moved {
                    // The bare row's page on screen stays the page.
                    target.active_session = active.map(|active| first + active);
                } else if first == 0 && moved && ws.active_root != into {
                    target.active_session = Some(0);
                }
                ws.remove_root(bare);
                if shown {
                    ws.select_root(if into > bare { into - 1 } else { into });
                    fx.push(Effect::Show);
                }
            }
            // Its folders change: the project's own and its worktrees come
            // with it, to be shown and read.
            None => {
                let shown = ws.active_root == bare;
                let root = &mut ws.roots[bare];
                let moved = root.path != project.path;
                root.path = project.path.clone();
                root.project = Some(project.id.clone());
                root.told(project);
                if shown && moved {
                    fx.push(Effect::Show);
                }
                if moved || !root.worktrees.is_empty() {
                    fx.push(Effect::Resweep);
                }
            }
        }
        fx.extend(self.save());
        fx.push(Effect::Changed);
        fx
    }

    /// `project` changed: every row of it draws it as it now is, its folders
    /// told again. A folder whose offload changed has its sessions closed,
    /// since they ran where it no longer does. Nothing here is written: the facts are the project's.
    #[must_use = "its effects are to be carried"]
    pub fn told(&mut self, project: &Project) -> Vec<Effect> {
        let mut fx = Vec::new();
        let mut changed = false;
        let mut moved = Vec::new();
        let mut dropped = Vec::new();
        for root in &mut self.workspace.roots {
            if root.project.as_ref() != Some(&project.id) {
                continue;
            }
            let before = (
                root.label.clone(),
                root.check.clone(),
                root.builds.clone(),
                root.offload.clone(),
                root.worktrees.clone(),
            );
            root.told(project);
            // A folder whose offload changed: its sessions ran where it no
            // longer does. One the project no longer records keeps running.
            if root.offload != before.3 {
                moved.push(root.path.clone());
            }
            // A worktree the record dropped (forgotten, removed) that no
            // session runs in is let go of, as a removed row's folders are.
            if root.host.is_none() {
                for gone in before.4.iter().filter(|w| !root.holds(&w.path)) {
                    if !root.sessions.iter().any(|s| s.folder == gone.path) {
                        dropped.push(gone.path.clone());
                    }
                }
            }
            for worktree in &root.worktrees {
                let was = before.4.iter().find(|w| w.path == worktree.path);
                if was.and_then(|w| w.offload.as_ref()) != worktree.offload.as_ref() {
                    moved.push(worktree.path.clone());
                }
            }
            changed |= (
                &root.label,
                &root.check,
                &root.builds,
                &root.offload,
                &root.worktrees,
            ) != (&before.0, &before.1, &before.2, &before.3, &before.4);
        }
        for dir in moved {
            fx.extend(self.drop_sessions_in(&dir));
        }
        for path in dropped {
            self.git.remove(&path);
            fx.push(Effect::ForgotHere(path.clone()));
            fx.push(Effect::Forgot(super::Forgotten { path, host: None }));
        }
        if changed {
            fx.push(Effect::Changed);
        }
        fx
    }
}
