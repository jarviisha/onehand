//! `onehand.toml` parsing — pure, GUI-free.
//!
//! Loads the appearance, the global agent definitions, the `[font]` preference
//! and the remote channels.
//!
//! Two separate tolerances keep a hand-written file working, and they are not
//! the same mechanism. `#[serde(default)]` everywhere covers keys the file
//! *omits*, so a partial file overrides only what it sets; a file that names
//! no agents gets the ones found on the machine ([`AppConfig::from_disk`]).
//! Keys the file has and this build does not are covered by serde's own
//! default of ignoring unknown fields — nothing here
//! opts into `deny_unknown_fields`, which is the attribute that would turn a
//! setting left over from an older build into a refusal to load at all.

use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};

mod agent;
pub mod drive;
pub mod recents;
pub use agent::{
    agent_or_first, default_adapter_args, find_command, join_args, look_blocking, on_machine,
    split_args, AgentEdit, AgentSpec, Launch, Preset, PresetId, PresetInfo, Runs,
    DEFAULT_ACP_ADAPTER,
};
pub(crate) use agent::{default_agents, found_agents, look_key};

mod document;
mod font;
pub use font::{resolve_monospace, FontConfig};

mod secret;
pub use secret::secret;

mod state;
pub use state::AppState;

mod workspace;
pub(crate) use workspace::ProjectConfig;
pub use workspace::{PanelLayout, WorkspaceConfig, WorkspaceLoad};

/// `mode` as a run takes it: a blank mode is none, and leaves the agent in
/// the mode it starts in, which asks more rather than less.
pub fn mode_set(mode: &str) -> Option<String> {
    (!mode.trim().is_empty()).then(|| mode.to_string())
}

/// The mode a lead's worker on agent `worker_agent` starts in: the one that
/// agent is known to need to work without a person granting each edit, else
/// none, which leaves the agent in the mode it starts in. The lead names no
/// mode.
pub fn worker_mode(agents: &[AgentSpec], worker_agent: &str) -> Option<String> {
    agent_or_first(agents, Some(worker_agent))
        .and_then(AgentSpec::claude_or_preset)
        .and_then(|preset| preset.info().worker_mode)
        .map(str::to_string)
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
    /// The chat ids allowed to reach the app, as Telegram numbers them. A
    /// group listed here admits only the members whose own ids are listed too.
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

/// `[control]` — who may use onehand's tools from outside a window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    /// A program run as the user (a script, cron) may use the tools over the
    /// socket beside the instance lock. Off until the user turns it on.
    pub outside: bool,
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
    pub control: ControlConfig,
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
            control: ControlConfig::default(),
            keymap: Default::default(),
        }
    }
}

impl AppConfig {
    /// Parse TOML text. A parse error falls back to defaults rather than
    /// failing the launch.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Bring a config read from disk up to what this build writes. Idempotent,
    /// and run on every read, so the list in memory is the list a save writes.
    ///
    /// - The old Claude default, written out in full by earlier builds (alone
    ///   or behind `env VAR=value`), becomes the preset — **only when `claude`
    ///   is on `path`**: that entry ran the agent bundled in its adapter, and
    ///   the preset runs the machine's, so without one it would stop working.
    /// - A preset with no name takes its own; a name already taken gets a
    ///   number, so every entry stays and a name finds one agent.
    pub(crate) fn settle(&mut self, path: Option<&std::ffi::OsStr>) {
        if find_command(Preset::ClaudeCode.info().binary, path, None).is_some() {
            agent::migrate(&mut self.agents);
        }
        agent::name_uniquely(&mut self.agents);
    }

