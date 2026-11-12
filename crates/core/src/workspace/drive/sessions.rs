//! A session's place in the workspace: where it is added, what goes with it
//! when it closes, and what shows after.

use super::{Effect, Forgotten, HostName, Refusal, Signal, StartsOn, WorkspaceDrive};
use crate::config::AgentSpec;
use crate::host::wire::HostSession;
use std::path::Path;

/// Why a fork goes nowhere when the project it was of left the workspace
/// while its conversation was read.
const FORK_PROJECT_GONE: &str = "its project left the workspace";

impl WorkspaceDrive {
    /// [`Self::add_session`], the session starting on `starts` once shown.
    #[must_use = "its effects are to be carried"]
    pub fn add_session_on(
        &mut self,
        spec: Option<AgentSpec>,
        uid: u64,
        home: Option<usize>,
        starts: StartsOn,
    ) -> (Result<u64, Refusal>, Vec<Effect>) {
        let (added, fx) = self.add_session(spec, uid, home);
        if let Ok(uid) = added {
            match &starts {
                StartsOn::Watch(id) => {
                    self.watches.insert(uid, id.clone());
                }
                StartsOn::Fresh | StartsOn::Resume(_) | StartsOn::Fork => {}
            }
            self.starts.insert(uid, starts);
        }
        (added, fx)
    }

    /// What session `uid` starts on, taken as it is first shown: what it was
    /// added on, or a fresh conversation. Shown again, it starts on nothing
    /// a second time.
    #[must_use = "it is what the session starts on"]
    pub fn take_start(&mut self, uid: u64) -> StartsOn {
        self.starts.remove(&uid).unwrap_or(StartsOn::Fresh)
    }

    /// Session `uid` is on host session `id`, as its host said.
    pub fn watching(&mut self, uid: u64, id: HostSession) {
        if self.locate(uid).is_some() {
            self.watches.insert(uid, id);
        }
    }

    /// The session already on host session `id` of `host`; in the folder
    /// `root` when one is named.
    pub fn watcher(&self, host: &HostName, id: &HostSession, root: Option<&Path>) -> Option<u64> {
        self.workspace
            .roots
            .iter()
            .filter(|project| project.session_host().is_some_and(|(on, _)| on == host))
            .filter(|project| root.is_none_or(|root| project.path == root))
            .flat_map(|project| project.sessions.iter().map(|session| session.uid))
            .find(|uid| self.watches.get(uid) == Some(id))
    }

    /// Show host session `id`, run by `spec`, on the active project: the
    /// session already on it there, selected, rather than a second one,
    /// which would send to it too, whether or not its agent is still set
    /// up; else a new session `uid` watching it.
    #[must_use = "its effects are to be carried"]
    pub fn watch(
        &mut self,
        id: HostSession,
        spec: Option<AgentSpec>,
        uid: u64,
    ) -> (Result<u64, Refusal>, Vec<Effect>) {
        let open = self.workspace.active_root().and_then(|project| {
            let (host, _) = project.session_host()?;
            self.watcher(host, &id, Some(&project.path))
        });
        match open {
            Some(open) => (Ok(open), self.select(open)),
            None => self.add_session_on(spec, uid, None, StartsOn::Watch(id)),
        }
    }

    /// The host and host session session `uid` watches, once its host has
    /// said which.
    pub fn watched_on(&self, uid: u64) -> Option<(HostName, HostSession)> {
        let (root, _) = self.locate(uid)?;
        let (host, _) = self.workspace.roots.get(root)?.session_host()?;
        Some((host.clone(), self.watches.get(&uid)?.clone()))
    }

    /// Add the session `uid` a fork of a conversation on the project at
    /// `root` opens on, running `spec` as the original did, selected; or, the
    /// project gone from the workspace meanwhile, say the fork failed.
    #[must_use = "its effects are to be carried"]
    pub fn fork(
        &mut self,
        root: &Path,
        spec: AgentSpec,
        uid: u64,
    ) -> (Result<u64, Refusal>, Vec<Effect>) {
        match self.local_index(root) {
            Some(idx) => self.add_session_on(Some(spec), uid, Some(idx), StartsOn::Fork),
            None => (
                Err(Refusal::NoProject),
                vec![Effect::Signal(Signal::ForkFailed(
                    FORK_PROJECT_GONE.to_string(),
                ))],
            ),
        }
    }

