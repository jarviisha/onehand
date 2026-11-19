//! A subagent the agent started, drawn as one card in the parent transcript.
//!
//! The card holds the subagent's own steps, folded by a reducer of their own:
//! the parent's turn bookkeeping (user bubbles, the replay window, what is on
//! disk) means nothing inside a card, so only the content kinds a subagent
//! sends are taken, and everything else it says is dropped.

use super::apply::{plan, seal_thought, settle_steps, tool_call, tool_update};
use super::items::{ChatItem, Md, Thought, TranscriptItemId};
use super::{Chat, Replay};
use crate::acp::{AcpEvent, SubagentEnd, ToolStatus};
use crate::Now;

/// Where a subagent is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentState {
    Working,
    Completed,
    Failed,
    Cancelled,
    /// The adapter went away under it: nothing will ever end it.
    Interrupted,
}

impl SubagentState {
    /// The state as one lowercase word, as it is saved and exported.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

/// One subagent and the steps it took.
pub struct SubagentItem {
    pub id: String,
    pub name: String,
    /// What it was asked, whole, when that says more than its name.
    pub task: Option<String>,
    pub state: SubagentState,
    pub items: Vec<ChatItem>,
    /// When it was handed the work, in epoch seconds; `None` for a card read
    /// back from disk, whose length is in `secs`.
    pub started: Option<u64>,
    /// How long it ran, once it has ended; `None` while it works and for a
    /// card saved before lengths were kept.
    pub secs: Option<u64>,
}

/// How a subagent stands, as the strip over the composer counts it, the most
/// urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    /// Working, with a permission or a question of its own waiting on the
    /// person.
    Waiting,
    Failed,
    Running,
    Done,
    /// Cancelled, or cut off by the agent going away.
    Stopped,
}

impl Standing {
    /// The word a count of it is said with.
    pub fn word(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Failed => "failed",
            Self::Running => "running",
            Self::Done => "done",
            Self::Stopped => "stopped",
        }
    }
}

impl SubagentItem {
    fn new(id: String, name: String, task: &str, now: Option<Now>) -> Self {
        // The adapter fills the task with its own words when it has none to
        // give; a line that only repeats the name says nothing either.
        let line = task.lines().next().unwrap_or("").trim();
        let task = (!line.is_empty() && line != name && !line.starts_with("Delegated task for "))
            .then(|| task.trim().to_string());
        Self {
            id,
            name,
            task,
            state: SubagentState::Working,
            items: Vec::new(),
            started: now.map(|now| now.secs),
            secs: None,
        }
    }

    /// Stop the card: whatever still runs inside it, a nested card included,
    /// will never be settled.
    fn close(&mut self, state: SubagentState, now: Option<Now>) {
        self.state = state;
        self.secs = self
            .started
            .zip(now)
            .map(|(started, now)| now.secs.saturating_sub(started));
        interrupt(&mut self.items, now);
        seal_thought(&mut self.items, now);
        // A cancelled subagent was stopped; any other end left its steps
        // failed.
        let to = match state {
            SubagentState::Cancelled => ToolStatus::Stopped,
            SubagentState::Working
            | SubagentState::Completed
            | SubagentState::Failed
            | SubagentState::Interrupted => ToolStatus::Failed,
        };
        settle_steps(&mut self.items, now, to);
    }
}

impl SubagentItem {
    /// The card's key: what its items' addresses name it by. Taken from its
    /// id, which is the subagent's own session and never shared.
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.id.hash(&mut hasher);
        hasher.finish()
    }

    /// How many steps it took: its tool calls, not its words or thoughts.
    pub fn steps(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, ChatItem::Tool(_)))
            .count()
    }
}

impl Chat {
    /// The card `key` names, wherever it is, the history and nested cards
    /// included.
    pub fn card(&self, key: u64) -> Option<&SubagentItem> {
        by_key(&self.history, key).or_else(|| by_key(&self.items, key))
    }

