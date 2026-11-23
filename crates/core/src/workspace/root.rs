//! One row of the workspace: a project, its folders, and the sessions running
//! in them.
//!
//! **A row is a project, not a folder.** A project's worktrees are folders of
//! its row, each told from the project with its branch and its offload; a
//! session runs in one of them, and the window's panels follow the folder of
//! the session on screen.

use super::{label_for, BuildHost, HostName};
use crate::agent::Session;
use crate::project::{Folder, Project, ProjectId};
use std::path::{Path, PathBuf};

/// A project folder bound to a workspace, holding its concurrent sessions.
#[derive(Debug)]
pub struct ProjectRoot {
    /// The project's own folder: what the row is keyed by.
    pub path: PathBuf,
    /// The host this folder lives on; `None` for a folder on this machine.
    /// A root is told apart by the pair, since one path can name a folder here
    /// and another there.
    pub host: Option<HostName>,
    /// The project this row is; none for a worker's transient folder, and for
    /// an entry no project could be found for.
    pub project: Option<ProjectId>,
    /// What the row is called: its project's name. Like `check`, `builds`,
    /// `offload` and `worktrees`, a fact of the project's as it was last told
    /// ([`Self::told`]), kept here so the window draws it without asking.
    pub label: String,
    /// The project's worktrees, in its order: further folders of this row a
    /// session may run in.
    pub worktrees: Vec<RowFolder>,
    pub sessions: Vec<Session>,
    /// The session on screen when this row is reached, or none: the
    /// project's own page, which can be on screen while its sessions run.
    pub active_session: Option<usize>,
    /// Whether this root is held at the top of the list.
    ///
    /// A *display* fact, not a structural one: `roots` keeps its own order and
    /// every index in the app still means the same root. Pinning only changes
    /// the order rows are drawn in, which is what makes it safe to toggle while
    /// sessions, editors and terminals are keyed by index and by path.
    pub pinned: bool,
    /// Whether this root is left out of the workspace file.
    ///
    /// Set on a project row added for a lead's worker or a run alone, when
    /// the workspace did not hold it: that row is dropped when its last
    /// session closes, and a crash between the add and the drop would
    /// otherwise leave it in the workspace for good.
    pub transient: bool,
    /// The command a workflow's work here must pass before it goes further:
    /// the app runs it itself rather than taking the agent's word for it.
    pub check: Option<String>,
    /// Where the sessions in the project's own folder run while it is
    /// offloaded. Only a folder on this machine is offloaded: its tree,
    /// editor, terminal and git stay this machine's.
    pub offload: Option<Offloaded>,
    /// The hosts this folder builds on; only for one whose sessions run here.
    pub builds: Vec<BuildHost>,
}

/// A worktree of a row's project, as the row was last told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFolder {
    pub path: PathBuf,
    pub branch: String,
    /// Where its sessions run while it is offloaded, each folder on its own.
    pub offload: Option<Offloaded>,
}

/// An offloaded folder's host, its mirror there, and how its code comes back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Offloaded {
    pub host: HostName,
    /// Absolute, on the host.
    pub mirror: PathBuf,
    /// The branch the host's sessions work on, in branch mode: no key, the
    /// branch fetched here for the person to merge. None for a mirror whose
    /// code comes back into the folder, one side holding it at a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

impl ProjectRoot {
    /// The hosts this folder builds on: none while its sessions run on a
    /// host, where it builds already. What it was given is kept meanwhile,
    /// for when it runs here again.
    pub fn build_hosts(&self) -> &[BuildHost] {
        match self.runs_here() {
            true => &self.builds,
            false => &[],
        }
    }

    /// Every host this row uses: those its sessions run on, its own folder's
    /// and its offloaded worktrees', and, its own folder running here, each
    /// it builds on.
    pub fn hosts_used(&self) -> Vec<&HostName> {
        let mut used = self.session_hosts();
        if self.session_host().is_none() {
            for entry in self.build_hosts() {
                if !used.contains(&&entry.host) {
                    used.push(&entry.host);
                }
            }
        }
        used
    }

