//! What the bridge remembers about each chat on the far side.

use super::command::Aim;
use super::types::{Button, ChatId, Outbound};
use crate::chat::Away;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// One session, as somebody reading about it from a phone would need it.
///
/// `uid` is both the session's identity and the number it is listed under. A
/// position in a list would be the wrong thing to print: the list is read, a
/// message is typed, and a session closed in between would slide every number
/// below it onto a different conversation.
pub struct RemoteSession {
    pub uid: u64,
    pub project: String,
    /// `None` until the conversation has earned a name, which is its first
    /// prompt — the same rule the rail falls back from.
    pub conversation: Option<String>,
    pub agent: String,
    /// What the session is doing, in the front end's own words for it. `None`
    /// is the case that draws nothing: connected, idle and already read.
    pub state: Option<&'static str>,
}

impl RemoteSession {
    /// The two lines this session takes in a listing.
    ///
    /// Says nothing about whether the chat reading it follows this session:
    /// that is a fact about the reader rather than about the session, so it is
    /// carried by the mark in the margin and not by this, which is printed in
    /// places where there is no margin.
    pub fn line(&self) -> String {
        let name = self.conversation.as_deref().unwrap_or("no name yet");
        let state = self.state.unwrap_or("Idle");
        format!(
            "{} · {} — {}\n    {} · {}",
            self.uid, self.project, name, self.agent, state
        )
    }
}

/// One row of an archive listing, and everything reopening it needs.
///
/// Carries the project as well as the conversation, because minting a session
/// goes to whichever project was last clicked — a conversation reopened from a
/// train would otherwise land on an unrelated checkout.
#[derive(Clone)]
pub struct ArchiveRow {
    /// The project root this conversation was had in.
    pub root: PathBuf,
    pub project: String,
    /// The conversation's directory, which is what a resume is given.
    pub dir: PathBuf,
    pub agent: String,
    pub title: String,
    /// When it was last written, for the listing's own "when".
    pub updated: u64,
}

/// Which session an announcement is about, in the words a reader who is not
/// looking at the app would need.
///
/// A desktop notification can afford to be vague, because the window it is about
/// is one keystroke away. A message on a phone is the only thing its reader has,
/// so it names the project and the conversation as well as the agent — and it
/// carries the uid, which is what a button pressed on it has to come back with.
pub struct Origin {
    pub uid: u64,
    pub agent: String,
    pub project: String,
    /// The conversation's name, or `None` while it has not earned one.
    pub conversation: Option<String>,
}

impl Origin {
    /// The line under the headline: which session this is, where, and which
    /// conversation.
    ///
    /// The number leads, spelled the same way the listing spells it, because it
    /// is what a reply is typed against. Without it a notification about a
    /// waiting agent means opening `/sessions` and matching a name to a row
    /// before anything can be said back to it.
    fn context(&self) -> String {
        match &self.conversation {
            Some(title) => format!("{} · {} — {title}", self.uid, self.project),
            None => format!("{} · {}", self.uid, self.project),
        }
    }
}

/// One thing to say about one session, outside the app.
pub struct Announcement {
    /// Which of the three moments this is.
    pub away: Away,
    /// The line a reader on the far side needs and a reader at the window does
    /// not: what the agent is asking permission for, what the question was.
    ///
    /// `None` where the headline is the whole of it. Somebody looking at the
    /// window can read the card; somebody holding a phone has only this.
    pub detail: Option<String>,
    /// What can be answered from the message itself.
    pub buttons: Vec<Vec<Button>>,
}

impl Announcement {
    /// News with nothing to add and nothing to press: the headline and where it
    /// happened are the whole of it.
    pub fn plain(away: Away) -> Self {
        Self {
            away,
            detail: None,
            buttons: Vec::new(),
        }
    }
}

