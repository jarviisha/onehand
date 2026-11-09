//! A session whose agent runs on a host: what this machine kept of it, the
//! line that says it waits on its host, and the connection to the host lost.
//!
//! Whether a connection was lost is the front end's to see, on the
//! connection; what follows from it is here.

use super::{Effect, SessionDrive, Signal};
use crate::acp::AcpEvent;
use crate::chat::Now;

impl SessionDrive {
    /// Show what this machine kept of the session's journal, folded into the
    /// transcript as it was then, and `away`, the line saying it is out of
    /// reach of its host. Nothing is said to whatever watches the session
    /// beyond that it grew, so a desktop notice or a run never takes the old
    /// turns for new ones.
    pub fn read_back(
        &mut self,
        kept: impl IntoIterator<Item = AcpEvent>,
        away: Option<String>,
        now: Now,
    ) -> Vec<Effect> {
        self.chat.tell_time(now);
        for event in kept {
            self.chat.apply(event);
        }
        if let Some(line) = away {
            self.chat.say_host_wait(line);
        }
        self.sent(vec![Effect::Changed, Effect::Signal(Signal::Appended)])
    }

    /// Say on the session, in `line`, that it waits on its host: out of
    /// reach, or behind the host's busy sessions. A line of its own rather
    /// than a failure, taken back once the wait is over.
    pub fn say_host_wait(&mut self, line: String) -> Vec<Effect> {
        self.chat.say_host_wait(line);
        vec![Effect::Signal(Signal::Appended)]
    }

    /// Take back the line [`Self::say_host_wait`] put, if it is still where
    /// it was put.
    pub fn unsay_host_wait(&mut self) -> Vec<Effect> {
        self.chat.unsay_host_wait();
        self.sent(Vec::new())
    }

    /// The connection to the session's host was lost, in a batch of events:
    /// what the link left running settles, its cards no longer offer answers,
    /// and `line` says the session is out of reach rather than that its agent
    /// went. A first prompt held for the agent is dropped, as on any lost
    /// link, and the batch has changed.
    pub fn host_lost(&mut self, line: String, now: Now) -> Vec<Effect> {
        self.chat.tell_time(now);
        self.chat.link_cut();
        self.chat.say_host_wait(line);
        self.held = None;
        self.batch.changed = true;
        vec![Effect::Signal(Signal::Appended)]
    }

    /// Carry the count of prompts the session this one restarts had sent, so
    /// a run or a lead reading it counts on from there.
    pub fn carry_sent(&mut self, sent: usize) {
        self.chat.carry_sent(sent);
    }
}
