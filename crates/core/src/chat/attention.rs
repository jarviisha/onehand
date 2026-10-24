//! What the user can see of a session, what that means anyone is told, and
//! the one signal a session shows for it.

use super::model::{Away, Link};

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

/// What a session is doing, when that is something the rail should say.
///
/// Only states that want the user's eye. A session that is connected, idle and
/// already read carries **no** signal — that is what keeps a rail full of
/// healthy sessions a clean list of names rather than a wall of dots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionSignal {
    /// The adapter went away. `Ctrl+Shift+R` brings it back.
    Lost,
    /// The last turn ended on an error rather than an answer.
    Failed,
    /// A parked permission or question, or an approval the run on the
    /// session waits for (`Drive::waits_on_person`). The only signal that is
    /// about *the user*: nothing moves until they answer.
    AwaitingUser,
    /// A turn is in flight.
    Busy,
    /// A turn finished while this session was not being looked at.
    UnseenTurn,
}

impl SessionSignal {
    /// How badly each state wants the user's eye, lowest first.
    ///
    /// **The one place the order lives.** A session reducing its own facts and
    /// a group of sessions reducing theirs both read it, and an order written
    /// out twice is an order that will disagree with itself the first time
    /// someone edits one copy.
    fn rank(self) -> u8 {
        match self {
            // A dead adapter outranks a question nobody can answer any more.
            Self::Lost => 0,
            // A turn that broke waits on the person as a question does, and
            // nothing else will move it.
            Self::Failed => 1,
            // A parked question outranks busy: only one of them moves on its
            // own.
            Self::AwaitingUser => 2,
            // What is happening now outranks what happened last turn.
            Self::Busy => 3,
            // The calmest, and the only one about the past rather than about
            // now.
            Self::UnseenTurn => 4,
        }
    }

    /// Reduce a session's five independent facts to the one thing the rail
    /// draws.
    ///
    /// One mark, not one per condition: two marks side by side on a rail row
    /// are a code nobody learns. Which one survives is `rank` — a
    /// judgement about which fact the user needs when several are true at once.
    ///
    /// Pure, and separate from the lookup, so the order is testable without a
    /// window: it is a rule about attention, and rules about attention are
    /// exactly what regresses silently.
    pub fn pick(
        link: Link,
        failed: bool,
        awaiting_user: bool,
        busy: bool,
        unseen: bool,
    ) -> Option<Self> {
        Self::most_urgent(
            [
                (link == Link::Lost).then_some(Self::Lost),
                failed.then_some(Self::Failed),
                awaiting_user.then_some(Self::AwaitingUser),
                busy.then_some(Self::Busy),
                unseen.then_some(Self::UnseenTurn),
            ]
            .into_iter()
            .flatten(),
        )
    }

    /// The signal a *group* of sessions carries — a project row's roll-up.
    ///
    /// Without it a collapsed project was silent about everything inside it:
    /// an agent could be waiting on an answer, or dead, with nothing on screen
    /// saying so until the user thought to expand that project. The same rank
    /// decides, so the mark on a project means exactly what the same mark means
    /// on the session it came from.
    pub fn most_urgent(signals: impl IntoIterator<Item = Self>) -> Option<Self> {
        signals.into_iter().min_by_key(|signal| signal.rank())
    }

    /// What a state is called where there is room for a name but not a
    /// sentence: the rail's meta line, the overview, the remote bridge's
    /// session listing. One place, so no two readers of one state call it
    /// two things.
    pub fn word(self) -> &'static str {
        match self {
            Self::Lost => "Disconnected",
            Self::Failed => "Failed",
            Self::AwaitingUser => "Needs input",
            Self::Busy => "Running",
            Self::UnseenTurn => "Done",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Link, UserAsk};
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

    /// A healthy, idle, already-read session draws **nothing**. This is the
    /// case that makes the other four legible: a rail where every row has a dot
    /// is a rail where no dot means anything.
    #[test]
    fn a_calm_session_carries_no_signal() {
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, false, false),
            None
        );
    }

    /// Connecting is not failing. Reading `tx.is_none()` would conflate them
    /// and paint a danger dot for the second or two every session spends
    /// coming up.
    #[test]
    fn coming_up_is_not_a_signal() {
        assert_eq!(
            SessionSignal::pick(Link::Connecting, false, false, false, false),
            None
        );
    }

    #[test]
    fn each_state_shows_when_it_is_the_only_one() {
        use SessionSignal::*;
        assert_eq!(
            SessionSignal::pick(Link::Lost, false, false, false, false),
            Some(Lost)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, true, false, false),
            Some(AwaitingUser)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, true, false),
            Some(Busy)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, false, true),
            Some(UnseenTurn)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, true, false, false, false),
            Some(Failed)
        );
    }

    /// The whole point of the reduction. Each of these is a real pairing:
    /// an adapter that died with a question still parked, a turn that finished
    /// unseen on a session that then lost its adapter, a busy session the user
    /// has not looked at since its last turn.
    #[test]
    fn the_more_urgent_state_wins() {
        use SessionSignal::*;
        assert_eq!(
            SessionSignal::pick(Link::Lost, false, true, false, true),
            Some(Lost),
            "a dead adapter outranks a question nobody can answer any more"
        );
        assert_eq!(
            SessionSignal::pick(Link::Lost, true, false, false, false),
            Some(Lost),
            "a dead adapter outranks the error it may have died on"
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, true, true, true, true),
            Some(Failed),
            "a broken turn outranks a question and a run"
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, true, true, true),
            Some(AwaitingUser),
            "a parked question outranks busy: only one of them moves on its own"
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, true, true),
            Some(Busy),
            "what is happening now outranks what happened last turn"
        );
    }

    /// A project's mark is the most urgent of its sessions', on the same
    /// ordering a single session uses — otherwise the same shape would mean two
    /// different things one row apart.
    #[test]
    fn a_project_rolls_up_to_its_most_urgent_session() {
        use SessionSignal::*;
        assert_eq!(SessionSignal::most_urgent([]), None);
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Lost, Busy]),
            Some(Lost)
        );
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Busy, AwaitingUser]),
            Some(AwaitingUser)
        );
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Busy]),
            Some(Busy),
            "a project with a turn running says so over a stale badge"
        );
    }

    /// A session with several facts true at once and a project holding those
    /// same facts one per session must land on the same mark. They are the same
    /// question asked at two altitudes, and `rank` is the single answer.
    #[test]
    fn one_session_and_one_project_agree() {
        use SessionSignal::*;
        for (link, failed, awaiting, busy, unseen) in [
            (Link::Lost, true, true, false, true),
            (Link::Connected, true, true, true, true),
            (Link::Connected, false, true, true, true),
            (Link::Connected, false, false, true, true),
            (Link::Connected, false, false, false, true),
        ] {
            let parts = [
                (link == Link::Lost).then_some(Lost),
                failed.then_some(Failed),
                awaiting.then_some(AwaitingUser),
                busy.then_some(Busy),
                unseen.then_some(UnseenTurn),
            ];
            assert_eq!(
                SessionSignal::pick(link, failed, awaiting, busy, unseen),
                SessionSignal::most_urgent(parts.into_iter().flatten()),
            );
        }
    }
}