    /// *End on <host>* on the host session `session` on `host`, called
    /// `label`: the session watching it here (`watcher`, a worker's `lead`)
    /// closes first, so the agent's going reads as nothing lost; then it is
    /// ended there.
    #[must_use = "its effects are to be carried"]
    pub fn end_on_host(
        &mut self,
        host: HostName,
        session: HostSession,
        label: String,
        watcher: Option<u64>,
        lead: Option<u64>,
    ) -> Vec<Effect> {
        let mut fx = watcher.map_or_else(Vec::new, |uid| self.end(uid, lead));
        fx.push(Effect::EndOnHost {
            host,
            session,
            label,
        });
        fx
    }

    /// A fork did not open, for `why`: its session, if one was added
    /// (`uid`), ends as a closed one does (`lead`, a worker's), and it is
    /// said.
    #[must_use = "its effects are to be carried"]
    pub fn fork_failed(&mut self, uid: Option<u64>, lead: Option<u64>, why: String) -> Vec<Effect> {
        let mut fx = uid.map_or_else(Vec::new, |uid| self.end(uid, lead));
        fx.push(Effect::Signal(Signal::ForkFailed(why)));
        fx
    }

    /// Add a session `uid` of `spec` on the active project, selected, for the
    /// front end to start and show. `home` is the project a new session goes
    /// to when the person is beside a worker, whose folder is kept for it
    /// alone. Refused with no agent or no project, and said.
    #[must_use = "its effects are to be carried"]
    pub fn add_session(
        &mut self,
        spec: Option<AgentSpec>,
        uid: u64,
        home: Option<usize>,
    ) -> (Result<u64, Refusal>, Vec<Effect>) {
        if let Some(home) = home {
            self.workspace.select_root(home);
        }
        let added = Self::agent(spec).and_then(|spec| {
            self.workspace
                .add_session(spec, uid)
                .map(|_| uid)
                .ok_or(Refusal::NoProject)
        });
        let fx = match added {
            Ok(_) => vec![Effect::Show, Effect::Changed],
            Err(why) => vec![Effect::Signal(Signal::Refused(why))],
        };
        (added, fx)
    }

    /// [`Self::add_session`] on project `root`, selected first: the panels
    /// follow the selection whether or not a session could be added, or the
    /// rail names one project while every panel shows another.
    #[must_use = "its effects are to be carried"]
    pub fn add_session_in(
        &mut self,
        root: usize,
        spec: Option<AgentSpec>,
        uid: u64,
    ) -> (Result<u64, Refusal>, Vec<Effect>) {
        self.workspace.select_root(root);
        let (added, mut fx) = self.add_session(spec, uid, None);
        if added.is_err() {
            fx.insert(0, Effect::Show);
        }
        (added, fx)
    }

    /// Add session `uid` of `spec` for a run in `dir`, selecting nothing: on
    /// the project there when the window has it, else on a project of its own
    /// that is never written and goes once empty. `None` when neither can be.
    pub fn add_run_session(&mut self, dir: &Path, spec: AgentSpec, uid: u64) -> Option<usize> {
        match self.workspace.add_session_quietly(dir, spec.clone(), uid) {
            Some(idx) => Some(idx),
            None => self.workspace.add_transient_root(dir, spec, uid),
        }
    }

