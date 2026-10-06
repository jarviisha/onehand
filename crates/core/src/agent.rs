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
use serde::{Deserialize, Serialize};

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

/// Which credential a Claude Code agent signs in with.
///
/// Claude Code takes the first credential it finds, and a variable in the
/// environment onehand was started from outranks the login `claude` keeps on
/// disk, so a key exported for something else quietly wins. Each choice other
/// than `Inherit` clears what would outrank it, so the session runs on the
/// credential that was picked.
///
/// **The token is never kept here.** It is read from onehand's own
/// environment, where the user put it; storing a claude.ai token on someone's
/// behalf is what Anthropic's terms forbid a third-party app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAuth {
    /// The environment passes through untouched, as for any other agent.
    #[default]
    Inherit,
    /// The login `claude` stored on this machine.
    Login,
    /// The long-lived token from `claude setup-token`, in [`OAUTH_TOKEN`].
    Token,
}

/// The variable Claude Code reads a `claude setup-token` token from.
pub const OAUTH_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// What Claude Code prefers to [`OAUTH_TOKEN`]: a cloud provider, then a
/// bearer token, then an API key.
const ABOVE_TOKEN: [&str; 5] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

impl AgentAuth {
    pub fn is_inherit(&self) -> bool {
        *self == Self::Inherit
    }

    /// The variables to clear from the adapter's environment, or why it cannot
    /// start. `var` reads onehand's own environment.
    pub(crate) fn cleared(
        self,
        var: impl Fn(&str) -> Option<String>,
    ) -> Result<Vec<&'static str>, String> {
        match self {
            Self::Inherit => Ok(Vec::new()),
            // A named profile also outranks the stored login.
            Self::Login => Ok([OAUTH_TOKEN, "ANTHROPIC_PROFILE"]
                .into_iter()
                .chain(ABOVE_TOKEN)
                .collect()),
            Self::Token if var(OAUTH_TOKEN).is_some_and(|t| !t.trim().is_empty()) => {
                Ok(ABOVE_TOKEN.to_vec())
            }
            Self::Token => Err(format!(
                "{OAUTH_TOKEN} is not set: run `claude setup-token`, export the token it \
                 prints, and start onehand from that shell"
            )),
        }
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
            auth: AgentAuth::Inherit,
        }
    }

    #[test]
    fn each_choice_clears_what_would_outrank_it() {
        let none = |_: &str| None;
        assert!(AgentAuth::Inherit.cleared(none).unwrap().is_empty());
        let login = AgentAuth::Login.cleared(none).unwrap();
        assert!(login.contains(&OAUTH_TOKEN) && login.contains(&"ANTHROPIC_API_KEY"));
        assert!(AgentAuth::Token.cleared(none).is_err());
        assert!(AgentAuth::Token.cleared(|_| Some(" ".into())).is_err());
        let token = AgentAuth::Token.cleared(|_| Some("t".into())).unwrap();
        assert!(!token.contains(&OAUTH_TOKEN) && token.contains(&"ANTHROPIC_API_KEY"));
    }

    #[test]
    fn a_session_is_its_spec_and_its_uid() {
        let s = Session::new(spec(), 7);
        assert_eq!(s.uid, 7);
        assert_eq!(s.title(), "Claude Code");
    }
}