    /// The item `target` addresses, wherever it is.
    pub fn item_at(&self, target: TranscriptItemId) -> Option<&ChatItem> {
        self.list(target).get(target.index())
    }

    /// How `card` stands: whether it waits on the person, has failed, works
    /// or has ended, and how.
    pub fn standing(&self, card: &SubagentItem) -> Standing {
        match card.state {
            SubagentState::Working if self.asks_for(&card.id) => Standing::Waiting,
            SubagentState::Working => Standing::Running,
            SubagentState::Failed => Standing::Failed,
            SubagentState::Completed => Standing::Done,
            SubagentState::Cancelled | SubagentState::Interrupted => Standing::Stopped,
        }
    }

    /// The subagents handed work since the last prompt, in the order they
    /// were, each with how it stands: what the strip over the composer counts.
    pub fn turn_subagents(&self) -> Vec<(u64, Standing)> {
        let start = self
            .items
            .iter()
            .rposition(|item| matches!(item, ChatItem::User(_)))
            .map_or(0, |at| at + 1);
        self.items[start..]
            .iter()
            .filter_map(|item| match item {
                ChatItem::Subagent(card) => Some((card.key(), self.standing(card))),
                _ => None,
            })
            .collect()
    }

    /// The subagents of the turn `key` was handed work in, in order, itself
    /// included: what the float steps through. A nested one's are the cards
    /// beside it in the card it is in.
    pub fn subagents_beside(&self, key: u64) -> Vec<u64> {
        let all: Vec<&ChatItem> = self.history.iter().chain(self.items.iter()).collect();
        beside(&all, key).unwrap_or_default()
    }

    /// Whether a permission or a question from subagent `id` waits.
    fn asks_for(&self, id: &str) -> bool {
        self.items.iter().any(|item| match item {
            ChatItem::Permission(p) => p.resolved.is_none() && p.req.child.as_deref() == Some(id),
            ChatItem::Ask(a) => a.resolved.is_none() && a.req.child.as_deref() == Some(id),
            _ => false,
        })
    }

    /// How many subagents are still working, nested ones included.
    pub fn subagents_working(&self) -> usize {
        working(&self.items)
    }

    /// The card of the subagent `id`, wherever it is nested.
    pub(super) fn subagent(&self, id: &str) -> Option<&SubagentItem> {
        find(&self.items, id)
    }

    /// Fold a subagent's spawn, end or step into its card; whether anything
    /// changed. A step for a subagent that was never spawned, or has ended,
    /// goes nowhere: it never lands in the parent.
    pub(super) fn apply_subagent(&mut self, event: AcpEvent) -> bool {
        // A resume replays what the agent did, and a card replayed now would
        // be working for ever: its end is not part of the replay.
        if !matches!(self.replay, Replay::Settled) {
            return false;
        }
        match event {
            AcpEvent::SubagentSpawned { id, name, task } => {
                if self.subagent(&id).is_some() {
                    return false;
                }
                seal_thought(&mut self.items, self.told);
                self.items.push(ChatItem::Subagent(SubagentItem::new(
                    id, name, &task, self.told,
                )));
                true
            }
            AcpEvent::SubagentEnded { id, end } => self.end_subagent(&id, end),
            AcpEvent::InChild { child, event } => match *event {
                // A nested subagent's end is found wherever its card is.
                AcpEvent::SubagentEnded { id, end } => self.end_subagent(&id, end),
                AcpEvent::SubagentSpawned { ref id, .. } if self.subagent(id).is_some() => false,
                event => match working_mut(&mut self.items, &child) {
                    Some(card) => step(&mut card.items, event, self.told),
                    None => false,
                },
            },
            AcpEvent::Connected { .. }
            | AcpEvent::SessionId(_)
            | AcpEvent::AgentChunk { .. }
            | AcpEvent::ThoughtChunk(_)
            | AcpEvent::UserChunk(_)
            | AcpEvent::ToolCall(_)
            | AcpEvent::ToolUpdate(_)
            | AcpEvent::Plan(_)
            | AcpEvent::Usage(_)
            | AcpEvent::Background(_)
            | AcpEvent::StopRefused { .. }
            | AcpEvent::Permission(_)
            | AcpEvent::Elicitation(_)
            | AcpEvent::AvailableCommands(_)
            | AcpEvent::Modes { .. }
            | AcpEvent::ModeChanged(_)
            | AcpEvent::PickConfirmed { .. }
            | AcpEvent::PickRefused { .. }
            | AcpEvent::ConfigOptions(_)
            | AcpEvent::TerminalOutput { .. }
            | AcpEvent::TerminalExit { .. }
            | AcpEvent::TurnEnded { .. }
            | AcpEvent::Error { .. }
            | AcpEvent::Interrupted
            | AcpEvent::LoginRequired { .. }
            | AcpEvent::Notice(_)
            | AcpEvent::Echo(_)
            | AcpEvent::At(_)
            | AcpEvent::Disconnected(_) => false,
        }
    }

