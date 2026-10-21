//! The agent definitions: what a session is started from, and how its
//! argument line is edited.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A global agent *definition* — the menu a new session spawns from. Each
/// `Session` holds a clone of its chosen spec.
/// Every agent is driven over the Agent Client Protocol (ACP); the legacy
/// terminal session kind has been removed.
///
/// Either a *built-in* agent, which names a [`Preset`] and leaves its command
/// to the code, or a *declared* one, written out in full. Both may carry an
/// environment of their own: the one way two entries can run the same agent
/// under two accounts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<Preset>,
    /// The adapter version a preset runs instead of its pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// An agent onehand knows by name: its adapter, the version of it tried with
/// this release, and the binary on the machine the adapter drives.
///
/// **The config names one by id and never copies its command.** Every
/// settings save writes the whole file back, so a command written out there
/// froze the adapter version at the first save, and a later release moving
/// the pin never reached anyone who had saved once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    ClaudeCode,
    Codex,
}

impl Preset {
    pub fn shown_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    /// The agent's own program, which the adapter is pointed at.
    pub fn binary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    /// The adapter version this release was tried with.
    pub fn pin(self) -> &'static str {
        match self {
            Self::ClaudeCode => "0.81.2",
            Self::Codex => "2.2.2",
        }
    }

    /// The mode an unattended run starts in when the config names none.
    ///
    /// Claude's default mode asks before every edit, so a run left in it parks
    /// on the first card nobody answers. Codex's default reviews its own
    /// escalations and finished a run that edited, fetched and pushed without
    /// asking once, so it needs none.
    pub(crate) fn unattended_mode(self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("acceptEdits"),
            Self::Codex => None,
        }
    }

    fn package(self) -> &'static str {
        match self {
            Self::ClaudeCode => "@agentclientprotocol/claude-agent-acp",
            Self::Codex => "@agentclientprotocol/codex-acp",
        }
    }

    /// The variable the adapter reads its agent's path from.
    fn binary_var(self) -> &'static str {
        match self {
            Self::ClaudeCode => "CLAUDE_CODE_EXECUTABLE",
            Self::Codex => "CODEX_PATH",
        }
    }
}

/// What starting an agent runs: the program, its arguments, and the variables
/// set on it, in the order they are applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The agent called `name` among `agents`, or the first configured when none
/// is named. A name nobody has is `None`, never the first: what names a gone
/// agent is told so, rather than started on another.
pub fn agent_or_first<'a>(agents: &'a [AgentSpec], name: Option<&str>) -> Option<&'a AgentSpec> {
    match name {
        Some(name) => agents.iter().find(|spec| spec.name == name),
        None => agents.first(),
    }
}

impl AgentSpec {
    /// `args` as one editable line.
    ///
    /// Paired with [`split_args`]; the two must round-trip. The agent form used
    /// to `join(" ")` on the way in and `split_whitespace()` on the way out, so
    /// opening an agent whose argument contained a space and pressing Save --
    /// without editing anything -- split that argument in two, permanently.
    pub fn args_line(&self) -> String {
        join_args(&self.args)
    }

    /// What starting this agent runs, with `path` the value of `PATH`.
    ///
    /// A preset runs its adapter without the agent bundled inside it, pointed
    /// at the agent on this machine, so the agent is the person's own and as
    /// new as their terminal's. **The binary is looked for first**: without it
    /// one adapter hangs at the first prompt and another dies with its reason
    /// on stderr only, and neither reaches the person as a failure.
    ///
    /// A declared agent runs as written; a missing command fails at spawn with
    /// the system's own words, as it always has. The entry's `env` goes last
    /// either way, so it can override what a preset sets.
    pub fn launch(&self, path: Option<&std::ffi::OsStr>) -> Result<Launch, String> {
        let own = self.env.iter().map(|(k, v)| (k.clone(), v.clone()));
        let Some(preset) = self.preset else {
            return Ok(Launch {
                command: self.command.clone(),
                args: self.args.clone(),
                env: own.collect(),
            });
        };
        let Some(binary) = find_command(preset.binary(), path, None) else {
            return Err(format!(
                "`{}` is not on this machine — install it again, or remove {} in Settings ▸ Agents",
                preset.binary(),
                self.name
            ));
        };
        let version = self.version.as_deref().unwrap_or(preset.pin());
        let mut env = vec![
            ("npm_config_omit".to_string(), "optional".to_string()),
            (
                preset.binary_var().to_string(),
                binary.display().to_string(),
            ),
        ];
        env.extend(own);
        Ok(Launch {
            command: "npx".into(),
            args: vec![
                "--prefer-offline".into(),
                "-y".into(),
                format!("{}@{version}", preset.package()),
            ],
            env,
        })
    }

