//! What this run knows of each host's attach, and the waits on it: a busy
//! host's for its sessions to finish, a lost one's to be attached again.
//! Read and written on the UI thread alone.

use super::{Ended, HostFailure, WaitEnd, taken_line, waiting_line};
use futures::FutureExt as _;
use futures::channel::oneshot;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::collections::BTreeMap;
use std::time::Duration;

/// How long after a connection is lost it is first attached again.
pub(crate) const RECONNECT_FIRST: Duration = Duration::from_secs(2);

/// The longest a lost host goes between attaches, the wait doubling up to it.
pub(crate) const RECONNECT_MAX: Duration = Duration::from_secs(60);

/// How long to wait before the next attach to a lost host, `prev` the wait
/// before the last miss, if there was one.
pub(crate) fn next_delay(prev: Option<Duration>) -> Duration {
    prev.map_or(RECONNECT_FIRST, |prev| (prev * 2).min(RECONNECT_MAX))
}

/// What this run knows of its attach to a host.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HostState {
    /// Never attached this run, nor tried.
    Unknown,
    /// A first attach, or a person's after the host was taken, under way;
    /// or the host found running another build, which its wait follows.
    Attaching,
    Live,
    /// Its connection went, or no attach reaches it, for why: attached again
    /// by itself while anything listens.
    Lost(String),
    /// Another machine attached, which holds the host now.
    Taken,
}

/// How many polls in a row may fail to reach a waited host before its wait
/// ends on why: one is a blip, a host gone stays gone.
pub(crate) const HOST_UNREACHABLE_POLLS: u32 = 2;

/// Hosts found busy, with the sessions they said, and the waits listened
/// to on them, one each; what is known of each host's attach, and the waits
/// for lost ones to be attached again.
#[derive(Default)]
pub(super) struct Waits {
    /// The sessions mid-turn, and how many terminals are open.
    busy: BTreeMap<HostName, (Vec<HostSession>, usize)>,
    waits: BTreeMap<HostName, Wait>,
    states: BTreeMap<HostName, HostState>,
    backs: BTreeMap<HostName, Wait>,
}

struct Wait {
    /// Sent when the wait ends; dropped unsent with its last listener.
    end: oneshot::Sender<WaitEnd>,
    ended: Ended,
    listeners: usize,
    polling: bool,
    /// Polls in a row that could not reach the host.
    missed: u32,
}

impl Waits {
    /// What an attach to `host` came to, `polled` when the wait's own poll
    /// made it: a busy host is kept with the sessions it said; one that
    /// lands ends the wait, and one that could not reach the host ends it
    /// on why, at once, or from the poll after a few misses in a row.
    ///
    /// The host is live once an attach lands, which also ends a wait for it
    /// to be back; one found busy ends that wait on why, the busy wait taking
    /// over; one out of reach leaves it lost, the wait going on.
    ///
    /// One refused ends every wait on why, and leaves nothing known: it is
    /// not lost, and nothing attaches to it again by itself.
    ///
    /// `true` when what is known of the host changed.
    pub(super) fn heard<C>(
        &mut self,
        host: &HostName,
        result: &Result<C, HostFailure>,
        polled: bool,
    ) -> bool {
        match result {
            Ok(_) => {
                self.end(host, Ok(()));
                end_in(&mut self.backs, host, Ok(()));
                self.set(host, HostState::Live)
            }
            Err(HostFailure::Refused(why)) => {
                self.end(host, Err(why.clone()));
                end_in(&mut self.backs, host, Err(why.clone()));
                self.set(host, HostState::Unknown)
            }
            Err(HostFailure::Taken(why)) => {
                self.end(host, Err(why.clone()));
                end_in(&mut self.backs, host, Err(why.clone()));
                self.set(host, HostState::Taken)
            }
            Err(HostFailure::Busy { busy, terms, .. }) => {
                let changed = self.set(host, HostState::Attaching);
                let line = waiting_line(host, busy.len(), *terms);
                end_in(&mut self.backs, host, Err(line));
                self.busy.insert(host.clone(), (busy.clone(), *terms));
                if let Some(wait) = self.waits.get_mut(host) {
                    wait.missed = 0;
                }
                changed
            }
            Err(HostFailure::Other(why)) => {
                let changed = self.set(host, HostState::Lost(why.clone()));
                let missed = self.waits.get_mut(host).map_or(0, |wait| {
                    wait.missed += 1;
                    wait.missed
                });
                if !polled || missed >= HOST_UNREACHABLE_POLLS {
                    self.end(host, Err(why.clone()));
                }
                changed
            }
        }
    }

    /// Know `host` to be in `state` from now on: `true` when that changed
    /// what was known.
    fn set(&mut self, host: &HostName, state: HostState) -> bool {
        let was = match state {
            HostState::Unknown => self.states.remove(host),
            HostState::Attaching | HostState::Live | HostState::Lost(_) | HostState::Taken => {
                self.states.insert(host.clone(), state.clone())
            }
        };
        was.unwrap_or(HostState::Unknown) != state
    }

    /// Whether a wait for `host` to be back stands: something listens, and
    /// it is attached again by itself.
    pub(super) fn retrying(&self, host: &HostName) -> bool {
        self.backs.contains_key(host)
    }

    /// Whether `host` was last found running another build with sessions
    /// mid-turn or terminals open, which its busy wait follows.
    pub(super) fn found_busy(&self, host: &HostName) -> bool {
        self.busy.contains_key(host)
    }

