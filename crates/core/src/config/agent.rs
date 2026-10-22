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
    pub preset: Option<PresetId>,
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
/// save of the agent list writes the whole list back, so a command written out there
/// froze the adapter version at the first save, and a later release moving
/// the pin never reached anyone who had saved once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    ClaudeCode,
    Codex,
    Copilot,
}

/// What onehand knows about one built-in agent, in one place.
#[derive(Debug, PartialEq, Eq)]
pub struct PresetInfo {
    pub shown_name: &'static str,
    /// The agent's own program, looked for on `PATH`.
    pub binary: &'static str,
    /// The command that installs this agent, as its makers publish it.
    pub install: &'static str,
    /// The command that logs this agent in, run in a terminal.
    pub(crate) login: &'static str,
    /// The variable that points this agent at one account's directory, which
    /// a login must be run under to log that account in.
    pub(crate) account_var: &'static str,
    /// The mode an unattended run starts in when the config names none.
    pub(crate) unattended_mode: Option<&'static str>,
    pub runs: Runs,
}

/// How a built-in agent speaks the protocol.
#[derive(Debug, PartialEq, Eq)]
pub enum Runs {
    /// Through an npm adapter, at the version this release was tried with,
    /// pointed at the agent's binary by `binary_var`.
    Adapter {
        package: &'static str,
        pin: &'static str,
        binary_var: &'static str,
    },
    /// The agent's binary itself, started with `args`. Nothing is pinned:
    /// the agent updates itself, and `tried` is the version this release was
    /// tried with.
    Native {
        args: &'static [&'static str],
        tried: &'static str,
    },
}

static CLAUDE_CODE: PresetInfo = PresetInfo {
    shown_name: "Claude Code",
    binary: "claude",
    install: "npm install -g @anthropic-ai/claude-code",
    login: "claude auth login",
    account_var: "CLAUDE_CONFIG_DIR",
    // Claude's default mode asks before every edit, so a run left in it parks
    // on the first card nobody answers.
    unattended_mode: Some("acceptEdits"),
    runs: Runs::Adapter {
        package: "@agentclientprotocol/claude-agent-acp",
        pin: "0.81.2",
        binary_var: "CLAUDE_CODE_EXECUTABLE",
    },
};

static CODEX: PresetInfo = PresetInfo {
    shown_name: "Codex",
    binary: "codex",
    install: "npm install -g @openai/codex",
    login: "codex login",
    account_var: "CODEX_HOME",
    // Codex's default reviews its own escalations and finished a run that
    // edited, fetched and pushed without asking once, so it needs none.
    unattended_mode: None,
    runs: Runs::Adapter {
        package: "@agentclientprotocol/codex-acp",
        pin: "2.2.2",
        binary_var: "CODEX_PATH",
    },
};

/// The `preset` an entry names: one this build knows, or one it does not.
///
/// **An id this build does not know is kept as written.** A newer build may
/// write one, and reading it as an error lost the whole file to defaults,
/// while reading it as a known one, or dropping it, would change what the
/// entry starts or lose it at the next save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PresetId {
    Known(Preset),
    Unknown(String),
}

impl From<Preset> for PresetId {
    fn from(preset: Preset) -> Self {
        Self::Known(preset)
    }
}

static COPILOT: PresetInfo = PresetInfo {
    shown_name: "Copilot",
    binary: "copilot",
    install: "npm install -g @github/copilot",
    login: "copilot login",
    account_var: "COPILOT_HOME",
    // None, on purpose. Its default mode asks before every step, and the one
    // that does not, autopilot, allows every tool: a run never answers a card
    // for a person, so a grant that wide is the config's to write, not ours.
    unattended_mode: None,
    runs: Runs::Native {
        args: &["--acp"],
        tried: "1.0.102",
    },
};

impl Preset {
    /// Every built-in agent, in the order a first launch lists them.
    pub const ALL: [Preset; 3] = [Preset::ClaudeCode, Preset::Codex, Preset::Copilot];

    pub fn info(self) -> &'static PresetInfo {
        match self {
            Self::ClaudeCode => &CLAUDE_CODE,
            Self::Codex => &CODEX,
            Self::Copilot => &COPILOT,
        }
    }
}

