//! `onehand.toml` parsing — pure, GUI-free.
//!
//! Loads global agent definitions plus the `[font]` and `[icons]` theming
//! sections. `#[serde(default)]` everywhere means a partial file overrides only
//! the keys it sets — a `[font]`-only file keeps the default agents.

use serde::{Deserialize, Serialize};
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

/// `[font]` — type + master zoom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Base body size (default ~14).
    pub size: f32,
    /// Master zoom, 0.5–3.0.
    pub scale: f32,
    pub sans: Option<String>,
    pub monospace: Option<String>,
    pub fallbacks: Vec<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            size: 14.0,
            scale: 1.0,
            sans: None,
            monospace: None,
            fallbacks: Vec::new(),
        }
    }
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

impl FontConfig {
    /// Clamp `scale` into the documented 0.5–3.0 range.
    ///
    /// Range-checked rather than `clamp`ed, for the reason
    /// [`PanelLayout::clamped`] spells out: `f32::clamp` passes NaN through.
    pub fn clamped_scale(&self) -> f32 {
        if (0.5..=3.0).contains(&self.scale) {
            self.scale
        } else if self.scale.is_finite() {
            self.scale.clamp(0.5, 3.0)
        } else {
            1.0
        }
    }

    /// Validated base size: a nonsensical `size` (0, negative, NaN, or absurd)
    /// falls back to the default instead of rendering the whole UI invisible.
    pub fn clamped_size(&self) -> f32 {
        if (6.0..=72.0).contains(&self.size) {
            self.size
        } else {
            14.0
        }
    }
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
    pub fn key(self) -> &'static str {
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

/// `[icons]` — per-role hex overrides; bad/missing hex keeps the dark default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IconConfig {
    pub accent: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub danger: Option<String>,
    pub muted: Option<String>,
    pub faint: Option<String>,
    pub strong: Option<String>,
    pub discovery: Option<String>,
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
    pub icons: IconConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            agents: default_agents(),
            font: FontConfig::default(),
            icons: IconConfig::default(),
        }
    }
}

/// The ACP adapter the default agent runs, pinned.
///
/// **Not `@latest`.** Every session in this app goes through this adapter, so
/// `@latest` meant the same onehand binary could talk to a different protocol
/// implementation on two consecutive days, with no way to reproduce a report
///. Bumping this is a commit, which is the point: the change is
/// visible, bisectable, and revertable.
///
/// The pin is a *default*, not a lock — `onehand.toml` and the agent manager
/// both override it, so anyone wanting the newest adapter can still ask for it.
pub const DEFAULT_ACP_ADAPTER: &str = "@agentclientprotocol/claude-agent-acp@0.70.0";

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
pub fn default_agents() -> Vec<AgentSpec> {
    vec![AgentSpec {
        name: "Claude Code".into(),
        command: "npx".into(),
        args: default_adapter_args(),
    }]
}

