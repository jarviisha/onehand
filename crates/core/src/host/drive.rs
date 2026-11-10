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

pub use waits::HOST_IDLE_POLL;

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

/// Who asks for a host's attach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// A person, or work a person is waiting on: the attach takes the host
    /// from another machine, and a miss ends a wait at once.
    Person,
    /// A wait's own poll, or work nobody watches: made by itself, it leaves
    /// a host another machine holds to it, and a miss is a blip.
    Poll,
}

/// What the drive asks for.
#[derive(Debug)]
pub enum Effect {
    /// Hand over wait `id`'s clock after this long, as [`Input::Elapsed`].
    /// A late answer is dropped: a clock whose wait has gone, or been made
    /// anew, finds no wait of its id and asks for nothing.
    Arm {
        host: HostName,
        kind: Kind,
        id: u64,
        after: Duration,
    },
    /// Attach to `host` as a poll for wait `id`, then answer
    /// [`Input::Polled`] whatever it came to. What the attach lands is
    /// recorded through its own [`Input::Landed`]; the `Polled` answer is
    /// dropped once the wait has gone, as an `Arm`'s is.
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
    /// The `made`th attempt, under way, and who asked for it.
    Attaching { made: u64, by: Asker },
}

/// What the world reports back.
pub enum Input {
    /// The `made`th attempt for `host` landed so. Recorded in its slot while
    /// the slot is still its own, and heard (what is known of the host) only
    /// while it is the host's latest: a replaced attempt's late answer is
    /// dropped. Whether its connection is the host's now is read after, from
    /// [`HostDrive::ready`].
    Landed {
        host: HostName,
        made: u64,
        result: Result<(), HostFailure>,
    },
    /// The connection a landed attempt made for `host` closed, `taken` by
    /// another machine or not, and whether a newer one is `up` by then.
    Closed {
        host: HostName,
        taken: bool,
        up: bool,
    },
    /// Wait `id`'s clock came round.
    Elapsed { host: HostName, kind: Kind, id: u64 },
    /// The attach wait `id`'s clock asked for came back.
    Polled { host: HostName, kind: Kind, id: u64 },
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
    /// The attach to `host` that `by` asks for: the one up, if `alive` says
    /// its connection still has its stream; the attempt under way; or a new
    /// one. With `fresh`, a new one whatever is there: what follows under way
    /// is no longer heard.
    pub fn ask(&mut self, host: &HostName, fresh: bool, by: Asker, alive: bool) -> Got {
        let found = match fresh {
            true => None,
            false => self.slots.get(host).copied(),
        };
        match (found, by) {
            (Some(Slot::Ready(_)), _) if alive => return Got::Ready,
            (
                Some(Slot::Attaching {
                    made,
                    by: Asker::Poll,
                }),
                Asker::Person,
            ) => return Got::Wait(made),
            (Some(Slot::Attaching { made, .. }), _) => return Got::Join(made),
            (Some(Slot::Ready(_)) | None, _) => {}
        }
        self.made += 1;
        let made = self.made;
        self.latest.insert(host.clone(), made);
        self.slots
            .insert(host.clone(), Slot::Attaching { made, by });
        // Before the first frame that could show it, so nothing to redraw.
        self.waits.attaching(host);
        Got::Make(made)
    }

    /// What the world reported.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Landed { host, made, result } => self.landed(&host, made, &result),
            Input::Closed { host, taken, up } => self.closed(&host, taken, up),
            Input::Elapsed { host, kind, id } => self.waits.elapsed(&host, kind, id),
            Input::Polled { host, kind, id } => self.waits.polled(&host, kind, id),
        }
    }

    fn landed(
        &mut self,
        host: &HostName,
        made: u64,
        result: &Result<(), HostFailure>,
    ) -> Vec<Effect> {
        let mut polled = false;
        if let Some(Slot::Attaching { made: m, by }) = self.slots.get(host).copied() {
            if m == made {
                polled = by == Asker::Poll;
                match result {
                    Ok(()) => {
                        self.slots.insert(host.clone(), Slot::Ready(made));
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
        fx
    }

    fn closed(&mut self, host: &HostName, taken: bool, up: bool) -> Vec<Effect> {
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
    /// it was taken. It ends no wait and asks for nothing: a live host has no
    /// wait for it to be back, and is drawn again once the closing is heard.
    pub fn settle(&mut self, host: &HostName, closed: Option<bool>) {
        let Some(taken) = closed else {
            return;
        };
        match self.waits.state(host) {
            HostState::Live => {
                self.waits.closed(host, taken);
            }
            HostState::Unknown | HostState::Attaching | HostState::Lost(_) | HostState::Taken => {}
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

/// Which of a window's waits on a host ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The wait for a lost host to be back.
    Back,
    /// The wait for a busy host's sessions to finish, on another build.
    Busy,
}

/// What a window does once one of its waits on a host ends, for the sessions
/// that waited (`uids`), its page if that waited, and its form's *Add* if
/// that waited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// Each session restarted, `after_service` when the host's service has
    /// just been restarted onto this build, said when `told`; the page lists
    /// again when `list`; the form's *Add* carries on when `add`.
    Restart {
        uids: Vec<u64>,
        after_service: bool,
        told: bool,
        list: bool,
        add: bool,
    },
    /// Each session ends as a lost agent does, on `why`; the page says so
    /// when `page`; the form fails on `why` when `add`.
    Ended {
        waited: Waited,
        uids: Vec<u64>,
        why: String,
        page: bool,
        add: bool,
    },
}

/// What a window's wait for lost `host` to be back comes to, given how it
/// ended ([`back_failure`]'s answer): what waited restarts quietly, as the
/// host simply being back is no news, and the page lists again only if it
/// waited.
pub fn back_landed(uids: Vec<u64>, page: bool, failed: Option<String>) -> Landed {
    match failed {
        None => Landed::Restart {
            uids,
            after_service: false,
            told: false,
            list: page,
            add: false,
        },
        Some(why) => Landed::Ended {
            waited: Waited::Back,
            uids,
            why,
            page,
            add: false,
        },
    }
}

/// What a window's wait on a busy host's sessions comes to, given how it
/// ended: what waited restarts onto the restarted service, each said, and
/// the page lists whatever waited, since its sessions are what the wait
/// changed; the form's *Add* carries on when it waited.
pub fn idle_landed(uids: Vec<u64>, page: bool, add: bool, end: WaitEnd) -> Landed {
    match end {
        Ok(()) => Landed::Restart {
            uids,
            after_service: true,
            told: true,
            list: true,
            add,
        },
        Err(why) => Landed::Ended {
            waited: Waited::Busy,
            uids,
            why,
            page,
            add,
        },
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
