//! Forking a conversation from one of the person's prompts: a new session
//! whose history runs up to just before that prompt, the original untouched.
//!
//! **The point is the agent message just before the prompt, and nothing
//! else.** An agent forks after a message it names by id; when the turn before
//! the prompt did not end on an answer whose id is known (a tool step, an
//! interruption, a failure, or a conversation saved before ids were kept),
//! there is no message to name that ends where the person pointed, and a fork
//! that landed somewhere near it would be worse than none.

use super::{Chat, ChatItem, Replay, TranscriptItemId};
use crate::attachment::AttachmentSnapshot;
use crate::chat::store::{ConversationSnapshot, Prefs};

/// Where a prompt forks from, and what the fork starts with.
#[derive(Debug, Clone)]
pub struct ForkPoint {
    /// The session forked from.
    pub from: String,
    /// The id of the agent message the fork's history ends on.
    pub message: String,
    /// The prompt's text, to put in the fork's composer.
    pub text: String,
    /// The prompt's attachments, staged with its text.
    pub attachments: Vec<AttachmentSnapshot>,
    /// The fork's name until it is renamed.
    pub title: String,
}

/// What a fork's transcript starts as: the original's, cut at the point.
pub struct ForkSeed {
    items: Vec<ChatItem>,
    prefs: Prefs,
    title: String,
}

impl Chat {
    /// Where the prompt at `target` forks from; `None` when it cannot be
    /// forked from at all.
    pub fn fork_point(&self, target: TranscriptItemId) -> Option<ForkPoint> {
        if !self.forkable {
            return None;
        }
        let from = self.session_id.clone()?;
        let at = target.index();
        let ChatItem::User(prompt) = self.list(target).get(at)? else {
            return None;
        };
        let mut before = self.list(target)[..at].iter().rev().chain(match target {
            TranscriptItemId::Live(_) => self.history.iter().rev(),
            TranscriptItemId::History(_) => [].iter().rev(),
        });
        let ChatItem::Agent(answer) = before.next()? else {
            return None;
        };
        let message = answer.message.clone()?;
        // The first prompt has nothing before it worth keeping: a new session
        // does the same.
        if !before.any(|item| matches!(item, ChatItem::User(_))) {
            return None;
        }
        let title = match self.conversation_title() {
            Some(title) => format!("Fork: {title}"),
            None => "Fork".to_string(),
        };
        Some(ForkPoint {
            from,
            message,
            text: prompt.text.clone(),
            attachments: prompt.attachments.clone(),
            title,
        })
    }

    /// Start this conversation as a fork: its history is the seed, all of it
    /// still to be written, under the fork's own id once the agent names it.
    pub fn adopt_fork(&mut self, seed: ForkSeed) {
        self.custom_title = Some(seed.title);
        self.arm_prefs(
            seed.prefs.mode,
            seed.prefs
                .config
                .into_iter()
                .map(|c| (c.id, c.value))
                .collect(),
        );
        self.items.clear();
        self.terminals.clear();
        self.history = seed.items;
        self.persisted = 0;
        self.replay = Replay::Settled;
        self.rewrite = false;
        self.touch();
    }
}

/// The original's saved transcript cut just after the agent message `point`
/// names; `None` when the saved transcript does not hold it. Blocking only in
/// that the snapshot was read from disk.
pub fn fork_seed(snapshot: ConversationSnapshot, point: &ForkPoint) -> Option<ForkSeed> {
    let mut items = snapshot.items;
    let end = items.iter().position(|item| {
        matches!(item, ChatItem::Agent(md) if md.message.as_deref() == Some(point.message.as_str()))
    })?;
    items.truncate(end + 1);
    Some(ForkSeed {
        items,
        prefs: snapshot.prefs,
        title: point.title.clone(),
    })
}

#[cfg(test)]
mod tests;
