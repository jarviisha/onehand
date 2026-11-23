use super::confirm::Confirm;
use super::{FocusedPanel, RailSession, Shell, TabCycle};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, SharedString, Window};
use gpui_component::WindowExt as _;
use onehand_core::chat::drive::Restarting;
use onehand_core::control::drive::CloseAsks;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::drive::StartsOn;
use std::path::PathBuf;

/// A conversation already open in some session, refused a second.
const ALREADY_OPEN: &str = "This conversation is already open in a session";

impl Shell {
    /// What the rail draws on a session row.
    ///
    /// Delegated rather than mirrored: the chat pane holds the conversations,
    /// and every attempt so far to keep a copy of this one level up went stale
    /// immediately.
    ///
    /// One call for both halves so there is a single answer to "what does the
    /// rail read?" — which is what [`Self::rail_sessions`] compares to decide
    /// whether a rebuild is worth it.
    pub fn session_row(&self, uid: u64, cx: &App) -> RailSession {
        let pane = self.chat.read(cx);
        let signal = pane.signal(uid, cx);
        // The clock runs on from the last repaint while the signal holds, and
        // starts again when it changes.
        let since = self
            .rail_sessions
            .iter()
            .find(|(at, row)| *at == uid && row.signal == signal)
            .map_or_else(std::time::Instant::now, |(_, row)| row.since);
        let (background, subagents, in_turn) = pane.activity(uid, cx);
        RailSession {
            signal,
            title: pane.title_for(uid, cx).map(SharedString::from),
            since,
            background,
            subagents,
            in_turn,
        }
    }

    /// Every session row, in tree order — the whole of what the rail asks the
    /// chat pane for, and therefore a sound repaint key.
    pub(super) fn rail_sessions(&self, cx: &App) -> Vec<(u64, RailSession)> {
        self.window
            .workspace
            .roots
            .iter()
            .flat_map(|root| root.sessions.iter())
            .map(|session| (session.uid, self.session_row(session.uid, cx)))
            .collect()
    }

    /// Restart a *named* session's adapter. Selecting it first is what makes
    /// the guard, the notification and the transcript all be about the session
    /// the user pointed at.
    pub fn restart_session_at(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root_session(root_idx, session_idx, window, cx);
        self.restart_session(window, cx);
    }

    /// Write a named session's conversation to a Markdown file.
    pub fn export_session_at(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root_session(root_idx, session_idx, window, cx);
        self.chat.update(cx, |pane, cx| pane.export(window, cx));
    }

