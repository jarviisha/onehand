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

pub mod drive;
use crate::config::{AgentSpec, PanelLayout, WorkspaceConfig};
pub use drive::WorkspaceDrive;
use std::fmt;
use std::path::{Path, PathBuf};

/// The name of a host a project can live on: what `ssh` is given, except that
/// `local` is this machine, reached without ssh.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct HostName(pub String);

impl HostName {
    pub fn addr(&self) -> crate::host::client::HostAddr {
        crate::host::client::HostAddr {
            ssh: self.0.clone(),
            local: self.0 == "local",
        }
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A project folder bound to a workspace, holding its concurrent sessions.
#[derive(Debug)]
pub struct ProjectRoot {
    pub path: PathBuf,
    /// The host this folder lives on; `None` for a folder on this machine.
    /// A root is told apart by the pair, since one path can name a folder here
    /// and another there.
    pub host: Option<HostName>,
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
    /// Whether this root is left out of the workspace file.
    ///
    /// Set on the worktree an unattended run adds for itself: that root is
    /// dropped when the run ends, and a crash between the add and the drop
    /// would otherwise leave a checkout of one issue in the workspace for good.
    /// Cleared when a person takes the run over, since from then on it is their
    /// project and losing it at the next launch would lose their work's place.
    pub transient: bool,
    /// Whether issues in this project's repository may be picked up and worked
    /// with nobody watching.
    ///
    /// Per project and off until the user turns it on. Being open in the rail
    /// says what somebody is working on, not what an agent may push to and open
    /// pull requests against, and the second is a permission worth giving one
    /// project at a time.
    pub unattended: bool,
    /// The command a workflow's work here must pass before it goes further:
    /// the app runs it itself rather than taking the agent's word for it.
    pub check: Option<String>,
    /// Where this folder's sessions run while it is offloaded. Only a folder
    /// on this machine is offloaded: its tree, editor, terminal and git stay
    /// this machine's.
    pub offload: Option<Offloaded>,
}

/// An offloaded folder's host, its mirror there, and how its code comes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offloaded {
    pub host: HostName,
    /// Absolute, on the host.
    pub mirror: PathBuf,
    /// The branch the host's sessions work on, in branch mode: no key, the
    /// branch fetched here for the person to merge. None for a mirror whose
    /// code comes back into the folder, one side holding it at a time.
    pub branch: Option<String>,
}

impl ProjectRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            label: label_for(&path),
            path,
            host: None,
            sessions: Vec::new(),
            active_session: 0,
            pinned: false,
            transient: false,
            unattended: false,
            check: None,
            offload: None,
        }
    }

    /// The host this root's folder is on, if it is not on this machine.
    pub fn on_host(&self) -> Option<&HostName> {
        self.host.as_ref()
    }

    /// The folder on this machine; `None` for a root on a host, whose path
    /// names nothing here for git, a shell or a file read to reach.
    pub fn local_path(&self) -> Option<&Path> {
        self.host.is_none().then_some(self.path.as_path())
    }

    /// Where this root's sessions run when not here: a host root's own host
    /// and folder, or an offloaded root's host and its mirror there.
    pub fn session_host(&self) -> Option<(&HostName, &Path)> {
        match (&self.host, &self.offload) {
            (Some(host), _) => Some((host, &self.path)),
            (None, Some(on)) => Some((&on.host, &on.mirror)),
            (None, None) => None,
        }
    }

    /// Whether the features that act on this machine's folder by themselves
    /// (workflows, tasks, schedules, unattended runs, worktrees, the check
    /// command, the remote bridge) reach it: a folder here whose sessions run
    /// here too.
    pub fn runs_here(&self) -> bool {
        self.host.is_none() && self.offload.is_none()
    }

    /// Read the active session, if any. Index is kept in-bounds by mutators.
    pub fn active_session(&self) -> Option<&Session> {
        self.sessions.get(self.active_session)
    }
}

