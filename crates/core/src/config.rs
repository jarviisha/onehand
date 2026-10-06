//! `onehand.toml` parsing — pure, GUI-free.
//!
//! Loads the appearance, the global agent definitions, the `[font]` preference
//! and the remote channels.
//!
//! Two separate tolerances keep a hand-written file working, and they are not
//! the same mechanism. `#[serde(default)]` everywhere covers keys the file
//! *omits*, so a partial file overrides only what it sets and a `[font]`-only
//! file keeps the default agents. Keys the file has and this build does not are
//! covered by serde's own default of ignoring unknown fields — nothing here
//! opts into `deny_unknown_fields`, which is the attribute that would turn a
//! setting left over from an older build into a refusal to load at all.

use crate::acp::AgentAuth;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// A global agent *definition* — the menu a new session spawns from. Each
/// `Session` holds a clone of its chosen spec.
/// Every agent is driven over the Agent Client Protocol (ACP); the legacy
/// terminal session kind has been removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Which credential a Claude Code agent signs in with; left out of the
    /// file while it is the default.
    #[serde(default, skip_serializing_if = "AgentAuth::is_inherit")]
    pub auth: AgentAuth,
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

/// `[font]` — the monospace family to prefer, if the machine has one by that
/// name.
///
/// One key, because one key is what is read. This table also carried a body
/// size, a master zoom, a sans family and a fallback list; all four parsed
/// cleanly and none of them reached the screen, so a file setting `size = 18`
/// loaded without complaint and changed nothing. A key that is honoured only by
/// the parser is worse than a missing one: the missing key is a feature the app
/// does not have, and the parsed one is a feature it appears to have and lies
/// about.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub monospace: Option<String>,
}

/// Monospace families to fall back through, in order, when nothing preferred
/// is installed.
///
/// Every platform's list in one, because only the installed ones can match:
/// picking by `cfg!(target_os)` would be guessing at the same thing the caller
/// can simply look up.
const MONO_FALLBACKS: &[&str] = &[
    // Platform defaults first — the family a user of this OS expects to see,
    // before the one a developer happened to install.
    "SF Mono",
    "Menlo",
    "Monaco",
    "Cascadia Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Ubuntu Mono",
    "Adwaita Mono",
    // Then the ones people go out of their way to install.
    "JetBrains Mono",
    "Fira Code",
    "Source Code Pro",
    "Cascadia Code",
    "Courier New",
];

/// The monospace family to actually ask for, given what is installed.
///
/// **A font family is a request, not a guarantee, and a missing one fails
/// silently** — the text renders in whatever the fallback face is and nothing
/// says why. That is not hypothetical: the component library's default mono
/// family is one name per platform, and on Linux it is DejaVu Sans Mono, which
/// plenty of distributions do not ship. Every diff, every command, every line
/// of terminal output then draws in the body face, and the code asking for
/// mono looks correct while the screen says otherwise.
///
/// So the family is chosen against the list of what the system actually has.
/// `preferred` is tried in order — the user's configured choice, then whatever
/// default was already in place — then the fallback ladder, and last **any**
/// installed family whose name says monospace, sorted so the answer is the
/// same on every launch. `None` means the machine offered nothing that
/// identifies itself as monospace, in which case the caller has no better move
/// than to leave the default alone.
///
/// Matching is case-insensitive because font enumeration capitalizes as the
/// foundry pleases and a config file is typed by a person.
pub fn resolve_monospace<'a>(
    preferred: impl IntoIterator<Item = &'a str>,
    installed: &[String],
) -> Option<String> {
    let find = |want: &str| {
        installed
            .iter()
            .find(|have| have.eq_ignore_ascii_case(want))
            .cloned()
    };

    preferred
        .into_iter()
        .filter(|want| !want.trim().is_empty())
        .find_map(&find)
        .or_else(|| MONO_FALLBACKS.iter().copied().find_map(find))
        .or_else(|| {
            let mut named: Vec<&String> = installed
                .iter()
                .filter(|have| have.to_ascii_lowercase().contains("mono"))
                .collect();
            named.sort();
            named.first().map(|name| name.to_string())
        })
}