    /// Open the rename field on the conversation showing.
    ///
    /// The same dialog the rail's *Rename…* opens, reached from the header of
    /// the conversation it renames — the two must not become two rules about
    /// what a name may be.
    pub fn rename_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uid) = self.active_session_uid() else {
            return;
        };
        self.begin_rename(uid, window, cx);
    }

    /// The session on screen, if there is one.
    pub(crate) fn active_session_uid(&self) -> Option<u64> {
        self.window.workspace.active_uid()
    }

    /// Delete the conversation showing: the directory on disk, and the session
    /// that was writing to it.
    ///
    /// **The session goes first, and it goes without being asked about again.**
    /// While it is alive its mark says the transcript up to here is already on
    /// disk, so the next turn would write the file back holding only what came
    /// after — the delete would not stay deleted, and what came back would be a
    /// fragment. Dropping the session is also what ends its agent. The mid-turn
    /// question `close_session` normally asks is skipped deliberately: the
    /// stronger question has already been asked and answered, and a second one
    /// about a decision already confirmed teaches the user to click through
    /// both.
    ///
    /// **Asked about in a modal first**, because it is the one thing the app
    /// offers that doing again does not undo — the same question the project
    /// page asks, so the conversation open in front of the user is not the one
    /// deleted on a lighter guard than the ones filed away behind it.
    pub fn confirm_delete_conversation(
        &mut self,
        dir: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The name of what is being deleted, read now while the session that
        // holds it is still on screen. An unnamed conversation is one nothing
        // has been asked in, so the question names it by what it is instead.
        let name = self
            .active_session_uid()
            .and_then(|uid| self.chat.read(cx).title_for(uid, cx));
        let ask = Confirm {
            id: "delete-open-conversation",
            title: match &name {
                Some(name) => format!("Delete \u{201c}{name}\u{201d}?").into(),
                None => "Delete this conversation?".into(),
            },
            description: match &name {
                Some(name) => format!(
                    "“{name}” will be removed from disk, with every message and image in \
                     it, and the agent running it will stop. This cannot be undone."
                ),
                None => "This conversation will be removed from disk, and the agent \
                         running it will stop. This cannot be undone."
                    .to_string(),
            }
            .into(),
            act: "Delete",
            ..Default::default()
        };
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.delete_conversation(dir.clone(), window, cx)
        });
    }

    /// Delete the conversation showing, the question already answered.
    fn delete_conversation(
        &mut self,
        dir: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(uid) = self.active_session_uid() {
            // Past the close's own mid-turn question: the one it would ask has
            // already been asked, in stronger terms.
            self.end_session(uid, window, cx);
        }

        cx.spawn_in(window, async move |shell, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { onehand_core::chat::delete(&dir) })
                .await;
            if let Err(e) = removed {
                shell
                    .update_in(cx, |_, window, cx| {
                        // A warning, not an error: nothing was lost. The
                        // conversation is exactly where it was, which is the
                        // opposite of a failed save.
                        window.push_notification(
                            crate::toast::failed("Deleting the conversation", e),
                            cx,
                        );
                    })
                    .ok();
            }
        })
        .detach();
    }

    /// Close the session on screen — the keyboard half of the rail's ✕.
    pub fn close_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root_idx = self.window.workspace.active_root;
        let Some(session_idx) = self
            .window
            .workspace
            .active_root()
            .and_then(|root| root.active_session)
        else {
            return;
        };
        self.close_session(root_idx, session_idx, window, cx);
    }

    /// Close one session and, with it, its agent — the project root stays.
    ///
    /// Until this existed the only way to end a session was to remove the whole
    /// project it belonged to, so a root accumulated agents nothing could stop.
    ///
    /// **Asked about in a modal while it leads workers, or while a turn is
    /// in flight**, as the tools' drive answers. A lead is asked about its
    /// workers ([`Self::ask_close_lead`]). The transcript is written at the end of every turn, so closing an idle
    /// session costs nothing that is not already on disk and does not deserve a
    /// question; closing one mid-turn throws away the turn that is running,
    /// which is the same loss a mid-turn restart asks about.
    pub fn close_session(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .window
            .workspace
            .roots
            .get(root_idx)
            .and_then(|root| root.sessions.get(session_idx))
        else {
            return;
        };
        let uid = session.uid;
        let chat = self.chat.read(cx).closing(uid, cx);
        match crate::control::close_asks(uid, chat) {
            CloseAsks::Now => return self.end_session(uid, window, cx),
            CloseAsks::Workers(workers) => return self.ask_close_lead(uid, &workers, window, cx),
            CloseAsks::Turn => {}
        }
        let label = self.session_label(uid, cx);
        let ask = Confirm {
            id: "close-session",
            title: format!("Close {label}?").into(),
            description: "A turn is running. Closing the session stops its agent, and that \
                          turn is lost."
                .into(),
            act: "Close",
            ..Default::default()
        };
        // By uid, not by place: the list can shift while the question is open.
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.end_session(uid, window, cx)
        });
    }

    /// Close session `uid` and its agent, no question asked.
    ///
    /// No toast: the row leaving the rail and the pane moving to whatever is
    /// left say it, where removing a project has to announce itself because
    /// most of what it destroys was never on screen.
    pub(crate) fn end_session(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        // A worker on screen closing gives the pane back to its lead rather
        // than to its worktree, emptied.
        let fx = self.window.workspace.end(uid, crate::control::lead_of(uid));
        self.carry_workspace(fx, window, cx);
    }

    /// Move `uid` to the front of its root's recency list.
    ///
    /// Skipped while a cycle is running: `Ctrl+Tab` *passing over* a session is
    /// not the user choosing it, and reordering as it goes would make the list
    /// shuffle under the key that is walking it.
    pub(super) fn touch_mru(&mut self, root: PathBuf, uid: u64) {
        if self.tab_cycle.is_some() {
            return;
        }
        let order = self.mru.entry(root).or_default();
        order.retain(|&seen| seen != uid);
        order.insert(0, uid);
    }

    /// The active root's sessions in recency order.
    ///
    /// Sessions the list has never seen (just added, or added in another
    /// window's copy of the tree) go on the end in tree order rather than
    /// being dropped -- an unlisted session must still be reachable.
    fn mru_order(&self) -> Vec<u64> {
        let Some(root) = self.window.workspace.active_root() else {
            return Vec::new();
        };
        let live: Vec<u64> = root.sessions.iter().map(|s| s.uid).collect();
        let mut order: Vec<u64> = self
            .mru
            .get(&root.path)
            .map(|seen| {
                seen.iter()
                    .copied()
                    .filter(|uid| live.contains(uid))
                    .collect()
            })
            .unwrap_or_default();
        let unseen: Vec<u64> = live
            .into_iter()
            .filter(|uid| !order.contains(uid))
            .collect();
        order.extend(unseen);
        order
    }

    /// Switch to the *n*-th session of the active root, by position.
    pub fn select_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self
            .window
            .workspace
            .active_root()
            .map(|root| root.sessions.len())
            .unwrap_or(0);
        if index >= count {
            return;
        }
        let fx = self.window.workspace.select_session(index);
        self.carry_workspace(fx, window, cx);
    }

    /// Walk the active root's sessions in recency order, VSCode-style.
    ///
    /// The order is snapshotted when the cycle starts and held until the shortcut's modifiers are
    /// released ([`Self::end_cycle`]). Recomputing it per press would make the
    /// second press walk back to where the first one came from, and the cycle
    /// would ping-pong between two sessions instead of reaching the third.
    pub(super) fn cycle_session(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_cycle.is_none() {
            let order = self.mru_order();
            // From the project's page no session is on screen, so the first
            // press lands on the latest (or, backward, the oldest) rather than
            // stepping past it, and a project of one session is reached too.
            let page = self.active_session_uid().is_none();
            if order.len() < if page { 1 } else { 2 } {
                return;
            }
            let pos = match (page, forward) {
                (true, true) => order.len() - 1,
                (true, false) | (false, _) => 0,
            };
            self.tab_cycle = Some(TabCycle {
                order,
                pos,
                modifiers: window.modifiers(),
            });
        }
        let Some(cycle) = self.tab_cycle.as_mut() else {
            return;
        };
        let len = cycle.order.len();
        cycle.pos = if forward {
            (cycle.pos + 1) % len
        } else {
            (cycle.pos + len - 1) % len
        };
        let uid = cycle.order[cycle.pos];

        let index = self
            .window
            .workspace
            .active_root()
            .and_then(|root| root.sessions.iter().position(|s| s.uid == uid));
        if let Some(index) = index {
            let fx = self.window.workspace.select_session(index);
            self.carry_workspace(fx, window, cx);
        }
        if self
            .tab_cycle
            .as_ref()
            .is_some_and(|cycle| !cycle.held(window.modifiers()))
        {
            self.end_cycle(cx);
        }
    }

    /// Commit a cycle: where it stopped becomes the most recent session.
    ///
    /// Fired when the shortcut modifiers are released. Until then nothing about the recency list has
    /// changed, so a cycle the user abandons by pressing on leaves no trace.
    pub(super) fn end_cycle(&mut self, cx: &mut Context<Self>) {
        if self.tab_cycle.take().is_none() {
            return;
        }
        let landed = self
            .window
            .workspace
            .active_root()
            .and_then(|root| root.active_session().map(|s| (root.path.clone(), s.uid)));
        if let Some((root, uid)) = landed {
            self.touch_mru(root, uid);
        }
        cx.notify();
    }

    /// Stop session `uid`'s turn.
    pub(crate) fn stop_turn(&mut self, uid: u64, cx: &mut Context<Self>) {
        self.chat.update(cx, |pane, cx| pane.stop_turn(uid, cx));
    }

    /// Whether session `uid` has a failed turn whose prompt can go again.
    pub(crate) fn can_resend(&self, uid: u64, cx: &App) -> bool {
        self.chat.read(cx).can_resend(uid, cx)
    }

    /// Send session `uid`'s last prompt again, after a turn that failed.
    pub(crate) fn resend_last_prompt(&mut self, uid: u64, cx: &mut Context<Self>) {
        self.chat
            .update(cx, |pane, cx| pane.resend_last_prompt(uid, cx));
    }

    /// Add a session on the active root using the default agent, and show it.
    ///
    /// The *default* is the first configured agent, which is the Claude Code
    /// built-in unless the user has reordered the list. One agent is the common
    /// case and it must stay one click; choosing a different one is the rail's
    /// agent list (see [`Self::new_session_with`]).
    pub fn new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_session_with(0, window, cx);
    }

    /// Add a session on a *named* root using the default agent.
    ///
    /// The root is selected first, so this cannot start an agent somewhere the
    /// user is not looking: a session is bound to one project root for its
    /// whole life, and starting one on a root the rail is not showing would be
    /// a prompt sent into a project nobody has open.
    pub fn new_session_in(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.new_session_at(root_idx, 0, window, cx);
    }

    /// Add a session on a *named* root running `agents[agent]`.
    pub fn new_session_at(
        &mut self,
        root_idx: usize,
        agent: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = Shared::global(cx).settings.agents.get(agent).cloned();
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let (_, fx) = self.window.workspace.add_session_in(root_idx, spec, uid);
        self.carry_workspace(fx, window, cx);
    }

    /// *New session* on a *named* root: in the folder of the session on
    /// screen when the root is the one on screen, as the key starts it; in
    /// the root's own folder otherwise.
    pub fn new_session_beside(
        &mut self,
        root_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = &self.window.workspace;
        let folder = (workspace.active_root == root_idx)
            .then(|| workspace.active_root())
            .flatten()
            .map(|root| root.folder_on_screen().to_path_buf());
        match folder {
            Some(folder) => {
                let _ = self.new_session_in_folder(root_idx, folder, None, window, cx);
            }
            None => self.new_session_in(root_idx, window, cx),
        }
    }

    /// Add a session of the agent named, else the first, on a *named* root,
    /// running in `folder`, one of its: *In <branch>*, a split's new
    /// worktree, a run's.
    pub fn new_session_in_folder(
        &mut self,
        root_idx: usize,
        folder: PathBuf,
        agent: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        self.session_in_folder(root_idx, folder, agent, StartsOn::Fresh, window, cx)
    }

    /// [`Self::new_session_in_folder`], starting on `starts` once shown.
    pub(super) fn session_in_folder(
        &mut self,
        root_idx: usize,
        folder: PathBuf,
        agent: Option<SharedString>,
        starts: StartsOn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        // The agent named, else the first: what *New session* starts.
        let spec = (Shared::global(cx).settings.agents)
            .get(agent_index(agent, cx))
            .cloned();
        self.add_by(
            |workspace, uid| workspace.add_session_in_folder(root_idx, folder, spec, uid, starts),
            window,
            cx,
            |_, _| {},
        )
    }

    /// Drop a session at another place under its project.
    ///
    /// Nothing to save: sessions are not persisted, they respawn.
    pub fn move_session(
        &mut self,
        root_idx: usize,
        from: usize,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        let fx = self.window.workspace.move_session(root_idx, from, to);
        // Nothing here needs the window: a move only redraws.
        for effect in fx {
            debug_assert!(matches!(
                effect,
                onehand_core::workspace::drive::Effect::Changed
            ));
            cx.notify();
        }
    }

    /// Add a session running `agents[idx]`.
    pub fn new_session_with(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Only here, where the project is the one on screen: a caller that
        // named a project keeps it.
        let home = self.worker_home();
        self.spawn_session(idx, None, home, window, cx);
    }

    /// Start a session on the active root, as the project page asks for it: on
    /// a named agent, and optionally opening straight onto an archived
    /// conversation.
    ///
    /// **An unknown agent falls back to the default rather than refusing.** The
    /// name comes off an archive, and the agent that wrote it can since have
    /// been renamed or removed in Settings — a conversation the user
    /// can see listed must still be openable, and which agent replays it is the
    /// smaller loss.
    /// Returns the new session's uid, for a caller that has to say which one it
    /// just made.
    pub fn start_session(
        &mut self,
        agent: Option<SharedString>,
        resume: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        // A conversation open in a session here, pressed on its project's
        // page, is that session to go back to rather than a second to refuse.
        let held = resume
            .as_deref()
            .and_then(|dir| self.chat.read(cx).holding(dir, cx));
        if let Some(uid) = held {
            self.show_session(uid, window, cx);
            return None;
        }
        if resume
            .as_deref()
            .is_some_and(|dir| self.refuse_open_twice(dir, window, cx))
        {
            return None;
        }
        let idx = agent_index(agent, cx);
        self.spawn_session(idx, resume, None, window, cx)
    }

    /// One conversation in one session: two would each write it back as only
    /// what came after. Whether the conversation kept in `dir` is open in a
    /// session already, said when it is.
    pub(super) fn refuse_open_twice(
        &self,
        dir: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        {
            let me = cx.entity_id();
            let open: Vec<String> = self
                .chat
                .read(cx)
                .open_ids(cx)
                .into_iter()
                .chain(
                    crate::windows::live(cx)
                        .into_iter()
                        .filter(|(_, shell)| shell.entity_id() != me)
                        .flat_map(|(_, shell)| shell.read(cx).chat.read(cx).open_ids(cx)),
                )
                .collect();
            if crate::chat::is_open(&open, dir) {
                window.push_notification(crate::toast::warning(ALREADY_OPEN), cx);
                return true;
            }
        }
        false
    }

    /// Start a session on the active root, a host's, attached to the host
    /// session `id` that `agent` runs there; the default agent stands in for
    /// one no longer configured, as for an archive.
    pub(super) fn watch_host_session(
        &mut self,
        agent: SharedString,
        id: HostSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idx = agent_index(Some(agent), cx);
        let spec = Shared::global(cx).settings.agents.get(idx).cloned();
        // One already on it is shown again rather than a second opened.
        let _ = self.add_by(
            |workspace, uid| workspace.watch(id, spec, uid),
            window,
            cx,
            |_, _| {},
        );
    }

    /// Beside a worker on screen, its lead's project, where a new session
    /// belongs: the worker's worktree is kept for it alone.
    fn worker_home(&self) -> Option<usize> {
        self.active_session_uid().and_then(|uid| self.home_of(uid))
    }

    /// Add a session as `add` asks the workspace's drive for one, minted a
    /// uid, and show it; the pane is told what it learned as `before` says,
    /// `None` when the drive refused it, before the session is shown, since
    /// showing is what spawns its adapter.
    pub(super) fn add_by(
        &mut self,
        add: impl FnOnce(
            &mut onehand_core::workspace::WorkspaceDrive,
            u64,
        ) -> (
            Result<u64, onehand_core::workspace::drive::Refusal>,
            Vec<onehand_core::workspace::drive::Effect>,
        ),
        window: &mut Window,
        cx: &mut Context<Self>,
        before: impl FnOnce(&mut crate::chat::ChatPane, Option<u64>),
    ) -> Option<u64> {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let (added, fx) = add(&mut self.window.workspace, uid);
        let given = added.as_ref().ok().copied();
        self.chat.update(cx, |pane, _| before(pane, given));
        self.carry_workspace(fx, window, cx);
        given
    }

    /// A session of `agents[idx]` on the active project, shown, resuming
    /// `archive` if one was named.
    pub(super) fn spawn_session(
        &mut self,
        idx: usize,
        archive: Option<PathBuf>,
        home: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let spec = Shared::global(cx).settings.agents.get(idx).cloned();
        let starts = archive.map_or(StartsOn::Fresh, StartsOn::Resume);
        self.add_by(
            |workspace, uid| workspace.add_session_on(spec, uid, home, starts),
            window,
            cx,
            |_, _| {},
        )
    }

    /// Tell the Workbench when the agent on screen started. Pushed rather
    /// than asked for, from the moments it changes: arriving at a session or a
    /// project, the workspace page taking the centre, and the pane announcing
    /// that it spawned an agent — which every start, restart and resume does.
    pub(super) fn sync_agent_started(&mut self, cx: &mut Context<Self>) {
        let since = self.chat.read(cx).active_started(cx);
        self.workbench
            .update(cx, |panel, cx| panel.agent_started(since, cx));
    }

    /// Restart the active session's adapter, asked about in a modal while a
    /// turn is running.
    pub fn restart_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_panel = FocusedPanel::Chat;
        let Some(uid) = self.active_session_uid() else {
            return;
        };
        match self.chat.read(cx).restarting(uid, cx) {
            Restarting::Host(host) => return self.ask_restart_host(host, window, cx),
            Restarting::Now => return self.restart_now(uid, window, cx),
            Restarting::Ask => {}
        }
        let label = self.session_label(uid, cx);
        let ask = Confirm {
            id: "restart-agent",
            title: format!("Restart the agent of {label}?").into(),
            description: "A turn is running. Restarting stops it, and that turn is lost.".into(),
            act: "Restart",
            ..Default::default()
        };
        // By uid: the question is about the session it was asked on,
        // whichever one shows by the time it is answered.
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.restart_now(uid, window, cx)
        });
    }

    /// Restart session `uid`'s adapter, no question asked, and say so.
    pub(super) fn restart_now(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.restart_told(uid, false, true, window, cx);
    }

    /// [`Self::restart_now`], saying so when `told`.
    pub(super) fn restart_told(
        &mut self,
        uid: u64,
        after_service: bool,
        told: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let watched = self.window.workspace.watched_on(uid).map(|(_, id)| id);
        let restarted = self
            .chat
            .update(cx, |pane, cx| pane.restart(uid, after_service, watched, cx));
        match (restarted, told) {
            (crate::chat::pane::Restart::Restarted, true) => {
                window.push_notification(crate::toast::info("Restarting the agent"), cx);
            }
            (crate::chat::pane::Restart::Restarted, false)
            | (crate::chat::pane::Restart::Nothing, _) => {}
        }
        cx.notify();
    }

    /// What a question about session `uid` calls it: whatever its row is
    /// named by, so the question and the row read as the same thing.
    pub fn session_label(&self, uid: u64, cx: &App) -> String {
        self.chat.read(cx).title_for(uid, cx).unwrap_or_else(|| {
            self.window
                .workspace
                .roots
                .iter()
                .flat_map(|root| root.sessions.iter())
                .find(|session| session.uid == uid)
                .map(|session| session.title().to_string())
                .unwrap_or_default()
        })
    }

    /// Put session `uid` on screen: its project selected, its conversation
    /// showing. For a run somebody picked by hand, who asked for it and is
    /// waiting to watch it, and for a session pressed on the workspace page.
    pub fn show_session(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let fx = self.window.workspace.select(uid);
        self.carry_workspace(fx, window, cx);
    }

    /// Show session `uid`, in whichever window holds it — bringing that window
    /// forward when it is not this one.
    pub(super) fn show_session_in(
        &mut self,
        uid: u64,
        at: gpui::AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if at == window.window_handle() {
            self.show_session(uid, window, cx);
            return;
        }
        // Later: that window's shell is not to be reached into from inside
        // this one's update.
        crate::windows::later(at, cx, move |shell, window, cx| {
            window.activate_window();
            shell.show_session(uid, window, cx);
        });
    }
}

/// Which of the configured agents is called `name`, the first when none is.
pub(super) fn agent_index(name: Option<SharedString>, cx: &App) -> usize {
    name.and_then(|name| {
        Shared::global(cx)
            .settings
            .agents
            .iter()
            .position(|spec| spec.name == name.as_ref())
    })
    .unwrap_or(0)
}