    fn end_subagent(&mut self, id: &str, end: SubagentEnd) -> bool {
        let now = self.told;
        let Some(card) = working_mut(&mut self.items, id) else {
            return false;
        };
        let state = match end {
            SubagentEnd::Completed => SubagentState::Completed,
            SubagentEnd::Failed => SubagentState::Failed,
            SubagentEnd::Cancelled => SubagentState::Cancelled,
            SubagentEnd::Disconnected => SubagentState::Interrupted,
        };
        card.close(state, now);
        true
    }

    /// How much of history ⧺ items can be written now: up to the first card
    /// with anything still working in it. The file is only ever added to, so
    /// a card written before it ends would stay half a card for ever; it and
    /// everything after it wait until it has ended.
    pub(super) fn saveable_len(&self) -> usize {
        let all = self.history.iter().chain(self.items.iter());
        all.clone()
            .position(|item| working(std::slice::from_ref(item)) > 0)
            .unwrap_or(all.count())
    }

    /// Every card still working is interrupted: the adapter that would end
    /// them is gone.
    pub(super) fn interrupt_subagents(&mut self) {
        // History too: a restart carries a working card over into it.
        interrupt(&mut self.history, self.told);
        interrupt(&mut self.items, self.told);
    }
}

fn working(items: &[ChatItem]) -> usize {
    items
        .iter()
        .map(|item| match item {
            ChatItem::Subagent(card) => {
                usize::from(card.state == SubagentState::Working) + working(&card.items)
            }
            _ => 0,
        })
        .sum()
}

fn by_key(items: &[ChatItem], key: u64) -> Option<&SubagentItem> {
    items.iter().find_map(|item| match item {
        ChatItem::Subagent(card) if card.key() == key => Some(card),
        ChatItem::Subagent(card) => by_key(&card.items, key),
        _ => None,
    })
}

/// The cards of the turn holding card `key` at its top, or of the card
/// holding it; `None` where it is in neither.
fn beside(items: &[&ChatItem], key: u64) -> Option<Vec<u64>> {
    let at = items
        .iter()
        .position(|item| matches!(item, ChatItem::Subagent(card) if card.key() == key));
    match at {
        Some(at) => {
            let start = items[..at]
                .iter()
                .rposition(|item| matches!(item, ChatItem::User(_)))
                .map_or(0, |p| p + 1);
            let end = items[at..]
                .iter()
                .position(|item| matches!(item, ChatItem::User(_)))
                .map_or(items.len(), |p| at + p);
            Some(
                items[start..end]
                    .iter()
                    .filter_map(|item| match item {
                        ChatItem::Subagent(card) => Some(card.key()),
                        _ => None,
                    })
                    .collect(),
            )
        }
        None => items.iter().find_map(|item| match item {
            ChatItem::Subagent(card) => {
                let inner: Vec<&ChatItem> = card.items.iter().collect();
                beside(&inner, key)
            }
            _ => None,
        }),
    }
}

