//! The central data model: the workspace tree.
//!
//! ```text
//! Workspace { name, roots, active_root }
//!   ProjectRoot { path, label, sessions, active_session }
//!     Session { spec, uid }                     (see agent.rs)
//! ```
//!
//! One window hosts exactly one workspace. A workspace groups one or more
//! project roots; each root runs one or more sessions. Pure tree logic lives
//! here, with no view code anywhere near it, so it is testable headless.

use crate::agent::Session;
use crate::config::{AgentSpec, PanelLayout, WorkspaceConfig};
use std::path::{Path, PathBuf};

/// A project folder bound to a workspace, holding its concurrent sessions.
#[derive(Debug)]
pub struct ProjectRoot {
    pub path: PathBuf,
    pub label: String,
    pub sessions: Vec<Session>,
    pub active_session: usize,
    /// Whether this root is held at the top of the list.
    ///
    /// A *display* fact, not a structural one: `roots` keeps its own order and
    /// every index in the app still means the same root. Pinning only changes
    /// the order rows are drawn in, which is what makes it safe to toggle while
    /// sessions, editors and terminals are keyed by index and by path.
    pub pinned: bool,
}

impl ProjectRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            label: label_for(&path),
            path,
            sessions: Vec::new(),
            active_session: 0,
            pinned: false,
        }
    }

    /// Read the active session, if any. Index is kept in-bounds by mutators.
    pub fn active_session(&self) -> Option<&Session> {
        self.sessions.get(self.active_session)
    }

    pub fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active_session)
    }
}

/// The workspace-icon initial for a name: the first letter of its first word,
/// uppercased. Empty names fall back to `"W"` so the badge never renders blank.
pub fn initials(name: &str) -> String {
    let out: String = name
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .take(1)
        .collect();
    if out.is_empty() {
        "W".into()
    } else {
        out.to_uppercase()
    }
}