impl AppConfig {
    /// Parse TOML text. A parse error falls back to defaults rather than
    /// failing the launch.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize back to TOML (round-trips all sections for `persist_agents`).
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
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
            Err(_) => Self::default(),
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
    /// Workspace icon tint as `#RRGGBB`. `None` (and any malformed hex) falls
    /// back to a stable palette color derived from the name.
    pub icon_color: Option<String>,
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
    pub fn clamped(self) -> Self {
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

/// Write `text` to `path` so a reader never sees half of it.
///
/// Write-then-rename with a per-call temp name, the same shape (and for the
/// same reason) as `chat::persist::write_stored`, which had it from the start
/// while every config writer used a plain `fs::write`. The
/// rename is atomic, so the file on disk is always one complete snapshot: a
/// crash mid-write leaves the previous one, not a truncated file — and a
/// truncated `onehand-workspace.toml` is a workspace that no longer loads.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp{seq}"));
    if let Err(e) = std::fs::write(&tmp, text) {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    Found(T),
    /// No such file. The folder holds no workspace.
    Missing,
    /// The file exists but could not be read or parsed — a permission problem,
    /// an unmounted share, a truncated write, a typo in the TOML. **Never treat
    /// this as an empty folder.**
    Unreadable,
}

impl<T> Load<T> {
    pub fn found(self) -> Option<T> {
        match self {
            Load::Found(v) => Some(v),
            Load::Missing | Load::Unreadable => None,
        }
    }

    /// Whether the folder is free to bind a new workspace into.
    pub fn is_missing(&self) -> bool {
        matches!(self, Load::Missing)
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
    ///. Only [`Load::Missing`] means the folder is free.
    pub fn load_from(dir: &Path) -> Load<Self> {
        let path = dir.join(Self::FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Load::Missing,
            Err(e) => {
                eprintln!("onehand: cannot read {}: {e}", path.display());
                return Load::Unreadable;
            }
        };
        match toml::from_str(&text) {
            Ok(cfg) => Load::Found(cfg),
            Err(e) => {
                eprintln!("onehand: bad workspace config in {}: {e}", dir.display());
                Load::Unreadable
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
    pub workspace_dir: Option<PathBuf>,
    /// Bound workspace storage dirs, most-recent-first, deduped, capped.
    pub recent_workspaces: Vec<PathBuf>,
}

impl AppState {
    /// Cap on `recent_workspaces`.
    pub const MAX_RECENTS: usize = 8;

    pub fn path() -> PathBuf {
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
mod tests {
    use super::*;

    #[test]
    fn empty_file_keeps_defaults() {
        let cfg = AppConfig::parse("").unwrap();
        assert_eq!(cfg.agents, default_agents());
        assert_eq!(cfg.font.size, 14.0);
    }

    #[test]
    fn font_only_file_keeps_default_agents() {
        // A `[font]`-only file keeps the default agents.
        let cfg = AppConfig::parse("[font]\nsize = 15.0\n").unwrap();
        assert_eq!(cfg.font.size, 15.0);
        assert_eq!(cfg.agents, default_agents());
    }

    #[test]
    fn appearance_parses_and_defaults_to_the_system() {
        assert_eq!(AppConfig::parse("").unwrap().appearance, Appearance::System);
        for (text, want) in [
            ("appearance = \"dark\"\n", Appearance::Dark),
            ("appearance = \"light\"\n", Appearance::Light),
            ("appearance = \"System\"\n", Appearance::System),
        ] {
            assert_eq!(AppConfig::parse(text).unwrap().appearance, want);
        }
    }

    #[test]
    fn a_misspelled_appearance_does_not_take_the_config_with_it() {
        // The agent list lives in this same file. One mistyped word must not be
        // able to reset it, so an unknown value reads as "follow the desktop".
        let cfg = AppConfig::parse(
            "appearance = \"sepia\"\n\n[[agents]]\nname = \"Gemini\"\ncommand = \"gemini-acp\"\n",
        )
        .unwrap();
        assert_eq!(cfg.appearance, Appearance::System);
        assert_eq!(cfg.agents[0].name, "Gemini");
    }

    #[test]
    fn a_config_carrying_an_appearance_still_serializes() {
        // A bare key has to be declared before the sections: TOML writes every
        // plain value above the first table, and a serializer asked to emit one
        // after a table fails outright rather than reordering.
        let cfg = AppConfig {
            appearance: Appearance::Dark,
            ..AppConfig::default()
        };
        let text = cfg
            .to_toml()
            .expect("a config with an appearance must save");
        assert_eq!(AppConfig::parse(&text).unwrap(), cfg);
    }

    #[test]
    fn agents_parse() {
        let text = r#"
            [[agents]]
            name = "Gemini"
            command = "gemini-acp"
            args = ["--stdio"]
        "#;
        let cfg = AppConfig::parse(text).unwrap();
        assert_eq!(cfg.agents.len(), 1);
        assert_eq!(cfg.agents[0].name, "Gemini");
        assert_eq!(cfg.agents[0].command, "gemini-acp");
    }

    #[test]
    fn legacy_kind_field_is_ignored() {
        // Old configs carried `kind = "acp"|"terminal"`; it's now an unknown key
        // and must parse without error (serde ignores it).
        let text = r#"
            [[agents]]
            name = "Old"
            kind = "terminal"
            command = "claude"
        "#;
        let cfg = AppConfig::parse(text).unwrap();
        assert_eq!(cfg.agents[0].name, "Old");
        assert_eq!(cfg.agents[0].command, "claude");
    }

    #[test]
    fn scale_is_clamped() {
        let mut f = FontConfig {
            scale: 9.0,
            ..FontConfig::default()
        };
        assert_eq!(f.clamped_scale(), 3.0);
        f.scale = 0.1;
        assert_eq!(f.clamped_scale(), 0.5);
    }

    #[test]
    fn nonsense_size_falls_back_to_default() {
        let mut f = FontConfig::default();
        for bad in [0.0, -3.0, f32::NAN, 500.0] {
            f.size = bad;
            assert_eq!(f.clamped_size(), 14.0);
        }
        f.size = 18.0;
        assert_eq!(f.clamped_size(), 18.0);
    }

    #[test]
    fn legacy_profile_section_is_ignored() {
        // Older configs carried a `[profile]` section; it must parse as an
        // unknown key, not an error.
        let cfg = AppConfig::parse("[profile]\nname = \"Jarviis\"\navatar = \"/home/me/a.png\"\n")
            .unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn roundtrips_through_toml() {
        let cfg = AppConfig::default();
        let text = cfg.to_toml().unwrap();
        let back = AppConfig::parse(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn workspace_config_roundtrips() {
        let cfg = WorkspaceConfig {
            name: "Mine".into(),
            roots: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            active_root: 1,
            icon_color: Some("#3B82F6".into()),
            layout: PanelLayout::default(),
            pinned: Vec::new(),
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: WorkspaceConfig = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn workspace_config_save_load_to_dir() {
        let dir = std::env::temp_dir().join(format!("onehand-ws-test-{}", std::process::id()));
        let cfg = WorkspaceConfig {
            name: "Persisted".into(),
            roots: vec![PathBuf::from("/x")],
            active_root: 0,
            icon_color: None,
            layout: PanelLayout::default(),
            pinned: Vec::new(),
        };
        cfg.save_to(&dir).unwrap();
        assert_eq!(WorkspaceConfig::load_from(&dir), Load::Found(cfg));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_finite_size_falls_back_instead_of_staying_nan() {
        // `f32::clamp` returns NaN for NaN, so the bound alone was no guard.
        let d = PanelLayout::default();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let got = PanelLayout {
                workbench_w: bad,
                terminal_h: bad,
                ..d
            }
            .clamped();
            assert_eq!(got.workbench_w, d.workbench_w, "for {bad}");
            assert_eq!(got.terminal_h, d.terminal_h, "for {bad}");
        }
        // Ordinary out-of-range values still clamp to the bound, not the default.
        let got = PanelLayout {
            workbench_w: 5.0,
            terminal_h: 99_999.0,
            ..d
        }
        .clamped();
        assert_eq!(got.workbench_w, PanelLayout::MIN);
        assert_eq!(got.terminal_h, PanelLayout::MAX);
    }

    /// The rail is clamped by its *own* range, not the docks'.
    ///
    /// Sharing one range would let a saved rail restore at 120px, where a
    /// project row is an icon and half a name, or at 2000px, where the rail is
    /// the window. The docks are sized by preference; the rail is sized by what
    /// its rows have to fit.
    #[test]
    fn the_rail_is_clamped_by_its_own_range() {
        let d = PanelLayout::default();
        assert_eq!(
            PanelLayout { rail_w: 120.0, ..d }.clamped().rail_w,
            PanelLayout::RAIL_MIN
        );
        assert_eq!(
            PanelLayout {
                rail_w: 2000.0,
                ..d
            }
            .clamped()
            .rail_w,
            PanelLayout::RAIL_MAX
        );
        assert_eq!(
            PanelLayout {
                rail_w: f32::NAN,
                ..d
            }
            .clamped()
            .rail_w,
            d.rail_w
        );
        // The default must itself be inside the range it is the fallback for.
        assert!((PanelLayout::RAIL_MIN..=PanelLayout::RAIL_MAX).contains(&d.rail_w));

        assert_eq!(
            FontConfig {
                scale: f32::NAN,
                ..FontConfig::default()
            }
            .clamped_scale(),
            1.0
        );
    }

    #[test]
    fn agent_args_survive_a_round_trip_through_the_form() {
        // The contract is exactly this: edit nothing, save, get the same list.
        for args in [
            vec![],
            vec!["-y".to_string(), "@scope/pkg@1.2.3".to_string()],
            vec!["--system-prompt".to_string(), "hello world".to_string()],
            vec!["--json".to_string(), r#"{"a": "b c"}"#.to_string()],
            vec![
                "a'b".to_string(),
                "c\"d".to_string(),
                "back\\slash".to_string(),
            ],
            vec!["".to_string(), "after-empty".to_string()],
        ] {
            let line = join_args(&args);
            assert_eq!(split_args(&line), args, "round trip failed for {args:?}");
        }
    }

    #[test]
    fn split_args_reads_ordinary_lines_the_obvious_way() {
        assert_eq!(split_args("  -y   pkg  "), vec!["-y", "pkg"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("'a b' c"), vec!["a b", "c"]);
        // A half-typed quote keeps what has been typed rather than eating it.
        assert_eq!(split_args("a \"b c"), vec!["a", "b c"]);
    }

    #[test]
    fn an_unreadable_workspace_config_is_not_an_empty_folder() {
        // The distinction the binding guard and the recents list both act on:
        // `Missing` frees the folder, `Unreadable` must not.
        let dir = std::env::temp_dir().join("onehand-ws-unreadable-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            WorkspaceConfig::load_from(&dir),
            Load::<WorkspaceConfig>::Missing
        );
        assert!(WorkspaceConfig::load_from(&dir).is_missing());

        std::fs::write(dir.join(WorkspaceConfig::FILE), "name = \"unclosed").unwrap();
        assert_eq!(
            WorkspaceConfig::load_from(&dir),
            Load::<WorkspaceConfig>::Unreadable
        );
        assert!(!WorkspaceConfig::load_from(&dir).is_missing());
        assert!(WorkspaceConfig::load_from(&dir).found().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_state_roundtrips() {
        let st = AppState {
            workspace_dir: Some(PathBuf::from("/home/me/ws")),
            recent_workspaces: vec![PathBuf::from("/home/me/ws"), PathBuf::from("/other")],
        };
        let text = toml::to_string_pretty(&st).unwrap();
        let back: AppState = toml::from_str(&text).unwrap();
        assert_eq!(st, back);
    }

    #[test]
    fn app_state_migrates_legacy_workspace_dir() {
        let mut st: AppState = toml::from_str("workspace_dir = \"/x\"\n").unwrap();
        st.migrate();
        assert_eq!(st.recent_workspaces, vec![PathBuf::from("/x")]);
    }

    #[test]
    fn app_state_migrate_does_not_duplicate() {
        let mut st = AppState {
            workspace_dir: Some(PathBuf::from("/x")),
            recent_workspaces: vec![PathBuf::from("/x"), PathBuf::from("/y")],
        };
        st.migrate();
        assert_eq!(
            st.recent_workspaces,
            vec![PathBuf::from("/x"), PathBuf::from("/y")]
        );
    }

    #[test]
    fn app_state_touch_moves_to_front_dedupes_and_caps() {
        let mut st = AppState::default();
        for i in 0..9 {
            st.touch(PathBuf::from(format!("/ws{i}")));
        }
        assert_eq!(st.recent_workspaces.len(), AppState::MAX_RECENTS);
        assert_eq!(st.recent_workspaces[0], PathBuf::from("/ws8"));
        // Re-touching an existing entry reorders without duplicating.
        st.touch(PathBuf::from("/ws3"));
        assert_eq!(st.recent_workspaces[0], PathBuf::from("/ws3"));
        assert_eq!(st.recent_workspaces.len(), AppState::MAX_RECENTS);
        assert_eq!(st.workspace_dir, Some(PathBuf::from("/ws3")));
    }

    #[test]
    fn app_state_forget_removes_and_remirrors() {
        let mut st = AppState::default();
        st.touch(PathBuf::from("/a"));
        st.touch(PathBuf::from("/b"));
        st.forget(Path::new("/b"));
        assert_eq!(st.recent_workspaces, vec![PathBuf::from("/a")]);
        assert_eq!(st.workspace_dir, Some(PathBuf::from("/a")));
        st.forget(Path::new("/a"));
        assert_eq!(st.workspace_dir, None);
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;

    /// The whole point of `update_in_place`: an unparseable file must survive.
    #[test]
    fn a_broken_config_is_never_overwritten() {
        let dir = std::env::temp_dir().join("onehand-cfg-guard");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let broken = "this is not = = valid toml [[[";
        std::fs::write(&path, broken).unwrap();

        let err = AppConfig::update_in_place(&path, |cfg| cfg.agents.clear())
            .expect_err("a broken config must be reported, not rewritten");
        assert!(err.contains("won't parse"), "unexpected error: {err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            broken,
            "the user's file must be left exactly as it was"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A missing file is a first save, not an error.
    #[test]
    fn a_missing_config_starts_from_defaults() {
        let dir = std::env::temp_dir().join("onehand-cfg-first-save");
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("config.toml");

        AppConfig::update_in_place(&path, |cfg| {
            cfg.agents = vec![AgentSpec {
                name: "Only".into(),
                command: "echo".into(),
                args: vec![],
            }]
        })
        .expect("first save must succeed");

        let saved = AppConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.agents.len(), 1);
        assert_eq!(saved.agents[0].name, "Only");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn installed(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_preference_that_is_not_installed_falls_through() {
        // The whole point: asking for a family that is not there renders in the
        // body face and says nothing, so a missing preference must not win.
        let have = installed(&["Liberation Mono", "Cantarell"]);
        assert_eq!(
            resolve_monospace(["DejaVu Sans Mono"], &have).as_deref(),
            Some("Liberation Mono")
        );
    }

    #[test]
    fn preferences_are_tried_in_order_and_case_insensitively() {
        let have = installed(&["JetBrains Mono", "Menlo"]);
        assert_eq!(
            resolve_monospace(["jetbrains mono", "Menlo"], &have).as_deref(),
            Some("JetBrains Mono"),
            "the enumerated spelling is returned, not the one asked for"
        );
    }

    #[test]
    fn anything_naming_itself_monospace_beats_giving_up() {
        let have = installed(&["Zed Mono", "Adwaita Mono", "Cantarell"]);
        assert_eq!(
            resolve_monospace([], &have).as_deref(),
            Some("Adwaita Mono"),
            "sorted, so two launches on one machine agree"
        );
    }

    #[test]
    fn nothing_monospace_means_no_answer() {
        // Not a wrong answer dressed as a right one: the caller keeps whatever
        // default it had, which is no worse.
        assert_eq!(
            resolve_monospace(["Menlo"], &installed(&["Cantarell"])),
            None
        );
    }
}
