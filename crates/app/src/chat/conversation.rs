//! Everything the pane knows about one session, in one place.
//!
//! **This exists to make an illegal state unrepresentable.** The same facts
//! used to live in six parallel maps keyed by the session's uid — the live
//! entity, the resume picker's choices, the root and spec to connect with, the
//! unseen badge, the scroll position, the event subscription. Nothing tied them
//! together, so "is this session connected?" was three `contains_key` calls in
//! a row, opening one meant remembering to write two maps, and closing one
//! meant remembering to clear six. A uid could sit in the picker map *and* the
//! connected map at once; only the order of the reads decided what the user
//! saw.
//!
//! Here the same question is one `match`, and a session that moves on cannot
//! leave half of itself behind because there is no half to leave.
//!
//! Not named `Session` — the workspace tree already has one of those, and it is
//! the *description* of a session (uid, root, agent) rather than its running
//! state. Not named `Chat` either: that is the model in core, and this owns one
//! at most.

use super::composer::Draft;
use super::session::ChatSession;
use super::viewport::Viewport;
use gpui::{Entity, Subscription};
use onehand_core::chat::ConvMeta;
use onehand_core::config::AgentSpec;
use std::path::PathBuf;

pub struct Conversation {
    /// The project root this session's agent runs in, and the agent it runs.
    ///
    /// Held for the session's whole life rather than only until it connects: a
    /// restart respawns from exactly these, so throwing them away at connect
    /// would make the first restart the last one.
    pub root: PathBuf,
    pub spec: AgentSpec,
    pub phase: SessionPhase,
    /// How this session's transcript is laid out and where it is scrolled, so
    /// coming back to it finds the conversation where it was left rather than
    /// snapped to the tail.
    pub viewport: Viewport,
    /// A turn ended here while nobody was looking.
    ///
    /// "Not looking" is two things at once: a session in the background, or any
    /// session while its window is not the active one. Both mean the same thing
    /// to the person who has to be told — something finished and you did not
    /// see it.
    pub unseen: bool,
    /// What was typed here and not sent.
    ///
    /// One composer serves the whole pane, so what it holds belongs to whatever
    /// was on screen at the time and has to come back here when the user moves
    /// away. Only a non-empty draft is kept, so a session never touched costs
    /// nothing.
    pub draft: Option<Draft>,
    /// Whether this conversation has ever had a live adapter in this pane.
    ///
    /// **What separates a first connect from a reconnect**, which want opposite
    /// things from the pane. Coming up for the first time there is nothing to
    /// look at yet and a transcript that is not live is a transcript that lies,
    /// so the pane waits out loud. Coming *back* up -- a restart, an adapter
    /// respawned after it died -- the conversation is right there on screen and
    /// hiding it for the seconds a spawn takes reads as data loss.
    ///
    /// One flag rather than a kind of connect passed in at each call site: the
    /// question is about the conversation's own history, and a caller that
    /// forgot to say which kind it was would blank a transcript the user was
    /// reading.
    pub was_live: bool,
    /// The last attempt to write this conversation to disk failed.
    ///
    /// Here rather than in a map beside the pane for the same reason everything
    /// else about a session is: closing one is a single `remove`, and a flag
    /// filed anywhere else is a flag somebody forgets to clear.
    ///
    /// It exists to be a **latch**. The transcript is written at the end of
    /// every turn, so a standing condition -- a full disk, a directory gone
    /// read-only -- fails every turn, and saying so every turn is a stream of
    /// identical messages that buries the first one. Only the change from
    /// working to failing is worth announcing, and the change back is what arms
    /// it to speak again.
    pub archive_failed: bool,
}

/// Where a session is between "the user asked for it" and "it is talking".
///
/// The three are exclusive **by construction**, which is the point: the middle
/// one used to be a separate map, and a session could be in it and connected at
/// the same time.
pub enum SessionPhase {
    /// Nothing is connected. Either the scan for past conversations is still
    /// running, or a restart has just dropped the old adapter and the new one
    /// is a line away.
    ///
    /// Deliberately not "connecting": no adapter exists in this state, which is
    /// exactly why a resume can still be chosen from it. Spawning first would
    /// start a fresh conversation and archive it before the user got to say
    /// which one they meant.
    Opening,
    /// The scan found past conversations, so the user chooses before anything
    /// connects.
    ChoosingHistory(Vec<ConvMeta>),
    /// Connected, and the transcript is live.
    ///
    /// The subscription is held rather than detached for the same reason the
    /// session's own pump is: dropping this phase drops the subscription *and*
    /// the session, which drops the pump, which kills the adapter. Nothing has
    /// to remember to shut an agent down.
    Live {
        session: Entity<ChatSession>,
        _watch: Subscription,
    },
}