/// Which of the theme's two modes the window is drawn in.
///
/// Three values rather than a `dark = true` flag, because "follow the desktop"
/// is a third answer and not the average of the other two: a machine that
/// switches to dark at sunset has to be able to say so once, and a machine
/// whose owner wants dark regardless has to be able to override it.
///
/// The look itself is the component library's — this only chooses which of its
/// two palettes is loaded, so there is nothing here to keep in step with a
/// colour written anywhere else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// Whatever the desktop reports, and it keeps following it as that changes.
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// The choices, in the order a picker offers them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// How the choice is written for a human.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// The config value this parses from, and what it serializes back to.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Read a config value. **Anything unrecognized reads as `System`** rather
    /// than failing the file: a typo in one word would otherwise take the whole
    /// config down with it, and the agent list is in that same file. Following
    /// the desktop is the answer that is wrong in the fewest ways.
    pub fn parse(text: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|choice| choice.key().eq_ignore_ascii_case(text.trim()))
            .unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for Appearance {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(d)?))
    }
}

/// `[remote]` — the ways a device outside this machine can reach the app.
///
/// One table per channel rather than one flat set of keys, because the channels
/// are meant to accumulate: a second one is a second table, and nothing about
/// the first has to be renamed to make room for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub telegram: TelegramConfig,
}

/// `[remote.telegram]` — everything about the Telegram bridge **except the
/// token**, which deliberately has no key here at all. See [`token_sources`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    /// Off unless asked for. A bridge that came up by default would put a
    /// process on the network on the strength of a file the user never edited.
    pub enabled: bool,
    /// The chat ids allowed to reach the app, as Telegram numbers them.
    ///
    /// **The empty list allows nobody**, and that is the useful reading rather
    /// than a degenerate one: an enabled bridge with no list is a bot anyone who
    /// finds it can drive, so the failure of forgetting to fill this in has to
    /// be "nothing works" and not "everything works for everyone".
    ///
    /// Strings rather than numbers because the whole bridge speaks ids as
    /// strings — Telegram numbers its chats, the next channel will not.
    pub allowed_chats: Vec<String>,
    /// The environment variable the bot token is read from, when the default
    /// name does not suit. See [`token_sources`].
    pub token_env: Option<String>,
}

/// `[unattended]` — picking up small issues and working them with nobody
/// watching.
///
/// **There is no switch here.** Whether runs happen is decided per project, in
/// the app, and no project is opted in until the user says so — so this table
/// only shapes runs once one has been allowed. A file still carrying the old
/// global `enabled` key keeps loading, since serde ignores a key it does not
/// know. An empty label still picks nothing at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnattendedConfig {
    /// The label whose presence on an issue asks for a run.
    pub label: String,
    /// How often to look for one, as `"30m"`, `"2h"` or `"90s"`.
    pub every: String,
    /// How long a run may go before it is cancelled, in the same form.
    pub timeout: String,
    /// The ACP session mode a run starts in — the adapter's own id for it.
    /// Empty leaves the agent in the mode it starts in.
    pub mode: String,
    /// Which configured agent runs it; the default agent when unset.
    pub agent: Option<String>,
    /// The id of the workflow an issue is worked with, unless one of its
    /// labels names another in `workflows`.
    pub workflow: String,
    /// Label → workflow id: an issue carrying one of these labels, beside the
    /// trigger label, is worked with that workflow. The first label in the
    /// table's order wins.
    pub workflows: BTreeMap<String, String>,
    /// How many issues may be worked at once across every window. One
    /// waiting on a person does not count.
    pub at_once: u32,
}

impl Default for UnattendedConfig {
    fn default() -> Self {
        Self {
            label: "auto".to_string(),
            every: "30m".to_string(),
            timeout: "45m".to_string(),
            mode: "acceptEdits".to_string(),
            agent: None,
            workflow: "builtin:issue".to_string(),
            workflows: BTreeMap::new(),
            at_once: 1,
        }
    }
}

