//! Each project's `git status`, as the window last read it: which projects a
//! sweep reads, and which sweep's answer is kept.

use super::{Effect, WorkspaceDrive};
use crate::gitstat::GitStatus;
use crate::workspace::{HostName, ProjectRoot, Workspace};
use std::collections::HashMap;
use std::path::PathBuf;

/// One project a sweep reads: its folder, and the host it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepRoot {
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
            .map(|root| SweepRoot {
                path: root.path.clone(),
                host: root.host.clone(),
            })
            .collect();
        Effect::Sweep {
            token: self.swept,
            roots,
        }
    }

    /// What sweep `token` found. An older sweep landing after a newer one
    /// started is dropped: `git status` on a big repository does not finish
    /// in the order asked.
    pub(super) fn swept(&mut self, token: u64, found: Vec<(PathBuf, GitStatus)>) -> Vec<Effect> {
        if token != self.swept {
            return Vec::new();
        }
        self.git = found.into_iter().collect();
        vec![Effect::GitRead]
    }
}
