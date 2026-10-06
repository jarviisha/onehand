//! Which credential a Claude Code adapter signs in with, applied to the
//! environment it is started in.

use serde::{Deserialize, Serialize};

/// Which credential a Claude Code agent signs in with.
///
/// Claude Code takes the first credential it finds, and a variable in the
/// environment onehand was started from outranks the login `claude` keeps on
/// disk, so a key exported for something else quietly wins. Each choice other
/// than `Inherit` clears the variables that would outrank it. Claude Code's own
/// `apiKeyHelper` and the `env` block of its settings are out of reach here and
/// can still win.
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
/// bearer token, then an API key. The base URL goes with them because a
/// subscription credential sent to an endpoint someone else set is a
/// credential handed to that endpoint.
const ABOVE_TOKEN: [&str; 6] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
];

impl AgentAuth {
    /// Every choice, in the order a picker shows them.
    pub const ALL: [Self; 3] = [Self::Inherit, Self::Login, Self::Token];

    pub fn label(self) -> &'static str {
        match self {
            Self::Inherit => "As launched",
            Self::Login => "Claude login",
            Self::Token => "OAuth token",
        }
    }

    pub fn is_inherit(&self) -> bool {
        *self == Self::Inherit
    }

    /// The variables to clear from the adapter's environment, or why the
    /// adapter must not start. `var` reads onehand's own environment.
    pub(crate) fn env_to_clear(
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

    #[test]
    fn each_choice_clears_what_would_outrank_it() {
        let none = |_: &str| None;
        assert!(AgentAuth::Inherit.env_to_clear(none).unwrap().is_empty());
        let login = AgentAuth::Login.env_to_clear(none).unwrap();
        assert!(login.contains(&OAUTH_TOKEN) && login.contains(&"ANTHROPIC_API_KEY"));
        assert!(AgentAuth::Token.env_to_clear(none).is_err());
        assert!(AgentAuth::Token.env_to_clear(|_| Some(" ".into())).is_err());
        let token = AgentAuth::Token.env_to_clear(|_| Some("t".into())).unwrap();
        assert!(!token.contains(&OAUTH_TOKEN) && token.contains(&"ANTHROPIC_BASE_URL"));
    }
}