/// The whole `onehand.toml`. (A legacy `[profile]`
/// section in an existing file is an unknown key now — serde ignores it.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Light, dark, or whatever the desktop is set to.
    ///
    /// **Declared before the sections on purpose.** TOML puts every bare key
    /// above the first table, and the serializer writes fields in declaration
    /// order — a plain value declared after `agents` is a value emitted after a
    /// table, which is not a document it can produce, so saving the config
    /// would start failing rather than moving the key.
    pub appearance: Appearance,
    pub agents: Vec<AgentSpec>,
    pub font: FontConfig,
    pub remote: RemoteConfig,
    pub unattended: UnattendedConfig,
    /// App command IDs mapped to replacement shortcuts. An empty list unbinds
    /// a command; an omitted ID uses its built-in defaults.
    pub keymap: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            agents: default_agents(),
            font: FontConfig::default(),
            remote: RemoteConfig::default(),
            unattended: UnattendedConfig::default(),
            keymap: Default::default(),
        }
    }
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
        auth: AgentAuth::Inherit,
    }]
}

impl AppConfig {
    /// Parse TOML text. A parse error falls back to defaults rather than
    /// failing the launch.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize back to TOML (round-trips all sections for `persist_agents`).
    pub(crate) fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Resolve config + the path it should be written back to. Search order:
    /// `./onehand.toml`, then `<config_dir>/onehand/config.toml`, else built-in
    /// defaults written back to the global path.
    pub fn load_resolved() -> (Self, PathBuf) {
        let local = PathBuf::from("onehand.toml");
        let global = config_dir().join("config.toml");
        for path in [local, global.clone()] {
            if let Ok(text) = std::fs::read_to_string(&path) {
                match Self::parse(&text) {
                    Ok(cfg) => return (cfg, path),
                    Err(e) => eprintln!("onehand: bad config {}: {e}", path.display()),
                }
            }
        }
        (Self::default(), global)
    }

    /// Read the config at `path`, apply `edit`, and write it back.
    ///
    /// A file that exists but **fails to parse** is left alone and reported:
    /// rewriting it from defaults would permanently destroy the user's other
    /// sections. A *missing* file is a first save and starts from defaults.
    ///
    /// Lives here rather than in the front end because the guard belongs to the
    /// file format, not to whoever is drawing the settings screen.
    pub fn update_in_place(path: &Path, edit: impl FnOnce(&mut Self)) -> Result<(), String> {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).map_err(|e| format!("{} won't parse: {e}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
        };
        edit(&mut cfg);
        cfg.save_to(path).map_err(|e| e.to_string())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = self.to_toml().map_err(std::io::Error::other)?;
        write_atomic(path, &text)
    }
}

/// `<config_dir>/onehand/` — the per-user data root (config, sessions, state).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("onehand")
}

/// A workspace's own persisted shape — name + project roots + which is active
///. Sessions are *not* persisted (launch
/// isn't persisted; sessions re-spawn). Stored as `onehand-workspace.toml` in a
/// workspace's chosen storage directory.
// No `Eq`: `PanelLayout` carries pixel sizes, and float equality is not an
// equivalence relation. `PartialEq` is all any caller here wants anyway.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub name: String,
    pub roots: Vec<PathBuf>,
    pub active_root: usize,
    /// How the window's side panels were arranged. `#[serde(default)]` on the
    /// struct means an older file without this section simply gets the
    /// built-in arrangement.
    pub layout: PanelLayout,
    /// Roots the user has pinned to the top of the rail, by path.
    ///
    /// By path rather than by index into `roots`: a hand-edited file, or a root
    /// removed by an older build, would otherwise slide every pin onto a
    /// different project. A path that is no longer a root simply matches
    /// nothing.
    pub pinned: Vec<PathBuf>,
    /// Roots whose labelled issues may be worked unattended, by path for the
    /// reason pins are.
    pub unattended: Vec<PathBuf>,
    /// The command a workflow's work must pass before it goes further, by
    /// root: onehand runs it itself rather than taking the agent's word.
    pub checks: std::collections::BTreeMap<PathBuf, String>,
}

