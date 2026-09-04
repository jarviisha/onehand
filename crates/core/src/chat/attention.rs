//! What the user can see of a session, and what that means anyone is told.

use super::model::Away;

/// Whether one piece of news reaches each of the three places it could.
///
/// Says only *whether*, never *what*: the badge, the desktop notification and
/// the message on a chat are built where the transcript is, because each needs
/// something only the front end holds. What is decided here is the part that has
/// to be the same every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Telling {
    /// Mark the session's row as holding something unread.
    pub badge: bool,
    /// Say it on the desktop, outside the window.
    pub desktop: bool,
    /// Say it on whatever channel reaches the user away from the machine.
    pub chat: bool,
}

impl Telling {
    /// Nothing at all is said.
    const NOTHING: Self = Self {
        badge: false,
        desktop: false,
        chat: false,
    };

    /// Whether this piece of news goes nowhere.
    ///
    /// Worth a name because a caller can then stop before building any of the
    /// three payloads, each of which reads the transcript to do it.
    pub fn silent(self) -> bool {
        self == Self::NOTHING
    }
}

/// Where the user is, as a front end can see it.
///
/// Three facts and no window: gathered at the call site and handed over, so the
/// rules built on them are decidable without one.
pub struct Presence {
    /// The user has said they are not at the machine.
    ///
    /// **The one fact none of the others can stand in for.** Every other
    /// question here is answered by looking at the screen, and a screen stays
    /// exactly as convincing in front of an empty chair — so a window that is
    /// focused and unattended reports every turn as already read, which loses
    /// each notification at the moment it was needed.
    pub away: bool,
    /// This window is the one in front of the user.
    ///
    /// *This* window, not "some window of this app": a background window's turn
    /// is not one anybody has seen.
    pub window_active: bool,
    /// The conversation this window is showing, if any.
    pub shown: Option<u64>,
}

/// How much of a given session the user can see right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// Looking at this very conversation.
    Reading,
    /// At this window, but on some other conversation. The session's row is in
    /// their eye line; its transcript is not.
    AtWindow,
    /// Not at this window at all — or not at the machine.
    Absent,
}

impl Attention {
    /// What is said about one piece of news, given how much of the session the
    /// user can already see.
    pub fn telling(self, what: Away) -> Telling {
        match (what, self) {
            (_, Self::Reading) => Telling::NOTHING,
            (Away::TurnEnded, Self::AtWindow) => Telling {
                badge: true,
                ..Telling::NOTHING
            },
            (Away::TurnEnded, Self::Absent) => Telling {
                badge: true,
                desktop: true,
                chat: true,
            },
            (Away::LinkLost, _) => Telling {
                chat: true,
                ..Telling::NOTHING
            },
            (Away::Asked(_), _) => Telling {
                desktop: true,
                chat: true,
                badge: false,
            },
        }
    }
}

