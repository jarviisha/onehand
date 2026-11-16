//! A lead's relation with its workers, as the person's view and the lead's
//! calls read it: which lead each worker has, what the lead last sent it,
//! and whether the person holds it. Written by the open once its first
//! prompt went, moved on by each turn the lead hands over, and given back
//! by the person.
//!
//! **What a lead holds is kept beside its conversation.** Every change of
//! it answers [`Effect::Keep`] with all of it, so a lead whose conversation
//! is opened again after a restart takes its workers back, and the handles
//! of those that ended still read closed.
//!
//! **The person's hold is kept as such.** A worker's prompts sent count
//! from nothing again on a resume, so the count alone would lose it: the
//! person prompting a worker marks it held ([`Input::Conversation`]), and
//! only the lead's own accepted turn or the person giving it back clears
//! the mark.

use super::super::kept::{Kept, KeptClosed, KeptWorker};
use super::super::open::{parse_handle, Handle};
use super::super::state::Held;
use super::super::turns::Last;
use super::super::workers::{offered, whose, Whose};
use super::{this_run, Closed, ControlDrive, Effect, Found, Worker};
use crate::chat::drive::Intake;
use std::path::PathBuf;

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
                person: relation.person,
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
                person: false,
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
    /// as the lead's. `None` when it is no longer offered.
    #[must_use = "its effects are to be carried"]
    pub fn give_back(
        &mut self,
        uid: u64,
        prompts_sent: usize,
        lead_open: bool,
    ) -> Option<Vec<Effect>> {
        if !self.give_back_offered(uid, prompts_sent, lead_open) {
            return None;
        }
        let relation = self.workers.get_mut(&uid)?;
        relation.sent = prompts_sent;
        relation.person = false;
        let lead = relation.lead;
        Some(vec![self.keep(lead)])
    }

    // A lead's calls on its workers.

    /// The session `handle` names while it is open.
    pub fn uid_of(&self, handle: &str) -> Option<u64> {
        self.handles.get(handle).copied()
    }

    /// The handle of worker `uid`, from the moment its session starts.
    pub fn handle_of(&self, uid: u64) -> Option<String> {
        self.handles
            .iter()
            .find(|(_, worker)| **worker == uid)
            .map(|(handle, _)| handle.clone())
    }

    /// The caller's worker `handle` names, `open` saying whether a window
    /// holds a session; anything that is not the caller's own worker, open
    /// or closed, gets the same refusal.
    pub fn find(
        &self,
        caller: u64,
        handle: &str,
        open: impl Fn(u64) -> bool,
    ) -> Result<Found, String> {
        let Handle::Stable(stable) = parse_handle(handle, &this_run())?;
        let (uid, whose) = match self.handles.get(&stable) {
            Some(&uid) => (uid, whose(self.workers.get(&uid), None, caller, open(uid))),
            None => {
                let closed = self.closed_workers.get(&stable).map(|closed| closed.lead);
                (0, whose(None, closed, caller, false))
            }
        };
        match whose {
            Whose::Mine => Ok(Found::Open { uid }),
            Whose::Closed => Ok(Found::Closed),
            Whose::NotYours => Err(format!("{} is not a worker you opened", handle.trim())),
        }
    }

    /// What the lead last sent worker `uid`, for `turns`' rules.
    pub fn last(&self, uid: u64) -> Option<Last> {
        self.workers.get(&uid).map(|relation| Last {
            turn: relation.turn,
            text: relation.text.clone(),
            sent: relation.sent,
            person: relation.person,
        })
    }

    /// What `session_state` reads of worker `uid`'s relation.
    pub fn held(&self, uid: u64) -> Option<Held> {
        self.workers.get(&uid).map(|relation| Held {
            turn: relation.turn,
            sent: relation.sent,
            dirs: relation.dirs.clone(),
            person: relation.person,
        })
    }

    /// The lead's turn `turn`, `text`, was handed to worker `uid` and came
    /// to `intake`, the worker having sent `sent` prompts since: what the
    /// lead is answered. Only a prompt the agent took moves the relation on.
    ///
    /// It clears the person's hold: a turn goes only while the lead holds
    /// the worker, and its own prompt, counted before this, may have been
    /// read as the person's meanwhile.
    #[must_use = "its effects are to be carried"]
    pub fn sent(
        &mut self,
        uid: u64,
        turn: u64,
        text: &str,
        intake: &Intake,
        sent: usize,
    ) -> (Result<String, String>, Vec<Effect>) {
        if *intake != Intake::Sent {
            return (Err("the prompt was not accepted".to_string()), Vec::new());
        }
        let mut fx = Vec::new();
        if let Some(relation) = self.workers.get_mut(&uid) {
            relation.turn = Some(turn);
            relation.text = Some(text.to_string());
            relation.sent = sent;
            relation.person = false;
            let lead = relation.lead;
            fx.push(self.keep(lead));
        }
        (Ok(format!("turn {turn} accepted")), fx)
    }

    // What is kept beside a lead's conversation.

    /// Session `uid`'s conversation is `session_id`, and it has sent
    /// `prompts_sent` prompts: a worker's is kept, and a prompt of the
    /// person's since the lead's last turn marks it held; a lead's conversation
    /// moving takes what it keeps with it.
    pub(super) fn conversation(
        &mut self,
        uid: u64,
        session_id: String,
        prompts_sent: usize,
    ) -> Vec<Effect> {
        let moved = self.conversations.get(&uid) != Some(&session_id);
        self.conversations.insert(uid, session_id.clone());
        let mut fx = Vec::new();
        if let Some(relation) = self.workers.get_mut(&uid) {
            let mut changed = false;
            if relation.conversation.as_ref() != Some(&session_id) {
                relation.conversation = Some(session_id);
                changed = true;
            }
            if !relation.person && prompts_sent > relation.sent {
                relation.person = true;
                changed = true;
            }
            if changed {
                let lead = relation.lead;
                fx.push(self.keep(lead));
            }
        }
        if moved && self.keeps(uid) {
            fx.push(self.keep(uid));
        }
        fx
    }

    /// Whether `lead` keeps anything: a worker, or a worker's end.
    pub(super) fn keeps(&self, lead: u64) -> bool {
        self.workers.values().any(|relation| relation.lead == lead)
            || self
                .closed_workers
                .values()
                .any(|closed| closed.lead == lead)
    }

    /// Everything `lead` keeps, to be written beside its conversation: its
    /// workers whose conversation is known, in the order they opened, and
    /// the handles of those that ended.
    pub(super) fn keep(&self, lead: u64) -> Effect {
        let mut held: Vec<_> = self
            .workers
            .values()
            .filter(|relation| relation.lead == lead)
            .filter_map(|relation| {
                let worker = KeptWorker {
                    handle: relation.handle.clone(),
                    conversation: relation.conversation.clone()?,
                    agent: relation.agent.clone(),
                    folder: relation.folder.clone(),
                    dirs: relation.dirs.clone(),
                    name: relation.name.clone(),
                    turn: relation.turn,
                    text: relation.text.clone(),
                    person: relation.person,
                };
                Some((relation.seq, worker))
            })
            .collect();
        held.sort_by_key(|(seq, _)| *seq);
        let mut closed: Vec<KeptClosed> = self
            .closed_workers
            .iter()
            .filter(|(_, closed)| closed.lead == lead)
            .map(|(handle, closed)| KeptClosed {
                handle: handle.clone(),
                why: closed.why.clone(),
            })
            .collect();
        closed.sort_by(|a, b| a.handle.cmp(&b.handle));
        Effect::Keep {
            lead,
            kept: Kept {
                workers: held.into_iter().map(|(_, worker)| worker).collect(),
                closed,
            },
        }
    }

    /// Lead `lead`'s conversation was opened again, keeping `kept`, the
    /// window's projects being `projects`: the handles of the workers that
    /// ended read closed to it from now on.
    pub fn back(
        &mut self,
        lead: u64,
        kept: Kept,
        projects: &[(String, PathBuf)],
    ) -> Vec<super::Begun> {
        let _ = projects;
        for closed in kept.closed {
            let why = closed.why;
            self.closed_workers
                .insert(closed.handle, Closed { lead, why });
        }
        Vec::new()
    }
}