/// The window's panel arrangement, as far as anything outside the front end
/// needs to know it.
///
/// Deliberately **not** the front end's own layout type. gpui-component can
/// serialize a whole `DockAreaState`, but restoring one rebuilds every panel
/// through a process-global registry — and onehand's panels are per window and
/// held by the shell, so the shell's handles would end up pointing at orphans.
///
/// This carries the part that is actually variable. onehand's arrangement is
/// fixed by design — conversation in the centre, Workbench right, terminal
/// bottom — so what the user changes is how wide, how tall, and whether each is
/// showing. Four facts, no GUI types, and nothing here that stops compiling
/// when the library's layout format moves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    /// Width of the Workbench dock, in pixels.
    pub workbench_w: f32,
    pub workbench_open: bool,
    /// Height of the terminal dock, in pixels.
    pub terminal_h: f32,
    pub terminal_open: bool,
    /// Width of the navigation rail, in pixels.
    ///
    /// Whether the rail is *showing* is deliberately not here. The docks
    /// persist their open state because they are opened for a task and left
    /// that way; the rail is how the window is navigated, and a workspace that
    /// reopened with no rail and no explanation would look broken rather than
    /// restored.
    pub rail_w: f32,
}

impl Default for PanelLayout {
    fn default() -> Self {
        // Both docks start closed: the conversation is the window's job, and a
        // panel nobody asked for is width taken from it.
        Self {
            workbench_w: 420.0,
            workbench_open: false,
            terminal_h: 240.0,
            terminal_open: false,
            rail_w: 255.0,
        }
    }
}

impl PanelLayout {
    /// Smallest a restored panel may be.
    ///
    /// A size read back from disk has not been through the dock's own drag
    /// clamps, and a hand-edited `0.0` would restore a panel that is open,
    /// focusable by its shortcut, and invisible.
    const MIN: f32 = 120.0;
    /// Largest, so a stale value from a much bigger monitor cannot restore a
    /// panel that covers the whole window on a smaller one.
    const MAX: f32 = 2000.0;

    /// The rail's narrowest useful width.
    ///
    /// Narrower than this and a project row is its icon, a truncated name and
    /// nothing else -- no branch, no change count, no room for the session
    /// titles nested under it, which is the whole content of the rail.
    pub const RAIL_MIN: f32 = 232.0;
    /// The widest it may be dragged.
    ///
    /// Past this the rail stops being chrome and starts competing with the
    /// conversation for the window, and everything in it is capped well before
    /// this point anyway -- the extra width would go to empty space.
    pub const RAIL_MAX: f32 = 320.0;

    /// The sizes, clamped into a range that is certainly usable.
    ///
    /// `f32::clamp` alone is not enough: it returns NaN for NaN, so a
    /// hand-edited or corrupted `workbench_w = nan` went straight through the
    /// guard and into the layout. A non-finite size is not a
    /// size at all, so it falls back to the default rather than to a bound.
    pub(crate) fn clamped(self) -> Self {
        fn size(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        }
        let default = Self::default();
        Self {
            workbench_w: size(self.workbench_w, default.workbench_w, Self::MIN, Self::MAX),
            terminal_h: size(self.terminal_h, default.terminal_h, Self::MIN, Self::MAX),
            // The rail's own range, not the docks'. It is the one panel with a
            // content width rather than a preference: too narrow and its rows
            // say nothing, too wide and it is taking the conversation's space
            // to show padding.
            rail_w: size(self.rail_w, default.rail_w, Self::RAIL_MIN, Self::RAIL_MAX),
            ..self
        }
    }
}

/// The temp file one atomic write stages into, beside its destination.
///
/// Two properties, and the second is the one that was missing. It **appends**
/// to the whole file name rather than replacing the extension, so a stem that
/// already carries a dot keeps its name and a stray temp still says what it
/// belongs to. And it carries the **process id**, so two onehand processes
/// staging the same destination cannot pick the same staging file — which they
/// did while the name was a counter alone, each process starting that counter
/// at zero: one was still writing its temp when the other renamed that very
/// temp onto the destination, and what landed was half a file. A pid plus a
/// counter needs no clock and no dependency, and two processes that are both
/// alive have different pids by definition.
fn tmp_path(path: &Path, pid: u32, seq: u64) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp{pid}-{seq}"));
    path.with_file_name(name)
}

