use super::confirm::Ask;
use super::{FocusedPanel, RailSession, Shell, TabCycle};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::dock::DockPlacement;
use gpui_component::notification::Notification;
use onehand_core::gitstat;
use std::path::PathBuf;

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
        RailSession {
            signal,
            title: pane.title_for(uid, cx).map(SharedString::from),
            since,
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
        self.chat.update(cx, |pane, cx| pane.export(cx));
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
        self.window
            .workspace
            .active_root()
            .and_then(|root| root.active_session())
            .map(|session| session.uid)
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
        let ask = Ask {
            id: "delete-open-conversation",
            title: "Delete this conversation?".into(),
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
        self.ask(ask, window, cx, move |shell, window, cx| {
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
                            Notification::warning(format!("Conversation not deleted — {e}")),
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
            .map(|root| root.active_session)
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
    /// **Asked about in a modal only while a turn is in flight.** The
    /// transcript is written at the end of every turn, so closing an idle
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
        if !self.chat.read(cx).turn_in_flight(uid, cx) {
            self.end_session(uid, window, cx);
            return;
        }
        let label = self.session_label(uid, cx);
        let ask = Ask {
            id: "close-session",
            title: format!("Close {label}?").into(),
            description: "A turn is running. Closing the session stops its agent, and that \
                          turn is lost."
                .into(),
            act: "Close",
            ..Default::default()
        };
        // By uid, not by place: the list can shift while the question is open.
        self.ask(ask, window, cx, move |shell, window, cx| {
            shell.end_session(uid, window, cx)
        });
    }

    /// Close session `uid` and its agent, no question asked.
    fn end_session(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((root_idx, session_idx, path)) = self
            .window
            .workspace
            .roots
            .iter()
            .enumerate()
            .find_map(|(ri, root)| {
                root.sessions
                    .iter()
                    .position(|session| session.uid == uid)
                    .map(|si| (ri, si, root.path.clone()))
            })
        else {
            return;
        };

        // The pane owns the conversation and, through it, the adapter: dropping
        // the session there is what ends the agent process. Nothing else has to
        // be shut down by hand.
        self.chat.update(cx, |pane, cx| pane.close(uid, cx));
        if let Some(order) = self.mru.get_mut(&path) {
            order.retain(|&seen| seen != uid);
        }
        // A cycle's frozen order can name a session that no longer exists.
        self.tab_cycle = None;

        self.window.workspace.close_session(root_idx, session_idx);
        // No toast: the row leaving the rail and the pane moving to whatever is
        // left say it, where removing a project has to announce itself because
        // most of what it destroys was never on screen.
        self.show_active_session(window, cx);
        cx.notify();
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
        self.window.workspace.select_session(index);
        self.show_active_session(window, cx);
        cx.notify();
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
            if order.len() < 2 {
                return;
            }
            self.tab_cycle = Some(TabCycle {
                order,
                pos: 0,
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
            self.window.workspace.select_session(index);
            self.show_active_session(window, cx);
            cx.notify();
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
        self.window.workspace.select_root(root_idx);
        self.new_session_with(agent, window, cx);
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
        self.window.workspace.move_session(root_idx, from, to);
        cx.notify();
    }

    /// Add a session running `agents[idx]`.
    pub fn new_session_with(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_session(idx, None, window, cx);
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
        let idx = agent
            .and_then(|name| {
                Shared::global(cx)
                    .agents
                    .iter()
                    .position(|spec| spec.name == name.as_ref())
            })
            .unwrap_or(0);
        self.spawn_session(idx, resume, window, cx)
    }

    /// Mint a session on the active root and show it, resuming `archive` if one
    /// was named.
    ///
    /// The archive is handed to the pane *before* the session is shown: showing
    /// is what spawns the adapter, and a resume arriving after that has already
    /// lost — the session would be up on a fresh conversation with the picker
    /// asking which one to open.
    pub(super) fn spawn_session(
        &mut self,
        idx: usize,
        archive: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            window.push_notification(Notification::warning("No agents configured"), cx);
            return None;
        };
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        if self.window.workspace.add_session(spec, uid).is_none() {
            window.push_notification(
                Notification::warning("Add a project before starting a session"),
                cx,
            );
            return None;
        }
        if let Some(archive) = archive {
            self.chat
                .update(cx, |pane, _| pane.resume_next(uid, archive));
        }
        self.show_active_session(window, cx);
        cx.notify();
        Some(uid)
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
        if !self.chat.read(cx).turn_in_flight(uid, cx) {
            self.restart_now(uid, window, cx);
            return;
        }
        let label = self.session_label(uid, cx);
        let ask = Ask {
            id: "restart-agent",
            title: format!("Restart the agent of {label}?").into(),
            description: "A turn is running. Restarting stops it, and that turn is lost.".into(),
            act: "Restart",
            ..Default::default()
        };
        // By uid: the question is about the session it was asked on,
        // whichever one shows by the time it is answered.
        self.ask(ask, window, cx, move |shell, window, cx| {
            shell.restart_now(uid, window, cx)
        });
    }

    /// Restart session `uid`'s adapter, no question asked.
    fn restart_now(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        match self.chat.update(cx, |pane, cx| pane.restart(uid, cx)) {
            crate::chat::pane::Restart::Restarted => {
                window.push_notification(Notification::info("Restarting the agent"), cx);
            }
            crate::chat::pane::Restart::Nothing => {}
        }
        cx.notify();
    }

    /// What a question about session `uid` calls it: whatever its row is
    /// named by, so the question and the row read as the same thing.
    fn session_label(&self, uid: u64, cx: &App) -> String {
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
        let found = self
            .window
            .workspace
            .roots
            .iter()
            .enumerate()
            .find_map(|(ri, root)| {
                root.sessions
                    .iter()
                    .position(|session| session.uid == uid)
                    .map(|si| (ri, si))
            });
        if let Some((root_idx, session_idx)) = found {
            self.select_root_session(root_idx, session_idx, window, cx);
        }
    }

    /// Show the workspace page: what waits and what works across every
    /// session and run, every project, the recent conversations and the open
    /// issues. Leaving it is [`Self::show_active_session`]'s, which every rail
    /// click goes through, and which puts back the Workbench this put away.
    pub fn show_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.page_projects();
        self.docks_aside(window, cx);
        self.chat.update(cx, |pane, cx| {
            pane.leave_issues_page(window, cx);
            pane.show_workspace(projects, window, cx)
        });
        // No session is showing now, so there is no running agent to be
        // behind on anything.
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Tasks page, narrowed to the project at `filter` or not.
    /// Left the way the workspace page is.
    pub fn show_tasks(
        &mut self,
        filter: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = self.page_projects();
        self.docks_aside(window, cx);
        self.chat.update(cx, |pane, cx| {
            pane.leave_issues_page(window, cx);
            pane.show_tasks(projects, filter, window, cx)
        });
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Tasks page with task `id` open in it.
    pub fn show_task(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tasks(None, window, cx);
        let id = id.to_string();
        self.chat
            .update(cx, |pane, cx| pane.open_task(Some(id), cx));
    }

    /// Both docks hold one project's things, and a page is about all of
    /// them. Put away, not closed: the terminal's state is filed under its
    /// project and `terminal_root` let go, so the next arrival is a handover
    /// that restores it, and the Workbench comes back on leaving.
    pub(super) fn docks_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page_shown(cx) {
            return;
        }
        if let Some(root) = self.terminal_root.take() {
            let live = self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
            self.terminal_open.insert(root, live);
        }
        self.set_terminal_visible(false, window, cx);
        self.workbench_aside =
            self.dock.read(cx).is_dock_open(DockPlacement::Right, cx) || self.stepped_aside;
        self.hide_workbench(window, cx);
    }

    /// Every project as the workspace page lists it, in rail order.
    pub(super) fn page_projects(&self) -> Vec<crate::chat::pane::PageProject> {
        let workspace = &self.window.workspace;
        workspace
            .display_order()
            .into_iter()
            .map(|idx| &workspace.roots[idx])
            // A run's own worktree is not a project anybody chose, and its
            // issues are the project's it was cut from.
            .filter(|root| !root.transient)
            .map(|root| crate::chat::pane::PageProject {
                label: SharedString::from(root.label.clone()),
                root: root.path.clone(),
                sessions: root
                    .sessions
                    .iter()
                    .map(|session| (session.uid, SharedString::from(session.title().to_string())))
                    .collect(),
                git: self
                    .window
                    .git
                    .get(&root.path)
                    .map(gitstat::GitStatus::label),
                issues: self.issues_file(&root.path),
            })
            .collect()
    }

    /// Whether the workspace page is what the centre of the window shows, for
    /// the rail row that leads to it.
    pub fn workspace_shown(&self, cx: &App) -> bool {
        self.chat.read(cx).showing_workspace()
    }

    /// Whether the Tasks page is what the centre of the window shows.
    pub fn tasks_shown(&self, cx: &App) -> bool {
        self.chat.read(cx).showing_tasks()
    }

    /// Whether either page is what the centre of the window shows: no
    /// project is on screen, and neither dock is.
    pub fn page_shown(&self, cx: &App) -> bool {
        self.chat.read(cx).showing_page()
    }

    /// The projects the pages list, as paths: every project but a run's own
    /// worktree.
    pub fn page_roots(&self) -> Vec<std::path::PathBuf> {
        self.window
            .workspace
            .roots
            .iter()
            .filter(|root| !root.transient)
            .map(|root| root.path.clone())
            .collect()
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
        let Some(shell) = Shared::global(cx)
            .windows
            .iter()
            .find(|w| w.handle == at)
            .map(|w| w.shell.clone())
        else {
            return;
        };
        // Deferred: that window's shell is not to be reached into from inside
        // this one's update.
        cx.defer(move |cx| {
            at.update(cx, |_, window, cx| {
                window.activate_window();
                shell
                    .update(cx, |shell, cx| shell.show_session(uid, window, cx))
                    .ok();
            })
            .ok();
        });
    }

    /// How `uid`'s last answer ended, for a run's report.
    pub fn answer_tail(&self, uid: u64, cx: &App) -> Option<String> {
        self.chat.read(cx).answer_tail(uid, cx)
    }
}