fn find<'a>(items: &'a [ChatItem], id: &str) -> Option<&'a SubagentItem> {
    items.iter().find_map(|item| match item {
        ChatItem::Subagent(card) if card.id == id => Some(card),
        ChatItem::Subagent(card) => find(&card.items, id),
        _ => None,
    })
}

/// The card `id` while it still takes steps.
fn working_mut<'a>(items: &'a mut [ChatItem], id: &str) -> Option<&'a mut SubagentItem> {
    for item in items {
        if let ChatItem::Subagent(card) = item {
            if card.id == id {
                return (card.state == SubagentState::Working).then_some(card);
            }
            if let Some(found) = working_mut(&mut card.items, id) {
                return Some(found);
            }
        }
    }
    None
}

fn interrupt(items: &mut [ChatItem], now: Option<Now>) {
    for item in items {
        if let ChatItem::Subagent(card) = item {
            interrupt(&mut card.items, now);
            if card.state == SubagentState::Working {
                card.close(SubagentState::Interrupted, now);
            }
        }
    }
}

/// One step of a subagent, into its card; whether it was taken. Only what a
/// subagent's transcript holds is: the rest of what its session says is about
/// the session, which the parent already shows.
fn step(items: &mut Vec<ChatItem>, event: AcpEvent, now: Option<Now>) -> bool {
    match event {
        AcpEvent::AgentChunk { text: s, .. } => {
            seal_thought(items, now);
            match items.last_mut() {
                Some(ChatItem::Agent(md)) => md.push(&s),
                _ => items.push(ChatItem::Agent(Md::parse(&s))),
            }
        }
        AcpEvent::ThoughtChunk(s) => match items.last_mut() {
            Some(ChatItem::Thought(th)) if th.elapsed_secs.is_none() => th.md.push(&s),
            _ => items.push(ChatItem::Thought(Thought::live(&s, now))),
        },
        AcpEvent::ToolCall(tc) => {
            seal_thought(items, now);
            tool_call(items, tc, now);
        }
        AcpEvent::ToolUpdate(tu) => tool_update(items, tu, now),
        AcpEvent::Plan(entries) => {
            seal_thought(items, now);
            plan(items, entries);
        }
        AcpEvent::SubagentSpawned { id, name, task } => {
            seal_thought(items, now);
            items.push(ChatItem::Subagent(SubagentItem::new(id, name, &task, now)));
        }
        AcpEvent::Connected { .. }
        | AcpEvent::SessionId(_)
        | AcpEvent::UserChunk(_)
        | AcpEvent::Usage(_)
        | AcpEvent::Background(_)
        | AcpEvent::SubagentEnded { .. }
        | AcpEvent::InChild { .. }
        | AcpEvent::StopRefused { .. }
        | AcpEvent::Permission(_)
        | AcpEvent::Elicitation(_)
        | AcpEvent::AvailableCommands(_)
        | AcpEvent::Modes { .. }
        | AcpEvent::ModeChanged(_)
        | AcpEvent::PickConfirmed { .. }
        | AcpEvent::PickRefused { .. }
        | AcpEvent::ConfigOptions(_)
        | AcpEvent::TerminalOutput { .. }
        | AcpEvent::TerminalExit { .. }
        | AcpEvent::TurnEnded { .. }
        | AcpEvent::Error { .. }
        | AcpEvent::Interrupted
        | AcpEvent::LoginRequired { .. }
        | AcpEvent::Notice(_)
        | AcpEvent::Echo(_)
        | AcpEvent::At(_)
        | AcpEvent::Disconnected(_) => return false,
    }
    true
}

#[cfg(test)]
mod tests;
