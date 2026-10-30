//! A lead's workers as the window shows them: never on their own in the
//! agent pane while their lead is open, but through the lead, in the
//! workers' region.

use super::Shell;

/// Where arriving at a session lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arrival {
    /// The session itself.
    Show,
    /// Its lead, with the region at it: a worker's transcript is drawn in
    /// one place only.
    Lead { lead: u64, worker: u64 },
}

/// Where arriving at session `uid` lands, `lead` its lead when it is a
/// worker and `lead_here` whether that lead is a session of this window.
pub(super) fn arrival(uid: u64, lead: Option<u64>, lead_here: bool) -> Arrival {
    match lead {
        Some(lead) if lead_here => Arrival::Lead { lead, worker: uid },
        Some(_) | None => Arrival::Show,
    }
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

    /// With the active session a worker of a lead in this window, select the
    /// lead instead, and say which worker was asked for. Every way to a
    /// session ends at the arrival, so this is the one place it is turned.
    pub(super) fn turn_to_lead(&mut self) -> Option<u64> {
        let uid = self.window.workspace.active_root()?.active_session()?.uid;
        let lead = crate::control::lead_of(uid);
        let place = lead.and_then(|lead| self.locate(lead));
        match arrival(uid, lead, place.is_some()) {
            Arrival::Show => None,
            Arrival::Lead { worker, .. } => {
                let (root, session) = place?;
                self.window.workspace.select_root(root);
                self.window.workspace.select_session(session);
                Some(worker)
            }
        }
    }
}

#[cfg(test)]
mod tests;
