//! A lead's relation with its workers, as the person's view and the lead's
//! calls read it: which lead each worker has, what the lead last sent it,
//! and whether the person holds it. Written by the open once its first
//! prompt went, moved on by each turn the lead hands over, and given back
//! by the person.

use super::super::state::Held;
use super::super::turns::Last;
use super::super::workers::{offered, whose, Whose};
use super::{ControlDrive, Found, Worker};
use crate::chat::drive::Intake;

impl ControlDrive {
    /// The lead of session `uid`, while that is a worker: from the moment its
    /// session starts, not only once its relation is written.
    pub fn lead_of(&self, uid: u64) -> Option<u64> {
        self.workers
            .get(&uid)
            .map(|relation| relation.lead)
            .or_else(|| self.requests.lead_opening(uid))
    }

    /// Lead `lead`'s workers, opening or open, oldest first.
    pub fn workers_of(&self, lead: u64) -> Vec<Worker> {
        let open = self
            .workers
            .iter()
            .filter(|(_, relation)| relation.lead == lead)
            .map(|(&uid, relation)| Worker {
                uid,
                turn: relation.turn,
                sent: Some(relation.sent),
            });
        let opening = self
            .requests
            .opening_of(lead)
            .into_iter()
            .filter(|uid| !self.workers.contains_key(uid))
            .map(|uid| Worker {
                uid,
                turn: None,
                sent: None,
            });
        let mut workers: Vec<Worker> = open.chain(opening).collect();
        workers.sort_by_key(|worker| worker.uid);
        workers
    }

    /// Whether *Give back to the lead* is offered for worker `uid`, which has
    /// sent `prompts_sent` prompts, `lead_open` saying its lead is still a
    /// session of some window.
    pub fn give_back_offered(&self, uid: u64, prompts_sent: usize, lead_open: bool) -> bool {
        let relation = self.workers.get(&uid);
        let on = relation.is_some_and(|relation| self.on.contains(&relation.lead));
        offered(relation, prompts_sent, on, lead_open)
    }

    /// Give worker `uid` back to its lead: what the person sent so far counts
    /// as the lead's. False when it is no longer offered.
    pub fn give_back(&mut self, uid: u64, prompts_sent: usize, lead_open: bool) -> bool {
        if !self.give_back_offered(uid, prompts_sent, lead_open) {
            return false;
        }
        if let Some(relation) = self.workers.get_mut(&uid) {
            relation.sent = prompts_sent;
        }
        true
    }

    // A lead's calls on its workers.

    /// The caller's worker `handle` names, `open` saying whether a window
    /// holds its session; anything that is not the caller's own worker, open
    /// or closed, gets the same refusal.
    pub fn find(&self, caller: u64, uid: u64, handle: &str, open: bool) -> Result<Found, String> {
        let relation = self.workers.get(&uid);
        let closed = self.closed_workers.get(&uid).copied();
        match whose(relation, closed, caller, open) {
            Whose::Mine if open => Ok(Found::Open { uid }),
            Whose::Mine | Whose::Closed => Ok(Found::Closed),
            Whose::NotYours => Err(format!("{handle} is not a worker you opened")),
        }
    }

    /// What the lead last sent worker `uid`, for `turns`' rules.
    pub fn last(&self, uid: u64) -> Option<Last> {
        self.workers.get(&uid).map(|relation| Last {
            turn: relation.turn,
            text: relation.text.clone(),
            sent: relation.sent,
        })
    }

    /// What `session_state` reads of worker `uid`'s relation.
    pub fn held(&self, uid: u64) -> Option<Held> {
        self.workers.get(&uid).map(|relation| Held {
            turn: relation.turn,
            sent: relation.sent,
            dirs: relation.dirs.clone(),
        })
    }

    /// The lead's turn `turn`, `text`, was handed to worker `uid` and came
    /// to `intake`, the worker having sent `sent` prompts since: what the
    /// lead is answered. Only a prompt the agent took moves the relation on.
    pub fn sent(
        &mut self,
        uid: u64,
        turn: u64,
        text: &str,
        intake: &Intake,
        sent: usize,
    ) -> Result<String, String> {
        if *intake != Intake::Sent {
            return Err("the prompt was not accepted".to_string());
        }
        if let Some(relation) = self.workers.get_mut(&uid) {
            relation.turn = Some(turn);
            relation.text = Some(text.to_string());
            relation.sent = sent;
        }
        Ok(format!("turn {turn} accepted"))
    }
}
