//! `onehand-workspace.toml`: one workspace's own persisted shape.

use super::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    /// Roots offloaded to a host, by root, each valued `<host>:<mirror>`: the
    /// host its sessions run on and the mirror they run in there.
    pub offloaded: std::collections::BTreeMap<PathBuf, String>,
    /// Of those, the ones on a branch, by root, each valued with the branch
    /// its host's sessions work on; a root offloaded and not here is a
    /// mirror, as every one kept before branches is.
    pub offloaded_branches: std::collections::BTreeMap<PathBuf, String>,
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
