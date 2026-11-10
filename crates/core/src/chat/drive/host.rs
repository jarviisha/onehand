//! A session whose agent runs on a host: what this machine kept of it, the
//! line that says it waits on its host, and the connection to the host lost.
//!
//! How the connection stands is the front end's to see ([`HostLink`]); what
//! follows from it is here, decided on the connection, never on words.

use super::{Effect, Next, SessionDrive, Signal};
use crate::acp::AcpEvent;
use crate::chat::Now;
use crate::workspace::HostName;

/// What a host session waits on, while it does.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hold {
    /// The host's busy sessions, before its service restarts onto this
    /// build: restarted once they are done.
    Busy(HostName),
    /// The host itself, out of reach: read from this machine's copy until
    /// it is back.
    Away(HostName),
}

/// How the connection a host session's watch rides stands, as the front end
/// sees it.
pub struct HostLink {
    pub host: HostName,
    /// The connection's stream has ended.
    pub closed: bool,
    /// The host closed it because another machine attached.
    pub taken: bool,
}

/// The line under a host session read from this machine's copy: how recent
/// the copy is when its entries kept a time, `last` (seconds since the
/// epoch), and that it carries on once the host is back.
pub(crate) fn offline_line(host: &HostName, last: Option<u64>, now: u64) -> String {
    let upto = last.map_or_else(String::new, |at| {
        format!(", up to {}", crate::rel_time(now, at))
    });
    format!(
        "Not connected to {host}. This is the copy kept on this machine{upto}; \
         it carries on once the host is back"
    )
}

impl SessionDrive {
    /// Show what this machine kept of the session's journal, folded into the
    /// transcript as it was then, and, `away` from its host, the line saying
    /// it is out of reach, the newest kept entry's time `last`. Nothing is
    /// said to whatever watches the session beyond that it grew, so a
    /// desktop notice or a run never takes the old turns for new ones.
    pub fn read_back(
        &mut self,
        kept: impl IntoIterator<Item = AcpEvent>,
        away: Option<(HostName, Option<u64>)>,
        now: Now,
    ) -> Vec<Effect> {
        self.chat.tell_time(now);
        for event in kept {
            self.chat.apply(event);
        }
        if let Some((host, last)) = away {
            self.chat.say_host_wait(offline_line(&host, last, now.secs));
            self.hold = Some(Hold::Away(host));
        }
        self.sent(vec![Effect::Changed, Effect::Signal(Signal::Appended)])
    }

    /// Say on the session that it waits on `host`'s `n` busy sessions and
    /// `terms` open terminals, as a line of its own rather than a failure:
    /// once they are done, it is restarted, and nothing has to be tried
    /// again by hand.
    pub fn wait_on_host(&mut self, host: HostName, n: usize, terms: usize) -> Vec<Effect> {
        self.chat
            .say_host_wait(crate::host::waiting_line(&host, n, terms));
        self.hold = Some(Hold::Busy(host));
        vec![Effect::Signal(Signal::Appended)]
    }

    /// The host the session waits on the busy sessions of, while it does.
    pub fn host_waiting(&self) -> Option<&HostName> {
        match &self.hold {
            Some(Hold::Busy(host)) => Some(host),
            Some(Hold::Away(_)) | None => None,
        }
    }

    /// The host the session is out of reach of, while it is.
    pub fn host_away(&self) -> Option<&HostName> {
        match &self.hold {
            Some(Hold::Away(host)) => Some(host),
            Some(Hold::Busy(_)) | None => None,
        }
    }

    /// Stop waiting on the host's busy sessions, taking back the line that
    /// said so: `true` when it was waiting.
    pub fn forget_host_wait(&mut self) -> (bool, Vec<Effect>) {
        match self.hold.take_if(|hold| matches!(hold, Hold::Busy(_))) {
            Some(_) => (true, self.unsay_host_wait()),
            None => (false, Vec::new()),
        }
    }

    /// No longer out of reach of the host, taking back the line that said
    /// so: `true` when it was.
    pub fn forget_host_away(&mut self) -> (bool, Vec<Effect>) {
        match self.hold.take_if(|hold| matches!(hold, Hold::Away(_))) {
            Some(_) => (true, self.unsay_host_wait()),
            None => (false, Vec::new()),
        }
    }

    /// The wait on the host's busy sessions ended on `why`, the host out of
    /// reach: the session ends as a lost agent does, which Restart tries
    /// again. Nothing when it was not waiting.
    pub fn wait_failed(&mut self, why: &str, now: Now) -> Vec<Effect> {
        match self.forget_host_wait() {
            (true, mut fx) => {
                fx.extend(self.end_as_lost(why, now));
                fx
            }
            (false, _) => Vec::new(),
        }
    }

    /// The wait for the host to be back ended on `why`, the host taken or
    /// refused: the session ends as a lost agent does. Nothing when it was not
    /// out of reach.
    pub fn back_failed(&mut self, why: &str, now: Now) -> Vec<Effect> {
        match self.forget_host_away() {
            (true, mut fx) => {
                fx.extend(self.end_as_lost(why, now));
                fx
            }
            (false, _) => Vec::new(),
        }
    }

    /// End the agent's link on `why`, as a batch of its own.
    fn end_as_lost(&mut self, why: &str, now: Now) -> Vec<Effect> {
        let (mut fx, _) = self.apply(AcpEvent::Disconnected(why.to_string()), now);
        fx.extend(self.end_batch());
        fx
    }

    /// Take back the line a wait on the host put, if it is still where it
    /// was put.
    fn unsay_host_wait(&mut self) -> Vec<Effect> {
        self.chat.unsay_host_wait();
        self.sent(Vec::new())
    }

    /// Fold `event`, from a watch on a host over `link`. A watch that ends
    /// with its connection, closed without another machine taking it, is
    /// out of reach rather than lost: what the link left running settles,
    /// its cards no longer offer answers, the copy's line says so up to now
    /// rather than that the agent went, and the session waits for the host
    /// to be back. A first prompt held for the agent is dropped, as on any
    /// lost link, and the batch ends there, changed. Without a link it is
    /// folded as any event is.
    pub fn apply_watched(
        &mut self,
        event: AcpEvent,
        link: Option<&HostLink>,
        now: Now,
    ) -> (Vec<Effect>, Next) {
        let Some(link) = link.filter(|link| {
            matches!(event, AcpEvent::Disconnected(_)) && link.closed && !link.taken
        }) else {
            return self.apply(event, now);
        };
        self.chat.tell_time(now);
        self.chat.link_cut();
        self.chat
            .say_host_wait(offline_line(&link.host, Some(now.secs), now.secs));
        self.hold = Some(Hold::Away(link.host.clone()));
        self.held = None;
        self.batch.changed = true;
        (vec![Effect::Signal(Signal::Appended)], Next::Stop)
    }

    /// Carry the count of prompts the session this one restarts had sent, so
    /// a run or a lead reading it counts on from there.
    pub fn carry_sent(&mut self, sent: usize) {
        self.chat.carry_sent(sent);
    }
}