impl Conversation {
    /// A session the user has just asked for, with nothing connected yet.
    pub fn opening(root: PathBuf, spec: AgentSpec) -> Self {
        Self {
            root,
            spec,
            phase: SessionPhase::Opening,
            viewport: Viewport::default(),
            unseen: false,
            draft: None,
            was_live: false,
            archive_failed: false,
        }
    }

    /// The live session, if there is one.
    pub fn session(&self) -> Option<&Entity<ChatSession>> {
        match &self.phase {
            SessionPhase::Live { session, .. } => Some(session),
            SessionPhase::Opening | SessionPhase::ChoosingHistory(_) => None,
        }
    }

    /// The past conversations to choose from, while that is what is showing.
    pub fn choices(&self) -> Option<&[ConvMeta]> {
        match &self.phase {
            SessionPhase::ChoosingHistory(past) => Some(past),
            SessionPhase::Opening | SessionPhase::Live { .. } => None,
        }
    }

    /// Whether nothing has connected yet, so a scan's result is still wanted.
    ///
    /// Asked before adopting a finished scan: the session may have been closed
    /// and reopened, or connected by hand, while the directory was being read.
    pub fn is_opening(&self) -> bool {
        matches!(self.phase, SessionPhase::Opening)
    }

    /// Put the session back to having no adapter, dropping the one it had.
    ///
    /// A restart goes through here rather than assigning the new phase over the
    /// old: the old adapter dies when its subscription and session drop, and
    /// doing that *after* spawning the replacement would briefly leave two
    /// processes on one conversation.
    pub fn disconnect(&mut self) {
        self.phase = SessionPhase::Opening;
    }
}

#[cfg(test)]
mod tests {
    use super::{Conversation, SessionPhase};
    use onehand_core::chat::ConvMeta;
    use onehand_core::config::AgentSpec;
    use std::path::PathBuf;

    fn conversation() -> Conversation {
        Conversation::opening(
            PathBuf::from("/tmp/project"),
            AgentSpec {
                name: "claude".to_string(),
                command: "npx".to_string(),
                args: Vec::new(),
            },
        )
    }

    fn meta() -> ConvMeta {
        ConvMeta {
            path: PathBuf::from("/tmp/sessions/one.json"),
            session_id: "one".to_string(),
            agent: "claude".to_string(),
            updated: 0,
            title: "a past conversation".to_string(),
            item_count: 4,
        }
    }

    /// Asking for a session is not connecting one. The scan for past
    /// conversations has to finish first, because connecting would start a
    /// fresh conversation and archive it before the user got to say they meant
    /// to resume an old one.
    #[test]
    fn a_new_conversation_has_nothing_connected() {
        let conv = conversation();
        assert!(conv.is_opening());
        assert!(conv.session().is_none());
        assert!(conv.choices().is_none());
    }

    /// The property the resume picker rests on: while the choice is open there
    /// is **no** adapter. As two maps this was expressible the other way -- a
    /// uid could be in the picker map and the connected map at once, and which
    /// one the user saw came down to the order of the reads.
    #[test]
    fn choosing_a_conversation_means_nothing_is_connected() {
        let mut conv = conversation();
        conv.phase = SessionPhase::ChoosingHistory(vec![meta()]);

        assert!(conv.session().is_none());
        assert_eq!(conv.choices().map(<[_]>::len), Some(1));
        assert!(
            !conv.is_opening(),
            "a finished scan must not be adopted over a choice already on screen"
        );
    }

    /// Dropping the adapter drops the choice with it, so a restart cannot land
    /// back on a picker the user answered a moment ago.
    #[test]
    fn disconnecting_returns_to_opening() {
        let mut conv = conversation();
        conv.phase = SessionPhase::ChoosingHistory(vec![meta()]);
        conv.disconnect();

        assert!(conv.is_opening());
        assert!(conv.choices().is_none());
        assert!(conv.session().is_none());
    }
}