/// Write `bytes` to `path` and do not return until the disk has them.
///
/// `std::fs::write` returns once the *kernel* has them, which is a different
/// promise: the bytes sit in the page cache and reach the platter whenever the
/// system gets round to it. That is fine for a file whose loss costs a re-typed
/// setting, and not fine for one whose loss costs a conversation — and it is
/// worse than it sounds when the write is followed by a rename, because losing
/// power in between can promote a file whose contents never landed.
///
/// One place, because "wait for it" is the kind of step that gets left out of
/// the second copy.
pub(crate) fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Wait until the entries of `dir` (a rename into it, say) are on disk, for
/// a caller about to remove the only other copy. Windows cannot open a
/// directory to wait on, and there it does nothing.
pub(crate) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Move every `.<ext>` file from `old` to `new`, each turned into what `new`
/// keeps by `convert`, and say what was left behind. Blocking.
///
/// Safe to run at every start and again after a crash part way: a file
/// `convert` refuses stays where it is, a name already in `new` keeps the
/// copy there (the old one goes only when `settled` says the copy there
/// stands for it, as a crash between the write and the removal leaves it),
/// and each file is written in full, and on disk, before its old one goes.
/// Anything else is left alone, and `old` goes once empty. Only a missing
/// `old` is nothing to report: one that cannot be listed is somebody's files
/// out of sight.
pub(crate) fn migrate_dir_blocking(
    old: &Path,
    new: &Path,
    ext: &str,
    convert: impl Fn(&str) -> Result<String, String>,
    settled: impl Fn(&str, &str) -> bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    let entries = match std::fs::read_dir(old) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return problems,
        Err(err) => {
            problems.push(format!("{} could not be read: {err}", old.display()));
            return problems;
        }
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                problems.push(format!("{} could not be read: {err}", old.display()));
                continue;
            }
        };
        if path.extension().is_none_or(|x| x != ext) {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let to = new.join(name);
        let moved = std::fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|text| convert(&text))
            .and_then(|text| match std::fs::read_to_string(&to) {
                Ok(there) if settled(&there, &text) => Ok(()),
                Ok(_) => Err(format!("{} differs from it", to.display())),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    write_atomic(&to, &text).map_err(|e| e.to_string())
                }
                Err(err) => Err(err.to_string()),
            })
            // The copy in `new` must be on disk before the only other one goes.
            .and_then(|()| sync_dir(new).map_err(|e| e.to_string()))
            .and_then(|()| std::fs::remove_file(&path).map_err(|e| e.to_string()));
        if let Err(why) = moved {
            problems.push(format!("{} was not moved: {why}", path.display()));
        }
    }
    let _ = std::fs::remove_dir(old);
    problems
}

/// Write `text` to `path` so a reader never sees half of it.
///
/// Write-then-rename, with the temp waited for before it is promoted: the
/// rename is atomic, so the file on disk is always one complete snapshot and a
/// crash mid-write leaves the previous one rather than a truncated file — and a
/// truncated `onehand-workspace.toml` is a workspace that no longer loads.
///
/// **Everything that must not be readable half-written goes through here.**
/// The scheme was written twice once, and the second copy is how a bug fixed in
/// the first survived. Two things it does *not* promise. Not a single writer:
/// two processes saving one destination still race, and the last rename wins
/// whole. And not that the *rename* is durable — the containing directory is
/// not waited for, so a crash immediately after one can leave the previous
/// version in place. That costs the newest save; it cannot cost a readable
/// file, which is the property worth paying an extra wait per write for.
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = tmp_path(path, std::process::id(), seq);
    if let Err(e) = write_synced(&tmp, text.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// What reading a config file found.
///
/// The distinction that matters is `Missing` vs `Unreadable`: the first says a
/// folder is free to write into, the second says something is there and we
/// could not make sense of it. Treating the second as the first destroys data.
// No `Eq`: `WorkspaceConfig` carries floats, so the generic form's `Eq` was
// only ever derivable because no caller had asked for it on this type.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceLoad {
    Found(WorkspaceConfig),
    /// No such file. The folder holds no workspace.
    Missing,
    /// The file exists but could not be read or parsed — a permission problem,
    /// an unmounted share, a truncated write, a typo in the TOML. **Never treat
    /// this as an empty folder.**
    Unreadable,
}

