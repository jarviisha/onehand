//! `onehand-workspace.toml`: one workspace's own persisted shape.

use super::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A workspace's own persisted shape: its name, its projects in the rail's
/// order, which is active, and how the window was arranged. Sessions are *not*
/// persisted; they re-spawn. Stored as `onehand-workspace.toml` in the
/// workspace's folder.
///
/// **One record per project.** Every fact about a project sits in its own
/// `[[projects]]` table, so a new one is a field rather than another table
/// keyed by path that has to be matched back to the list. A file written by an
/// earlier build, which kept `roots` beside such tables, still reads
/// ([`Raw`]); it is written in this shape at its next save.
// No `Eq`: `PanelLayout` carries pixel sizes, and float equality is not an
// equivalence relation. `PartialEq` is all any caller here wants anyway.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "Raw")]
pub struct WorkspaceConfig {
    pub name: String,
    /// The active project, by its id (`id:<id>`), or for a row of no project
    /// by its folder's key (`<host>:<path>` on a host, the bare path here),
    /// rather than by index: an index slides onto another project when one
    /// before it is removed. A key matching no project reads as the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    pub(crate) projects: Vec<ProjectConfig>,
    /// How the window's side panels were arranged. A file without this
    /// section gets the built-in arrangement.
    pub layout: PanelLayout,
}

/// One project in a workspace file: the project, by id, and its own folder.
/// A file an earlier build wrote may hold an entry per worktree too; each
/// folds into its project's row as it is read. Everything but `path` is left out when it holds
/// its default, so a hand-read file says only what was chosen.
///
/// **The folder is kept beside the id for an older build**, which reads only
/// the folder and so still opens the workspace. The project's own facts
/// (`check`, `offload`, `builds`) live in its project file; an entry carries
/// them only while it names no project, as one an older build wrote does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectConfig {
    /// The project this folder is of; none for an entry no project holds yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<crate::project::ProjectId>,
    pub path: PathBuf,
    /// The host the folder lives on; none for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<crate::workspace::HostName>,
    /// Held at the top of the rail.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// The command a workflow's work must pass before it goes further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The host its sessions run on, and how; only for a folder here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload: Option<crate::workspace::Offloaded>,
    /// The hosts it builds on, and what comes back from each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builds: Vec<crate::workspace::BuildHost>,
}

/// What a workspace file may hold, this build's shape and the one before it.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Raw {
    name: String,
    active: Option<String>,
    projects: Vec<ProjectConfig>,
    layout: PanelLayout,
    roots: Vec<PathBuf>,
    active_root: usize,
    pinned: Vec<PathBuf>,
    checks: BTreeMap<PathBuf, String>,
    /// Each valued `<host>:<mirror>`.
    offloaded: BTreeMap<PathBuf, String>,
    offloaded_branches: BTreeMap<PathBuf, String>,
}

impl From<Raw> for WorkspaceConfig {
    /// This build's shape when the file has any of it; else the one before,
    /// each root's facts gathered from the tables keyed by its path.
    fn from(raw: Raw) -> Self {
        use crate::workspace::{parse_root_key, Offloaded};
        if !raw.projects.is_empty() || raw.roots.is_empty() {
            return Self {
                name: raw.name,
                active: raw.active,
                projects: raw.projects,
                layout: raw.layout,
            };
        }
        let active = raw
            .roots
            .get(raw.active_root)
            .map(|key| key.to_string_lossy().into_owned());
        let mut branches = raw.offloaded_branches;
        let projects = raw
            .roots
            .iter()
            .map(|key| {
                let (host, path) = parse_root_key(key);
                // Only a folder here is offloaded; an entry naming a host's
                // folder is passed over.
                let offload = raw
                    .offloaded
                    .get(key)
                    .filter(|_| host.is_none())
                    .and_then(|on| match parse_root_key(Path::new(on)) {
                        (Some(host), mirror) => Some(Offloaded {
                            host,
                            mirror,
                            branch: branches.remove(key),
                        }),
                        (None, _) => None,
                    });
                ProjectConfig {
                    id: None,
                    pinned: raw.pinned.contains(key),
                    check: raw.checks.get(key).cloned(),
                    offload,
                    host,
                    path,
                    builds: Vec::new(),
                }
            })
            .collect();
        Self {
            name: raw.name,
            active,
            projects,
            layout: raw.layout,
        }
    }
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
/// fixed by design — conversation and Workbench side by side in the centre,
/// the terminal under both — so what the user changes is how wide, how tall, and whether each is
/// showing. Four facts, no GUI types, and nothing here that stops compiling
/// when the library's layout format moves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    /// Width of the Workbench's slot beside the conversation, in pixels.
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
            workbench_w: 480.0,
            workbench_open: false,
            terminal_h: 240.0,
            terminal_open: false,
            rail_w: 320.0,
        }
    }
}

impl PanelLayout {
    /// Smallest a restored panel may be.
    ///
    /// A size read back from disk has not been through the dock's own drag
    /// clamps, and a hand-edited `0.0` would restore a panel that is open,
    /// focusable by its shortcut, and invisible.
    pub(crate) const MIN: f32 = 120.0;
    /// Largest, so a stale value from a much bigger monitor cannot restore a
    /// panel that covers the whole window on a smaller one.
    pub(crate) const MAX: f32 = 2000.0;

    /// The rail's narrowest useful width.
    ///
    /// Narrower than this and a project row is its icon, a truncated name and
    /// nothing else -- no branch, no change count, no room for the session
    /// titles nested under it, which is the whole content of the rail.
    pub const RAIL_MIN: f32 = 232.0;
    /// The widest it may be dragged.
    ///
    /// Past this the rail stops being chrome and starts competing with the
    /// conversation for the window. Wide enough that a long session title and
    /// a project's git line both read whole.
    pub const RAIL_MAX: f32 = 448.0;

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
    Unreadable(String),
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
                return WorkspaceLoad::Unreadable(e.to_string());
            }
        };
        match toml::from_str(&text) {
            Ok(cfg) => WorkspaceLoad::Found(cfg),
            Err(e) => {
                eprintln!("onehand: bad workspace config in {}: {e}", dir.display());
                WorkspaceLoad::Unreadable(e.to_string())
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

#[cfg(test)]
mod tests;