/// Where each chat types, and what each chat has asked to hear about.
///
/// Both are facts about a *reader* rather than about a session, which is why
/// they are held per chat and why they live together: two people can want
/// different things from one machine, and pointing one phone somewhere must not
/// decide another phone's notifications.
///
/// Owned as plain data with no interior mutability, so the front end decides how
/// it is reached and this stays something a test can drive directly.
pub struct Chats {
    /// The chats allowed to reach the app.
    ///
    /// **Permission, not audience, and the two are different lists.** Being here
    /// is what makes a chat able to say anything and able to be told anything.
    /// What a chat hears about a *session* is `followed`, which is narrower and
    /// is the reader's own choice.
    allowed: Vec<ChatId>,
    /// Which session each chat has pointed itself at.
    ///
    /// In memory only, so a restart forgets it. That is the honest lifetime:
    /// sessions are not persisted either, so a binding that outlived the process
    /// would name a session that no longer exists.
    pointed: HashMap<ChatId, u64>,
    /// Which sessions each chat has asked to hear about.
    ///
    /// **Silence is the default.** A session absent from a chat's set is a
    /// session that chat hears nothing about — not a finished turn, not a parked
    /// question, not an agent that died.
    followed: HashMap<ChatId, HashSet<u64>>,
    /// The archive listing each chat was last sent.
    ///
    /// **Kept exactly as it went out, and never read off disk again.** A saved
    /// conversation has no small stable name to put in a message — on disk it is
    /// an agent-chosen session id, too long to type and too long for a button to
    /// carry — so a chat holds a place in a list, and a place is only safe while
    /// the list it counts into cannot move underneath it. Re-scanning when a
    /// number comes back would reintroduce exactly the drift that made session
    /// numbers uids.
    archives: HashMap<ChatId, Vec<ArchiveRow>>,
}

impl Chats {
    /// A bridge that remembers nothing yet, for the chats on `allowed`.
    pub fn new(allowed: Vec<ChatId>) -> Self {
        Self {
            allowed,
            pointed: HashMap::new(),
            followed: HashMap::new(),
            archives: HashMap::new(),
        }
    }

    /// Keep the archive listing `chat` is about to be sent.
    ///
    /// Replaced whole rather than added to, so it is one page per chat and not a
    /// history of them: the number typed back counts into the page that went
    /// out, and a page that grew would move every row under it.
    pub fn remember_archive(&mut self, chat: &str, rows: Vec<ArchiveRow>) {
        self.archives.insert(chat.to_string(), rows);
    }

    /// The row `chat` means by `place`, counted from one as the listing prints
    /// it.
    ///
    /// `None` covers both ways of missing: a chat that has not asked for a
    /// listing yet, and a number past the end of the one it has. Both are
    /// answered the same way, by asking for the listing again.
    pub fn archive_at(&self, chat: &str, place: usize) -> Option<ArchiveRow> {
        let page = self.archives.get(chat)?;
        page.get(place.checked_sub(1)?).cloned()
    }

    /// Whether `chat` may reach the app at all.
    ///
    /// Asked through here rather than against the list directly, so the rule and
    /// the list stay in one place: a chat that is not allowed is answered with
    /// nothing at all, and the one function that could accidentally start
    /// replying is the one holding the list it would have to ignore.
    pub fn allows(&self, chat: &str) -> bool {
        super::access::is_allowed(&self.allowed, &chat.to_string())
    }

    /// Every chat allowed to hear what is about the bridge rather than about a
    /// session.
    ///
    /// The away switch thrown at the keyboard is the only such news today. It
    /// has no session to be subscribed to, and a reader who has asked for
    /// nothing yet is exactly who needs telling that the machine has been left.
    pub fn everyone(&self) -> &[ChatId] {
        &self.allowed
    }

    /// The session `chat` has pointed itself at, if it has.
    pub fn bound(&self, chat: &str) -> Option<u64> {
        self.pointed.get(chat).copied()
    }

    /// Whether `chat` has asked to hear about `uid`.
    pub fn follows(&self, chat: &str, uid: u64) -> bool {
        self.followed
            .get(chat)
            .is_some_and(|set| set.contains(&uid))
    }

