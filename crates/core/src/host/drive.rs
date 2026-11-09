//! Hosts' drive: one attach per host for the whole process, what this run
//! knows of each, and the waits on a host: a busy one's for its sessions to
//! finish, a lost one's to be attached again.
//!
//! **Decided here, carried out elsewhere.** A [`HostDrive`] is asked for a
//! host's attach ([`HostDrive::ask`]) and answers whether the one up serves,
//! an attempt under way is joined, or a new one is made ([`Got`]); it is told
//! what each attempt landed, and when a connection closed. The attach itself,
//! its stream and every call over it are the front end's to run, on the
//! runtime the I/O runs on.
//!
//! **One attach per host.** The host service takes a single attach at a
//! time, and a second one from this machine would only kick the first, so
//! every ask while one attempt runs joins it. A person's ask that finds an
//! attempt made by itself under way (one that does not take the host from
//! another machine) waits on it and, should it fail, attaches again, taking
//! the host. Only the latest attempt for a host says what is known of it:
//! one a restart replaced is not heard.
//!
//! **The waits.** A host found running another build with sessions mid-turn
//! is waited on, once per host, attached again every [`HOST_IDLE_POLL`] until
//! an attach lands. A connection that goes without another machine taking
//! the host is lost: it is attached again by itself, the wait doubling from
//! [`RECONNECT_FIRST`] to [`RECONNECT_MAX`], for as long as anything listens.
//! One taken by another machine is that machine's, and only a person attaches
//! to it again. Each wait lasts while something listens to it; the last
//! listener going ends it and its clock.
//!
//! **Late answers.** An attempt's landing is *recorded* in its slot only while
//! the slot is still that attempt's, and *heard* (what is known of the host)
//! only when it is the host's latest. A wait's clock past its wait is
//! *dropped*: each carries the wait's id.

use crate::host::wire::HostSession;
use crate::host::HostFailure;
use crate::workspace::HostName;
use std::collections::BTreeMap;
use std::time::Duration;

#[cfg(test)]
mod tests;
mod waits;

pub use waits::{
    next_delay, HOST_IDLE_POLL, HOST_UNREACHABLE_POLLS, RECONNECT_FIRST, RECONNECT_MAX,
};

/// How a host's wait ended: its service on this build, or why it could not
/// be reached.
pub type WaitEnd = Result<(), String>;

/// What this run knows of its attach to a host.
#[derive(Clone, Debug, PartialEq)]
pub enum HostState {
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

/// Which wait on a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// For its busy sessions to finish.
    Busy,
    /// For it, lost, to be attached again.
    Back,
}

/// What an ask for a host's attach comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Got {
    /// The connection up serves.
    Ready,
    /// The attempt under way, the `made`th, is joined.
    Join(u64),
    /// The attempt under way, made by itself, is waited on; should it fail,
    /// the ask attaches again, taking the host.
    Wait(u64),
    /// A new attempt, the `made`th, is made.
    Make(u64),
}

/// What the drive asks for.
#[derive(Debug)]
pub enum Effect {
    /// Hand over wait `id`'s clock after this long ([`HostDrive::elapsed`]).
    Arm {
        host: HostName,
        kind: Kind,
        id: u64,
        after: Duration,
    },
    /// Attach to `host` by itself for wait `id`, then say so
    /// ([`HostDrive::polled`]) whatever it came to.
    Attach { host: HostName, kind: Kind, id: u64 },
    /// Wait `id` ended so: tell everything that listens to it.
    Ended { id: u64, how: WaitEnd },
    /// What is known of a host changed: whatever shows it is drawn again.
    Refresh,
}

/// A host's attach, as its slot holds it.
#[derive(Clone, Copy, Debug)]
enum Slot {
    /// The `made`th attempt's connection.
    Ready(u64),
    /// The `made`th attempt, under way: whether it takes the host from
    /// another machine, and whether only a wait's own poll asked for it.
    Attaching { made: u64, take: bool, polled: bool },
}