/// How a root is written in the workspace file: its path, or `host:path` for a
/// folder on a host. A local root is stored absolute, so the two never read
/// alike.
fn root_key(host: Option<&HostName>, path: &Path) -> PathBuf {
    match host {
        Some(_) => PathBuf::from(host_path(host, path)),
        // Kept as it is: a local path need not be text.
        None => path.to_path_buf(),
    }
}

/// A folder as a person names it: `<host>:<path>` on a host, as ssh names
/// one, and the bare path here.
pub fn host_path(host: Option<&HostName>, path: &Path) -> String {
    match host {
        Some(host) => format!("{host}:{}", path.display()),
        None => path.display().to_string(),
    }
}

fn root_key_of(root: &ProjectRoot) -> PathBuf {
    root_key(root.host.as_ref(), &root.path)
}

/// The inverse of [`root_key`]: an entry that is not absolute and has a host
/// name before its first `:/` is a folder on that host. A host path always
/// starts with `/`, and a host name may hold colons of its own (an IPv6
/// address), so the split is there and not at the first `:`.
fn parse_root_key(stored: &Path) -> (Option<HostName>, PathBuf) {
    let text = stored.to_string_lossy();
    if !stored.is_absolute() {
        if let Some(at) = text.find(":/") {
            if at > 0 {
                return (
                    Some(HostName(text[..at].into())),
                    PathBuf::from(&text[at + 1..]),
                );
            }
        }
    }
    (None, stored.to_path_buf())
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

/// Where a workspace created *from* a project folder keeps its config.
///
/// Under the per-user data root, never inside the project. A workspace file
/// written into the folder that is also its first project root leaves a clean
/// repository dirty with a file that shows in that project's own tree and
/// change count, and committing it publishes every root's absolute path on this
/// machine. Nothing else the app persists lives outside the data root either.
///
/// The name carries the folder's, so the folder is recognisable in a picker and
/// in the recents list, plus a digest of the whole path — two checkouts of one
/// repository are two projects with one folder name, and the digest is also
/// what makes the answer *stable*: creating a workspace on a folder that
/// already has one finds it again instead of quietly making a second.
///
/// The digest is written out here rather than taken from `DefaultHasher`,
/// which is explicitly not promised to be the same from one Rust release to the
/// next — a hash that moved under a toolchain upgrade would point every
/// existing project at a fresh empty workspace.
pub fn storage_for(root: &Path) -> PathBuf {
    crate::config::config_dir()
        .join("workspaces")
        .join(stem_for(root))
}

/// `root` as a name something about it can be kept under: its folder's name,
/// then a digest of its whole path. One rule for every such name, so a
/// workspace's folder and a project's issues can never disagree about which
/// project a path is.
pub fn stem_for(root: &Path) -> String {
    let name = root
        .file_name()
        .map(|s| slug(&s.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "workspace".to_string());
    let digest = fnv1a(root.as_os_str().as_encoded_bytes());
    format!("{name}-{digest:08x}")
}

/// A folder name as a name a folder may have: what is not plainly a letter,
/// digit, `_` or `.` becomes a dash, runs collapse and the ends are trimmed.
fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// FNV-1a, 32-bit. Not a security property — this only has to spread folder
/// paths apart and give the same answer every launch.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// One window's single workspace.
#[derive(Debug)]
pub struct Workspace {
    pub name: String,
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
        // A host's folder means nothing here, so it is kept as typed; a local
        // one is normalized.
        let key = |stored: PathBuf| match parse_root_key(&stored) {
            (Some(host), path) => (Some(host), path),
            (None, path) => (None, normalize_root(path)),
        };
        let active_key = cfg.roots.get(cfg.active_root).cloned().map(key);
        let mut roots: Vec<ProjectRoot> = Vec::new();
        for path in cfg.roots {
            let (host, path) = key(path);
            if !roots.iter().any(|r| r.host == host && r.path == path) {
                let mut root = ProjectRoot::new(path);
                root.host = host;
                roots.push(root);
            }
        }
        // Pins are stored as paths rather than indices, so a root added or
        // removed elsewhere in the file cannot silently move a pin onto a
        // different project.
        let pinned: Vec<_> = cfg.pinned.into_iter().map(key).collect();
        let unattended: Vec<_> = cfg.unattended.into_iter().map(key).collect();
        let checks: Vec<_> = cfg
            .checks
            .into_iter()
            .map(|(path, check)| (key(path), check))
            .collect();
        let mut branches = cfg.offloaded_branches;
        let offloaded: Vec<_> = cfg
            .offloaded
            .into_iter()
            .filter_map(|(path, on)| match parse_root_key(Path::new(&on)) {
                (Some(host), mirror) => {
                    let branch = branches.remove(&path);
                    Some((
                        key(path),
                        Offloaded {
                            host,
                            mirror,
                            branch,
                        },
                    ))
                }
                (None, _) => None,
            })
            .collect();
        for root in &mut roots {
            let own = (root.host.clone(), root.path.clone());
            // Only a folder here is offloaded; a hand-edited entry naming a
            // host's folder is passed over.
            root.offload = offloaded
                .iter()
                .find(|(k, _)| *k == own && root.host.is_none())
                .map(|(_, on)| on.clone());
            root.pinned = pinned.contains(&own);
            root.unattended = unattended.contains(&own);
            root.check = checks
                .iter()
                .find(|(k, _)| *k == own)
                .map(|(_, check)| check.clone());
        }
        let active_root = active_key
            .and_then(|(h, p)| roots.iter().position(|r| r.host == h && r.path == p))
            .unwrap_or(0);
        Self {
            name: if cfg.name.is_empty() {
                "Workspace".into()
            } else {
                cfg.name
            },
            roots,
            active_root,
            storage_dir: Some(storage_dir),
            // Clamped on the way in, not on the way out: a size that came back
            // from disk has not been through the dock's own drag limits.
            layout: cfg.layout.clamped(),
        }
    }

    /// The persisted shape of this workspace (name + root paths + active index).
    ///
    /// Transient roots are left out, and the active index is counted among the
    /// roots that remain — pointing at the first of them when the active root
    /// is itself one of those left out.
    pub fn to_config(&self) -> WorkspaceConfig {
        let kept: Vec<&ProjectRoot> = self.roots.iter().filter(|r| !r.transient).collect();
        let active_root = self
            .active_root()
            .and_then(|active| {
                kept.iter()
                    .position(|r| r.host == active.host && r.path == active.path)
            })
            .unwrap_or(0);
        WorkspaceConfig {
            name: self.name.clone(),
            roots: kept.iter().map(|r| root_key_of(r)).collect(),
            active_root,
            layout: self.layout,
            pinned: kept
                .iter()
                .filter(|root| root.pinned)
                .map(|root| root_key_of(root))
                .collect(),
            unattended: kept
                .iter()
                .filter(|root| root.unattended)
                .map(|root| root_key_of(root))
                .collect(),
            checks: kept
                .iter()
                .filter_map(|root| Some((root_key_of(root), root.check.clone()?)))
                .collect(),
            offloaded: kept
                .iter()
                .filter(|root| root.host.is_none())
                .filter_map(|root| {
                    let on = root.offload.as_ref()?;
                    Some((root_key_of(root), host_path(Some(&on.host), &on.mirror)))
                })
                .collect(),
            offloaded_branches: kept
                .iter()
                .filter(|root| root.host.is_none())
                .filter_map(|root| {
                    let branch = root.offload.as_ref()?.branch.clone()?;
                    Some((root_key_of(root), branch))
                })
                .collect(),
        }
    }

    /// Pin or unpin a root.
    ///
    /// Explicit, never automatic. Reordering the project list on the app's own
    /// initiative destroys the thing the list is good for -- the user knows
    /// where their projects are, and a list that rearranges itself has to be
    /// re-read every time it is looked at.
    pub(crate) fn toggle_pin(&mut self, idx: usize) {
        if let Some(root) = self.roots.get_mut(idx) {
            root.pinned = !root.pinned;
        }
    }

    /// Turn unattended runs on or off for a root.
    pub(crate) fn toggle_unattended(&mut self, idx: usize) {
        if let Some(root) = self.roots.get_mut(idx) {
            root.unattended = !root.unattended;
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

    /// Move the root drawn at display position `from` to display position `to`.
    ///
    /// **Display positions rather than `roots` indices**, because the rail drags
    /// what it draws and pinned roots are drawn first. The permutation is
    /// written back into `roots`, so the order the user dropped a row into is
    /// the order that persists and [`Self::display_order`] stays a stable
    /// partition of it.
    ///
    /// A drag that would cross the pin line is **clamped to the near side of
    /// it**: pinned roots are held at the top on purpose, and a row that
    /// appeared to cross and then sprang back would say nothing about why. So
    /// dropping a pinned project onto an unpinned one puts it last among the
    /// pinned instead, and the pin stays the stronger statement.
    ///
    /// Answers the display position the root landed at, or `None` when nothing
    /// moved; the way back is `move_root(landed, from)`, never `(to, from)`,
    /// since `to` may have been clamped.
    pub(crate) fn move_root(&mut self, from: usize, to: usize) -> Option<usize> {
        if from >= self.roots.len() {
            return None;
        }
        let mut order = self.display_order();
        let pinned = self.roots.iter().filter(|root| root.pinned).count();
        let (first, last) = match from < pinned {
            true => (0, pinned - 1),
            false => (pinned, order.len() - 1),
        };
        let to = to.clamp(first, last);
        if from == to {
            return None;
        }
        let moved = order.remove(from);
        order.insert(to, moved);
        // `active_root` is the only index into `roots` this permutation can
        // move: everything else about a root -- its fold, its terminal, its git
        // status, its editors -- is keyed by path.
        self.active_root = order
            .iter()
            .position(|&i| i == self.active_root)
            .unwrap_or(self.active_root);
        let mut taken: Vec<Option<ProjectRoot>> = std::mem::take(&mut self.roots)
            .into_iter()
            .map(Some)
            .collect();
        self.roots = order.into_iter().filter_map(|i| taken[i].take()).collect();
        Some(to)
    }

    /// Each root's path, in the order the file keeps them.
    pub fn root_order(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|root| root.path.clone()).collect()
    }

    /// Put the roots back in `order`, as [`Self::root_order`] gave it: what
    /// puts a move back to the order last saved, however many moves came
    /// after it. A root `order` does not name keeps its place among the
    /// others after every root it names; a root it names that has gone is
    /// passed over.
    pub(crate) fn put_order(&mut self, order: &[PathBuf]) {
        let active = self
            .roots
            .get(self.active_root)
            .map(|r| (r.host.clone(), r.path.clone()));
        let rank = |root: &ProjectRoot| {
            order
                .iter()
                .position(|path| *path == root.path)
                .unwrap_or(order.len())
        };
        self.roots.sort_by_key(rank);
        if let Some(i) =
            active.and_then(|(h, p)| self.roots.iter().position(|r| r.host == h && r.path == p))
        {
            self.active_root = i;
        }
    }

    /// Move a session within its root.
    ///
    /// Within one root and never across: a session is an agent bound to that
    /// project's files, so there is nowhere else for it to be.
    pub(crate) fn move_session(&mut self, root_idx: usize, from: usize, to: usize) {
        let Some(root) = self.roots.get_mut(root_idx) else {
            return;
        };
        if from >= root.sessions.len() || to >= root.sessions.len() || from == to {
            return;
        }
        let session = root.sessions.remove(from);
        root.sessions.insert(to, session);
        // The session on screen has to stay the session on screen: the index
        // moved, and it is the index that says which one is showing.
        root.active_session = match root.active_session {
            active if active == from => to,
            active if from < active && active <= to => active - 1,
            active if to <= active && active < from => active + 1,
            active => active,
        };
    }

    pub fn active_root(&self) -> Option<&ProjectRoot> {
        self.roots.get(self.active_root)
    }

    pub(crate) fn active_root_mut(&mut self) -> Option<&mut ProjectRoot> {
        self.roots.get_mut(self.active_root)
    }

    /// Add a project root and make it active. Returns its index.
    ///
    /// The path is normalized first, and adding a folder on this machine that's
    /// already a root (directly, or via a symlink/`..` alias) *selects* it
    /// instead of creating a twin; a root on a host with the same path is
    /// another root, so only `(None, path)` is compared. Twins of one local folder would
    /// share terminals/layouts/editors while splitting sessions.
    pub(crate) fn add_root(&mut self, path: impl Into<PathBuf>) -> usize {
        self.add_found(None, normalize_root(path.into()))
    }

    /// [`Self::add_root`] for a folder on `host`. The path is kept as typed:
    /// it names nothing on this machine, so there is nothing to resolve it
    /// against. Adding one that is already a root on that host selects it.
    pub(crate) fn add_host_root(&mut self, host: HostName, path: PathBuf) -> usize {
        self.add_found(Some(host), path)
    }

    fn add_found(&mut self, host: Option<HostName>, path: PathBuf) -> usize {
        if let Some(i) = self
            .roots
            .iter()
            .position(|r| r.host == host && r.path == path)
        {
            self.active_root = i;
            return i;
        }
        let mut root = ProjectRoot::new(path);
        root.host = host;
        self.roots.push(root);
        self.active_root = self.roots.len() - 1;
        self.active_root
    }

    /// [`Self::add_root`], **selecting nothing**: a worktree cut for a task
    /// nobody here started (a schedule's, a tool's) must not move what the
    /// person has selected, which is also what *Run on* offers first.
    pub(crate) fn add_root_quietly(&mut self, path: impl Into<PathBuf>) -> usize {
        let selected = self.active_root;
        let idx = self.add_root(path);
        self.active_root = selected;
        idx
    }

    /// Drop a root, keeping `active_root` valid and in-bounds.
    pub(crate) fn remove_root(&mut self, idx: usize) {
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

    /// Add `path` as a transient root holding one session, **without selecting
    /// it** — the root an unattended run works in, which must not move what the
    /// user is looking at. Returns its index.
    ///
    /// `None` when `path` is already a project: marking a root the user added as
    /// transient would quietly drop it from their workspace file.
    pub(crate) fn add_transient_root(
        &mut self,
        path: impl Into<PathBuf>,
        spec: AgentSpec,
        uid: u64,
    ) -> Option<usize> {
        let path = normalize_root(path.into());
        if self
            .roots
            .iter()
            .any(|r| r.host.is_none() && r.path == path)
        {
            return None;
        }
        let mut root = ProjectRoot::new(path);
        root.transient = true;
        root.sessions.push(Session::new(spec, uid));
        self.roots.push(root);
        Some(self.roots.len() - 1)
    }

    pub(crate) fn select_root(&mut self, idx: usize) {
        if idx < self.roots.len() {
            self.active_root = idx;
        }
    }

    /// Add a session of `spec` to the root at `path`, **selecting nothing**:
    /// a run taken up again on a project already open must not move what the
    /// person is looking at. Returns the root's index; `None` without one.
    pub(crate) fn add_session_quietly(
        &mut self,
        path: &Path,
        spec: AgentSpec,
        uid: u64,
    ) -> Option<usize> {
        let path = normalize_root(path.to_path_buf());
        let idx = self
            .roots
            .iter()
            .position(|r| r.host.is_none() && r.path == path)?;
        self.roots[idx].sessions.push(Session::new(spec, uid));
        Some(idx)
    }

    /// Spawn a new session on the active root from `spec`, making it active.
    /// Returns `(root_idx, session_idx)`, or `None` if there is no active root.
    pub(crate) fn add_session(&mut self, spec: AgentSpec, uid: u64) -> Option<(usize, usize)> {
        let ri = self.active_root;
        let root = self.roots.get_mut(ri)?;
        root.sessions.push(Session::new(spec, uid));
        root.active_session = root.sessions.len() - 1;
        Some((ri, root.sessions.len() - 1))
    }

    /// Close a session on a given root, keeping `active_session` valid.
    pub(crate) fn close_session(&mut self, root_idx: usize, session_idx: usize) {
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

    pub(crate) fn select_session(&mut self, idx: usize) {
        if let Some(root) = self.active_root_mut() {
            if idx < root.sessions.len() {
                root.active_session = idx;
            }
        }
    }
}

#[cfg(test)]
mod tests;