/// What preset `info` runs once its binary has been found at `binary`, with
/// `own` the entry's own variables, which go last so they can override.
///
/// An adapter runs without the agent bundled inside it, pointed at the
/// agent on this machine, so the agent is the person's own and as new as
/// their terminal's; `version` replaces its pin. A native agent is its binary.
pub(crate) fn preset_launch(
    info: &PresetInfo,
    version: Option<&str>,
    binary: &Path,
    own: impl Iterator<Item = (String, String)>,
) -> Launch {
    match &info.runs {
        Runs::Adapter {
            package,
            pin,
            binary_var,
        } => {
            let mut env = vec![
                ("npm_config_omit".to_string(), "optional".to_string()),
                (binary_var.to_string(), binary.display().to_string()),
            ];
            env.extend(own);
            Launch {
                command: "npx".into(),
                args: vec![
                    "--prefer-offline".into(),
                    "-y".into(),
                    format!("{package}@{}", version.unwrap_or(pin)),
                ],
                env,
            }
        }
        Runs::Native { args, .. } => Launch {
            command: binary.display().to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: own.collect(),
        },
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

/// One change Settings makes to the agent list.
///
/// **Applied to the list, never written over it.** The same edit lands on the
/// list on screen and on the one re-read from disk at save time, each found by
/// the whole spec rather than by position, so an entry written into the file
/// by hand while the app runs survives the next save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEdit {
    /// `new` in place of `old`, or added at the end when `old` is absent.
    Put {
        old: Option<AgentSpec>,
        new: AgentSpec,
    },
    Remove(AgentSpec),
    /// To the front, which makes it the one *New session* starts.
    Promote(AgentSpec),
}

impl AgentEdit {
    pub fn apply(&self, agents: &mut Vec<AgentSpec>) {
        let at = |agents: &[AgentSpec], spec: &AgentSpec| agents.iter().position(|a| a == spec);
        match self {
            AgentEdit::Put { old, new } => match old.as_ref().and_then(|old| at(agents, old)) {
                Some(idx) => agents[idx] = new.clone(),
                None => agents.push(new.clone()),
            },
            AgentEdit::Remove(spec) => {
                if let Some(idx) = at(agents, spec) {
                    agents.remove(idx);
                }
            }
            AgentEdit::Promote(spec) => {
                if let Some(idx) = at(agents, spec) {
                    let spec = agents.remove(idx);
                    agents.insert(0, spec);
                }
            }
        }
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
        let own = self.env.iter().map(|(k, v)| (k.clone(), home_expanded(v)));
        if let Some(id) = self.unknown_preset() {
            return Err(format!(
                "`{id}` is not an agent this build of onehand knows; update onehand, or remove it in Settings ▸ Agents"
            ));
        }
        let Some(preset) = self.preset() else {
            return Ok(Launch {
                command: self.command.clone(),
                args: self.args.clone(),
                env: own.collect(),
            });
        };
        let info = preset.info();
        let Some(binary) = find_command(info.binary, path, None) else {
            return Err(format!(
                "`{}` is not on this machine — install it again, or remove {} in Settings ▸ Agents",
                info.binary, self.name
            ));
        };
        Ok(preset_launch(info, self.version.as_deref(), &binary, own))
    }

    /// The adapter package a preset runs, at the version it runs:
    /// `@agentclientprotocol/codex-acp@2.2.2`. `None` for a declared agent.
    pub fn adapter(&self) -> Option<String> {
        match self.preset()?.info().runs {
            Runs::Adapter { package, pin, .. } => Some(format!(
                "{package}@{}",
                self.version.as_deref().unwrap_or(pin)
            )),
            Runs::Native { .. } => None,
        }
    }

    /// A bare entry for `preset`, named as it is shown, or with the first
    /// ` (n)` after that no entry in `taken` has, since a name finds one agent.
    pub fn of(preset: Preset, taken: &[AgentSpec]) -> AgentSpec {
        let free = |name: &String| taken.iter().all(|spec| &spec.name != name);
        let shown = preset.info().shown_name.to_string();
        let name = std::iter::once(shown.clone())
            .chain((2..).map(|n| format!("{shown} ({n})")))
            .find(free)
            .expect("an unbounded range finds a free number");
        AgentSpec {
            name,
            preset: Some(preset.into()),
            ..Default::default()
        }
    }

    /// The built-in agent this entry is, when this build knows it.
    pub fn preset(&self) -> Option<Preset> {
        match self.preset {
            Some(PresetId::Known(preset)) => Some(preset),
            Some(PresetId::Unknown(_)) | None => None,
        }
    }

    /// The id of a preset this build does not know, when the entry names one.
    pub fn unknown_preset(&self) -> Option<&str> {
        match &self.preset {
            Some(PresetId::Unknown(id)) => Some(id),
            Some(PresetId::Known(_)) | None => None,
        }
    }

    /// What to tell someone whose agent answered that it is not logged in,
    /// when this entry knows: a built-in agent's words carry the command that
    /// logs it in, under the entry's own account where its environment names
    /// one, since a second account logged in without it would log the first
    /// one in again. Any other entry has none, and the agent's own words, or
    /// its name, stand in.
    pub fn login_hint(&self) -> Option<String> {
        // The preset itself, not the old default left declared: that one stays
        // declared only where the agent's binary is missing, so no command of
        // ours is any help there, and it is told what the agent says instead.
        let preset = self.preset()?;
        let info = preset.info();
        let account = self
            .env
            .get(info.account_var)
            .map(|dir| {
                let dir = shell_word(&home_expanded(dir));
                format!("{}={dir} ", info.account_var)
            })
            .unwrap_or_default();
        // No backticks: the line is drawn as text, where they would show.
        Some(format!(
            "{} is not logged in. Run {account}{} in a terminal, then restart the agent",
            self.name, info.login
        ))
    }

    /// The preset this agent is, counting the old Claude default left
    /// declared (for want of `claude` on `PATH`) as the Claude it still runs.
    pub fn claude_or_preset(&self) -> Option<Preset> {
        self.preset()
            .or_else(|| old_default_env(self).map(|_| Preset::ClaudeCode))
    }

    /// Whether this preset runs an adapter other than the one this release
    /// was tried with.
    pub fn outside_pin(&self) -> bool {
        match self.preset().map(|preset| &preset.info().runs) {
            Some(Runs::Adapter { pin, .. }) => self.version.as_deref().is_some_and(|v| v != *pin),
            _ => false,
        }
    }
}

/// The built-in agents whose binary is on `path`.
pub fn on_machine(path: Option<&std::ffi::OsStr>) -> Vec<Preset> {
    Preset::ALL
        .into_iter()
        .filter(|preset| find_command(preset.info().binary, path, None).is_some())
        .collect()
}

/// The built-in agents whose binary is on `path`, as entries of their own.
pub(crate) fn found_agents(path: Option<&std::ffi::OsStr>) -> Vec<AgentSpec> {
    let mut found = Vec::new();
    for preset in on_machine(path) {
        let spec = AgentSpec::of(preset, &found);
        found.push(spec);
    }
    found
}

/// `value` as one word a shell reads back unchanged: as typed when it holds
/// nothing a shell would split or expand, else in single quotes, a quote in
/// it closed, escaped and reopened. A login command is pasted into a terminal,
/// and an account directory with a space in it would otherwise run as two.
fn shell_word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c));
    match plain {
        true => value.to_string(),
        false => format!("'{}'", value.replace('\'', r"'\''")),
    }
}

