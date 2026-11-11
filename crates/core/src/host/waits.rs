//! One window's share of the waits on its hosts: what here waits on each
//! host kept waiting by its busy sessions, or on each lost host being back,
//! and what each wait's end comes to here. The waits themselves are the
//! process's, decided by [`super::drive::HostDrive`]; this decides only what
//! the window carries on with when one ends. Listening for a wait's end is
//! the front end's: [`WindowWaits::wait`] says when to start, and the hosts
//! [`WindowWaits::prune`] lets go of are the ones no longer listened for.

use super::drive::{back_failure, back_landed, idle_landed, HostState, Landing, WaitEnd};
use crate::workspace::HostName;
use std::collections::BTreeMap;

/// What waits on a host's busy sessions going idle, or on a lost host being
/// attached again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// The session that was not attached, restarted.
    Session(u64),
    /// The form's *Add*, on this attempt, carried on.
    Add(u64),
    /// The project page's list of the host's sessions, asked again.
    Page,
    /// The session out of reach of its lost host, restarted once it is back.
    Back(u64),
    /// The project page showing what it kept of a lost host's sessions,
    /// listed again once it is back.
    PageBack,
}

impl Want {
    /// Whether it waits on the host being back rather than on its busy
    /// sessions.
    pub fn back(self) -> bool {
        match self {
            Want::Back(_) | Want::PageBack => true,
            Want::Session(_) | Want::Add(_) | Want::Page => false,
        }
    }
}

/// What waits here on one host: the sessions to restart, the form's attempt
/// to carry on, and whether a page waits.
#[derive(Default, Debug, PartialEq, Eq)]
struct Waiting {
    uids: Vec<u64>,
    add: Option<u64>,
    page: bool,
}

/// What waits here on each host, and the count that tells one attach of the
/// form from the next.
#[derive(Default)]
pub struct WindowWaits {
    waiting: BTreeMap<HostName, Waiting>,
    /// What waits on each lost host being back, kept apart: a host is lost
    /// or busy, and the two waits end differently.
    back: BTreeMap<HostName, Waiting>,
    attempts: u64,
}

impl WindowWaits {
    /// Add `want` to what waits here on `host`: `true` when nothing did, so
    /// the end of the host's wait (on its being back, `want.back()`) is to be
    /// listened for.
    pub fn wait(&mut self, host: &HostName, want: Want) -> bool {
        let map = self.map(want.back());
        let fresh = !map.contains_key(host);
        let waiting = map.entry(host.clone()).or_default();
        match want {
            Want::Session(uid) | Want::Back(uid) if !waiting.uids.contains(&uid) => {
                waiting.uids.push(uid)
            }
            Want::Session(_) | Want::Back(_) => {}
            Want::Page | Want::PageBack => waiting.page = true,
            Want::Add(attempt) => waiting.add = Some(attempt),
        }
        fresh
    }

    fn map(&mut self, back: bool) -> &mut BTreeMap<HostName, Waiting> {
        match back {
            true => &mut self.back,
            false => &mut self.waiting,
        }
    }

    /// The wait for lost `host` to be back ended (`end`), the host now as
    /// `state` says and found running another build (`busy`): what that
    /// comes to for what waited here, once.
    pub fn back_ended(
        &mut self,
        host: &HostName,
        state: &HostState,
        busy: bool,
        end: WaitEnd,
    ) -> Vec<Landing> {
        let Some(waiting) = self.back.remove(host) else {
            return Vec::new();
        };
        let failed = back_failure(host, state, busy, end);
        back_landed(waiting.uids, waiting.page, failed)
    }

    /// `host`'s busy wait ended (`end`), the form on attach `form`: what
    /// that comes to for what waited here, once, and whether the form is the
    /// one that waited, carried on, so it waits no longer.
    pub fn idle_ended(
        &mut self,
        host: &HostName,
        form: Option<u64>,
        end: WaitEnd,
    ) -> Option<(Vec<Landing>, bool)> {
        let waiting = self.waiting.remove(host)?;
        let add = waiting.add.is_some_and(|held| landed_on(form, held));
        Some((idle_landed(waiting.uids, waiting.page, add, end), add))
    }

    /// Keep only what `still` says waits yet. Answers each host nothing here
    /// waits on any more, and whether that was its wait on being back, so
    /// its wait is no longer listened to from here.
    pub fn prune(&mut self, still: impl Fn(&HostName, Want) -> bool) -> Vec<(HostName, bool)> {
        let mut gone = Vec::new();
        self.waiting.retain(|host, waiting| {
            waiting.uids.retain(|uid| still(host, Want::Session(*uid)));
            waiting.add = waiting.add.filter(|add| still(host, Want::Add(*add)));
            waiting.page = waiting.page && still(host, Want::Page);
            let keep = !waiting.uids.is_empty() || waiting.add.is_some() || waiting.page;
            if !keep {
                gone.push((host.clone(), false));
            }
            keep
        });
        self.back.retain(|host, waiting| {
            waiting.uids.retain(|uid| still(host, Want::Back(*uid)));
            waiting.page = waiting.page && still(host, Want::PageBack);
            let keep = !waiting.uids.is_empty() || waiting.page;
            if !keep {
                gone.push((host.clone(), true));
            }
            keep
        });
        gone
    }

    /// Whether nothing here waits on any host.
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty() && self.back.is_empty()
    }

    /// The form went away: no wait carries its *Add* on, so a form opened
    /// again is never committed by a wait that ended for this one.
    pub fn forget_add(&mut self) {
        for waiting in self.waiting.values_mut() {
            waiting.add = None;
        }
    }

    /// A new attach of the form's.
    pub fn attempt(&mut self) -> u64 {
        self.attempts += 1;
        self.attempts
    }
}

/// Whether an attach's answer is for the form's attach under way: a form
/// cancelled and opened again has another, or none.
pub fn landed_on(current: Option<u64>, landed: u64) -> bool {
    current == Some(landed)
}

#[cfg(test)]
mod tests;
