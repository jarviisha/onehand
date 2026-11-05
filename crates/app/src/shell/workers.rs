//! Where a session is in the workspace, and where a worker stands: under its
//! lead's project, whose row in the rail it is drawn in, while that lead is a
//! session of this window.

use super::Shell;

/// The project a session stands under, from where it is (`own`) and where
/// its lead is in this window (`lead`), each a project and a place.
pub(super) fn home(own: Option<(usize, usize)>, lead: Option<(usize, usize)>) -> Option<usize> {
    lead.or(own).map(|(root, _)| root)
}

impl Shell {
    /// Where session `uid` is in the workspace: its project and its place.
    pub(super) fn locate(&self, uid: u64) -> Option<(usize, usize)> {
        self.window
            .workspace
            .roots
            .iter()
            .enumerate()
            .find_map(|(ri, root)| {
                root.sessions
                    .iter()
                    .position(|session| session.uid == uid)
                    .map(|si| (ri, si))
            })
    }

    /// The project session `uid` stands under in the rail: its lead's for a
    /// worker whose lead is in this window, else its own. A worker's own is
    /// mostly a worktree kept for it alone, which nothing else belongs in.
    pub(crate) fn home_of(&self, uid: u64) -> Option<usize> {
        let lead = crate::control::lead_of(uid).and_then(|lead| self.locate(lead));
        home(self.locate(uid), lead)
    }
}

#[cfg(test)]
mod tests;