impl Presence {
    /// How much of `uid` the user can see.
    pub fn seeing(&self, uid: u64) -> Attention {
        if self.away || !self.window_active {
            Attention::Absent
        } else if self.shown == Some(uid) {
            Attention::Reading
        } else {
            Attention::AtWindow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::UserAsk;
    use super::*;

    /// At the window, on this very conversation — and the user has said they
    /// are not there.
    fn said_they_left() -> Presence {
        Presence {
            away: true,
            window_active: true,
            shown: Some(3),
        }
    }

    /// The one fact none of the others can stand in for. A focused window in
    /// front of an empty chair reports every turn as read, which loses every
    /// notification the app has at precisely the moment one was needed — so the
    /// user gets to say they have gone, and while they have said so nothing
    /// counts as seen.
    #[test]
    fn saying_you_left_beats_a_window_sitting_in_front_of_you() {
        assert_eq!(said_they_left().seeing(3), Attention::Absent);
    }

    /// A window being in front is not enough on its own: a conversation in the
    /// background is one the user cannot see even while its window is. And a
    /// window that is behind another application is not being read at all,
    /// whichever conversation happens to be mounted in it.
    #[test]
    fn a_conversation_is_only_read_when_its_window_is_the_one_in_front() {
        let at_it = Presence {
            away: false,
            window_active: true,
            shown: Some(3),
        };
        assert_eq!(at_it.seeing(3), Attention::Reading);
        assert_eq!(at_it.seeing(9), Attention::AtWindow, "9 is behind 3");

        let behind = Presence {
            window_active: false,
            ..at_it
        };
        assert_eq!(behind.seeing(3), Attention::Absent);

        let nothing_shown = Presence {
            shown: None,
            ..at_it
        };
        assert_eq!(nothing_shown.seeing(3), Attention::AtWindow);
    }

    /// Every kind of news, in the order a reader would meet them.
    fn every_kind() -> [Away; 4] {
        [
            Away::TurnEnded,
            Away::LinkLost,
            Away::Asked(UserAsk::Permission),
            Away::Asked(UserAsk::Question),
        ]
    }

    /// The one row of the table that is the same all the way across: somebody
    /// looking at the conversation has already been told everything an
    /// announcement could say, so saying it interrupts them about what is in
    /// front of them.
    #[test]
    fn a_conversation_being_read_is_told_nothing_whatever_happened() {
        for what in every_kind() {
            assert!(
                Attention::Reading.telling(what).silent(),
                "{what:?} spoke over the reader"
            );
        }
    }

    /// A turn that ended did its work, so the mark on the session's row loses
    /// nothing by being read late — and that row is already in the eye line of
    /// somebody sitting at this window. Interrupting them, or reaching their
    /// phone, would be saying twice what the badge says once.
    #[test]
    fn a_finished_turn_at_the_same_window_earns_only_a_badge() {
        let say = Attention::AtWindow.telling(Away::TurnEnded);

        assert!(say.badge);
        assert!(!say.desktop, "they are sitting in front of the badge");
        assert!(!say.chat, "and their phone is not where they are looking");
    }

    /// The same turn with nobody at the window is the whole reason the other
    /// two places exist. The badge still goes up, because it is what they will
    /// see when they come back.
    #[test]
    fn a_finished_turn_with_nobody_there_reaches_everywhere() {
        let say = Attention::Absent.telling(Away::TurnEnded);

        assert!(say.badge);
        assert!(say.desktop);
        assert!(say.chat);
    }

    /// An agent that stopped answering says nothing on the desktop, at either
    /// distance, and that is deliberate rather than an omission: the rail's mark
    /// and the conversation's header both carry it for as long as it is true and
    /// both are visible without leaving the app. What the outside channel adds
    /// is the one case neither covers — nobody being at the machine.
    ///
    /// No badge either. A badge is for a turn nobody read; this is a standing
    /// condition, drawn from the session's own state and cleared by nothing.
    #[test]
    fn a_lost_agent_is_never_put_on_the_desktop() {
        for seen in [Attention::AtWindow, Attention::Absent] {
            let say = seen.telling(Away::LinkLost);

            assert!(!say.desktop, "{seen:?}");
            assert!(!say.badge, "{seen:?}");
            assert!(say.chat, "{seen:?}");
        }
    }

    /// **The distinction the whole table exists for.** A parked question earns
    /// the desktop from the same window a finished turn is silent from, and the
    /// two are not the same event: a turn that ended has done its work, while an
    /// agent waiting stands still for exactly as long as it takes somebody to
    /// notice — and reading one conversation is precisely when the mark on
    /// another one's row goes unseen.
    ///
    /// No badge: the rail already carries this the whole time it is true, drawn
    /// from the transcript's own unanswered card rather than from a flag that
    /// would then have to be cleared.
    #[test]
    fn a_waiting_agent_is_said_from_a_window_a_finished_turn_stays_quiet_in() {
        for ask in [UserAsk::Permission, UserAsk::Question] {
            let say = Attention::AtWindow.telling(Away::Asked(ask));

            assert!(say.desktop, "{ask:?}");
            assert!(say.chat, "{ask:?}");
            assert!(!say.badge, "{ask:?}");
            assert!(
                !Attention::AtWindow.telling(Away::TurnEnded).desktop,
                "the same window stays quiet about a finished turn"
            );
        }
    }
}