    /// Close session `uid`, no question asked, and show what is left. A
    /// project only a worker or a run was on, left empty, goes with it. When
    /// the session was on screen and `lead` (its lead, for a worker) is in
    /// this window, the lead shows instead. Nothing when `uid` is not here.
    #[must_use = "its effects are to be carried"]
    pub fn end(&mut self, uid: u64, lead: Option<u64>) -> Vec<Effect> {
        let shown = self.active_uid() == Some(uid);
        let Some((root, _)) = self.locate(uid) else {
            return Vec::new();
        };
        let mut fx = self.drop_session(uid);
        if self.left_behind(root) {
            fx.extend(self.forget(root));
        }
        if let Some((root, session)) = lead.filter(|_| shown).and_then(|lead| self.locate(lead)) {
            self.workspace.select_root(root);
            self.workspace.select_session(session);
        }
        fx.push(Effect::Show);
        fx
    }

    /// Take session `uid` out of the window, showing nothing in its place;
    /// nothing when it is not here.
    #[must_use = "its effects are to be carried"]
    pub fn drop_session(&mut self, uid: u64) -> Vec<Effect> {
        let Some((root, session)) = self.locate(uid) else {
            return Vec::new();
        };
        self.workspace.close_session(root, session);
        self.let_go(uid);
        vec![Effect::Close(uid), Effect::Changed]
    }

    /// What is kept about session `uid`, closed, goes with it.
    fn let_go(&mut self, uid: u64) {
        self.starts.remove(&uid);
        self.watches.remove(&uid);
    }

    /// Close every session of the project at `dir`, a folder here, showing
    /// what is left: what its code moving to or from a host does.
    pub(super) fn drop_sessions_in(&mut self, dir: &Path) -> Vec<Effect> {
        let uids: Vec<u64> = self
            .workspace
            .roots
            .iter()
            .filter(|root| root.local_path() == Some(dir))
            .flat_map(|root| root.sessions.iter().map(|session| session.uid))
            .collect();
        uids.into_iter()
            .flat_map(|uid| self.drop_session(uid))
            .collect()
    }

    /// Close session `uid` that a tool opened and that never finished
    /// opening, and the project at `dir` when the open `added` it and nothing
    /// else stands on it; quietly, showing what is left only when the person
    /// was `watching` it or its project goes from under them.
    #[must_use = "its effects are to be carried"]
    pub fn drop_opened(
        &mut self,
        uid: u64,
        dir: &Path,
        added: bool,
        watching: bool,
    ) -> Vec<Effect> {
        let mut fx = self.drop_session(uid);
        let empty = self.local_index(dir).filter(|&idx| {
            let root = &self.workspace.roots[idx];
            added && root.transient && root.sessions.is_empty()
        });
        let mut shown = watching;
        if let Some(idx) = empty {
            shown |= self.workspace.active_root == idx;
            fx.extend(self.forget(idx));
        }
        if shown {
            fx.push(Effect::Show);
        }
        fx
    }

    /// Close the session of a run that ended with nothing in it to look at;
    /// left alone while the person is `watching` it, since taking it away
    /// would move them.
    #[must_use = "its effects are to be carried"]
    pub fn close_finished(&mut self, uid: u64, watching: bool) -> Vec<Effect> {
        match watching {
            true => Vec::new(),
            false => self.drop_session(uid),
        }
    }

    /// End a run's project at `dir`, when the run added it itself: its
    /// session closed and its row gone, quietly, showing what is left only
    /// when it was the project selected. A project a person has open stays.
    #[must_use = "its effects are to be carried"]
    pub fn end_run(&mut self, dir: &Path) -> Vec<Effect> {
        let Some(idx) = self
            .local_index(dir)
            .filter(|&idx| self.workspace.roots[idx].transient)
        else {
            return Vec::new();
        };
        let was_active = self.workspace.active_root == idx;
        let mut fx = self.forget(idx);
        if was_active {
            fx.push(Effect::Show);
        }
        fx
    }

    /// Keep a run's project at `dir` for good, written into the workspace: a
    /// person has taken the run over, so it is their project now.
    #[must_use = "its effects are to be carried"]
    pub fn take_over_run(&mut self, dir: &Path) -> Vec<Effect> {
        let Some(idx) = self.local_index(dir) else {
            return Vec::new();
        };
        self.workspace.roots[idx].transient = false;
        self.save()
    }

