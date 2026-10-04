use super::storage::pick_folder;
use super::{Shell, WARM_DELAY};
use crate::state::Shared;
use gpui::{Context, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::dock::DockPlacement;
use gpui_component::notification::Notification;
use onehand_core::gitstat;
use std::path::{Path, PathBuf};

impl Shell {
    /// Tell the project page what the selected project is, beyond its name.
    ///
    /// Two facts its menu needs and cannot work out: pinning lives in the
    /// workspace tree, and "is this a repository" is whatever the last `git
    /// status` sweep answered. Pushed from the three moments either can change —
    /// arriving at a project, pinning one, and a sweep landing — because the
    /// page is a separate panel and nothing about it is re-read per frame.
    pub(super) fn sync_project_facts(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            return;
        };
        let status = self.window.git.get(&root.path);
        let facts = crate::chat::pane::ProjectFacts::of(root, status.is_some());
        // The same line the rail prints beside the project's name, from core's
        // own rule rather than composed again here.
        let line = status.map(|status| gpui::SharedString::from(status.label()));
        self.chat.update(cx, |pane, cx| {
            pane.set_project_facts(facts, cx);
            pane.set_git(line, cx);
        });
    }

    /// Whether a project's sessions are showing under it in the rail.
    ///
    /// **A fact and never a derivation.** This used to answer `is_active` for a
    /// project nobody had touched, which made the fold a function of the
    /// selection -- so it moved when the selection did, and a single click on
    /// one project folded a different one away. The project the user was
    /// leaving had never been written down: `reveal_root` records the project
    /// being arrived *at*, and the one already on screen at launch was never
    /// arrived at, so its open state was only ever implied. It lost the
    /// implication and shut itself, which is exactly the click-toggles-the-row
    /// behaviour the caret was introduced to take away.
    ///
    /// Shut is the right answer for a project nobody has opened: a workspace of
    /// ten roots is otherwise a rail nobody can see the bottom of. The selected
    /// project still shows what is in it, because arriving at one opens it --
    /// that is [`Shell::show_active_session`]'s job now, and it happens once,
    /// as a write.
    pub fn project_unfolded(&self, path: &std::path::Path) -> bool {
        self.folds.get(path).copied().unwrap_or(false)
    }

    /// Open a project's sessions, or put them away.
    pub fn toggle_fold(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let open = self.project_unfolded(&path);
        self.folds.insert(path, !open);
        cx.notify();
    }

    /// Going to a project is asking what is in it, so arriving opens it.
    ///
    /// Written explicitly rather than by dropping the entry: falling back to
    /// `is_active` would snap the project shut again the moment the selection
    /// moved on, and a project left open is what the user last saw.
    fn reveal_root(&mut self) {
        if let Some(root) = self.window.workspace.active_root() {
            self.folds.insert(root.path.clone(), true);
        }
    }

    /// Select a root, and show whatever session it was last on.
    pub fn select_root(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.select_root(idx);
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Select root *and* session in one touch, mirroring
    /// `Message::SelectRootSession`. The rail is session-first: a session row
    /// is the switcher, so it must not take two clicks to reach.
    pub fn select_root_session(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window.workspace.select_root(root_idx);
        self.window.workspace.select_session(session_idx);
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Point the chat pane at the workspace's active session, spawning its
    /// adapter on first view.
    ///
    /// Lazy: a workspace with a dozen roots must not launch a dozen agent
    /// processes at boot.
    pub(super) fn show_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.arrive_at_active_root(window, cx);
        // Leaving the workspace page brings back the Workbench it put away.
        // After the arrival, not before: until the pane has left the page the
        // Workbench refuses to open, and the request would be spent on nothing.
        // The terminal needs nothing here, since the arrival's handover
        // restores it.
        //
        // Opened as a dock and not through `show_workbench`, which would take
        // the caret: the arrival has just given it to the conversation, and
        // whoever left the page for a session wants to type there.
        if !self.page_shown(cx)
            && std::mem::take(&mut self.workbench_aside)
            && !self.dock.read(cx).is_dock_open(DockPlacement::Right, cx)
        {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
    }

    /// The body of [`Self::show_active_session`]: every panel pointed at the
    /// active root, and the pane at its session or its project page.
    fn arrive_at_active_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Arriving at a project is asking what is in it, so arriving opens it
        // -- and this is the one place every arrival passes through, which is
        // what the two callers that used to do it themselves could not be.
        // A root becomes the active one down paths that never touch
        // `select_root`: `Ctrl+Tab` across projects, a session closing, a
        // project being removed from under the selection, a chat on the remote
        // bridge pointing itself somewhere. Each of those left the project it
        // landed on unwritten, and the fold then had to be guessed from the
        // selection -- which is the guess that made a click fold a row the user
        // had not clicked.
        //
        // It is a *write*, so the answer stops moving once it is made. The one
        // consequence worth naming: folding the active project with the caret
        // and then reaching a session inside it by keyboard opens it again,
        // because asking to see a session in a project is asking to see the
        // project.
        self.reveal_root();
        // Every arrival at a project passes through here, whether or not it has
        // a session on it -- which is what the branch below cannot be, since it
        // only runs where the project page is what is shown. The strip under
        // the composer names the project's branch, so a switch that did not
        // push would leave the previous project's branch under the new
        // project's conversation until the next sweep landed.
        self.sync_project_facts(cx);
        let Some(root) = self.window.workspace.active_root() else {
            // No roots at all. The pane has to be told, or removing the last
            // project leaves the centre of the window inviting the user to
            // start a session in a project that is no longer in the workspace.
            self.chat
                .update(cx, |pane, cx| pane.clear_active(None, window, cx));
            // A parked adapter is bound to a project root, and there is no
            // longer one to be bound to -- unlike every other case, this one
            // has no project the spare could still turn out to be for.
            self._pending_warm = None;
            Shared::global(cx).acp.drop_warm();
            return;
        };
        let label = SharedString::from(root.label.clone());
        let path = root.path.clone();
        // Read out of the tree before anything below borrows the shell mutably,
        // rather than reached for again further down.
        let session = root
            .active_session()
            .map(|session| (session.uid, session.spec.clone()));
        // Editor tabs and the file tree are per root, so the Workbench follows
        // the selection rather than mixing two projects' state.
        self.workbench
            .update(cx, |panel, cx| panel.set_root(path.clone(), cx));
        self.terminal
            .update(cx, |panel, cx| panel.set_root(path.clone(), cx));
        self.follow_terminal_dock(&path, window, cx);
        // Whether a shell is alive is a fact about the project being arrived at,
        // not about the window, and the conversation header draws it. The
        // observers that normally push it only fire when a panel notifies, and
        // switching projects is a moment where nothing did.
        self.sync_terminal_live(cx);
        let Some((uid, spec)) = session else {
            // The other two panels have already followed the selection, so the
            // chat must not be the one panel still showing the root the user
            // just left -- the composer would keep prompting that root's agent.
            // The path goes with the label: what the pane draws in place of a
            // conversation is that project's own past ones, and they are keyed
            // by where the project is rather than by what it is called.
            self.chat.update(cx, |pane, cx| {
                pane.clear_active(Some((label, path.clone())), window, cx)
            });
            // After, never before: `clear_active` builds the page's state fresh
            // for the project being arrived at, so anything pushed into the old
            // one is thrown away with it.
            self.sync_project_facts(cx);
            // Nothing is running on this project and the page now on screen is
            // a list of past conversations over a *New session* button, so the
            // next thing asked of it is almost certainly a session. Start the
            // agent against this root now: bringing one up costs seconds that
            // are entirely the adapter's and the SDK's, and spending them while
            // the user reads the page is spending them for free.
            self.warm_default_agent(path, cx);
            self.sync_agent_started(cx);
            return;
        };
        self.chat.update(cx, |pane, cx| {
            pane.show(uid, path.clone(), &spec, window, cx)
        });
        // After `show`, which is what connects a session shown for the first
        // time — before it, a fresh session has no agent to have started.
        self.sync_agent_started(cx);
        // This project has a session on screen, so nothing here is waiting on a
        // fresh agent. A pre-start still in its delay is dropped rather than
        // allowed to fire -- but one that has already *started* is left alone.
        // Killing it here is what turned clicking between projects into a spawn
        // and a kill per click, and an adapter cut off mid-handshake does not go
        // quietly. Holding one spare process costs less than that, and the next
        // pre-start for a different project replaces it anyway.
        self._pending_warm = None;
        self.touch_mru(path, uid);
    }

    /// Start the default agent against `root` ahead of the session that would
    /// run it, once the selection has held still for [`WARM_DELAY`].
    ///
    /// The default is the first configured agent -- the same choice *New
    /// session* makes, which is what makes the guess worth acting on. Starting
    /// a *different* agent is a separate rail action, and a miss costs only the
    /// spare process: the adapter is claimed by matching what it was told to
    /// run against what the session asks for, so a mismatch simply spawns.
    fn warm_default_agent(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if Shared::global(cx).agents.is_empty() {
            return;
        }
        // Replacing the task cancels the one before it, which is the debounce.
        self._pending_warm = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(WARM_DELAY).await;
            cx.update(|cx| {
                let shared = Shared::global(cx);
                if let Some(spec) = shared.agents.first() {
                    shared.acp.warm(spec, root);
                }
            });
        }));
    }

    /// Add a project root, picked with the native folder dialog.
    ///
    /// `Workspace::add_root` normalizes the path and *selects* an existing root
    /// rather than making a twin, so picking a folder that is already in the
    /// workspace (or a symlink to one) is a navigation, not a duplicate --
    /// every per-root map in the app is keyed by that path.
    pub fn add_root(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    let idx = shell.window.workspace.add_root(dir);
                    shell.window.workspace.select_root(idx);
                    shell.show_active_session(window, cx);
                    shell.refresh_git(cx);
                    shell.save_workspace(window, cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Remove a project root, and everything the window keeps for it.
    ///
    /// Two-step while the root has sessions: the first choice arms and says so,
    /// the second removes -- the same guard shape as a mid-turn restart, and
    /// for the same reason. Arming a different root replaces the arming rather
    /// than stacking, so the confirmation always belongs to the row just
    /// acted on.
    pub fn remove_root(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(idx) else {
            return;
        };
        let (path, label, live) = (root.path.clone(), root.label.clone(), root.sessions.len());
        // Unsaved editor buffers are the other thing this click destroys, and
        // they have nowhere to go afterwards -- the tab strip they belong to
        // leaves with the root. Guarded on the same second click rather than a
        // second one of its own.
        let unsaved = self.workbench.read(cx).unsaved_in(&path, cx);

        if (live > 0 || unsaved > 0) && self.pending_remove != Some(idx) {
            self.pending_remove = Some(idx);
            let mut losses = Vec::new();
            if live > 0 {
                let s = if live == 1 { "session" } else { "sessions" };
                losses.push(format!("close {live} {s}"));
            }
            if unsaved > 0 {
                let s = if unsaved == 1 { "file" } else { "files" };
                losses.push(format!("discard {unsaved} unsaved {s}"));
            }
            window.push_notification(
                Notification::warning(format!(
                    "Remove {label} and {}? Choose Remove from workspace again to confirm",
                    losses.join(" and ")
                )),
                cx,
            );
            cx.notify();
            return;
        }
        self.pending_remove = None;
        self.forget_root(idx, cx);
        window.push_notification(Notification::info(format!("Removed {label}")), cx);
        self.show_active_session(window, cx);
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Drop root `idx` and everything the window keeps for it, with no question
    /// asked and nothing on screen moved.
    pub(super) fn forget_root(&mut self, idx: usize, cx: &mut Context<Self>) {
        let path = self.window.workspace.roots[idx].path.clone();
        // Sessions go first: dropping a chat session is what kills its adapter,
        // and the workspace tree is where their uids are recorded.
        let uids: Vec<u64> = self.window.workspace.roots[idx]
            .sessions
            .iter()
            .map(|session| session.uid)
            .collect();
        self.chat.update(cx, |pane, cx| {
            for &uid in &uids {
                pane.close(uid, cx);
            }
        });
        self.workbench
            .update(cx, |panel, cx| panel.forget_root(&path, cx));
        self.terminal
            .update(cx, |panel, cx| panel.forget_root(&path, cx));
        self.window.git.remove(&path);
        self.mru.remove(&path);
        self.terminal_open.remove(&path);
        self.folds.remove(&path);
        // The dock on screen no longer belongs to anyone. Left naming this root,
        // the next handover would file the live state under a project that is
        // gone and hand a re-added one a state it never chose.
        if self.terminal_root.as_deref() == Some(path.as_path()) {
            self.terminal_root = None;
        }
        // A cycle's frozen order can name sessions that no longer exist.
        self.tab_cycle = None;

        self.window.workspace.remove_root(idx);
    }

    /// Hold a root at the top of the rail, or let it go.
    ///
    /// Only the drawing order moves; `roots` and every index into it stay
    /// exactly as they were, so nothing keyed by position has to be told.
    pub fn toggle_pin(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.toggle_pin(root_idx);
        self.save_workspace(window, cx);
        // The project page's menu is the other place that says whether this is
        // pinned, and it says it in the label of the entry that was just used.
        self.sync_project_facts(cx);
        cx.notify();
    }

    /// Drop a project at another place in the rail.
    ///
    /// Both numbers are *display* positions, which is what the rail drags; the
    /// permutation and the pin clamp are `Workspace::move_root`'s.
    ///
    /// Persisted, unlike a session move: the roots' order is written into the
    /// workspace file, so a list somebody arranged by hand comes back arranged.
    pub fn move_root(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window.workspace.move_root(from, to);
        // A root index, and every one of them has just changed hands. Left
        // armed, the confirming click for a removal would land on whichever
        // project moved into that slot.
        self.pending_remove = None;
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Put a root's path on the clipboard.
    ///
    /// The rail shows a folder's *name*, which is not enough to paste anywhere
    /// or to tell two checkouts of one repository apart.
    pub fn copy_root_path(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let path = root.path.display().to_string();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
        window.push_notification(Notification::info(format!("Copied {path}")), cx);
    }

    /// Where the project at `root` sits in this window's list of roots.
    pub(super) fn root_index(&self, root: &Path) -> Option<usize> {
        self.window
            .workspace
            .roots
            .iter()
            .position(|r| r.path == root)
    }

    /// Whether `root` is one of this window's projects.
    pub fn holds_root(&self, root: &Path) -> bool {
        self.window.workspace.roots.iter().any(|r| r.path == root)
    }

    /// Re-read everything derived from the files on disk.
    ///
    /// Two panels read the working tree and neither notices it change: the
    /// rail's `branch · N changed` and the Files tree. Both were seeded once
    /// and left, which made them stale from the first turn
    /// onwards -- the moment they start being worth reading.
    ///
    /// One call for both, because there is one cause. Cheap enough to run on
    /// every turn end: `git status` is one process per root off the UI loop,
    /// and the rescan covers only the directories currently on screen.
    pub(super) fn refresh_worktree(&mut self, cx: &mut Context<Self>) {
        self.refresh_git(cx);
        self.workbench.update(cx, |panel, cx| panel.rescan(cx));
        // A run leaving a note or a sync landing changes the issue files too;
        // a no-op unless the workspace page is what is showing.
        self.chat.update(cx, |pane, cx| pane.reload_workspace(cx));
    }

    /// Refresh `git status` for every root, off the UI loop.
    ///
    /// Uses core's blocking reader on GPUI's background executor rather than
    /// its tokio wrapper: tokio's process driver is not running under GPUI, so
    /// the async path would panic looking for a reactor.
    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let roots = self
            .window
            .workspace
            .roots
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();

        // Two refreshes can be in flight at once -- a turn ends while the
        // window is being activated, say -- and `git status` on a big repo does
        // not finish in the order it was asked for. Without this, the slower
        // (older) scan lands last and overwrites the newer snapshot.
        self.git_generation = self.git_generation.wrapping_add(1);
        let generation = self.git_generation;

        cx.spawn(async move |shell, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move {
                    roots
                        .into_iter()
                        .filter_map(|root| {
                            gitstat::read_blocking(&root).map(|status| (root, status))
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            shell
                .update(cx, |shell, cx| {
                    if shell.git_generation != generation {
                        return;
                    }
                    shell.window.git = scanned.into_iter().collect();
                    let git = shell.window.git.clone();
                    shell
                        .workbench
                        .update(cx, |panel, cx| panel.set_git(git, cx));
                    // The workspace page's project tiles name each branch.
                    if shell.page_shown(cx) {
                        let projects = shell.page_projects();
                        shell
                            .chat
                            .update(cx, |pane, cx| pane.set_page_projects(projects, cx));
                    }
                    // This sweep is also the answer to "is the selected project
                    // a repository", which decides whether its page offers to
                    // split it into a worktree.
                    shell.sync_project_facts(cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }
}
