//! What this run knows of each host's attach, and the waits on it: a busy
//! host's for its sessions to finish, a lost one's to be attached again.

use super::{Effect, HostState, Kind, WaitEnd};
use crate::host::wire::HostSession;
use crate::host::{taken_line, waiting_line, HostFailure};
use crate::workspace::HostName;
use std::collections::BTreeMap;
use std::time::Duration;

/// How often a host kept waiting by its busy sessions is attached again.
pub const HOST_IDLE_POLL: Duration = Duration::from_secs(15);

/// How long after a connection is lost it is first attached again.
pub(crate) const RECONNECT_FIRST: Duration = Duration::from_secs(2);

/// The longest a lost host goes between attaches, the wait doubling up to it.
pub(crate) const RECONNECT_MAX: Duration = Duration::from_secs(60);

/// How many polls in a row may fail to reach a waited host before its wait
/// ends on why: one is a blip, a host gone stays gone.
pub(crate) const HOST_UNREACHABLE_POLLS: u32 = 2;

/// How long to wait before the next attach to a lost host, `prev` the wait
/// before the last miss, if there was one.
pub(crate) fn next_delay(prev: Option<Duration>) -> Duration {
    prev.map_or(RECONNECT_FIRST, |prev| (prev * 2).min(RECONNECT_MAX))
}

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
    minted: u64,
    /// The waits ended since the drive last took them, with how.
    ended: Vec<Effect>,
}

struct Wait {
    id: u64,
    listeners: usize,
    polling: bool,
    /// Polls in a row that could not reach the host.
    missed: u32,
    /// The last wait between a lost host's attaches.
    delay: Option<Duration>,
}

impl Waits {
    /// The waits ended since the last call, each with how.
    pub(super) fn take_ended(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.ended)
    }

    /// What an attach to `host` came to, `polled` when only a wait's own
    /// poll asked for it: a busy host is kept with the sessions it said; one
    /// that lands ends the wait, and one that could not reach the host ends
    /// it on why, at once, or from the poll after a few misses in a row.
    ///
    /// The host is live once an attach lands, which also ends a wait for it
    /// to be back; one found busy ends that wait on why, the busy wait taking
    /// over; one out of reach leaves it lost, the wait going on.
    ///
    /// One refused ends every wait on why, and leaves nothing known: it is
    /// not lost, and nothing attaches to it again by itself.
    ///
    /// `true` when what is known of the host changed.
    pub(super) fn heard(
        &mut self,
        host: &HostName,
        result: &Result<(), HostFailure>,
        polled: bool,
    ) -> bool {
        match result {
            Ok(()) => {
                self.end(host, Ok(()));
                self.end_back(host, Ok(()));
                self.set(host, HostState::Live)
            }
            Err(HostFailure::Refused(why)) => {
                self.end(host, Err(why.clone()));
                self.end_back(host, Err(why.clone()));
                self.set(host, HostState::Unknown)
            }
            Err(HostFailure::Taken(why)) => {
                self.end(host, Err(why.clone()));
                self.end_back(host, Err(why.clone()));
                self.set(host, HostState::Taken)
            }
            Err(HostFailure::Busy { busy, terms, .. }) => {
                let changed = self.set(host, HostState::Attaching);
                let line = waiting_line(host, busy.len(), *terms);
                self.end_back(host, Err(line));
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
        if let Some(wait) = self.waits.remove(host) {
            self.ended.push(Effect::Ended { id: wait.id, how });
        }
    }

    fn end_back(&mut self, host: &HostName, how: WaitEnd) {
        if let Some(wait) = self.backs.remove(host) {
            self.ended.push(Effect::Ended { id: wait.id, how });
        }
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
                self.end_back(host, Err(taken_line(host)));
                HostState::Taken
            }
            false => HostState::Lost(format!("lost the connection to {host}")),
        };
        self.set(host, state)
    }

    /// One more listener to `host`'s wait of `kind`, made with the first,
    /// and its poll's first clock when it has yet to be polled; `None` when
    /// nothing is to be waited for: a host not known busy, or not lost.
    pub(super) fn listen(&mut self, host: &HostName, kind: Kind) -> Option<(u64, Vec<Effect>)> {
        let waits = match kind {
            Kind::Busy if self.busy.contains_key(host) => &mut self.waits,
            Kind::Back if matches!(self.state(host), HostState::Lost(_)) => &mut self.backs,
            Kind::Busy | Kind::Back => return None,
        };
        let minted = &mut self.minted;
        let wait = waits.entry(host.clone()).or_insert_with(|| {
            *minted += 1;
            Wait {
                id: *minted,
                listeners: 0,
                polling: false,
                missed: 0,
                delay: None,
            }
        });
        wait.listeners += 1;
        let id = wait.id;
        if std::mem::replace(&mut wait.polling, true) {
            return Some((id, Vec::new()));
        }
        let after = match kind {
            Kind::Busy => HOST_IDLE_POLL,
            Kind::Back => {
                let after = next_delay(None);
                wait.delay = Some(after);
                after
            }
        };
        let arm = Effect::Arm {
            host: host.clone(),
            kind,
            id,
            after,
        };
        Some((id, vec![arm]))
    }

    /// The wait `id` on `host`, if it still stands.
    fn standing(&mut self, host: &HostName, kind: Kind, id: u64) -> Option<&mut Wait> {
        let waits = match kind {
            Kind::Busy => &mut self.waits,
            Kind::Back => &mut self.backs,
        };
        waits
            .get_mut(host)
            .filter(|wait| wait.id == id && wait.polling)
    }

    /// The clock of wait `id` came round: attach again, if it still stands.
    pub(super) fn elapsed(&mut self, host: &HostName, kind: Kind, id: u64) -> Vec<Effect> {
        match self.standing(host, kind, id) {
            Some(_) => vec![Effect::Attach {
                host: host.clone(),
                kind,
                id,
            }],
            None => Vec::new(),
        }
    }

    /// The attach wait `id`'s clock asked for came back: its next clock, if
    /// it still stands, a lost host's twice as long as the last.
    pub(super) fn polled(&mut self, host: &HostName, kind: Kind, id: u64) -> Vec<Effect> {
        let Some(wait) = self.standing(host, kind, id) else {
            return Vec::new();
        };
        let after = match kind {
            Kind::Busy => HOST_IDLE_POLL,
            Kind::Back => {
                let after = next_delay(wait.delay);
                wait.delay = Some(after);
                after
            }
        };
        vec![Effect::Arm {
            host: host.clone(),
            kind,
            id,
            after,
        }]
    }

    /// A listener to the wait `id` went; with the last, the wait goes, and
    /// with a busy one what the host said. `true` when the wait went.
    pub(super) fn leave(&mut self, host: &HostName, id: u64) -> bool {
        if leave_in(&mut self.waits, host, id) {
            self.busy.remove(host);
            return true;
        }
        leave_in(&mut self.backs, host, id)
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

    /// Whether anything waits on `host`.
    pub(super) fn waited(&self, host: &HostName) -> bool {
        self.waits.contains_key(host)
    }
}

/// A listener to the wait `id` in `waits` went: `true` when it was the last,
/// and the wait went with it.
fn leave_in(waits: &mut BTreeMap<HostName, Wait>, host: &HostName, id: u64) -> bool {
    let Some(wait) = waits.get_mut(host).filter(|wait| wait.id == id) else {
        return false;
    };
    wait.listeners = wait.listeners.saturating_sub(1);
    if wait.listeners > 0 {
        return false;
    }
    waits.remove(host);
    true
}
