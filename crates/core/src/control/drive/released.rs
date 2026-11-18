//! A lead the person closed alone, and its workers waiting for it.
//!
//! **Closing a lead releases its workers rather than ending them.** They
//! stay open, listed under their own projects and the person's to work
//! with, but their relation stays the closed lead's, unusable, and is still
//! kept beside its conversation: so a lead closed by accident takes them back
//! when its conversation is opened again ([`ControlDrive::take_back`]), in
//! this run or a later one. Only the person closing a worker, or the lead's
//! `close_worker`, ends one for good.
//!
//! **A lead with no conversation on disk yet releases nothing**: there is no
//! file to take its workers back from, so they end with it as before.
//!
//! **A worker still coming back is parked**, not failed: its open is
//! dropped, but it stays in the file, to come back with the lead next time.

use super::super::kept::{Kept, KeptWorker};
use super::{ControlDrive, Effect, Key, Phase};
use std::collections::HashSet;

impl ControlDrive {
    /// Whether closing `lead` releases its workers: it holds some, open or
    /// coming back, and its conversation is kept in a folder.
    pub(super) fn releases(&self, lead: u64) -> bool {
        let holds = self.workers.values().any(|relation| relation.lead == lead)
            || self
                .pending
                .iter()
                .any(|((of, _), pending)| *of == lead && pending.open.back.is_some());
        holds && self.kept_in.contains_key(&lead)
    }

    /// Whether `lead` was closed alone, its workers released.
    pub(super) fn released(&self, lead: u64) -> bool {
        self.released.contains(&lead)
    }

    /// Release `lead`'s workers: each one still coming back is parked, kept
    /// for the next time its conversation opens rather than ended.
    pub(super) fn release_workers(&mut self, lead: u64) {
        let keys: Vec<Key> = self
            .pending
            .iter()
            .filter(|((of, _), pending)| *of == lead && pending.open.back.is_some())
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
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
            if let Some(back) = pending.open.back {
                self.parked
                    .entry(lead)
                    .or_default()
                    .push((pending.token, back));
            }
        }
        self.released.insert(lead);
    }

    /// What is written for `lead` once one of its workers ended: the last
    /// write, and all of it forgotten, for a released lead left holding
    /// nothing; else the usual one.
    pub(super) fn kept_after_end(&mut self, lead: u64) -> Option<Effect> {
        let holds = self.workers.values().any(|relation| relation.lead == lead)
            || self.parked.contains_key(&lead);
        if !self.released(lead) || holds {
            return self.keep(lead);
        }
        let last = self.keep_last(lead);
        self.released.remove(&lead);
        self.closed_workers.retain(|_, closed| closed.lead != lead);
        self.kept_in.remove(&lead);
        self.written.remove(&lead);
        last
    }

    /// The workers parked for `lead`, to be written with the ones it holds.
    pub(super) fn parked_of(&self, lead: u64) -> Vec<(u64, KeptWorker)> {
        self.parked.get(&lead).cloned().unwrap_or_default()
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
            .parked
            .iter()
            .filter(|(_, parked)| {
                parked
                    .iter()
                    .any(|(_, back)| named.contains(back.handle.as_str()))
            })
            .map(|(of, _)| *of);
        let from: HashSet<u64> = open
            .chain(parked)
            .filter(|of| *of != lead && self.released(*of))
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
            self.parked.remove(&of);
            self.released.remove(&of);
            self.kept_in.remove(&of);
            self.written.remove(&of);
        }
        let mut fx = vec![Effect::Changed];
        fx.extend(self.keep(lead));
        fx
    }

    /// Whether worker `handle` is open now: one released and taken back
    /// does not come back a second time.
    pub(super) fn is_open(&self, handle: &str) -> bool {
        self.handles.contains_key(handle)
    }
}
