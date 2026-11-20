use super::Shell;
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, SharedString, Window};
use onehand_core::config::AgentSpec;
use onehand_core::workbench::KeptIssues;
use onehand_core::workspace::drive::StartsOn;
use std::path::{Path, PathBuf};

impl Shell {
    /// Session `uid`'s live session, when it is one of this window's.
    pub fn session_entity(
        &self,
        uid: u64,
        cx: &App,
    ) -> Option<Entity<crate::chat::session::ChatSession>> {
        self.chat.read(cx).session_entity(uid)
    }

    /// Every session's uid in this window.
    pub fn session_uids(&self) -> Vec<u64> {
        self.window
            .workspace
            .roots
            .iter()
            .flat_map(|root| root.sessions.iter().map(|session| session.uid))
            .collect()
    }

    /// Every root on this machine, with how many sessions are open on it.
    pub fn roots_with_sessions(&self) -> Vec<(PathBuf, usize)> {
        self.window
            .workspace
            .roots
            .iter()
            // A host's sessions occupy no folder here, nor do an offloaded
            // project's, which run in its mirror there.
            .filter(|root| root.runs_here())
            .map(|root| (root.path.clone(), root.sessions.len()))
            .collect()
    }

    /// The process id of every shell in this window's terminal.
    pub fn shell_pids(&self, cx: &App) -> Vec<u32> {
        self.terminal
            .read(cx)
            .processes()
            .into_iter()
            .map(|(_, pid)| pid)
            .collect()
    }

    /// Every project root on this machine this window holds, as `(path,
    /// label)`: for the bridge, which has to know what there is before it can
    /// go looking on disk for what was said in it, and for onehand's tools. A
    /// folder on a host has nothing here either can reach.
    pub fn remote_roots(&self) -> Vec<(PathBuf, String)> {
        self.roots_where(|root| root.runs_here())
    }

    /// The projects on a host, or offloaded to one, which onehand's tools
    /// refuse by name.
    pub fn host_roots(&self) -> Vec<(PathBuf, String)> {
        self.roots_where(|root| !root.runs_here())
    }

    fn roots_where(
        &self,
        keep: impl Fn(&onehand_core::workspace::ProjectRoot) -> bool,
    ) -> Vec<(PathBuf, String)> {
        self.window
            .workspace
            .roots
            .iter()
            .filter(|root| keep(root))
            .map(|root| (root.path.clone(), root.label.clone()))
            .collect()
    }

    /// Reopen a saved conversation on `root`, from outside the app.
    ///
    /// `None` for a root this window does not hold, which is the same handshake
    /// the prompt and press paths use to find the window that does.
    ///
    /// **The project is named rather than assumed.** Minting a session goes to
    /// the workspace's active root, which is whichever project somebody last
    /// clicked — so without selecting it first, a conversation reopened from a
    /// train would land on an unrelated checkout and run its first prompt there.
    /// Returns the new session's number, so the reply can name what a later
    /// `/use` would.
    pub fn remote_open(
        &mut self,
        root: &Path,
        archive: PathBuf,
        agent: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let idx = self.root_index(root)?;
        self.select_root(idx, window, cx);
        self.start_session(agent, Some(archive), window, cx)
    }

    /// Where the issues of `root`'s project are kept: a row's, else, for a
    /// folder no row holds (a worker's, a run's), the project recording it.
    /// None for a folder of no project, or one on a host.
    pub fn issues_file(&self, root: &Path, cx: &App) -> Option<KeptIssues> {
        self.issue_files(cx).remove(root).or_else(|| {
            let project = crate::projects::of(root, cx).filter(|p| p.host.is_none())?;
            Some(KeptIssues {
                file: onehand_core::project::file::issues_file(&project.id),
                home: project.path,
            })
        })
    }

    /// Each project folder of the window on this machine, with where its
    /// project keeps its issues: one file for every folder of a project.
    pub(crate) fn issue_files(&self, cx: &App) -> std::collections::HashMap<PathBuf, KeptIssues> {
        let projects = &crate::state::Shared::global(cx).projects;
        self.window
            .workspace
            .roots
            .iter()
            .filter_map(|root| {
                let project = projects.get(root.project.as_ref()?)?;
                let path = root.local_path()?;
                let kept = KeptIssues {
                    file: onehand_core::project::file::issues_file(&project.id),
                    home: project.path.clone(),
                };
                Some((path.to_path_buf(), kept))
            })
            .collect()
    }

    /// Tell the Workbench and the Issues page each folder's issues file, when
    /// it is not what they were last told.
    pub(crate) fn tell_issue_files(&mut self, cx: &mut Context<Self>) {
        let files = self.issue_files(cx);
        if files != self.told_issue_files {
            self.tell_issue_files_whole(files, cx);
        }
    }