    /// Every host sessions of this row run on: its own folder's, then each
    /// offloaded worktree's, each once.
    pub fn session_hosts(&self) -> Vec<&HostName> {
        let mut hosts: Vec<&HostName> = Vec::new();
        let own = self.session_host().map(|(host, _)| host);
        let worktrees =
            (self.worktrees.iter()).filter_map(|w| w.offload.as_ref().map(|on| &on.host));
        for host in own.into_iter().chain(worktrees) {
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        hosts
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            label: label_for(&path),
            path,
            host: None,
            project: None,
            worktrees: Vec::new(),
            sessions: Vec::new(),
            active_session: None,
            pinned: false,
            transient: false,
            check: None,
            offload: None,
            builds: Vec::new(),
        }
    }

    /// The row of `project`, at its own folder, drawing its facts.
    pub fn of(project: &Project) -> Self {
        let mut root = Self::new(project.path.clone());
        root.host = project.host.clone();
        root.project = Some(project.id.clone());
        root.told(project);
        root
    }

    /// Take `project`'s facts as this row draws them. A row not at the
    /// project's own folder keeps what it had.
    pub(crate) fn told(&mut self, project: &Project) {
        if project.folder(self.host.as_ref(), &self.path) != Some(Folder::Home) {
            return;
        }
        // Only a folder here is offloaded.
        let here = self.host.is_none();
        self.label = project.name.clone();
        self.check = project.check.clone();
        self.builds = project.builds.clone();
        self.offload = project.offload.clone().filter(|_| here);
        self.worktrees = project
            .worktrees
            .iter()
            .map(|worktree| RowFolder {
                path: worktree.path.clone(),
                branch: worktree.branch.clone(),
                offload: worktree.offload.clone().filter(|_| here),
            })
            .collect();
    }

    /// Every folder of this row: its own, then its worktrees'.
    pub fn folders(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.path.as_path()).chain(self.worktrees.iter().map(|w| w.path.as_path()))
    }

    /// Every folder this row uses: its own, its worktrees', and any other a
    /// session of it runs in (a worktree its project no longer records).
    pub fn folders_used(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self.folders().map(Path::to_path_buf).collect();
        for session in &self.sessions {
            if !folders.contains(&session.folder) {
                folders.push(session.folder.clone());
            }
        }
        folders
    }

    /// Whether `folder` is one of this row's.
    pub fn holds(&self, folder: &Path) -> bool {
        self.folders().any(|at| at == folder)
    }

    /// The branch of the worktree at `folder`; none for the row's own folder
    /// and for one it does not hold.
    pub fn branch_of(&self, folder: &Path) -> Option<&str> {
        self.worktrees
            .iter()
            .find(|worktree| worktree.path == folder)
            .map(|worktree| worktree.branch.as_str())
    }

    /// Where a session in `folder` runs when not here: an offloaded
    /// worktree's host and mirror, else the row's own ([`Self::session_host`]).
    pub fn host_of(&self, folder: &Path) -> Option<(&HostName, &Path)> {
        match self
            .worktrees
            .iter()
            .find(|worktree| worktree.path == folder)
        {
            Some(worktree) => worktree
                .offload
                .as_ref()
                .map(|on| (&on.host, on.mirror.as_path())),
            // The row's own folder runs where the row does; a folder the row
            // no longer records (a worktree forgotten under a session) was
            // never offloaded with it, so it runs here. A host's row has no
            // folder but its own.
            None if folder == self.path => self.session_host(),
            None => None,
        }
    }

    /// Each folder of this row that is offloaded, with where to: its own,
    /// then its worktrees', each on its own.
    pub fn offloaded(&self) -> impl Iterator<Item = (&Path, &Offloaded)> {
        let own = self.offload.as_ref().map(|on| (self.path.as_path(), on));
        own.into_iter().chain(
            self.worktrees
                .iter()
                .filter_map(|w| Some((w.path.as_path(), w.offload.as_ref()?))),
        )
    }

    /// Whether the sessions in `folder`, one of this row's, run on this
    /// machine: neither the row on a host nor that folder offloaded.
    pub fn runs_here_in(&self, folder: &Path) -> bool {
        self.host.is_none() && self.host_of(folder).is_none()
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

    /// Where the sessions in this row's own folder run when not here: a
    /// host root's own host and folder, or an offloaded root's host and its
    /// mirror there.
    pub fn session_host(&self) -> Option<(&HostName, &Path)> {
        match (&self.host, &self.offload) {
            (Some(host), _) => Some((host, &self.path)),
            (None, Some(on)) => Some((&on.host, &on.mirror)),
            (None, None) => None,
        }
    }

    /// Whether the features that act on this machine's folder by themselves
    /// (workflows, tasks, schedules, worktrees, the check command, the remote
    /// bridge) reach the project's own folder: a folder here whose sessions
    /// run here too.
    pub fn runs_here(&self) -> bool {
        self.host.is_none() && self.offload.is_none()
    }

    /// Read the session on screen, if any: none while the project's page is.
    /// The index is kept in bounds by mutators.
    pub fn active_session(&self) -> Option<&Session> {
        self.active_session.and_then(|i| self.sessions.get(i))
    }

    /// The folder the window's panels show for this row: its session on
    /// screen's, else the row's own.
    pub fn folder_on_screen(&self) -> &Path {
        self.active_session()
            .map_or(self.path.as_path(), |session| session.folder.as_path())
    }
}

