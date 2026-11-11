//! Telling a person what a session did: which of its signals is news, the
//! badge on its row, and where the news is said.
//!
//! **The drive asks where the person is, and the front end answers.** A
//! session's news (a turn that ended, a card that waits, an agent lost) is
//! said with [`Effect::Look`]; the front end answers at once with what the
//! person can see of the session ([`Input::Seen`]), read as it stands then, so
//! no change of focus can be missed. What that means anyone is told is
//! [`Attention::telling`]'s; what is kept of it, the badge, is the drive's.

use super::{Effect, Ended, SessionDrive, Signal};
use crate::chat::attention::Attention;
use crate::chat::model::Away;

impl SessionDrive {
    /// Whether a turn ended out of the person's sight and they have not looked
    /// since.
    pub fn unseen(&self) -> bool {
        self.unseen
    }

    /// Say `signal`, and ask where the person is when it is news.
    pub(super) fn say(&self, fx: &mut Vec<Effect>, signal: Signal) {
        let news = Self::news(&signal);
        fx.push(Effect::Signal(signal));
        fx.extend(news.map(Effect::Look));
    }

    /// What `signal` tells a person, if anything.
    ///
    /// A stream that ended is news only as a lost agent: a fork that never
    /// opened is said where it goes, and a host session out of reach of its
    /// host, or waiting on its busy sessions, is not lost, and is restarted
    /// once the host is back or done.
    fn news(signal: &Signal) -> Option<Away> {
        match signal {
            Signal::TurnEnded | Signal::CycleEnded => Some(Away::TurnEnded),
            Signal::AwaitingUser(ask) => Some(Away::Asked(*ask)),
            Signal::Disconnected(Ended::Lost) => Some(Away::LinkLost),
            Signal::Disconnected(
                Ended::ForkNeverOpened(_) | Ended::OutOfReach(_) | Ended::Waiting(_),
            ) => None,
            Signal::Appended
            | Signal::PickRefused(_)
            | Signal::StopRefused(_)
            | Signal::SaveFailed(_)
            | Signal::CodeFailed(_)
            | Signal::RenameFailed(_) => None,
        }
    }

    /// How the stream that just ended ended.
    pub(super) fn ended(&self) -> Ended {
        match (
            self.chat.fork_failure(),
            self.host_away(),
            self.host_waiting(),
        ) {
            (Some(why), _, _) => Ended::ForkNeverOpened(why),
            (None, Some(host), _) => Ended::OutOfReach(host.clone()),
            (None, None, Some(host)) => Ended::Waiting(host.clone()),
            (None, None, None) => Ended::Lost,
        }
    }

    /// The person was where `attention` says when `what` came: badge it, and
    /// say it outside the window where they should hear it.
    pub(super) fn seen(&mut self, what: Away, attention: Attention) -> Vec<Effect> {
        let say = attention.telling(what);
        if say.badge {
            self.unseen = true;
        }
        if say.desktop || say.chat {
            vec![Effect::Tell {
                what,
                desktop: say.desktop,
                chat: say.chat,
            }]
        } else {
            Vec::new()
        }
    }

    /// The person is looking at the conversation: its news is read.
    pub(super) fn shown(&mut self) -> Vec<Effect> {
        self.unseen = false;
        Vec::new()
    }
}