/// `value` with a leading `~/` as the home directory, as a shell would have
/// read it: no shell stands between onehand and the adapter, so the agent would
/// otherwise be handed a directory literally named `~`.
fn home_expanded(value: &str) -> String {
    match (value.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest).display().to_string(),
        _ => value.to_string(),
    }
}

/// Turn every entry that is the old Claude default into the preset, keeping
/// its name and moving any `env VAR=value` in front of it into `env`.
pub(super) fn migrate(agents: &mut [AgentSpec]) {
    for spec in agents.iter_mut() {
        if let Some(env) = old_default_env(spec) {
            *spec = AgentSpec {
                name: std::mem::take(&mut spec.name),
                preset: Some(Preset::ClaudeCode.into()),
                env,
                ..Default::default()
            };
        }
    }
}

/// The environment of `spec` when it is the old default, written either as
/// `npx <the default arguments>` or as `env VAR=value… npx <the same>`.
/// An entry edited in any other way is the person's own, and is `None`.
fn old_default_env(spec: &AgentSpec) -> Option<BTreeMap<String, String>> {
    if spec.preset.is_some() {
        return None;
    }
    let mut env = spec.env.clone();
    let rest = match spec.command.as_str() {
        "npx" => spec.args.as_slice(),
        "env" => {
            let pairs = spec
                .args
                .iter()
                .map_while(|arg| arg.split_once('=').filter(|(key, _)| !key.is_empty()))
                .collect::<Vec<_>>();
            env.extend(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            match spec.args.get(pairs.len()).map(String::as_str) {
                Some("npx") => &spec.args[pairs.len() + 1..],
                _ => return None,
            }
        }
        _ => return None,
    };
    (rest == default_adapter_args().as_slice()).then_some(env)
}

/// Give a nameless preset its own name, and number every name already taken
/// earlier in the list, skipping numbers some other entry already has.
pub(super) fn name_uniquely(agents: &mut [AgentSpec]) {
    for spec in agents.iter_mut() {
        if !spec.name.trim().is_empty() {
            continue;
        }
        // One this build does not know is called by its id, so it can still
        // be found, and removed.
        match &spec.preset {
            Some(PresetId::Known(preset)) => spec.name = preset.info().shown_name.to_string(),
            Some(PresetId::Unknown(id)) => spec.name = id.clone(),
            None => {}
        }
    }
    let mut all: std::collections::HashSet<String> =
        agents.iter().map(|spec| spec.name.clone()).collect();
    let mut seen = std::collections::HashSet::new();
    for spec in agents.iter_mut() {
        if !seen.insert(spec.name.clone()) {
            let name = (2..)
                .map(|n| format!("{} ({n})", spec.name))
                .find(|name| !all.contains(name))
                .expect("an unbounded range finds a free number");
            all.insert(name.clone());
            seen.insert(name.clone());
            spec.name = name;
        }
    }
}

/// Render an argument list as one shell-ish line, quoting what needs it.
///
/// Not a general shell quoter: it exists so a round trip through the agent
/// form is lossless, and its only contract is `split_args(join_args(a)) == a`.
pub fn join_args(args: &[String]) -> String {
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
