//! A session's life: the conversation it starts as (resumed from a file, or
//! forked from another), what it hands on when it is restarted, and the lines
//! onehand writes into it in its own voice.

use super::{Effect, SessionDrive, Signal};
use crate::chat::model::ChatItem;
use crate::chat::store::ConversationSnapshot;
use crate::chat::{ForkSeed, Now};

impl SessionDrive {
    /// Start as the conversation `snapshot` holds: its transcript, its mark
    /// in the file, its settings and when it was last active.
    pub fn resume(&mut self, snapshot: ConversationSnapshot) -> Vec<Effect> {
        self.chat.resume_from(snapshot);
        vec![Effect::Changed]
    }

    /// Start as a fork of another conversation, seeded with its history up
    /// to the point, all of it still to be written under the fork's own id.
    pub fn fork(&mut self, seed: ForkSeed) -> Vec<Effect> {
        self.chat.adopt_fork(seed);
        vec![Effect::Changed]
    }

    /// Hand the conversation on to the session that restarts this one: a
    /// move, carrying the mark in the file, so this one's drop does not write
    /// it a second time and the next continues the same file.
    ///
    /// **A save still on its way is honoured.** The mark it carries is the
    /// one that save moved; were the save to fail once this drive is gone,
    /// its turn would be in neither write. So the next session writes the
    /// whole transcript at its first save instead, as a close does, and the
    /// store drops the lines of any save built before that write lands.
    pub fn take_snapshot(&mut self, now: Now) -> Option<ConversationSnapshot> {
        self.chat.tell_time(now);
        let mut snapshot = self.chat.take_snapshot()?;
        snapshot.rewrite = self.saving > 0;
        Some(snapshot)
    }

    /// Add a line in onehand's own voice: what a run driving the session
    /// did, for the person reading it.
    pub fn note(&mut self, text: String) -> Vec<Effect> {
        self.chat.items.push(ChatItem::notice(text));
        vec![Effect::Signal(Signal::Appended)]
    }
}