    /// Serialize back to TOML (round-trips all sections for `persist_agents`).
    pub(crate) fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Resolve config + the path it should be written back to. Search order:
    /// `search_order`, else what a first launch gets (the agents found on
    /// the machine), written back to the global path at the first save.
    pub fn load_resolved() -> (Self, PathBuf) {
        let global = config_dir().join("config.toml");
        let path_var = std::env::var_os("PATH");
        for path in search_order(own_root().is_some(), &global) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                match Self::from_disk(Some(&text), path_var.as_deref()) {
                    Ok(cfg) => return (cfg, path),
                    Err(e) => eprintln!("onehand: bad config {}: {e}", path.display()),
                }
            }
        }
        let cfg = Self::from_disk(None, path_var.as_deref()).expect("no text has nothing to parse");
        (cfg, global)
    }

    /// The config `text` holds, or a first launch's when there is none, as this
    /// build runs it.
    ///
    /// **A file that names no agents is a first launch too**: it gets the
    /// built-in agents installed on the machine, Claude Code first, rather
    /// than a command written out for an agent that may not be there. A list
    /// the person emptied (`agents = []`) is theirs and is kept.
    pub(crate) fn from_disk(
        text: Option<&str>,
        path: Option<&std::ffi::OsStr>,
    ) -> Result<Self, toml::de::Error> {
        let mut cfg = match text {
            Some(text) => Self::parse(text)?,
            None => Self::default(),
        };
        let names_agents = text.is_some_and(|text| {
            toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("agents"))
        });
        if !names_agents {
            cfg.agents = found_agents(path);
        }
        cfg.settle(path);
        Ok(cfg)
    }

    /// Read the config at `path`, apply `edit`, and write back what changed.
    ///
    /// A file that exists but **fails to parse** is left alone and reported:
    /// rewriting it from defaults would permanently destroy the user's other
    /// sections. A *missing* file is a first save and starts from defaults.
    ///
    /// **What changed is measured from the file as read, before it is
    /// settled**, so a save still writes the settled agent list and the
    /// agents a first launch found, as it always has; everything the two
    /// agree on, every comment and every key this build does not know stays
    /// as the file had it.
    ///
    /// Lives here rather than in the front end because the guard belongs to the
    /// file format, not to whoever is drawing the settings screen.
    pub fn update_in_place(path: &Path, edit: impl FnOnce(&mut Self)) -> Result<(), String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
        };
        let unparsed = |e: &dyn std::fmt::Display| format!("{} won't parse: {e}", path.display());
        let before = Self::parse(text.as_deref().unwrap_or("")).map_err(|e| unparsed(&e))?;
        let mut doc: toml_edit::DocumentMut = text
            .as_deref()
            .unwrap_or("")
            .parse()
            .map_err(|e| unparsed(&e))?;
        let mut cfg = Self::from_disk(text.as_deref(), std::env::var_os("PATH").as_deref())
            .map_err(|e| unparsed(&e))?;
        edit(&mut cfg);
        let table = |cfg: &Self| toml::Table::try_from(cfg).map_err(|e| e.to_string());
        document::merge(&mut doc, &table(&before)?, &table(&cfg)?);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        write_atomic(path, &doc.to_string()).map_err(|e| e.to_string())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = self.to_toml().map_err(std::io::Error::other)?;
        write_atomic(path, &text)
    }
}

/// `<config_dir>/onehand/` — the per-user data root (config, sessions, state),
/// or `$ONEHAND_CONFIG_DIR` when it is set.
///
/// The variable lets a second onehand run beside the one in use on a root of
/// its own: two on one root refuse each other at the instance lock. It names
/// onehand's root only, where moving `XDG_CONFIG_HOME` would move every
/// tool's, and `gh`, git and the agents a session starts read theirs there.
///
/// **A test build never reaches the person's own root.** Every workspace has
/// a folder of its own there, so a test that renames one or adds a project
/// would write into the real data; a build with tests in it falls back to a
/// scratch root per process instead.
pub fn config_dir() -> PathBuf {
    own_root().unwrap_or_else(|| {
        #[cfg(any(test, feature = "test-support"))]
        return std::env::temp_dir().join(format!("onehand-test-root-{}", std::process::id()));
        #[cfg(not(any(test, feature = "test-support")))]
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("onehand")
    })
}

/// Where onehand keeps its workspaces, one folder each.
pub(crate) fn workspaces_dir() -> PathBuf {
    workspaces_in(&config_dir())
}

/// Where the root `root` keeps its workspaces.
pub(crate) fn workspaces_in(root: &Path) -> PathBuf {
    root.join("workspaces")
}

/// Where the root `root` keeps what belongs to a project itself, one folder
/// each.
pub(crate) fn projects_in(root: &Path) -> PathBuf {
    root.join("projects")
}

/// `$ONEHAND_CONFIG_DIR`, when it names a root.
fn own_root() -> Option<PathBuf> {
    std::env::var_os("ONEHAND_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// Where a config is looked for, first to last: `./onehand.toml`, then
/// `global`.
///
/// **A root of its own skips the working directory's file.** That file belongs
/// to the instance in use, and one started beside it on another root from the
/// same folder would read it, write into it and lose the other's settings: the
/// instance lock guards a root, not a working directory.
fn search_order(own_root: bool, global: &Path) -> Vec<PathBuf> {
    let local = (!own_root).then(|| PathBuf::from("onehand.toml"));
    local.into_iter().chain([global.to_path_buf()]).collect()
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

/// Put the file or folder `from` at `to`, whole, in one step: the rename a
/// folder made aside is promoted by, or one moved within onehand's root.
/// Beside [`write_atomic`] so the one step both rest on is written once.
pub(crate) fn move_into(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
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
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // Through a symlink to the file it names: renaming over the link would
    // replace it with a plain file, and whatever keeps the real one (a
    // dotfiles checkout) would never see the save.
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = tmp_path(path, std::process::id(), seq);
    // The file's own permissions, set before anything is written: a config a
    // person narrowed because it holds keys must not come back readable by
    // all, not even for the moment between the write and the rename.
    let kept = std::fs::metadata(path).ok().map(|meta| meta.permissions());
    let staged = match kept {
        Some(perms) => std::fs::File::create(&tmp)
            .and_then(|_| std::fs::set_permissions(&tmp, perms))
            .and_then(|()| write_synced(&tmp, text.as_bytes())),
        None => write_synced(&tmp, text.as_bytes()),
    };
    if let Err(e) = staged {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod persist_tests;