impl super::Workspace {
    /// The row a session in `folder` stands under: the one it is a folder
    /// of, else one a session of it runs in.
    fn row_of_folder(&self, folder: &Path) -> Option<&ProjectRoot> {
        (self.roots.iter().find(|root| root.holds(folder))).or_else(|| {
            (self.roots.iter()).find(|root| root.sessions.iter().any(|s| s.folder == folder))
        })
    }

    /// What the project whose row holds `folder` is called.
    pub fn label_of_folder(&self, folder: &Path) -> Option<&str> {
        self.row_of_folder(folder).map(|root| root.label.as_str())
    }

    /// Whether the sessions in `folder` run on this machine, the folder
    /// judged on its own: a project's worktree is offloaded apart from it.
    pub fn runs_here_in(&self, folder: &Path) -> bool {
        self.row_of_folder(folder)
            .is_some_and(|root| root.runs_here_in(folder))
    }

    /// Every folder whose sessions run on this machine, with how many are
    /// open in it: a host's sessions occupy no folder here, nor do an
    /// offloaded folder's, which run in its mirror there.
    pub fn sessions_by_folder(&self) -> Vec<(PathBuf, usize)> {
        (self.roots.iter())
            .flat_map(|root| {
                (root.folders_used().into_iter())
                    .filter(|folder| root.runs_here_in(folder))
                    .map(|folder| {
                        let open = root.sessions.iter().filter(|s| s.folder == folder);
                        let open = open.count();
                        (folder, open)
                    })
            })
            .collect()
    }

    /// Each project as onehand's tools name it, by those of its folders
    /// whose sessions run here (`here`) or not: the first of them, its own
    /// when it is one, then the worktrees'. A project with none is left out.
    pub fn named(&self, here: bool) -> Vec<crate::control::Named> {
        (self.roots.iter())
            .filter_map(|root| {
                let own =
                    (root.runs_here_in(&root.path) == here).then(|| (None, root.path.clone()));
                let worktrees = (root.worktrees.iter())
                    .filter(|w| root.runs_here_in(&w.path) == here)
                    .map(|w| (Some(w.branch.clone()), w.path.clone()));
                let mut folders = own.into_iter().chain(worktrees);
                let (_, path) = folders.next()?;
                Some(crate::control::Named {
                    name: root.label.clone(),
                    path,
                    worktrees: folders
                        .map(|(branch, folder)| (branch.unwrap_or_default(), folder))
                        .collect(),
                })
            })
            .collect()
    }
}