/// What an attempt's landing came to.
pub struct Landed {
    /// Its connection is the host's now.
    pub ready: bool,
    pub fx: Vec<Effect>,
}

/// Every host's attach, and the waits on them.
#[derive(Default)]
pub struct HostDrive {
    slots: BTreeMap<HostName, Slot>,
    /// The last attempt made for each host.
    latest: BTreeMap<HostName, u64>,
    made: u64,
    waits: waits::Waits,
}

impl HostDrive {
    /// The attach to `host`: the one up, if `alive` says its connection still
    /// has its stream; the attempt under way; or a new one. With `fresh`, a
    /// new one whatever is there: what follows under way is no longer heard.
    /// `take` when the ask takes the host from another machine, `polled`
    /// when only a wait's own poll asks.
    pub fn ask(
        &mut self,
        host: &HostName,
        fresh: bool,
        take: bool,
        polled: bool,
        alive: bool,
    ) -> Got {
        let found = match fresh {
            true => None,
            false => self.slots.get_mut(host),
        };
        match found {
            Some(Slot::Ready(_)) if alive => return Got::Ready,
            Some(Slot::Attaching {
                made, take: takes, ..
            }) if take && !*takes => return Got::Wait(*made),
            Some(Slot::Attaching {
                made,
                polled: only_polled,
                ..
            }) => {
                // Heard as a person's once a person's ask joins it.
                *only_polled &= polled;
                return Got::Join(*made);
            }
            Some(Slot::Ready(_)) | None => {}
        }
        self.made += 1;
        let made = self.made;
        self.latest.insert(host.clone(), made);
        self.slots
            .insert(host.clone(), Slot::Attaching { made, take, polled });
        // Before the first frame that could show it, so nothing to redraw.
        self.waits.attaching(host);
        Got::Make(made)
    }

    /// The `made`th attempt for `host` landed so: kept in its slot while the
    /// slot is still its own, and heard while it is the host's latest.
    pub fn landed(
        &mut self,
        host: &HostName,
        made: u64,
        result: &Result<(), HostFailure>,
    ) -> Landed {
        let mut ready = false;
        let mut polled = false;
        if let Some(Slot::Attaching {
            made: m, polled: p, ..
        }) = self.slots.get(host).copied()
        {
            if m == made {
                polled = p;
                match result {
                    Ok(()) => {
                        self.slots.insert(host.clone(), Slot::Ready(made));
                        ready = true;
                    }
                    Err(_) => {
                        self.slots.remove(host);
                    }
                }
            }
        }
        let mut fx = Vec::new();
        if self.latest.get(host) == Some(&made) && self.waits.heard(host, result, polled) {
            fx.push(Effect::Refresh);
        }
        fx.splice(0..0, self.waits.take_ended());
        Landed { ready, fx }
    }

    /// The connection a landed attempt made for `host` closed, `taken` by
    /// another machine or not: unless a newer one is up (`up`), the host is
    /// lost or taken from now on.
    pub fn closed(&mut self, host: &HostName, taken: bool, up: bool) -> Vec<Effect> {
        if up {
            return Vec::new();
        }
        let changed = self.waits.closed(host, taken);
        let mut fx = self.waits.take_ended();
        if changed {
            fx.push(Effect::Refresh);
        }
        fx
    }

    /// A live host whose connection has closed is lost, or taken, from now
    /// on, whether or not the watch on its closing has been heard yet:
    /// `closed` is whether the connection up has closed, and with it whether
    /// it was taken.
    pub fn settle(&mut self, host: &HostName, closed: Option<bool>) -> Vec<Effect> {
        match (self.waits.state(host), closed) {
            (HostState::Live, Some(taken)) => {
                self.waits.closed(host, taken);
                self.waits.take_ended()
            }
            _ => Vec::new(),
        }
    }