    fn end(&mut self, host: &HostName, how: WaitEnd) {
        self.busy.remove(host);
        end_in(&mut self.waits, host, how);
    }

    /// What is known of `host`'s attach.
    pub(super) fn state(&self, host: &HostName) -> HostState {
        self.states.get(host).cloned().unwrap_or(HostState::Unknown)
    }

    /// An attach to `host` has started: a first, or a person's to a host
    /// taken; a lost host stays lost until one lands.
    pub(super) fn attaching(&mut self, host: &HostName) -> bool {
        match self.state(host) {
            HostState::Unknown | HostState::Taken => self.set(host, HostState::Attaching),
            HostState::Attaching | HostState::Live | HostState::Lost(_) => false,
        }
    }

    /// `host`'s connection closed: taken by another machine, which ends any
    /// wait for it to be back, or lost. `true` when that changed what was
    /// known.
    pub(super) fn closed(&mut self, host: &HostName, taken: bool) -> bool {
        let state = match taken {
            true => {
                let why = taken_line(host);
                end_in(&mut self.backs, host, Err(why));
                HostState::Taken
            }
            false => HostState::Lost(format!("lost the connection to {host}")),
        };
        self.set(host, state)
    }

    /// One more listener to the wait for lost `host` to be back; `None`
    /// unless it is lost.
    pub(super) fn listen_back(&mut self, host: &HostName) -> Option<Ended> {
        match self.state(host) {
            HostState::Lost(_) => Some(join_in(&mut self.backs, host)),
            HostState::Unknown | HostState::Attaching | HostState::Live | HostState::Taken => None,
        }
    }

    /// Whether the loop of `host`'s wait to be back has yet to start.
    pub(super) fn poll_back(&mut self, host: &HostName) -> bool {
        poll_in(&mut self.backs, host)
    }

    /// Whether the loop of the wait `ended` for `host` to be back goes on.
    pub(super) fn polling_back(&self, host: &HostName, ended: &Ended) -> bool {
        polling_in(&self.backs, host, ended)
    }

    /// One more listener to `host`'s wait, made with the first; `None` when
    /// the host is not known busy. A wait is only ever made here, so none
    /// stands with nobody listening.
    pub(super) fn listen(&mut self, host: &HostName) -> Option<Ended> {
        if !self.busy.contains_key(host) {
            return None;
        }
        Some(join_in(&mut self.waits, host))
    }

    /// A listener to the wait `ended`, on busy sessions or for the host to
    /// be back, went; with the last, the wait goes, and with a busy one what
    /// the host said.
    pub(super) fn leave(&mut self, host: &HostName, ended: &Ended) {
        if leave_in(&mut self.waits, host, ended) {
            self.busy.remove(host);
        }
        leave_in(&mut self.backs, host, ended);
    }

    /// The sessions `host` said were mid-turn, while it is waited on.
    pub(super) fn busy(&self, host: &HostName) -> (&[HostSession], usize) {
        match self.waits.contains_key(host) {
            true => self
                .busy
                .get(host)
                .map_or((&[], 0), |(busy, terms)| (busy.as_slice(), *terms)),
            false => (&[], 0),
        }
    }

    /// Whether `host`'s wait has yet to be polled; it is from now on.
    pub(super) fn poll(&mut self, host: &HostName) -> bool {
        poll_in(&mut self.waits, host)
    }

    /// Whether the poll of the wait `ended` goes on: that wait still stands.
    pub(super) fn polling(&self, host: &HostName, ended: &Ended) -> bool {
        polling_in(&self.waits, host, ended)
    }

    /// Whether anything waits on `host`.
    pub(super) fn waited(&self, host: &HostName) -> bool {
        self.waits.contains_key(host)
    }
}

/// End `host`'s wait in `waits`, if one stands, as `how`.
fn end_in(waits: &mut BTreeMap<HostName, Wait>, host: &HostName, how: WaitEnd) {
    if let Some(wait) = waits.remove(host) {
        let _ = wait.end.send(how);
    }
}

/// One more listener to `host`'s wait in `waits`, made with the first.
fn join_in(waits: &mut BTreeMap<HostName, Wait>, host: &HostName) -> Ended {
    let wait = waits.entry(host.clone()).or_insert_with(|| {
        let (end, ended) = oneshot::channel();
        Wait {
            end,
            ended: ended.shared(),
            listeners: 0,
            polling: false,
            missed: 0,
        }
    });
    wait.listeners += 1;
    wait.ended.clone()
}

/// A listener to the wait `ended` in `waits` went: `true` when it was the
/// last, and the wait went with it.
fn leave_in(waits: &mut BTreeMap<HostName, Wait>, host: &HostName, ended: &Ended) -> bool {
    let Some(wait) = waits.get_mut(host).filter(|wait| wait.ended.ptr_eq(ended)) else {
        return false;
    };
    wait.listeners = wait.listeners.saturating_sub(1);
    if wait.listeners > 0 {
        return false;
    }
    waits.remove(host);
    true
}

/// Whether `host`'s wait in `waits` has yet to be polled; it is from now on.
fn poll_in(waits: &mut BTreeMap<HostName, Wait>, host: &HostName) -> bool {
    waits
        .get_mut(host)
        .is_some_and(|wait| !std::mem::replace(&mut wait.polling, true))
}

/// Whether the poll of the wait `ended` in `waits` goes on.
fn polling_in(waits: &BTreeMap<HostName, Wait>, host: &HostName, ended: &Ended) -> bool {
    waits
        .get(host)
        .is_some_and(|wait| wait.polling && wait.ended.ptr_eq(ended))
}