    /// The `/sessions` reply, marking the one this chat is pointed at and the
    /// ones it will hear about.
    ///
    /// **Two marks and one column**, because they are two different questions
    /// asked while reading the same row — where does what I type go, and which
    /// of these will tell me anything — and answering the second in a sentence
    /// underneath would mean counting rows to use it. The pointed-at session is
    /// always followed too, so its arrow stands for both; what the dot is really
    /// for is the rows that are followed without being pointed at, which are
    /// otherwise indistinguishable from the silent ones.
    pub fn listing(&self, chat: &str, open: &[RemoteSession]) -> String {
        if open.is_empty() {
            // Not an error and not an empty list: onehand is running and has
            // nothing open, which is a different thing from onehand not being
            // there.
            return "No sessions are open.".to_string();
        }
        let here = self.bound(chat);
        let mut out = String::from("Sessions\n\n");
        for session in open {
            out.push_str(if here == Some(session.uid) {
                "→ "
            } else if self.follows(chat, session.uid) {
                "• "
            } else {
                "  "
            });
            out.push_str(&session.line());
            out.push('\n');
        }
        out.push_str(
            "\n→ is where what you type goes · is one you'll hear about\n\
             /use <number> points this chat at one and follows it.",
        );
        out
    }

    /// Every chat that should be told about `uid`.
    ///
    /// **Walked out of the allow list and not out of the subscriptions**, so a
    /// chat taken off the list stops hearing anything at once rather than
    /// keeping whatever it had already asked for. The two lists answer different
    /// questions — may this chat be told anything at all, and did this chat ask
    /// about this session — and a session's own news needs both to say yes.
    ///
    /// An empty answer is the ordinary case, not a failure: a bridge nobody has
    /// subscribed from says nothing.
    pub fn audience_for(&self, uid: u64) -> Vec<ChatId> {
        self.allowed
            .iter()
            .filter(|chat| self.follows(chat, uid))
            .cloned()
            .collect()
    }

    /// One message per chat that should hear about this session, ready to send.
    ///
    /// **Whether it is worth saying at all is the caller's decision; who hears
    /// it is this one's.** What is on screen decides the first, and only the
    /// front end can see a screen — the same split that already decides whether
    /// the desktop is told. Which sessions a chat subscribed to is a fact only
    /// the bridge holds, and pushing it up to the caller would quieten the
    /// desktop over instructions that never mentioned it.
    ///
    /// An empty answer is the ordinary case rather than a failure, and it is the
    /// whole of the silence rule: a session nobody follows produces no messages.
    pub fn announcement(&self, origin: &Origin, what: &Announcement) -> Vec<Outbound> {
        let mut text = format!(
            "{}\n{}",
            what.away.headline(&origin.agent),
            origin.context()
        );
        if let Some(detail) = &what.detail {
            text.push_str("\n\n");
            text.push_str(detail);
        }
        self.audience_for(origin.uid)
            .into_iter()
            .map(|chat| Outbound {
                chat,
                text: text.clone(),
                buttons: what.buttons.clone(),
            })
            .collect()
    }

