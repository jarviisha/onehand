//! A lead the person closed alone, and its workers waiting for it.
//!
//! **Closing a lead releases its workers rather than ending them.** They
//! stay open, listed under their own projects and the person's to work
//! with, but their relation stays the closed lead's, unusable, and is still
//! kept beside its conversation: so a lead closed by accident takes them back
//! when its conversation is opened again ([`ControlDrive::take_back`]), in
//! this run or a later one, in whatever window each is open. Only the person
//! closing a worker, or the lead's `close_worker`, ends one for good.
//!
//! **A lead with no conversation on disk yet releases nothing**: there is no
//! file to take its workers back from, so they end with it as before.
//!
//! **A worker still coming back is parked**, not failed: its open is
//! dropped, but it stays in the file, to come back with the lead next time.
//! So is a released worker whose window closed: the file already holds it.

use super::super::kept::{Kept, KeptWorker};
use super::relations::kept_worker;
use super::{ControlDrive, Effect, Key, Phase};
use std::collections::HashSet;

impl ControlDrive {
    /// The opens of workers coming back to `lead`.
    pub(super) fn coming_back_of(&self, lead: u64) -> Vec<Key> {
        self.pending
            .iter()
            .filter(|((of, _), pending)| *of == lead && pending.open.back.is_some())
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Whether `lead` holds a worker: open, coming back, or parked.
    fn holds_workers(&self, lead: u64) -> bool {
        self.workers.values().any(|relation| relation.lead == lead)
            || !self.coming_back_of(lead).is_empty()
            || self
                .released
                .get(&lead)
                .is_some_and(|parked| !parked.is_empty())
    }

    /// Whether closing `lead` releases its workers rather than ending them:
    /// it holds some, and its conversation is kept in a folder.
    pub(super) fn would_release(&self, lead: u64) -> bool {
        self.holds_workers(lead) && self.kept_in.contains_key(&lead)
    }

    /// Whether `lead` was closed alone, its workers released.
    pub(super) fn is_released(&self, lead: u64) -> bool {
        self.released.contains_key(&lead)
    }

    /// Release `lead`'s workers: each one still coming back is parked, kept
    /// for the next time its conversation opens rather than ended.
    pub(super) fn release_workers(&mut self, lead: u64) {
        let mut parked = Vec::new();
        for key in self.coming_back_of(lead) {
            let Some(pending) = self.pending.remove(&key) else {
                continue;
            };
            self.requests.failed(key.0, &key.1);
            // Its session may still be starting: the late start is closed.
            if let Phase::Starting {
                root,
                added,
                folder,
                ..
            } = &pending.phase
            {
                self.abandoned.insert(key.clone(), (root.clone(), *added));
                self.reserved.release(folder);
            }
            parked.extend(pending.open.back.map(|back| (pending.token, back)));
        }
        self.released.insert(lead, parked);
    }

    /// What is written for `lead` once one of its workers ended: the last
    /// write, and all of it forgotten, for a released lead left holding
    /// nothing; else the usual one.
    pub(super) fn keep_after_worker_ended(&mut self, lead: u64) -> Option<Effect> {
        if !self.is_released(lead) || self.holds_workers(lead) {
            return self.keep(lead);
        }
        let last = self.keep_last(lead);
        self.forget_released(lead);
        last
    }

    /// Forget everything of released `lead`, writing nothing.
    fn forget_released(&mut self, lead: u64) {
        self.released.remove(&lead);
        self.closed_workers.retain(|_, closed| closed.lead != lead);
        self.kept_in.remove(&lead);
        self.written.remove(&lead);
    }

    /// The workers parked for `lead`, to be written with the ones it holds.
    pub(super) fn parked_of(&self, lead: u64) -> Vec<(u64, KeptWorker)> {
        self.released.get(&lead).cloned().unwrap_or_default()
    }

    /// The window holding sessions `uids` closed: a released worker among
    /// them is parked, as the file already has it, to come back with its
    /// lead rather than be taken back as a session that is gone. Nothing is
    /// written: the file holds it already.
    pub(super) fn released_window_closed(&mut self, uids: &[u64]) {
        let gone: Vec<(u64, u64)> = self
            .workers
            .iter()
            .filter(|(uid, relation)| uids.contains(uid) && self.is_released(relation.lead))
            .map(|(&uid, relation)| (uid, relation.lead))
            .collect();
        for (uid, lead) in gone {
            let Some(relation) = self.workers.remove(&uid) else {
                continue;
            };
            self.handles.remove(&relation.handle);
            if let Some(back) = kept_worker(&relation) {
                let parked = self.released.entry(lead).or_default();
                parked.push((relation.seq, back));
                parked.sort_by_key(|(seq, _)| *seq);
            }
        }
    }

    /// Lead `lead`'s conversation was opened again, keeping `kept`: every
    /// released worker it names that is still open is `lead`'s again, as it
    /// stands now rather than as the file has it, and the lead it was
    /// released from is forgotten, its ended workers' handles read closed to
    /// `lead`. Call before [`Self::back`], which brings back the rest.
    #[must_use = "its effects are to be carried"]
    pub fn take_back(&mut self, lead: u64, kept: &Kept) -> Vec<Effect> {
        let named: HashSet<&str> = kept
            .workers
            .iter()
            .map(|worker| worker.handle.as_str())
            .collect();
        let open = self
            .workers
            .values()
            .filter(|relation| named.contains(relation.handle.as_str()))
            .map(|relation| relation.lead);
        let parked = self
            .released
            .iter()
            .filter(|(_, parked)| {
                parked
                    .iter()
                    .any(|(_, back)| named.contains(back.handle.as_str()))
            })
            .map(|(of, _)| *of);
        let from: HashSet<u64> = open
            .chain(parked)
            .filter(|of| *of != lead && self.is_released(*of))
            .collect();
        if from.is_empty() {
            return Vec::new();
        }
        for (&uid, relation) in &mut self.workers {
            if from.contains(&relation.lead) {
                relation.lead = lead;
                self.opened.entry(lead).or_default().push(uid);
            }
        }
        for closed in self.closed_workers.values_mut() {
            if from.contains(&closed.lead) {
                closed.lead = lead;
            }
        }
        for of in from {
            // Parked ones are in the file and come back through `back`.
            self.released.remove(&of);
            self.kept_in.remove(&of);
            self.written.remove(&of);
        }
        // Nothing to write: the file already lists every one of them, and
        // the next change writes it as `lead`'s.
        vec![Effect::Changed]
    }

    /// Whether worker `handle` is `lead`'s open worker already, taken back
    /// by [`Self::take_back`]: it does not come back a second time.
    pub(super) fn taken_back(&self, lead: u64, handle: &str) -> bool {
        self.uid_of(handle)
            .and_then(|uid| self.workers.get(&uid))
            .is_some_and(|relation| relation.lead == lead)
    }
}
