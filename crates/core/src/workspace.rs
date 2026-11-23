//! The central data model: the workspace tree.
//!
//! ```text
//! Workspace { name, roots, active_root }
//!   ProjectRoot { path, label, worktrees, sessions, active_session }  (root.rs)
//!     Session { spec, uid, folder }             (see agent.rs)
//! ```
//!
//! One window hosts exactly one workspace. A workspace groups one or more
//! project roots, one per project; each root runs one or more sessions, each
//! in the project's own folder or one of its worktrees. Pure tree logic lives
//! here, with no view code anywhere near it, so it is testable headless.

use crate::agent::Session;
use crate::project::Projects;

mod build_host;
pub(crate) use build_host::inside_refusal;
pub use build_host::BuildHost;
pub mod drive;
mod migrate;
mod root;
use crate::config::{AgentSpec, PanelLayout, WorkspaceConfig};
pub use drive::WorkspaceDrive;
pub use migrate::migrate_storage_blocking;
pub(crate) use migrate::OFFLOAD;
pub use root::{Offloaded, ProjectRoot, RowFolder};
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

/// A folder as a person names it: `<host>:<path>` on a host, as ssh names
/// one, and the bare path here.
pub fn host_path(host: Option<&HostName>, path: &Path) -> String {
    match host {
        Some(host) => format!("{host}:{}", path.display()),
        None => path.display().to_string(),
    }
}

/// The inverse of [`host_path`]: an entry that is not absolute and has a host
/// name before its first `:/` is a folder on that host. A host path always
/// starts with `/`, and a host name may hold colons of its own (an IPv6
/// address), so the split is there and not at the first `:`.
pub(crate) fn parse_root_key(stored: &Path) -> (Option<HostName>, PathBuf) {
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

/// Canonicalize a folder a person picked when it resolves; keep it as given
/// otherwise, so symlink/`..` aliases of one folder name one workspace —
/// the workspace made from a folder is kept under a name taken from its path.
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
    crate::config::workspaces_dir().join(stem_for(root))
}

/// Where what an offload of the folder at `path` keeps goes: its manifest,
/// what comes back, the backups taken. Kept by folder, as offload is: each
/// folder of a project is offloaded on its own, and the offload code finds
/// its data from the folder alone.
pub(crate) fn offload_dir(path: &Path) -> PathBuf {
    crate::config::offload_in(&crate::config::config_dir()).join(stem_for(path))
}

/// What a launch opens.
#[derive(Debug)]
pub enum Launch {
    /// A workspace already kept, as its file holds it.
    Found(Workspace),
    /// The seed, which nothing on disk holds yet: written once its window
    /// is up.
    Seeded(Workspace),
}

/// What a launch opens: the first of `listed` whose file loads, or else
/// `seed`. Blocking: it reads each file it tries.
///
/// **A folder whose file cannot be read is never written over**, as
/// anywhere else: a seed whose own folder holds one is kept in the first free
/// folder beside it (`<id>-2`, `<id>-3`, …) instead, and the one that cannot
/// be read stays listed with its reason.
pub fn launch_blocking(
    listed: &[crate::config::recents::Listed],
    mut seed: Workspace,
    projects: &Projects,
) -> Launch {
    use crate::config::{WorkspaceConfig, WorkspaceLoad};
    for listed in listed.iter().filter(|l| l.unreadable.is_none()) {
        if let WorkspaceLoad::Found(cfg) = WorkspaceConfig::load_from(&listed.dir) {
            return Launch::Found(Workspace::from_config(cfg, listed.dir.clone(), projects));
        }
    }
    let base = seed.storage_dir.clone();
    let id = crate::config::recents::id_of(&base);
    let mut n = 1;
    loop {
        match WorkspaceConfig::load_from(&seed.storage_dir) {
            WorkspaceLoad::Found(cfg) => {
                let dir = seed.storage_dir.clone();
                return Launch::Found(Workspace::from_config(cfg, dir, projects));
            }
            WorkspaceLoad::Missing => return Launch::Seeded(seed),
            WorkspaceLoad::Unreadable(_) => {
                n += 1;
                seed.storage_dir = base.with_file_name(format!("{id}-{n}"));
            }
        }
    }
}