    /// Answer `/status`: what reaches this chat, and what does not.
    ///
    /// **The command silence makes necessary.** Nothing announces itself unless
    /// a chat asked for it, so a bridge working perfectly and a bridge whose
    /// process died last night look the same from a phone — as does one whose
    /// subscriptions were forgotten in a restart. This is the question that
    /// separates them, which is why it names what it follows rather than
    /// counting it: the whole point is to check the list against what you
    /// believe you asked for.
    ///
    /// **Not a second listing.** That one answers what the app is running and
    /// marks these rows in passing; this one answers what will reach you, and
    /// leads with the two facts no session carries — whether the user has said
    /// they are away, and where this chat is pointed.
    ///
    /// It reads and never writes. Anything naming a session that has closed is
    /// gone before this runs, because the caller reconciles against the open
    /// sessions first.
    pub fn status(&self, chat: &str, away: bool, open: &[RemoteSession]) -> String {
        let mut out = String::from(if away {
            "Away is on — everything you follow gets announced here, whatever is on screen.\n"
        } else {
            "Away is off — while you're at the keyboard, what you could already see is held back.\n"
        });

        match self.bound(chat) {
            None => {
                out.push_str("This chat isn't pointed at a session — /use <number> picks one.\n")
            }
            Some(uid) => {
                if let Some(session) = open.iter().find(|session| session.uid == uid) {
                    out.push_str(&format!("Pointed at {}\n", session.line().trim_start()));
                }
            }
        }

        if open.is_empty() {
            // A different answer from "you follow nothing", and they are the two
            // ways this chat ends up hearing nothing. Offering to follow one
            // here would send the reader looking for a list that is empty.
            out.push_str("\nNo sessions are open.");
            return out;
        }
        let following: Vec<u64> = open
            .iter()
            .map(|session| session.uid)
            .filter(|uid| self.follows(chat, *uid))
            .collect();
        if following.is_empty() {
            out.push_str(&format!(
                "\nYou're following nothing, so nothing will reach this chat.\n\
                 {} session{} open — /use <number> points at one and follows it, or \
                 /follow <number> follows one without pointing at it.",
                open.len(),
                if open.len() == 1 { " is" } else { "s are" }
            ));
            return out;
        }
        out.push_str(&format!(
            "\nYou'll hear about {} of {} open session{}:\n",
            following.len(),
            open.len(),
            if open.len() == 1 { "" } else { "s" }
        ));
        for session in open.iter().filter(|s| following.contains(&s.uid)) {
            out.push_str("  ");
            out.push_str(&session.line());
            out.push('\n');
        }
        out.push_str("\n/unfollow <number> drops one. /sessions lists them all.");
        out
    }

    /// What to say when a chat asked for something and is pointed at nothing.
    ///
    /// The listing comes back with it, so the next message has somewhere to
    /// land rather than ending in the same place this one did.
    pub fn not_pointed(&self, chat: &str, what: &str, open: &[RemoteSession]) -> String {
        format!(
            "This chat isn't pointed at a session, so there is nothing to {what}.\n\n{}",
            self.listing(chat, open)
        )
    }

    /// Answer `/follow` and `/unfollow`.
    ///
    /// **The session has to still be open.** Subscribing to one that has closed
    /// is a message saying it worked about something that does not exist, and
    /// the entry would then sit in the set unprintable — every listing shows
    /// only what is running, so a subscription none of them can name is one
    /// nobody can find to drop.
    ///
    /// Both directions move all three announcements together — a finished turn,
    /// a parked question, an agent that stopped — because that is the honest
    /// reading of being told to say nothing about a session. Half a subscription
    /// is a mode whose rule nobody could state.
    pub fn follow(&mut self, chat: &str, aim: Aim, on: bool, open: &[RemoteSession]) -> String {
        let word = if on { "follow" } else { "unfollow" };
        let uid = match aim {
            // Not read as the bound session: the user named something, it just
            // was not a number. On `/unfollow` that would silence a conversation
            // nobody typed, which is the mistake whose only symptom is nothing
            // happening afterwards.
            Aim::Unreadable => {
                return format!(
                    "Which session? /{word} <number>, or /{word} on its own for the one this chat is pointed at.\n\n{}",
                    self.listing(chat, open)
                );
            }
            Aim::Bound => match self.bound(chat) {
                Some(uid) => uid,
                None => return self.not_pointed(chat, word, open),
            },
            Aim::Session(uid) => uid,
        };
        if !open.iter().any(|session| session.uid == uid) {
            return format!("There's no session {uid}.\n\n{}", self.listing(chat, open));
        }
        let changed = if on {
            self.followed
                .entry(chat.to_string())
                .or_default()
                .insert(uid)
        } else {
            // The chat's own set and not every chat's: one reader losing
            // interest says nothing about what anybody else asked for.
            self.followed
                .get_mut(chat)
                .is_some_and(|set| set.remove(&uid))
        };
        match (on, changed) {
            (true, true) => format!(
                "Following {uid}. You'll hear here when it finishes a turn, when it stops to \
                 ask you something, and if the agent dies. /unfollow {uid} to stop."
            ),
            (true, false) => format!("You were already following {uid}."),
            // Named rather than left to be inferred: this is a command whose
            // whole effect is that nothing happens afterwards, so the reply is
            // the only evidence it did anything at all.
            (false, true) => format!(
                "Stopped following {uid}. Nothing about it reaches this chat now — not a \
                 finished turn, not a question it stops on, not the agent dying. It keeps \
                 running, and /sessions still shows it."
            ),
            (false, false) => format!("You weren't following {uid}, so nothing changed."),
        }
    }

