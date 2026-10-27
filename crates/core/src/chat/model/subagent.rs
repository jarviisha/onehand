//! A subagent the agent started, drawn as one card in the parent transcript.
//!
//! The card holds the subagent's own steps, folded by a reducer of their own:
//! the parent's turn bookkeeping (user bubbles, the replay window, what is on
//! disk) means nothing inside a card, so only the content kinds a subagent
//! sends are taken, and everything else it says is dropped.

use super::apply::{plan, seal_thought, settle_steps, tool_call, tool_update};
use super::items::{ChatItem, Md, Thought};
use super::{Chat, Replay};
use crate::acp::{AcpEvent, SubagentEnd};

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
    /// The first line of what it was asked, when that says more than its name.
    pub task: Option<String>,
    pub state: SubagentState,
    pub items: Vec<ChatItem>,
}

impl SubagentItem {
    fn new(id: String, name: String, task: &str) -> Self {
        // The adapter fills the task with its own words when it has none to
        // give; a line that only repeats the name says nothing either.
        let line = task.lines().next().unwrap_or("").trim();
        let task = (!line.is_empty() && line != name && !line.starts_with("Delegated task for "))
            .then(|| line.to_string());
        Self {
            id,
            name,
            task,
            state: SubagentState::Working,
            items: Vec::new(),
        }
    }

    /// Stop the card: whatever still runs inside it, a nested card included,
    /// will never be settled.
    fn close(&mut self, state: SubagentState) {
        self.state = state;
        interrupt(&mut self.items);
        seal_thought(&mut self.items);
        settle_steps(&mut self.items);
    }
}

impl Chat {
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
                seal_thought(&mut self.items);
                self.items
                    .push(ChatItem::Subagent(SubagentItem::new(id, name, &task)));
                true
            }
            AcpEvent::SubagentEnded { id, end } => self.end_subagent(&id, end),
            AcpEvent::InChild { child, event } => match *event {
                // A nested subagent's end is found wherever its card is.
                AcpEvent::SubagentEnded { id, end } => self.end_subagent(&id, end),
                AcpEvent::SubagentSpawned { ref id, .. } if self.subagent(id).is_some() => false,
                event => match working_mut(&mut self.items, &child) {
                    Some(card) => step(&mut card.items, event),
                    None => false,
                },
            },
            AcpEvent::Connected { .. }
            | AcpEvent::SessionId(_)
            | AcpEvent::AgentChunk(_)
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
            | AcpEvent::Disconnected(_) => false,
        }
    }

    fn end_subagent(&mut self, id: &str, end: SubagentEnd) -> bool {
        let Some(card) = working_mut(&mut self.items, id) else {
            return false;
        };
        card.close(match end {
            SubagentEnd::Completed => SubagentState::Completed,
            SubagentEnd::Failed => SubagentState::Failed,
            SubagentEnd::Cancelled => SubagentState::Cancelled,
            SubagentEnd::Disconnected => SubagentState::Interrupted,
        });
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
        interrupt(&mut self.history);
        interrupt(&mut self.items);
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

fn interrupt(items: &mut [ChatItem]) {
    for item in items {
        if let ChatItem::Subagent(card) = item {
            interrupt(&mut card.items);
            if card.state == SubagentState::Working {
                card.close(SubagentState::Interrupted);
            }
        }
    }
}

/// One step of a subagent, into its card; whether it was taken. Only what a
/// subagent's transcript holds is: the rest of what its session says is about
/// the session, which the parent already shows.
fn step(items: &mut Vec<ChatItem>, event: AcpEvent) -> bool {
    match event {
        AcpEvent::AgentChunk(s) => {
            seal_thought(items);
            match items.last_mut() {
                Some(ChatItem::Agent(md)) => md.push(&s),
                _ => items.push(ChatItem::Agent(Md::parse(&s))),
            }
        }
        AcpEvent::ThoughtChunk(s) => match items.last_mut() {
            Some(ChatItem::Thought(th)) if th.elapsed_secs.is_none() => th.md.push(&s),
            _ => items.push(ChatItem::Thought(Thought::live(&s))),
        },
        AcpEvent::ToolCall(tc) => {
            seal_thought(items);
            tool_call(items, tc);
        }
        AcpEvent::ToolUpdate(tu) => tool_update(items, tu),
        AcpEvent::Plan(entries) => {
            seal_thought(items);
            plan(items, entries);
        }
        AcpEvent::SubagentSpawned { id, name, task } => {
            seal_thought(items);
            items.push(ChatItem::Subagent(SubagentItem::new(id, name, &task)));
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
        | AcpEvent::Disconnected(_) => return false,
    }
    true
}

#[cfg(test)]
mod tests;
