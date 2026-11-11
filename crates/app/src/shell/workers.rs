//! Where a session is in the workspace, and where a worker stands: under its
//! lead's project, whose row in the rail it is drawn in, while that lead is a
//! session of this window.

use super::Shell;

impl Shell {
    /// Where session `uid` is in the workspace: its project and its place.
    pub(super) fn locate(&self, uid: u64) -> Option<(usize, usize)> {
        self.window.workspace.locate(uid)
    }

    /// The project session `uid` stands under in the rail: its lead's for a
    /// worker whose lead is in this window, else its own. A worker's own is
    /// mostly a worktree kept for it alone, which nothing else belongs in.
    pub(crate) fn home_of(&self, uid: u64) -> Option<usize> {
        self.window
            .workspace
            .home_of(uid, crate::control::lead_of(uid))
    }
}