    /// Tell them `files`, whatever they were last told.
    pub(crate) fn tell_issue_files_whole(
        &mut self,
        files: std::collections::HashMap<PathBuf, KeptIssues>,
        cx: &mut Context<Self>,
    ) {
        self.workbench
            .update(cx, |panel, cx| panel.set_issue_files(&files, cx));
        self.tell_issues_page(&onehand_workbench::Request::IssueFiles(&files), cx);
        self.told_issue_files = files;
    }

    /// Start `spec` on `dir`, off screen, for a lead's worker or a task
    /// started where nothing may move: on the project already open there,
    /// or on `dir` added as a transient project. Its agent may also write
    /// `dirs`.
    ///
    /// **Nothing the user is looking at moves.** No root is selected and the
    /// pane connects the session without showing it. A root added here is
    /// transient, so the workspace file never holds it; one the user already
    /// had keeps what it was, since marking it transient would quietly drop it
    /// from their workspace.
    pub fn open_off_screen(
        &mut self,
        dir: PathBuf,
        spec: AgentSpec,
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<(u64, Entity<crate::chat::session::ChatSession>)> {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        self.window
            .workspace
            .add_run_session(&dir, spec.clone(), uid)?;
        let folder = self.window.workspace.folder_of(uid)?;
        let session = self.chat.update(cx, |pane, cx| {
            pane.open_unshown(uid, folder, &spec, dirs, cx)
        });
        self.refresh_git(cx);
        cx.notify();
        Some((uid, session?))
    }

    /// Add a session of `spec` on `dir`, off screen, that resumes the
    /// conversation `conversation`, kept in `archive`, once it is shown or
    /// called on, its
    /// agent then also writing `dirs`; its project as
    /// [`Self::open_off_screen`] finds or adds it. Nothing connects now.
    pub fn add_unshown_resuming(
        &mut self,
        dir: PathBuf,
        spec: AgentSpec,
        dirs: Vec<PathBuf>,
        archive: PathBuf,
        conversation: String,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        self.window
            .workspace
            .add_run_session_on(&dir, spec, uid, StartsOn::Resume(archive))?;
        self.chat
            .update(cx, |pane, _| pane.hold(uid, dirs, conversation));
        self.refresh_git(cx);
        cx.notify();
        Some(uid)
    }

    /// Connect session `uid` without showing it, on what it was added to
    /// start on; nothing when it is not here or is connected already.
    pub fn connect_unshown(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some((root, spec)) = self.window.workspace.roots.iter().find_map(|root| {
            let session = root.sessions.iter().find(|session| session.uid == uid)?;
            Some((session.folder.clone(), session.spec.clone()))
        }) else {
            return;
        };
        let starts = self.window.workspace.take_start(uid);
        self.chat.update(cx, |pane, cx| {
            pane.connect_unshown(uid, root, &spec, starts, cx)
        });
    }

    /// Close session `uid`, opened by a tool and never finished opening, and
    /// drop its project when the open added it and nothing else stands on it.
    ///
    /// Quietly, as a run's project is dropped: nothing is shown and the caret
    /// stays where it is, unless the person was watching that session or its
    /// project, which then gives way to whatever is active. A project the
    /// window had, or one another session is on, stays.
    pub fn drop_opened(
        &mut self,
        uid: u64,
        dir: &Path,
        added: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let watching = self.chat.read(cx).viewing() == Some(uid);
        let fx = self.window.workspace.drop_opened(uid, dir, added, watching);
        self.carry_workspace(fx, window, cx);
    }

    /// Close the session of a run that ended with nothing left in it to look
    /// at, as a finished schedule's run does.
    ///
    /// Quietly, as a run's project is dropped: nothing else is shown and the
    /// caret stays where it is. The one session it leaves is the one whose
    /// conversation is on screen, since taking that away would move the
    /// person, and they can close it themselves. Its project's active session
    /// behind a page is not on screen: it goes, the project's next session
    /// becomes its active one, and leaving the page shows that one, or the
    /// project page when none is left.
    pub fn close_finished(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let watching = self.chat.read(cx).viewing() == Some(uid);
        let fx = self.window.workspace.close_finished(uid, watching);
        self.carry_workspace(fx, window, cx);
    }

    /// Every session this window is running, for the bridge that has to describe
    /// them to somebody who is not looking at the window.
    ///
    /// A pass-through rather than a second walk of the workspace tree: the pane
    /// is where a session's running state actually lives, and the tree's
    /// `Session` is its description rather than its condition.
    pub fn remote_sessions(&self, cx: &App) -> Vec<crate::remote::RemoteSession> {
        let roots = &self.window.workspace.roots;
        // A session's folder is its project's own or a worktree of it.
        let runs_here = |path: &Path| {
            roots
                .iter()
                .find(|root| root.path == path || root.sessions.iter().any(|s| s.folder == path))
                .is_some_and(|root| root.runs_here())
        };
        self.chat.read(cx).remote_sessions(runs_here, cx)
    }
}
