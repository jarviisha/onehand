//! Which lead each worker has, for the person's view of them, and a worker
//! given back to its lead. The relation is the tools' drive's; this asks the
//! windows whether a lead is still open.

use super::super::drive;
use super::session_here_of_lead;
use gpui::App;

pub(crate) use onehand_core::control::workers::Worker;

/// The lead of session `uid`, while that is a worker.
pub(crate) fn lead_of(uid: u64) -> Option<u64> {
    drive().lead_of(uid)
}

/// Lead `lead`'s workers, opening or open, oldest first.
pub(crate) fn workers_of(lead: u64) -> Vec<Worker> {
    drive().workers_of(lead)
}

/// Whether *Give back to the lead* is offered for worker `uid`, which has
/// sent `prompts_sent` prompts.
pub(crate) fn give_back_offered(uid: u64, prompts_sent: usize, cx: &App) -> bool {
    let open = session_here_of_lead(uid, cx);
    drive().give_back_offered(uid, prompts_sent, open)
}

/// Give worker `uid` back to its lead: what the person sent so far is
/// counted as the lead's. Nothing is sent, nothing said. False when it is
/// no longer offered by the time it is pressed.
pub(crate) fn give_back(uid: u64, prompts_sent: usize, cx: &App) -> bool {
    let open = session_here_of_lead(uid, cx);
    drive().give_back(uid, prompts_sent, open)
}
