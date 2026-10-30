//! Which lead each worker has, for the person's view of them, and a worker
//! given back to its lead.
//!
//! **A worker is a worker from the moment its session starts**, not only
//! once its relation is written: an open in flight shows its session
//! connecting under its lead, never in the rail for the seconds it takes.
//!
//! Each call copies what it needs out of the shared standing and lets go of
//! it before anything else is read, since the socket's runtime takes it too.

use super::super::{Sessions, sessions};
use super::requests::Relation;
use gpui::App;

/// One of a lead's workers, as the person's view draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Worker {
    pub(crate) uid: u64,
    /// The last turn the lead sent it.
    pub(crate) turn: Option<u64>,
    /// Its prompts sent once the lead's last went; `None` while it is still
    /// opening, before any relation.
    pub(crate) sent: Option<usize>,
}

/// The lead of session `uid`, while that is a worker.
pub(crate) fn lead_of(uid: u64) -> Option<u64> {
    lead_in(&sessions(), uid)
}

/// Lead `lead`'s workers, opening or open, oldest first.
pub(crate) fn workers_of(lead: u64) -> Vec<Worker> {
    workers_in(&sessions(), lead)
}

/// Whether *Give back to the lead* is offered for worker `uid`, which has
/// sent `prompts_sent` prompts.
pub(crate) fn give_back_offered(uid: u64, prompts_sent: usize, cx: &App) -> bool {
    let (relation, on) = {
        let sessions = sessions();
        let relation = sessions.workers.get(&uid).cloned();
        let on = relation
            .as_ref()
            .is_some_and(|relation| sessions.on.contains(&relation.lead));
        (relation, on)
    };
    let open = relation
        .as_ref()
        .is_some_and(|relation| lead_open(relation.lead, cx));
    offered(relation.as_ref(), prompts_sent, on, open)
}

/// Give worker `uid` back to its lead: what the person sent so far is
/// counted as the lead's. Nothing is sent, nothing said. False when it is
/// no longer offered by the time it is pressed.
pub(crate) fn give_back(uid: u64, prompts_sent: usize, cx: &App) -> bool {
    if !give_back_offered(uid, prompts_sent, cx) {
        return false;
    }
    match sessions().workers.get_mut(&uid) {
        Some(relation) => {
            relation.sent = prompts_sent;
            true
        }
        None => false,
    }
}

/// Whether `lead` is still a session of some window.
fn lead_open(lead: u64, cx: &App) -> bool {
    crate::state::Shared::global(cx)
        .windows
        .iter()
        .filter_map(|window| window.shell.upgrade())
        .any(|shell| shell.read(cx).session_project(lead).is_some())
}

pub(in crate::control) fn lead_in(sessions: &Sessions, uid: u64) -> Option<u64> {
    sessions
        .workers
        .get(&uid)
        .map(|relation| relation.lead)
        .or_else(|| sessions.requests.lead_opening(uid))
}

pub(in crate::control) fn workers_in(sessions: &Sessions, lead: u64) -> Vec<Worker> {
    let open = sessions
        .workers
        .iter()
        .filter(|(_, relation)| relation.lead == lead)
        .map(|(&uid, relation)| Worker {
            uid,
            turn: relation.turn,
            sent: Some(relation.sent),
        });
    let opening = sessions
        .requests
        .opening_of(lead)
        .into_iter()
        .filter(|uid| !sessions.workers.contains_key(uid))
        .map(|uid| Worker {
            uid,
            turn: None,
            sent: None,
        });
    let mut workers: Vec<Worker> = open.chain(opening).collect();
    workers.sort_by_key(|worker| worker.uid);
    workers
}

/// Offered while the person holds the worker and its lead is open and
/// switched on; with no such lead the worker stays the person's.
pub(in crate::control) fn offered(
    relation: Option<&Relation>,
    prompts_sent: usize,
    lead_on: bool,
    lead_open: bool,
) -> bool {
    relation.is_some_and(|relation| prompts_sent > relation.sent) && lead_on && lead_open
}