    /// Drop every binding and every subscription naming a session that is no
    /// longer open.
    ///
    /// **One policy, applied wherever the open sessions are already in hand**,
    /// rather than at each command that happens to notice a session has gone.
    /// Noticing separately is how a chat ends up still subscribed to a
    /// conversation it can no longer be told it is subscribed to: every listing
    /// shows only what is running, so an entry naming a closed session is
    /// unprintable, and an unprintable entry is one nobody can find to remove.
    ///
    /// A uid is never reused — the salt only counts up — so a stale entry could
    /// not have started announcing a different conversation in the meantime.
    /// What it does instead is make `/status` unable to keep its promise of
    /// naming everything a chat will hear about.
    pub fn reconcile(&mut self, open: &[RemoteSession]) {
        let live: HashSet<u64> = open.iter().map(|session| session.uid).collect();
        self.pointed.retain(|_, uid| live.contains(uid));
        for set in self.followed.values_mut() {
            set.retain(|uid| live.contains(uid));
        }
    }

    /// Point `chat` at `uid`, replacing whatever it was pointed at.
    ///
    /// **Pointing at a session subscribes to it**, and that is what keeps a
    /// channel which says nothing by default from being a channel that appears
    /// broken. Pointing a chat somewhere is the gesture that says "this is the
    /// one I am attending to" — it is what somebody does before walking away
    /// from the machine — so requiring a second command afterwards would mean
    /// the ordinary path ends in silence, and silence is the one outcome a
    /// reader cannot tell apart from a crash.
    ///
    /// A number naming no open session leaves both facts as they were: the chat
    /// keeps typing where it was typing. Repointing it at nothing on the
    /// strength of a typo would take away the session somebody was working in
    /// and send the next prompt nowhere.
    pub fn point_at(&mut self, chat: &str, uid: u64, open: &[RemoteSession]) -> String {
        let Some(session) = open.iter().find(|session| session.uid == uid) else {
            return format!("There's no session {uid}.\n\n{}", self.listing(chat, open));
        };
        let already = self.follows(chat, uid);
        self.pointed.insert(chat.to_string(), uid);
        self.followed
            .entry(chat.to_string())
            .or_default()
            .insert(uid);
        format!(
            "Pointed at {}.\nAnything you type here now goes to it as a prompt{}",
            session.line().trim_start(),
            if already {
                ", and you were already following it."
            } else {
                ", and you'll hear about it here — /unfollow if you'd rather not."
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowing(ids: &[&str]) -> Chats {
        Chats::new(ids.iter().map(|s| s.to_string()).collect())
    }

    fn session(uid: u64) -> RemoteSession {
        RemoteSession {
            uid,
            project: format!("project-{uid}"),
            conversation: Some(format!("conversation {uid}")),
            agent: "Claude Code".to_string(),
            state: None,
        }
    }

    /// Pointing at a session is also subscribing to it, and that is the half of
    /// `/use` nobody asks for out loud: a channel that says nothing until asked
    /// would otherwise end its ordinary path in silence.
    #[test]
    fn pointing_a_chat_at_a_session_also_follows_it() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];

        chats.point_at("7", 3, &open);

        assert_eq!(chats.bound("7"), Some(3));
        assert!(chats.follows("7", 3));
    }

    /// The one cleanup, and the reason this type exists. An entry naming a
    /// session that has closed cannot be printed by anything that only lists
    /// what is running, so it is also an entry nobody could afterwards remove —
    /// and a chat still pointed at a dead session sends its next prompt nowhere.
    #[test]
    fn reconcile_drops_what_named_a_session_that_has_closed() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3), session(9)];
        chats.point_at("7", 3, &open);
        chats.point_at("7", 9, &open);

        chats.reconcile(&[session(3)]);

        assert_eq!(chats.bound("7"), None, "the binding named 9, which is gone");
        assert!(chats.follows("7", 3), "3 is still open and still followed");
        assert!(!chats.follows("7", 9), "9 closed, so the subscription goes");
    }

    /// The two marks a row can carry, read out of the margin rather than out of
    /// the rest of the line.
    fn mark_for(listing: &str, uid: u64) -> String {
        let head = format!("{uid} · project-{uid}");
        listing
            .lines()
            .find(|line| line.contains(&head))
            .unwrap_or_else(|| panic!("no row for session {uid} in:\n{listing}"))
            .chars()
            .take(2)
            .collect()
    }

    /// Two questions are asked while reading one row — where does what I type
    /// go, and which of these will tell me anything — so they get one margin
    /// column and two marks. The dot is the whole point: a followed session that
    /// is not the pointed-at one is otherwise indistinguishable from a silent
    /// row.
    #[test]
    fn the_listing_tells_where_typing_goes_apart_from_what_is_merely_followed() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3), session(9), session(11)];
        chats.point_at("7", 9, &open);
        chats.point_at("7", 3, &open);

        let listing = chats.listing("7", &open);

        assert_eq!(mark_for(&listing, 3), "→ ", "3 is where typing goes");
        assert_eq!(mark_for(&listing, 9), "• ", "9 is followed, not pointed at");
        assert_eq!(mark_for(&listing, 11), "  ", "11 was never asked for");
    }

    /// A number that names nothing leaves the chat where it was. Repointing it
    /// at nothing would take away the session somebody was working in, on the
    /// strength of a typo, and the next prompt would go nowhere.
    #[test]
    fn pointing_at_a_session_that_is_not_open_changes_nothing() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);

        let said = chats.point_at("7", 99, &open);

        assert!(
            said.contains("99"),
            "the reply names what was typed: {said}"
        );
        assert_eq!(chats.bound("7"), Some(3), "still pointed where it was");
        assert!(!chats.follows("7", 99));
    }

    /// The subscription is said out loud rather than left to be discovered:
    /// `/use` reads as "send my typing here", so a chat that then started
    /// announcing turns without having said so would be the bridge acting on its
    /// own. Said once — the second `/use` on the same session has nothing new to
    /// report.
    #[test]
    fn pointing_says_that_it_also_subscribed_only_the_first_time() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];

        let first = chats.point_at("7", 3, &open);
        chats.point_at("7", 99, &open);
        let again = chats.point_at("7", 3, &open);

        assert!(first.contains("you'll hear about it here"), "{first}");
        assert!(again.contains("already following"), "{again}");
    }

    /// A chat follows many and types into one, so subscribing must not move
    /// where the typing goes. The reply distinguishes doing something from
    /// having already done it: a command that answers the same way either way
    /// teaches the reader to distrust it.
    #[test]
    fn following_a_session_leaves_the_typing_where_it_was() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3), session(9)];
        chats.point_at("7", 3, &open);

        let first = chats.follow("7", Aim::Session(9), true, &open);
        let again = chats.follow("7", Aim::Session(9), true, &open);

        assert!(chats.follows("7", 9));
        assert_eq!(chats.bound("7"), Some(3), "typing still goes to 3");
        assert!(first.contains("Following 9"), "{first}");
        assert!(again.contains("already following 9"), "{again}");
    }

    /// A word where a number was meant is refused rather than read as the bound
    /// session. Falling back would silence a conversation nobody named, and what
    /// that costs is every message it would have sent, until somebody thinks to
    /// wonder why it went quiet.
    #[test]
    fn a_word_where_a_number_was_meant_never_falls_back_to_the_bound_session() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);

        let said = chats.follow("7", Aim::Unreadable, false, &open);

        assert!(chats.follows("7", 3), "3 was never named, so it stays");
        assert!(said.contains("Which session?"), "{said}");
    }

    /// Unfollowing something already silent is not an error, but it must not
    /// claim to have done something: the whole effect of this command is that
    /// nothing arrives afterwards, so the reply is the only evidence there is.
    #[test]
    fn unfollowing_twice_says_the_second_one_changed_nothing() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);

        let first = chats.follow("7", Aim::Bound, false, &open);
        let again = chats.follow("7", Aim::Session(3), false, &open);

        assert!(!chats.follows("7", 3));
        assert!(first.contains("Stopped following 3"), "{first}");
        assert!(again.contains("weren't following 3"), "{again}");
    }

    /// The audience is walked out of the allow list rather than out of the
    /// subscriptions, so a chat that has since been taken off the list cannot be
    /// told anything by an entry it left behind. Being allowed is permission;
    /// following is the reader's own choice; a session's news needs both.
    #[test]
    fn the_audience_is_the_chats_that_asked_and_are_still_allowed() {
        let mut chats = allowing(&["7", "8"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);
        chats.point_at("9", 3, &open);

        assert_eq!(chats.audience_for(3), vec!["7".to_string()]);
        assert!(
            chats.audience_for(11).is_empty(),
            "nobody asked about 11, so nobody hears about it"
        );
    }

    fn origin(uid: u64) -> Origin {
        Origin {
            uid,
            agent: "Claude Code".to_string(),
            project: "onehand".to_string(),
            conversation: Some("fix the rail".to_string()),
        }
    }

    /// The number leads the line under the headline, spelled the way the listing
    /// spells it, because it is what a reply is typed against. Without it a
    /// message about a waiting agent means opening `/sessions` and matching a
    /// name to a row before anything can be said back.
    #[test]
    fn an_announcement_names_the_session_a_reply_would_be_typed_against() {
        let mut chats = allowing(&["7", "8"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);

        let out = chats.announcement(&origin(3), &Announcement::plain(Away::TurnEnded));

        assert_eq!(out.len(), 1, "8 never asked about 3");
        assert_eq!(out[0].chat, "7");
        assert_eq!(
            out[0].text,
            "Claude Code finished a turn\n3 · onehand — fix the rail"
        );
    }

    /// Silence being the default is what makes this command necessary: every
    /// fact that decides whether anything arrives is invisible from the far
    /// side. Following nothing looks exactly like being at the keyboard, which
    /// looks exactly like a bridge whose process died an hour ago.
    #[test]
    fn status_says_why_a_chat_that_asked_for_nothing_hears_nothing() {
        let chats = allowing(&["7"]);
        let open = vec![session(3)];

        let said = chats.status("7", false, &open);

        assert!(said.contains("Away is off"), "{said}");
        assert!(said.contains("isn't pointed at a session"), "{said}");
        assert!(said.contains("following nothing"), "{said}");
    }

    /// It names them rather than counting them, because the point is to check
    /// the list against what you believe you asked for. A count answers a
    /// question nobody has.
    #[test]
    fn status_names_what_it_follows_rather_than_counting_it() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3), session(9), session(11)];
        chats.point_at("7", 3, &open);
        chats.follow("7", Aim::Session(9), true, &open);

        let said = chats.status("7", true, &open);

        assert!(said.contains("Away is on"), "{said}");
        assert!(said.contains("Pointed at 3"), "{said}");
        assert!(said.contains("hear about 2 of 3 open sessions"), "{said}");
        assert!(said.contains("conversation 9"), "9 is named: {said}");
        assert!(
            !said.contains("conversation 11"),
            "11 is not followed, so it is not named: {said}"
        );
    }

    /// Two different answers that both end in nothing arriving: this chat asked
    /// for nothing, and the app is running nothing. Offering to follow a session
    /// when there is none to follow sends the reader looking for a list that is
    /// empty.
    #[test]
    fn status_with_nothing_running_says_so_rather_than_that_you_follow_nothing() {
        let chats = allowing(&["7"]);

        let said = chats.status("7", false, &[]);

        assert!(said.contains("No sessions are open"), "{said}");
        assert!(!said.contains("following nothing"), "{said}");
    }

    /// The line a reader on the far side needs and a reader at the window does
    /// not. Somebody looking at the app can read the card the agent parked;
    /// somebody holding a phone has only this, so it travels with the headline
    /// and the buttons that answer it.
    #[test]
    fn an_announcement_carries_the_detail_a_phone_has_no_other_way_to_see() {
        let mut chats = allowing(&["7"]);
        let open = vec![session(3)];
        chats.point_at("7", 3, &open);
        let buttons = vec![vec![Button {
            label: "Allow".to_string(),
            data: "p:3:1:0".to_string(),
        }]];

        let out = chats.announcement(
            &origin(3),
            &Announcement {
                away: Away::LinkLost,
                detail: Some("Run `cargo test`?".to_string()),
                buttons: buttons.clone(),
            },
        );

        assert_eq!(out.len(), 1);
        assert!(
            out[0].text.ends_with("\n\nRun `cargo test`?"),
            "the detail comes last, off the headline: {}",
            out[0].text
        );
        assert_eq!(out[0].buttons, buttons);
    }

    fn row(place: u64) -> ArchiveRow {
        ArchiveRow {
            root: format!("/tmp/project-{place}").into(),
            project: format!("project-{place}"),
            dir: format!("/tmp/store/{place}").into(),
            agent: "Claude Code".to_string(),
            title: format!("saved {place}"),
            updated: place,
        }
    }

    /// A saved conversation has no small stable name to put in a message, so a
    /// chat holds a *place* in the listing it was sent. The listing counts from
    /// one because that is how it prints, and the number typed back is read
    /// against exactly the listing that went out.
    #[test]
    fn an_archive_place_counts_from_one_as_the_listing_prints_it() {
        let mut chats = allowing(&["7"]);

        chats.remember_archive("7", vec![row(1), row(2)]);

        assert_eq!(
            chats.archive_at("7", 1).map(|row| row.title),
            Some("saved 1".to_string())
        );
        assert_eq!(
            chats.archive_at("7", 2).map(|row| row.title),
            Some("saved 2".to_string())
        );
        assert!(chats.archive_at("7", 0).is_none(), "there is no zeroth row");
        assert!(
            chats.archive_at("7", 3).is_none(),
            "past the end of the page"
        );
        assert!(
            chats.archive_at("8", 1).is_none(),
            "one chat's listing is not another's"
        );
    }

    /// Replaced whole rather than added to, so it is one listing per chat and
    /// not a history of them: a number typed back has to count into the page
    /// that was last sent, and a page that grew would move every row under it.
    #[test]
    fn asking_for_the_archive_again_replaces_the_page_it_counts_into() {
        let mut chats = allowing(&["7"]);
        chats.remember_archive("7", vec![row(1), row(2)]);

        chats.remember_archive("7", vec![row(9)]);

        assert_eq!(
            chats.archive_at("7", 1).map(|row| row.title),
            Some("saved 9".to_string())
        );
        assert!(chats.archive_at("7", 2).is_none(), "the old page is gone");
    }

    /// The gate the whole bridge rests on, asked through this type so the one
    /// place that could accidentally start replying is the one place that holds
    /// the list. An enabled bridge whose list was never filled in has to be
    /// reachable by nobody, not by everybody.
    #[test]
    fn an_empty_allow_list_lets_nobody_in() {
        assert!(!allowing(&[]).allows("7"));
        assert!(allowing(&["7", "8"]).allows("8"));
        assert!(!allowing(&["7", "8"]).allows("9"));
    }

    /// What is about the bridge rather than about a session goes to everyone
    /// allowed — the away switch thrown at the keyboard is the whole of it
    /// today. It has no session to be subscribed to, and a reader who has asked
    /// for nothing yet is exactly who needs telling that the machine has been
    /// left.
    #[test]
    fn news_about_the_bridge_reaches_chats_that_follow_nothing() {
        let chats = allowing(&["7", "8"]);

        assert_eq!(chats.everyone(), ["7".to_string(), "8".to_string()]);
        assert!(
            chats.audience_for(3).is_empty(),
            "a session's own news still needs asking for"
        );
    }
}