    /// The attempt whose connection is `host`'s now, if one landed.
    pub fn ready(&self, host: &HostName) -> Option<u64> {
        match self.slots.get(host)? {
            Slot::Ready(made) => Some(*made),
            Slot::Attaching { .. } => None,
        }
    }

    /// One more listener to `host`'s wait of `kind`, and its first clock
    /// when it has none; `None` when there is nothing to wait for.
    pub fn listen(&mut self, host: &HostName, kind: Kind) -> Option<(u64, Vec<Effect>)> {
        self.waits.listen(host, kind)
    }

    /// A listener to wait `id` on `host` went: `true` when the wait went with
    /// it, the last.
    pub fn leave(&mut self, host: &HostName, id: u64) -> bool {
        self.waits.leave(host, id)
    }

    /// Wait `id`'s clock came round.
    pub fn elapsed(&mut self, host: &HostName, kind: Kind, id: u64) -> Vec<Effect> {
        self.waits.elapsed(host, kind, id)
    }

    /// The attach wait `id`'s clock asked for came back.
    pub fn polled(&mut self, host: &HostName, kind: Kind, id: u64) -> Vec<Effect> {
        self.waits.polled(host, kind, id)
    }

    /// What is known of `host`'s attach.
    pub fn state(&self, host: &HostName) -> HostState {
        self.waits.state(host)
    }

    /// Whether lost `host` is being attached again by itself: something
    /// listens for it to be back.
    pub fn retrying(&self, host: &HostName) -> bool {
        self.waits.retrying(host)
    }

    /// Whether `host` was last found running another build with sessions
    /// mid-turn.
    pub fn found_busy(&self, host: &HostName) -> bool {
        self.waits.found_busy(host)
    }

    /// The sessions mid-turn on `host`, and how many terminals it has open,
    /// as it last said, while it is waited on.
    pub fn busy(&self, host: &HostName) -> (&[HostSession], usize) {
        self.waits.busy(host)
    }

    /// Whether anything waits on `host`'s busy sessions.
    pub fn waited(&self, host: &HostName) -> bool {
        self.waits.waited(host)
    }
}

/// Why what waited on `host` being back ends rather than carries on, given
/// what is known of it now, whether it was last found busy, and how the wait
/// ended; `None` to restart and list again. A host taken by another machine
/// is never attached again by itself, however the wait ended. One found busy
/// carries on, which puts what waited on its busy wait; any other end, a
/// host refused, is why.
pub fn back_failure(
    host: &HostName,
    state: &HostState,
    busy: bool,
    end: WaitEnd,
) -> Option<String> {
    match state {
        HostState::Taken => Some(crate::host::taken_line(host)),
        HostState::Unknown | HostState::Attaching | HostState::Live | HostState::Lost(_) => {
            match end {
                Ok(()) => None,
                Err(_) if busy => None,
                Err(why) => Some(why),
            }
        }
    }
}

/// Whether a yes to restarting a host's service still has to restart it:
/// the wait may have ended since the question was asked, and restarting it
/// again would stop what that started.
pub fn needs_restart<C>(attached: &Result<C, HostFailure>) -> bool {
    match attached {
        Err(HostFailure::Busy { .. }) => true,
        Ok(_) | Err(HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_)) => {
            false
        }
    }
}

/// Whether a failed restart is already said where the wait it ended was
/// waited on, so nothing else says it: a host out of reach ends its wait on
/// why, and one still busy ends none.
pub fn said_by_wait(waited: bool, failure: &HostFailure) -> bool {
    match failure {
        HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_) => waited,
        HostFailure::Busy { .. } => false,
    }
}

/// What a restart that failed says.
pub fn restart_failure(failure: HostFailure) -> String {
    match failure {
        HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why) => why,
        HostFailure::Busy { host, .. } => {
            format!("{host} still runs another build of the host service")
        }
    }
}