    /// Select project `root`, and show the session it was last on.
    #[must_use = "its effects are to be carried"]
    pub fn select_project(&mut self, root: usize) -> Vec<Effect> {
        self.workspace.select_root(root);
        vec![Effect::Show, Effect::Changed]
    }

    /// Select project `root` and its session `session` in one touch.
    #[must_use = "its effects are to be carried"]
    pub fn select_project_session(&mut self, root: usize, session: usize) -> Vec<Effect> {
        self.workspace.select_root(root);
        self.workspace.select_session(session);
        vec![Effect::Show, Effect::Changed]
    }

    /// Select session `uid`, wherever it is in this window.
    #[must_use = "its effects are to be carried"]
    pub fn select(&mut self, uid: u64) -> Vec<Effect> {
        match self.locate(uid) {
            Some((root, session)) => self.select_project_session(root, session),
            None => Vec::new(),
        }
    }

    /// Select the active project's session at `index`; nothing when there is
    /// none there.
    #[must_use = "its effects are to be carried"]
    pub fn select_session(&mut self, index: usize) -> Vec<Effect> {
        let count = self
            .workspace
            .active_root()
            .map_or(0, |root| root.sessions.len());
        if index >= count {
            return Vec::new();
        }
        self.workspace.select_session(index);
        vec![Effect::Show, Effect::Changed]
    }

    /// Move a session within its project. Nothing is written: sessions are
    /// not kept in the file; they start again.
    #[must_use = "its effects are to be carried"]
    pub fn move_session(&mut self, root: usize, from: usize, to: usize) -> Vec<Effect> {
        self.workspace.move_session(root, from, to);
        vec![Effect::Changed]
    }

    /// Whether the project at `root` is left with nothing a person keeps: one
    /// added for a worker or a run alone, never saved, whose last session
    /// closed. Its row would stand for nothing, so it goes with that session.
    pub(crate) fn left_behind(&self, root: usize) -> bool {
        self.workspace
            .roots
            .get(root)
            .is_some_and(|root| root.transient && root.sessions.is_empty())
    }

    /// Drop project `idx` and every session on it, with no question asked
    /// and nothing on screen moved, and write nothing: what a run's or a
    /// worker's project leaving does.
    #[must_use = "its effects are to be carried"]
    pub fn forget(&mut self, idx: usize) -> Vec<Effect> {
        let Some(root) = self.workspace.roots.get(idx) else {
            return Vec::new();
        };
        let uids: Vec<u64> = root.sessions.iter().map(|session| session.uid).collect();
        let mut fx: Vec<Effect> = uids.iter().map(|&uid| Effect::Close(uid)).collect();
        // Its code is no longer checked for here; offloaded again, it comes
        // back as any does.
        if root.offload.is_some() {
            fx.push(Effect::ForgotOffload(root.path.clone()));
        }
        // What is kept under a folder here goes with it: what was read of it,
        // and what the window files under its path. A host's folder never had
        // any of its own, and what is filed under its path belongs to a
        // folder of the same path here.
        if root.host.is_none() {
            self.git.remove(&root.path);
            fx.push(Effect::ForgotHere(root.path.clone()));
        }
        fx.push(Effect::Forgot(Forgotten {
            path: root.path.clone(),
            host: root.host.clone(),
        }));
        self.workspace.remove_root(idx);
        uids.into_iter().for_each(|uid| self.let_go(uid));
        fx.push(Effect::Changed);
        fx
    }

    /// The project session `uid` stands under: its lead's, a worker's, when
    /// `lead` is a session of this window; else its own.
    pub fn home_of(&self, uid: u64, lead: Option<u64>) -> Option<usize> {
        lead.and_then(|lead| self.locate(lead))
            .or_else(|| self.locate(uid))
            .map(|(root, _)| root)
    }

    /// Where the project at `dir`, a folder on this machine, sits.
    pub fn local_index(&self, dir: &Path) -> Option<usize> {
        self.workspace
            .roots
            .iter()
            .position(|root| root.local_path() == Some(dir))
    }
}