impl WorkspaceLoad {
    pub fn found(self) -> Option<WorkspaceConfig> {
        match self {
            WorkspaceLoad::Found(v) => Some(v),
            WorkspaceLoad::Missing | WorkspaceLoad::Unreadable => None,
        }
    }
}

impl WorkspaceConfig {
    /// File name inside a workspace's storage directory.
    pub const FILE: &'static str = "onehand-workspace.toml";

    /// Load `<dir>/onehand-workspace.toml`.
    ///
    /// Three outcomes, not two. Folding them into one `None` meant every caller
    /// read "unreadable" as "empty folder", and the two consequences were both
    /// destructive: binding overwrote a workspace whose config had one bad
    /// character, and a recent whose folder was briefly unreachable (an
    /// unmounted share, a permission blip) was forgotten for good
    ///. Only [`WorkspaceLoad::Missing`] means the folder is free.
    pub fn load_from(dir: &Path) -> WorkspaceLoad {
        let path = dir.join(Self::FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return WorkspaceLoad::Missing,
            Err(e) => {
                eprintln!("onehand: cannot read {}: {e}", path.display());
                return WorkspaceLoad::Unreadable;
            }
        };
        match toml::from_str(&text) {
            Ok(cfg) => WorkspaceLoad::Found(cfg),
            Err(e) => {
                eprintln!("onehand: bad workspace config in {}: {e}", dir.display());
                WorkspaceLoad::Unreadable
            }
        }
    }

    /// Write `<dir>/onehand-workspace.toml`, creating the directory if needed.
    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&dir.join(Self::FILE), &text)
    }
}

/// Global, cross-launch app state at `<config_dir>/onehand/state.toml`.
/// Remembers which workspace storage directories were used, most-recent-first;
/// the next launch reopens `recent_workspaces[0]` (taking precedence over the
/// CLI root).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Legacy single remembered dir (pre-recents). Folded into
    /// `recent_workspaces` on load; mirrors `recent_workspaces[0]` on save so
    /// an older binary reading this file still reopens the right workspace.
    pub(crate) workspace_dir: Option<PathBuf>,
    /// Bound workspace storage dirs, most-recent-first, deduped, capped.
    pub recent_workspaces: Vec<PathBuf>,
}

impl AppState {
    /// Cap on `recent_workspaces`.
    pub const MAX_RECENTS: usize = 8;

    pub(crate) fn path() -> PathBuf {
        config_dir().join("state.toml")
    }

    /// Load state, falling back to the default (nothing remembered) on any error.
    pub fn load() -> Self {
        let mut state: Self = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        state.migrate();
        state
    }

    /// Fold the legacy `workspace_dir` into `recent_workspaces`, dedup, cap.
    fn migrate(&mut self) {
        if let Some(dir) = self.workspace_dir.clone() {
            if !self.recent_workspaces.contains(&dir) {
                self.recent_workspaces.insert(0, dir);
            }
        }
        let mut seen = Vec::new();
        self.recent_workspaces.retain(|d| {
            let fresh = !seen.contains(d);
            if fresh {
                seen.push(d.clone());
            }
            fresh
        });
        self.recent_workspaces.truncate(Self::MAX_RECENTS);
    }

    /// Move `dir` to the front of the recents (inserting if absent), keep the
    /// list capped, and mirror the legacy field. Pure — callers canonicalize.
    pub fn touch(&mut self, dir: PathBuf) {
        self.recent_workspaces.retain(|d| *d != dir);
        self.recent_workspaces.insert(0, dir);
        self.recent_workspaces.truncate(Self::MAX_RECENTS);
        self.workspace_dir = self.recent_workspaces.first().cloned();
    }

    /// Drop `dir` from the recents and re-mirror the legacy field (used by
    /// Unbind: an unbound workspace must not be reopened at boot).
    pub fn forget(&mut self, dir: &Path) {
        self.recent_workspaces.retain(|d| d != dir);
        self.workspace_dir = self.recent_workspaces.first().cloned();
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&Self::path(), &text)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod persist_tests;
