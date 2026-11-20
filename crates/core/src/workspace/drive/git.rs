//! Each project's `git status`, as the window last read it: which projects a
//! sweep reads, and which sweep's answer is kept.

use super::{Effect, WorkspaceDrive};
use crate::gitstat::GitStatus;
use crate::workspace::{HostName, ProjectRoot, Workspace};
use std::collections::HashMap;
use std::path::PathBuf;

/// One project a sweep reads: its folder, and the host it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepProject {
    pub path: PathBuf,
    pub host: Option<HostName>,
}

impl Workspace {
    /// Whether `root` is a host's folder on a path a folder here also has.
    /// What is read of a project's files is kept by path alone, so such a
    /// folder is given none: what is kept under that path is the local one's.
    pub fn shares_local_path(&self, root: &ProjectRoot) -> bool {
        root.on_host().is_some()
            && self
                .roots
                .iter()
                .any(|r| r.local_path() == Some(root.path.as_path()))
    }
}

impl WorkspaceDrive {
    /// Every project's `git status`, by path, as the last sweep kept it. A
    /// folder that is not a repository has no entry.
    pub fn git(&self) -> &HashMap<PathBuf, GitStatus> {
        &self.git
    }

    /// `root`'s `git status`; none for a host's folder on a path a folder here
    /// also has.
    pub fn git_of(&self, root: &ProjectRoot) -> Option<&GitStatus> {
        match self.workspace.shares_local_path(root) {
            true => None,
            false => self.git.get(&root.path),
        }
    }

    /// The `git status` of the folder `root` shows: its session on screen's,
    /// a worktree's on its own branch, else the row's own.
    pub fn git_on_screen(&self, root: &ProjectRoot) -> Option<&GitStatus> {
        match root.folder_on_screen() {
            folder if folder == root.path => self.git_of(root),
            folder => self.git.get(folder),
        }
    }

    /// Read every project's `git status` again: those here, and those on a
    /// host `attached` says is attached, which are never attached for a
    /// sweep; never a host's folder on a local one's path.
    #[must_use = "its effects are to be carried"]
    pub fn sweep(&mut self, attached: impl Fn(&HostName) -> bool) -> Effect {
        self.swept += 1;
        let roots = self
            .workspace
            .roots
            .iter()
            .filter(|root| match root.on_host() {
                None => true,
                Some(_) if self.workspace.shares_local_path(root) => false,
                Some(host) => attached(host),
            })
            // The row's own folder, and each other one a session of it runs
            // in: what the git badges show is the folder on screen's.
            .flat_map(|root| {
                let mut folders = vec![root.path.clone()];
                for session in &root.sessions {
                    if !folders.contains(&session.folder) {
                        folders.push(session.folder.clone());
                    }
                }
                folders.into_iter().map(|path| SweepProject {
                    path,
                    host: root.host.clone(),
                })
            })
            .collect::<Vec<_>>();
        self.sweeping = roots.clone();
        Effect::Sweep {
            token: self.swept,
            roots,
        }
    }

    /// What sweep `token` found. An older sweep landing after a newer one
    /// started is dropped: `git status` on a big repository does not finish
    /// in the order asked. What it read of a project that left while it ran
    /// is dropped too, by host and path, rather than kept for a project no
    /// longer here or shown as another's of the same path.
    pub(super) fn swept(&mut self, token: u64, found: Vec<(PathBuf, GitStatus)>) -> Vec<Effect> {
        if token != self.swept {
            return Vec::new();
        }
        let read = std::mem::take(&mut self.sweeping);
        let ws = &self.workspace;
        let still = |path: &PathBuf| {
            read.iter().filter(|at| at.path == *path).any(|at| {
                ws.roots
                    .iter()
                    .filter(|r| r.host == at.host)
                    .any(|r| r.path == at.path || r.sessions.iter().any(|s| s.folder == at.path))
            })
        };
        self.git = found.into_iter().filter(|(path, _)| still(path)).collect();
        vec![Effect::GitRead]
    }
}