/// The folder's display label — its final path component, else the whole path.
pub fn label_for(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Normalize a root path: canonicalize when it resolves — collapsing symlink /
/// `..` / `./` aliases that would otherwise split the app's per-root state
/// (every satellite map is keyed by this path). A path that doesn't resolve
/// (unmounted, deleted) is kept as given.
fn normalize_root(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Canonicalize a workspace *storage* dir when it resolves; keep it as given
/// otherwise. Applied at every point a storage dir enters the system (bind,
/// open, recents) so symlink/`..` aliases of the same dir compare equal —
/// the open-workspace dedup and the recents list key off this path.
pub fn canon_dir(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// One window's single workspace.
#[derive(Debug)]
pub struct Workspace {
    pub name: String,
    /// Icon tint as `#RRGGBB` (workspace settings pick it; persisted). `None`
    /// ⇒ a stable palette color derived from the name at render time.
    pub icon_color: Option<String>,
    pub roots: Vec<ProjectRoot>,
    pub active_root: usize,
    /// Storage directory this workspace persists to, if bound. `None` ⇒
    /// nothing persists.
    pub storage_dir: Option<PathBuf>,
    /// How the window's side panels were left. Per window, like the panels
    /// themselves — not per root, because the dock owns the sizes and the dock
    /// is the window's.
    pub layout: PanelLayout,
}

impl Workspace {
    /// Seed a workspace from a single project root (the CLI-arg path).
    pub fn seeded(root: impl Into<PathBuf>) -> Self {
        let root = ProjectRoot::new(root);
        Self {
            name: "Workspace".into(),
            icon_color: None,
            roots: vec![root],
            active_root: 0,
            storage_dir: None,
            layout: PanelLayout::default(),
        }
    }

    /// Rebuild a workspace from its persisted shape, bound to `storage_dir`
    ///. Sessions start empty (they re-spawn);
    /// root paths are normalized + deduped (a hand-edited config may hold
    /// aliases) and `active_root` follows its path through the dedup, clamped
    /// in-bounds.
    pub fn from_config(cfg: WorkspaceConfig, storage_dir: PathBuf) -> Self {
        let active_path = cfg.roots.get(cfg.active_root).cloned().map(normalize_root);
        let mut roots: Vec<ProjectRoot> = Vec::new();
        for path in cfg.roots {
            let path = normalize_root(path);
            if !roots.iter().any(|r| r.path == path) {
                roots.push(ProjectRoot::new(path));
            }
        }
        // Pins are stored as paths rather than indices, so a root added or
        // removed elsewhere in the file cannot silently move a pin onto a
        // different project.
        let pinned: Vec<PathBuf> = cfg.pinned.into_iter().map(normalize_root).collect();
        for root in &mut roots {
            root.pinned = pinned.contains(&root.path);
        }
        let active_root = active_path
            .and_then(|p| roots.iter().position(|r| r.path == p))
            .unwrap_or(0);
        Self {
            name: if cfg.name.is_empty() {
                "Workspace".into()
            } else {
                cfg.name
            },
            icon_color: cfg.icon_color,
            roots,
            active_root,
            storage_dir: Some(storage_dir),
            // Clamped on the way in, not on the way out: a size that came back
            // from disk has not been through the dock's own drag limits.
            layout: cfg.layout.clamped(),
        }
    }

    /// The persisted shape of this workspace (name + root paths + active index).
    pub fn to_config(&self) -> WorkspaceConfig {
        WorkspaceConfig {
            name: self.name.clone(),
            roots: self.roots.iter().map(|r| r.path.clone()).collect(),
            active_root: self.active_root,
            icon_color: self.icon_color.clone(),
            layout: self.layout,
            pinned: self
                .roots
                .iter()
                .filter(|root| root.pinned)
                .map(|root| root.path.clone())
                .collect(),
        }
    }

    /// Pin or unpin a root.
    ///
    /// Explicit, never automatic. Reordering the project list on the app's own
    /// initiative destroys the thing the list is good for -- the user knows
    /// where their projects are, and a list that rearranges itself has to be
    /// re-read every time it is looked at.
    pub fn toggle_pin(&mut self, idx: usize) {
        if let Some(root) = self.roots.get_mut(idx) {
            root.pinned = !root.pinned;
        }
    }

    /// The order rows are drawn in: pinned roots first, each group otherwise
    /// untouched.
    ///
    /// Indices into `roots`, not roots — every caller acts on a root *by index*
    /// (select, remove, add a session), so handing back a reordered list of
    /// roots would make the display order and the addressing order disagree,
    /// which is how a click lands on the project next to the one clicked.
    ///
    /// A stable partition, so unpinning a project puts it back exactly where it
    /// was rather than at the end.
    pub fn display_order(&self) -> Vec<usize> {
        let (mut pinned, rest): (Vec<usize>, Vec<usize>) =
            (0..self.roots.len()).partition(|&i| self.roots[i].pinned);
        pinned.extend(rest);
        pinned
    }

    pub fn active_root(&self) -> Option<&ProjectRoot> {
        self.roots.get(self.active_root)
    }

    pub fn active_root_mut(&mut self) -> Option<&mut ProjectRoot> {
        self.roots.get_mut(self.active_root)
    }

    /// Add a project root and make it active. Returns its index.
    ///
    /// The path is normalized first, and adding a folder that's already a root
    /// (directly, or via a symlink/`..` alias) *selects* it instead of creating
    /// a twin — every per-root map in the app is keyed by this path, so twins
    /// would silently share terminals/layouts/editors while splitting sessions.
    pub fn add_root(&mut self, path: impl Into<PathBuf>) -> usize {
        let path = normalize_root(path.into());
        if let Some(i) = self.roots.iter().position(|r| r.path == path) {
            self.active_root = i;
            return i;
        }
        self.roots.push(ProjectRoot::new(path));
        self.active_root = self.roots.len() - 1;
        self.active_root
    }

    /// Drop a root, keeping `active_root` valid and in-bounds.
    pub fn remove_root(&mut self, idx: usize) {
        if idx >= self.roots.len() {
            return;
        }
        self.roots.remove(idx);
        if self.roots.is_empty() {
            self.active_root = 0;
        } else if self.active_root >= self.roots.len() {
            self.active_root = self.roots.len() - 1;
        } else if idx < self.active_root {
            self.active_root -= 1;
        }
    }

    pub fn select_root(&mut self, idx: usize) {
        if idx < self.roots.len() {
            self.active_root = idx;
        }
    }

    /// Spawn a new session on the active root from `spec`, making it active.
    /// Returns `(root_idx, session_idx)`, or `None` if there is no active root.
    pub fn add_session(&mut self, spec: AgentSpec, uid: u64) -> Option<(usize, usize)> {
        let ri = self.active_root;
        let root = self.roots.get_mut(ri)?;
        root.sessions.push(Session::new(spec, uid));
        root.active_session = root.sessions.len() - 1;
        Some((ri, root.sessions.len() - 1))
    }

    /// Close a session on a given root, keeping `active_session` valid.
    pub fn close_session(&mut self, root_idx: usize, session_idx: usize) {
        let Some(root) = self.roots.get_mut(root_idx) else {
            return;
        };
        if session_idx >= root.sessions.len() {
            return;
        }
        root.sessions.remove(session_idx);
        if root.sessions.is_empty() {
            root.active_session = 0;
        } else if root.active_session >= root.sessions.len() {
            root.active_session = root.sessions.len() - 1;
        } else if session_idx < root.active_session {
            root.active_session -= 1;
        }
    }

    pub fn select_session(&mut self, idx: usize) {
        if let Some(root) = self.active_root_mut() {
            if idx < root.sessions.len() {
                root.active_session = idx;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentSpec;

    fn spec(name: &str) -> AgentSpec {
        AgentSpec {
            name: name.into(),
            command: "x".into(),
            args: vec![],
        }
    }

    #[test]
    fn initials_take_first_letter_of_first_word() {
        assert_eq!(initials("onehand dev"), "O");
        assert_eq!(initials("My Big Workspace"), "M");
        assert_eq!(initials("workspace"), "W");
        assert_eq!(initials("x"), "X");
        assert_eq!(initials(""), "W");
        assert_eq!(initials("   "), "W");
        assert_eq!(initials("việt nam"), "V"); // unicode-safe
    }

    #[test]
    fn label_uses_last_component() {
        assert_eq!(label_for(Path::new("/home/me/proj")), "proj");
        assert_eq!(label_for(Path::new("/")), "/");
    }

    #[test]
    fn remove_root_keeps_active_in_bounds() {
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.add_root("/c"); // active = 2
        ws.remove_root(2);
        assert_eq!(ws.active_root, 1);
        ws.remove_root(0);
        assert_eq!(ws.active_root, 0);
        assert_eq!(ws.roots.len(), 1);
    }

    #[test]
    fn removing_below_active_shifts_active_down() {
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.add_root("/c");
        ws.select_root(2);
        ws.remove_root(0); // remove below active
        assert_eq!(ws.active_root, 1); // still points at /c
        assert_eq!(ws.roots[1].path, PathBuf::from("/c"));
    }

    #[test]
    fn add_root_dedupes_an_existing_path() {
        // Adding a folder that's already a root selects it instead of creating
        // a twin (per-root state is keyed by path — twins would alias it).
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.select_root(0);
        let i = ws.add_root("/b");
        assert_eq!(ws.roots.len(), 2);
        assert_eq!(i, 1);
        assert_eq!(ws.active_root, 1);
    }

    /// Pinning reorders the *drawing*, never the roots.
    ///
    /// Every caller addresses a root by its index -- select it, remove it, add
    /// a session to it -- so if a pin moved roots in the list, a click would
    /// land on the project next to the one clicked.
    #[test]
    fn pinning_reorders_the_drawing_and_nothing_else() {
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.add_root("/c");
        let paths: Vec<_> = ws.roots.iter().map(|r| r.path.clone()).collect();

        ws.toggle_pin(2);
        assert_eq!(ws.display_order(), vec![2, 0, 1]);
        assert_eq!(
            ws.roots.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
            paths,
            "the roots themselves must not move"
        );
    }

    /// A stable partition, so unpinning puts a project back where it was rather
    /// than at the end of the list.
    #[test]
    fn unpinning_restores_the_original_place() {
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.add_root("/c");
        ws.toggle_pin(1);
        assert_eq!(ws.display_order(), vec![1, 0, 2]);
        ws.toggle_pin(1);
        assert_eq!(ws.display_order(), vec![0, 1, 2]);
    }

    /// Pins survive a save/load, and follow their project rather than its slot.
    #[test]
    fn pins_are_stored_by_path() {
        let mut ws = Workspace::seeded("/a");
        ws.add_root("/b");
        ws.storage_dir = Some(PathBuf::from("/store"));
        ws.toggle_pin(1);

        let mut cfg = ws.to_config();
        assert_eq!(cfg.pinned, vec![PathBuf::from("/b")]);
        // A root inserted ahead of it in the file must not slide the pin onto
        // the project that took its index.
        cfg.roots.insert(0, PathBuf::from("/z"));
        let back = Workspace::from_config(cfg, PathBuf::from("/store"));
        let pinned: Vec<_> = back
            .roots
            .iter()
            .filter(|r| r.pinned)
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(pinned, vec![PathBuf::from("/b")]);
    }

    #[test]
    fn from_config_dedupes_and_follows_active_path() {
        let cfg = WorkspaceConfig {
            name: "W".into(),
            roots: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/a"),
            ],
            active_root: 2, // the duplicate /a
            icon_color: None,
            layout: PanelLayout::default(),
            pinned: Vec::new(),
        };
        let ws = Workspace::from_config(cfg, PathBuf::from("/store"));
        assert_eq!(ws.roots.len(), 2);
        assert_eq!(ws.active_root, 0); // follows /a to its surviving slot
    }

    #[test]
    fn config_roundtrips_roots_and_active() {
        let mut ws = Workspace::seeded("/a");
        ws.name = "Mine".into();
        ws.icon_color = Some("#55C38C".into());
        ws.add_root("/b");
        ws.add_root("/c");
        ws.select_root(1);
        let cfg = ws.to_config();
        assert_eq!(cfg.name, "Mine");
        assert_eq!(cfg.icon_color.as_deref(), Some("#55C38C"));
        assert_eq!(
            cfg.roots,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ]
        );
        assert_eq!(cfg.active_root, 1);

        let back = Workspace::from_config(cfg, PathBuf::from("/store"));
        assert_eq!(back.name, "Mine");
        assert_eq!(back.icon_color.as_deref(), Some("#55C38C"));
        assert_eq!(back.roots.len(), 3);
        assert_eq!(back.active_root, 1);
        assert_eq!(back.storage_dir, Some(PathBuf::from("/store")));
        assert!(back.roots[0].sessions.is_empty());
    }

    /// A hand-edited or stale layout must not restore a panel that is open,
    /// reachable by its shortcut, and too small to see — or one that covers the
    /// window because it was last sized on a much bigger monitor.
    #[test]
    fn a_nonsense_layout_is_clamped_but_its_flags_are_kept() {
        let cfg = WorkspaceConfig {
            name: "W".into(),
            roots: vec![PathBuf::from("/a")],
            active_root: 0,
            icon_color: None,
            layout: PanelLayout {
                workbench_w: 0.0,
                workbench_open: true,
                terminal_h: 99_999.0,
                terminal_open: false,
                rail_w: 9_999.0,
            },
            pinned: Vec::new(),
        };
        let ws = Workspace::from_config(cfg, PathBuf::from("/store"));
        assert!(ws.layout.workbench_w >= 120.0);
        assert!(ws.layout.terminal_h <= 2000.0);
        // The rail has a narrower range of its own: it is sized by what its
        // rows have to say, not by preference.
        assert!(ws.layout.rail_w <= PanelLayout::RAIL_MAX);
        // Sizes are corrected; what the user chose to have open is not.
        assert!(ws.layout.workbench_open);
        assert!(!ws.layout.terminal_open);
    }

    #[test]
    fn layout_survives_a_config_round_trip() {
        let mut ws = Workspace::seeded("/a");
        ws.storage_dir = Some(PathBuf::from("/store"));
        ws.layout = PanelLayout {
            workbench_w: 512.0,
            workbench_open: true,
            terminal_h: 300.0,
            terminal_open: true,
            rail_w: 280.0,
        };
        let back = Workspace::from_config(ws.to_config(), PathBuf::from("/store"));
        assert_eq!(back.layout, ws.layout);
    }

    #[test]
    fn from_config_clamps_out_of_bounds_active() {
        let cfg = WorkspaceConfig {
            name: String::new(),
            roots: vec![PathBuf::from("/a")],
            active_root: 9,
            icon_color: None,
            layout: PanelLayout::default(),
            pinned: Vec::new(),
        };
        let ws = Workspace::from_config(cfg, PathBuf::from("/store"));
        assert_eq!(ws.active_root, 0);
        assert_eq!(ws.name, "Workspace"); // empty name falls back
    }

    #[test]
    fn canon_dir_keeps_unresolvable_path() {
        let p = PathBuf::from("/definitely/not/a/real/dir-xyz");
        assert_eq!(canon_dir(p.clone()), p);
    }

    #[test]
    fn canon_dir_collapses_dot_dot_alias() {
        let tmp = std::env::temp_dir();
        let sub = tmp.join(format!("onehand-canon-test-{}", std::process::id()));
        std::fs::create_dir_all(&sub).unwrap();
        let alias = sub.join("..").join(sub.file_name().unwrap());
        assert_eq!(canon_dir(alias), canon_dir(sub.clone()));
        let _ = std::fs::remove_dir_all(&sub);
    }

    #[test]
    fn add_and_close_session_tracks_active() {
        let mut ws = Workspace::seeded("/a");
        ws.add_session(spec("one"), 1);
        ws.add_session(spec("two"), 2);
        assert_eq!(ws.active_root().unwrap().active_session, 1);
        ws.close_session(0, 1);
        assert_eq!(ws.active_root().unwrap().active_session, 0);
        ws.close_session(0, 0);
        assert!(ws.active_root().unwrap().sessions.is_empty());
        assert_eq!(ws.active_root().unwrap().active_session, 0);
    }
}