    /// Whether this preset runs an adapter other than the one this release
    /// was tried with.
    pub fn outside_pin(&self) -> bool {
        self.preset
            .is_some_and(|preset| self.version.as_deref().is_some_and(|v| v != preset.pin()))
    }
}

/// Render an argument list as one shell-ish line, quoting what needs it.
///
/// Not a general shell quoter: it exists so a round trip through the agent
/// form is lossless, and its only contract is `split_args(join_args(a)) == a`.
pub(crate) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() {
                "\"\"".to_string()
            } else if arg.contains([' ', '\t', '\n', '"', '\'', '\\']) {
                let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where an agent's command would be found, if anywhere.
///
/// A command naming a path (anything with a separator in it) is checked as that
/// path, a relative one from `cwd` -- the directory the session would start it
/// in, which is the project's root, since checked from the app's own launch
/// directory it would answer for a place the agent never runs. A bare name is
/// looked for in each directory of `path`, the value of `PATH`, in order, as
/// the shell would. `None` is what a session started with this command would
/// fail on, which is what checking it ahead of time is for.
///
/// It asks whether a file is there, not whether it may be run or speaks the
/// protocol: the permission bit is platform-specific, and the protocol can only
/// be known by starting the program, which is the thing a check done ahead of a
/// session must not do.
pub fn find_command(
    command: &str,
    path: Option<&std::ffi::OsStr>,
    cwd: Option<&Path>,
) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        let candidate = match cwd {
            Some(cwd) => cwd.join(command),
            None => PathBuf::from(command),
        };
        return candidate.is_file().then_some(candidate);
    }
    std::env::split_paths(path?)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

/// Parse one line of arguments back into a list.
///
/// Whitespace separates; `'…'` is literal; `"…"` honours `\` escapes; a bare
/// `\` escapes the next character. An unterminated quote yields what it has
/// rather than dropping it — a half-typed line in a form is a normal state, and
/// silently losing the tail is worse than accepting it.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    cur.push(c);
                }
            }
            '"' => {
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => cur.extend(chars.next()),
                        c => cur.push(c),
                    }
                }
            }
            '\\' => {
                started = true;
                cur.extend(chars.next());
            }
            c => {
                started = true;
                cur.push(c);
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// The ACP adapter the default agent runs, pinned.
///
/// **Not `@latest`.** Every session in this app goes through this adapter, so
/// `@latest` meant the same onehand binary could talk to a different protocol
/// implementation on two consecutive days, with no way to reproduce a report.
/// Bumping this is a commit, which is the point: the change is
/// visible, bisectable, and revertable.
///
/// The pin is a *default*, not a lock — `onehand.toml` and the agent manager
/// both override it, so anyone wanting the newest adapter can still ask for it.
pub const DEFAULT_ACP_ADAPTER: &str = "@agentclientprotocol/claude-agent-acp@0.81.2";

/// The `npx` argument list that launches [`DEFAULT_ACP_ADAPTER`].
///
/// **`--prefer-offline` is a latency fix, not a preference.** `npx` re-validates
/// the package against the registry on every launch, even when the exact
/// version asked for is already unpacked in its cache — measured here, that is
/// six to seven seconds of network round-trips in front of an adapter that
/// otherwise boots in three tenths of one. With the flag, npm uses what it has
/// and only reaches for the network when the cache cannot answer, which is
/// exactly right for a version that is pinned: a pin has no newer build to go
/// looking for.
///
/// The flag is also what makes the pin worth having offline — without it, an
/// adapter sitting in the cache still fails to start on a dead connection.
pub fn default_adapter_args() -> Vec<String> {
    vec![
        "--prefer-offline".into(),
        "-y".into(),
        DEFAULT_ACP_ADAPTER.into(),
    ]
}

/// Built-in default: Claude Code as an ACP agent.
pub(crate) fn default_agents() -> Vec<AgentSpec> {
    vec![AgentSpec {
        name: "Claude Code".into(),
        command: "npx".into(),
        args: default_adapter_args(),
        ..Default::default()
    }]
}

#[cfg(test)]
mod tests;
