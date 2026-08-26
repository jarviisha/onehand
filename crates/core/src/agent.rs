//! A session: one agent bound to one project root.
//!
//! Deliberately thin. A session is an *identity* plus the spec it was spawned
//! with — everything about how the conversation is going lives on the
//! conversation (`chat::Chat`), because the reducer there is the only thing
//! that sees the agent's events.
//!
//! **Resist adding state here.** It once carried an `AgentRuntime` state
//! machine, a `LaunchIntent`, a connection `generation` and a frozen
//! `resume_id`, and every one of them died: the workspace tree is a node in a
//! list, and a node has no way to learn that an adapter answered or a turn
//! began. `runtime` was the worst of them — the rail *read* it to colour a
//! session's signal dot and nothing ever wrote it, so that dot could not light
//! up at all.
//!
//! Anything about how a session is *doing* belongs on the conversation, where
//! the reducer sees the events: `chat::Link` + `Chat::busy` +
//! `Chat::awaiting_permission`, read through the chat pane.

use crate::config::AgentSpec;

/// One agent bound to a project root.
#[derive(Debug)]
pub struct Session {
    pub spec: AgentSpec,
    /// Process-wide-unique id salt so a session's widget state survives
    /// switching roots/sessions, and so an agent event can find its window.
    pub uid: u64,
}

impl Session {
    pub fn new(spec: AgentSpec, uid: u64) -> Self {
        Self { spec, uid }
    }

    /// A short label for the session row — the agent name.
    pub fn title(&self) -> &str {
        &self.spec.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> AgentSpec {
        AgentSpec {
            name: "Claude Code".into(),
            command: "npx".into(),
            args: vec![],
        }
    }

    #[test]
    fn a_session_is_its_spec_and_its_uid() {
        let s = Session::new(spec(), 7);
        assert_eq!(s.uid, 7);
        assert_eq!(s.title(), "Claude Code");
    }
}
