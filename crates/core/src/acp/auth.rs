//! Which credential a Claude Code adapter signs in with, applied to the
//! environment it is started in.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which credential a Claude Code agent signs in with.
///
/// Claude Code takes the first credential it finds, and a variable in the
/// environment onehand was started from outranks the login `claude` keeps on
/// disk, so a key exported for something else quietly wins. Each choice other
/// than `Inherit` clears the variables that would outrank it, and refuses to
/// start when something it cannot clear would still win. Claude Code's own
/// `apiKeyHelper` and the `env` block of its settings are out of reach here and
/// can still win.
///
/// **The token is never kept here.** It is read from the environment the user
/// started onehand in and reaches only the adapter's own process. That is all
/// this type settles: whether a subscription credential may drive this adapter
/// at all is a question about the adapter, not about where the token lives.
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

/// The variables that sign in by federation, which outranks the stored login.
const FEDERATION: [&str; 7] = [
    "ANTHROPIC_FEDERATION_RULE_ID",
    "ANTHROPIC_ORGANIZATION_ID",
    "ANTHROPIC_SERVICE_ACCOUNT_ID",
    "ANTHROPIC_IDENTITY_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN_FILE",
    "ANTHROPIC_WORKSPACE_ID",
    // A named profile outranks the stored login whatever it signs in with.
    "ANTHROPIC_PROFILE",
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
    /// adapter must not start. `var` reads onehand's own environment and `read`
    /// a file, so a test can stand in for both.
    pub(crate) fn env_to_clear(
        self,
        var: impl Fn(&str) -> Option<String>,
        read: impl Fn(&Path) -> std::io::Result<String>,
    ) -> Result<Vec<&'static str>, String> {
        match self {
            Self::Inherit => Ok(Vec::new()),
            Self::Login => {
                login_is_outranked(&var, &read)?;
                Ok([OAUTH_TOKEN]
                    .into_iter()
                    .chain(ABOVE_TOKEN)
                    .chain(FEDERATION)
                    .collect())
            }
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

/// Why the stored login would lose to the active Anthropic profile, if it would.
///
/// No variable switches this one off: with `ANTHROPIC_PROFILE` cleared, the
/// profile named in `active_config`, or else the one called `default`, still
/// signs Claude Code in ahead of the login when it federates. So the profile is
/// read, and a login that cannot be the credential in use refuses to start
/// rather than running on someone else's account. A profile that cannot be read
/// refuses too, since what it would do cannot be known.
fn login_is_outranked(
    var: &impl Fn(&str) -> Option<String>,
    read: &impl Fn(&Path) -> std::io::Result<String>,
) -> Result<(), String> {
    let Some(dir) = anthropic_config_dir(var) else {
        return Ok(());
    };
    let name = match read(&dir.join("active_config")) {
        Ok(name) if !name.trim().is_empty() => name.trim().to_string(),
        Ok(_) => "default".to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "default".to_string(),
        Err(e) => return Err(unknowable(&dir.join("active_config"), &e.to_string())),
    };
    let path = dir.join("configs").join(format!("{name}.json"));
    let text = match read(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(unknowable(&path, &e.to_string())),
    };
    let profile: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| unknowable(&path, &e.to_string()))?;
    match profile
        .pointer("/authentication/type")
        .and_then(|t| t.as_str())
    {
        Some("oidc_federation") => Err(format!(
            "The Anthropic profile {name} ({}) signs in by federation, which Claude Code \
             prefers to the stored claude login: activate another profile, or choose \
             As launched for this agent",
            path.display()
        )),
        _ => Ok(()),
    }
}

/// The refusal for a profile that could not be read.
fn unknowable(path: &Path, why: &str) -> String {
    format!(
        "Could not read {} ({why}), so whether it outranks the stored claude login cannot be \
         told",
        path.display()
    )
}

/// Where Anthropic's tools keep their profiles.
fn anthropic_config_dir(var: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(dir) = var("ANTHROPIC_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        dirs::config_dir().map(|d| d.join("Anthropic"))
    } else {
        dirs::home_dir().map(|d| d.join(".config").join("anthropic"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};

    /// An environment pointing the profiles at `/p`, and the files under it.
    fn probe(
        files: &[(&str, &str)],
    ) -> (
        impl Fn(&str) -> Option<String>,
        impl Fn(&Path) -> std::io::Result<String>,
    ) {
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(p, t)| (Path::new("/p").join(p), t.to_string()))
            .collect();
        (
            |k: &str| (k == "ANTHROPIC_CONFIG_DIR").then(|| "/p".to_string()),
            move |p: &Path| {
                files
                    .get(p)
                    .cloned()
                    .ok_or_else(|| Error::from(ErrorKind::NotFound))
            },
        )
    }

    const FEDERATED: &str = r#"{"authentication":{"type":"oidc_federation"}}"#;

    #[test]
    fn each_choice_clears_what_would_outrank_it() {
        let (var, read) = probe(&[]);
        assert!(AgentAuth::Inherit
            .env_to_clear(&var, &read)
            .unwrap()
            .is_empty());
        let login = AgentAuth::Login.env_to_clear(&var, &read).unwrap();
        for v in [
            OAUTH_TOKEN,
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_PROFILE",
        ] {
            assert!(login.contains(&v), "{v}");
        }
        assert!(AgentAuth::Token.env_to_clear(&var, &read).is_err());
        assert!(AgentAuth::Token
            .env_to_clear(|_| Some(" ".into()), &read)
            .is_err());
        let token = AgentAuth::Token
            .env_to_clear(|_| Some("t".into()), &read)
            .unwrap();
        assert!(!token.contains(&OAUTH_TOKEN) && token.contains(&"ANTHROPIC_BASE_URL"));
    }

    /// Both federation variables set would sign Claude Code in ahead of the
    /// login; clearing either one is what switches that path off.
    #[test]
    fn login_clears_the_federation_variables() {
        let (var, read) = probe(&[]);
        let login = AgentAuth::Login.env_to_clear(&var, &read).unwrap();
        assert!(login.contains(&"ANTHROPIC_FEDERATION_RULE_ID"));
        assert!(login.contains(&"ANTHROPIC_ORGANIZATION_ID"));
    }

    #[test]
    fn login_refuses_an_active_federation_profile() {
        let (var, read) = probe(&[
            ("active_config", "work\n"),
            ("configs/work.json", FEDERATED),
        ]);
        let why = AgentAuth::Login.env_to_clear(&var, &read).unwrap_err();
        assert!(why.contains("work"), "{why}");
        // Without `active_config`, the profile called `default` is the active one.
        let (var, read) = probe(&[("configs/default.json", FEDERATED)]);
        assert!(AgentAuth::Login.env_to_clear(&var, &read).is_err());
    }

    #[test]
    fn login_starts_beside_a_profile_it_outranks() {
        let user = r#"{"authentication":{"type":"user_oauth"}}"#;
        let (var, read) = probe(&[("configs/default.json", user)]);
        assert!(AgentAuth::Login.env_to_clear(&var, &read).is_ok());
        // A federation profile that is not the active one is never read.
        let (var, read) = probe(&[("active_config", "mine"), ("configs/work.json", FEDERATED)]);
        assert!(AgentAuth::Login.env_to_clear(&var, &read).is_ok());
    }

    #[test]
    fn login_refuses_a_profile_it_cannot_read() {
        let (var, read) = probe(&[("configs/default.json", "{ not json")]);
        assert!(AgentAuth::Login.env_to_clear(&var, &read).is_err());
    }

    /// The token outranks every profile, so a federation profile does not stop
    /// it.
    #[test]
    fn a_token_starts_beside_a_federation_profile() {
        let (_, read) = probe(&[("configs/default.json", FEDERATED)]);
        let var = |k: &str| match k {
            "ANTHROPIC_CONFIG_DIR" => Some("/p".to_string()),
            OAUTH_TOKEN => Some("t".to_string()),
            _ => None,
        };
        assert!(AgentAuth::Token.env_to_clear(var, read).is_ok());
    }
}