/// Remove the workspace kept in `dir`: its file, its issues and its note of
/// where it was copied from, then the folder once nothing else is in it.
/// Blocking.
///
/// **Only what is its own.** An earlier build kept a project's offload in
/// the folder of the workspace made from that project, and an offload can
/// hold a whole checkout coming back; a folder holding anything else is left
/// with it, and the answer says so.
pub fn remove_workspace_blocking(dir: &Path) -> Result<(), String> {
    let gone = |result: std::io::Result<()>| match result {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    };
    gone(std::fs::remove_file(
        dir.join(crate::config::WorkspaceConfig::FILE),
    ))?;
    gone(std::fs::remove_file(dir.join(migrate::FROM)))?;
    gone(std::fs::remove_dir_all(crate::issues::issues_dir(dir)))?;
    std::fs::remove_dir(dir).map_err(|e| format!("{} kept: {e}", dir.display()))
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
pub(crate) fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// What the workspace file's `active` starts with when it names a project
/// by id rather than a folder by its key.
const ACTIVE_ID: &str = "id:";

/// One window's single workspace.
#[derive(Debug)]
pub struct Workspace {
    pub name: String,
    pub roots: Vec<ProjectRoot>,
    pub active_root: usize,
    /// The folder onehand keeps this workspace in. Every workspace has one:
    /// none is on screen that nothing on disk remembers.
    pub storage_dir: PathBuf,
    /// How the window's side panels were left. Per window, like the panels
    /// themselves — not per root, because the dock owns the sizes and the dock
    /// is the window's.
    pub layout: PanelLayout,
}

impl Workspace {
    /// Seed a workspace from a single folder (the CLI-arg path), kept in the
    /// folder a workspace made from that folder is kept in. Its row names no
    /// project yet: its window places it as it opens, as any such row.
    pub fn seeded(root: impl Into<PathBuf>) -> Self {
        let root = ProjectRoot::new(root);
        Self {
            name: "Workspace".into(),
            storage_dir: storage_for(&root.path),
            roots: vec![root],
            active_root: 0,
            layout: PanelLayout::default(),
        }
    }

    /// Rebuild a workspace from its persisted shape, bound to `storage_dir`
    ///. Sessions start empty (they re-spawn);
    /// root paths are normalized + deduped (a hand-edited config may hold
    /// aliases) and `active_root` follows its path through the dedup, clamped
    /// in-bounds.
    ///
    /// Each entry draws its project's facts: the project its id names, else
    /// the one holding its folder. An entry no project holds keeps the facts
    /// it carries itself, as one an older build wrote does.
    pub fn from_config(cfg: WorkspaceConfig, storage_dir: PathBuf, projects: &Projects) -> Self {
        // A host's folder means nothing here, so it is kept as typed; a local
        // one is normalized.
        let key = |host: Option<HostName>, path: PathBuf| match host {
            Some(host) => (Some(host), path),
            None => (None, normalize_root(path)),
        };
        let mut roots: Vec<ProjectRoot> = Vec::new();
        for project in cfg.projects {
            let (host, path) = key(project.host, project.path);
            let named = project
                .id
                .as_ref()
                .and_then(|id| projects.get(id))
                .filter(|p| p.folder(host.as_ref(), &path).is_some())
                .or_else(|| projects.find(host.as_ref(), &path).map(|(p, _)| p));
            // Every entry of one project, its own folder's or a worktree's
            // an earlier build kept as a row of its own, folds into one row.
            let at = roots.iter().position(|r| match named {
                Some(p) => r.project.as_ref() == Some(&p.id),
                None => r.project.is_none() && r.host == host && r.path == path,
            });
            if let Some(at) = at {
                roots[at].pinned |= project.pinned;
                continue;
            }
            let mut root = match named {
                Some(p) => ProjectRoot::of(p),
                None => {
                    let mut root = ProjectRoot::new(path);
                    // Only a folder here is offloaded; a hand-edited entry
                    // naming a host's folder is passed over.
                    root.offload = project.offload.filter(|_| host.is_none());
                    root.host = host;
                    root.check = project.check;
                    root.builds = project.builds;
                    root
                }
            };
            root.pinned = project.pinned;
            roots.push(root);
        }
        // The active row by its project's id, or, written by an earlier
        // build, by a folder it holds.
        let active_root = cfg
            .active
            .as_deref()
            .and_then(|active| match active.strip_prefix(ACTIVE_ID) {
                Some(id) => roots
                    .iter()
                    .position(|r| r.project.as_ref().is_some_and(|p| p.as_str() == id)),
                None => {
                    let (host, path) = parse_root_key(Path::new(active));
                    let (host, path) = key(host, path);
                    roots.iter().position(|r| r.host == host && r.holds(&path))
                }
            })
            .unwrap_or(0);
        Self {
            name: if cfg.name.is_empty() {
                "Workspace".into()
            } else {
                cfg.name
            },
            roots,
            active_root,
            storage_dir,
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
        let active = self
            .active_root()
            .filter(|active| !active.transient)
            .or_else(|| kept.first().copied())
            .map(|root| match &root.project {
                Some(id) => format!("{ACTIVE_ID}{id}"),
                None => host_path(root.host.as_ref(), &root.path),
            });
        WorkspaceConfig {
            name: self.name.clone(),
            active,
            projects: kept
                .iter()
                .map(|root| {
                    // A project's facts are its file's; only an entry no
                    // project holds carries its own.
                    let own = root.project.is_none();
                    crate::config::ProjectConfig {
                        id: root.project.clone(),
                        path: root.path.clone(),
                        host: root.host.clone(),
                        pinned: root.pinned,
                        check: root.check.clone().filter(|_| own),
                        offload: root.offload.clone().filter(|_| own && root.host.is_none()),
                        builds: root.builds.clone().into_iter().filter(|_| own).collect(),
                    }
                })
                .collect(),
            layout: self.layout,
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

    /// Each root, by its host and path, in the order the file keeps them: a
    /// local root and a host's may share a path.
    pub fn root_order(&self) -> Vec<(Option<HostName>, PathBuf)> {
        self.roots
            .iter()
            .map(|root| (root.host.clone(), root.path.clone()))
            .collect()
    }

    /// Put the roots back in `order`, as [`Self::root_order`] gave it: what
    /// puts a move back to the order last saved, however many moves came
    /// after it. A root `order` does not name keeps its place among the
    /// others after every root it names; a root it names that has gone is
    /// passed over.
    pub(crate) fn put_order(&mut self, order: &[(Option<HostName>, PathBuf)]) {
        let active = self
            .roots
            .get(self.active_root)
            .map(|r| (r.host.clone(), r.path.clone()));
        let rank = |root: &ProjectRoot| {
            order
                .iter()
                .position(|(host, path)| *host == root.host && *path == root.path)
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
        root.active_session = root.active_session.map(|active| match active {
            active if active == from => to,
            active if from < active && active <= to => active - 1,
            active if to <= active && active < from => active + 1,
            active => active,
        });
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
    #[cfg(test)]
    pub(crate) fn add_root(&mut self, path: impl Into<PathBuf>) -> usize {
        self.add_placed(ProjectRoot::new(normalize_root(path.into())))
    }

    /// [`Self::add_root`] for a folder on `host`. The path is kept as typed:
    /// it names nothing on this machine, so there is nothing to resolve it
    /// against. Adding one that is already a root on that host selects it.
    #[cfg(test)]
    pub(crate) fn add_host_root(&mut self, host: HostName, path: PathBuf) -> usize {
        let mut root = ProjectRoot::new(path);
        root.host = Some(host);
        self.add_placed(root)
    }

    /// Add `root`, a row built for its project, and make it active; a row of
    /// its folder already there is selected instead. Returns its index.
    pub(crate) fn add_placed(&mut self, root: ProjectRoot) -> usize {
        if let Some(i) = self
            .roots
            .iter()
            .position(|r| r.host == root.host && r.path == root.path)
        {
            self.active_root = i;
            return i;
        }
        self.roots.push(root);
        self.active_root = self.roots.len() - 1;
        self.active_root
    }

    /// [`Self::add_root`], **selecting nothing**: a worktree cut for a task
    /// nobody here started (a schedule's, a tool's) must not move what the
    /// person has selected, which is also what *Run on* offers first.
    #[cfg(test)]
    pub(crate) fn add_root_quietly(&mut self, path: impl Into<PathBuf>) -> usize {
        self.add_placed_quietly(ProjectRoot::new(normalize_root(path.into())))
    }

    pub(crate) fn add_placed_quietly(&mut self, root: ProjectRoot) -> usize {
        let selected = self.active_root;
        let idx = self.add_placed(root);
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

    /// Add a transient row holding one session, in `path`, **without
    /// selecting it**: the row of `project`, the project `path` is a folder
    /// of, added for a lead's worker or a run alone, which must not move what
    /// the person is looking at; a row of `path` itself when no project is
    /// known for it. Returns its index.
    ///
    /// `None` when `path` is a folder of a row already, or `project` has a
    /// row: marking a row the person added as transient would quietly drop it
    /// from their workspace file, and a second row of one project is none.
    pub(crate) fn add_transient_root(
        &mut self,
        path: impl Into<PathBuf>,
        project: Option<&crate::project::Project>,
        spec: AgentSpec,
        uid: u64,
    ) -> Option<usize> {
        let path = normalize_root(path.into());
        let of_project =
            |r: &ProjectRoot| project.is_some_and(|p| r.project.as_ref() == Some(&p.id));
        if self
            .roots
            .iter()
            .any(|r| (r.host.is_none() && r.holds(&path)) || of_project(r))
        {
            return None;
        }
        let mut root = match project.filter(|p| p.folder(None, &path).is_some()) {
            Some(project) => ProjectRoot::of(project),
            None => ProjectRoot::new(path.clone()),
        };
        root.transient = true;
        root.sessions.push(Session::new(spec, uid, path));
        root.active_session = Some(0);
        self.roots.push(root);
        Some(self.roots.len() - 1)
    }

    pub(crate) fn select_root(&mut self, idx: usize) {
        if idx < self.roots.len() {
            self.active_root = idx;
        }
    }

    /// Add a session of `spec` running in `path` to the root holding it,
    /// **selecting nothing**:
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
            .position(|r| r.host.is_none() && r.holds(&path))?;
        let shown = idx == self.active_root;
        let root = &mut self.roots[idx];
        // A row with nothing on it shows what it now holds when it is next
        // reached -- unless it is on screen, where its page stays.
        if root.sessions.is_empty() && !shown {
            root.active_session = Some(0);
        }
        root.sessions.push(Session::new(spec, uid, path));
        Some(idx)
    }

    /// Spawn a new session on the active root from `spec`, making it active.
    /// Returns `(root_idx, session_idx)`, or `None` if there is no active root.
    #[cfg(test)]
    pub(crate) fn add_session(&mut self, spec: AgentSpec, uid: u64) -> Option<(usize, usize)> {
        let folder = self.active_root()?.path.clone();
        self.add_session_at(spec, uid, folder)
    }

    /// Spawn a new session on the active root from `spec`, running in
    /// `folder`, one of the root's, and make it active. `None` with no
    /// active root.
    pub(crate) fn add_session_at(
        &mut self,
        spec: AgentSpec,
        uid: u64,
        folder: PathBuf,
    ) -> Option<(usize, usize)> {
        let ri = self.active_root;
        let root = self.roots.get_mut(ri)?;
        root.sessions.push(Session::new(spec, uid, folder));
        root.active_session = Some(root.sessions.len() - 1);
        Some((ri, root.sessions.len() - 1))
    }

    /// Close a session on a given root, keeping `active_session` valid: the
    /// session on screen closing shows a neighbour, the last one the page,
    /// and with the page on screen the page stays.
    pub(crate) fn close_session(&mut self, root_idx: usize, session_idx: usize) {
        let Some(root) = self.roots.get_mut(root_idx) else {
            return;
        };
        if session_idx >= root.sessions.len() {
            return;
        }
        root.sessions.remove(session_idx);
        let Some(active) = root.active_session else {
            return;
        };
        root.active_session = if root.sessions.is_empty() {
            None
        } else if active >= root.sessions.len() {
            Some(root.sessions.len() - 1)
        } else if session_idx < active {
            Some(active - 1)
        } else {
            Some(active)
        };
    }

    pub(crate) fn select_session(&mut self, idx: usize) {
        if let Some(root) = self.active_root_mut() {
            if idx < root.sessions.len() {
                root.active_session = Some(idx);
            }
        }
    }

    /// Put the active project's page on screen, its sessions left open.
    pub(crate) fn show_page(&mut self) {
        if let Some(root) = self.active_root_mut() {
            root.active_session = None;
        }
    }
}

#[cfg(test)]
mod tests;
