use super::confirm::Confirm;
use super::storage::{Thing, WorkspaceChange, pick_folder};
use super::{Shell, WARM_DELAY};
use crate::state::Shared;
use crate::workbench::Refusal;
use gpui::{App, Context, Focusable as _, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::dock::DockPlacement;
use onehand_core::disk::Disk;
use onehand_core::workspace::HostName;
use std::path::{Path, PathBuf};

/// Why the terminal has nothing for a project on a host.
pub(super) const TERMINALS_ON_HOST_LATER: &str = "Terminals on a host come later";

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
        let status = self.window.git_of(root);
        let facts = crate::chat::pane::ProjectFacts {
            asks: !self
                .removal_losses(self.window.workspace.active_root, cx)
                .is_empty(),
            ..crate::chat::pane::ProjectFacts::of(root, status.is_some())
        };
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
            && std::mem::take(&mut self.page_hid_workbench)
            && !self.dock.read(cx).is_dock_open(DockPlacement::Right)
        {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
    }

    /// The body of [`Self::show_active_session`]: every panel pointed at the
    /// active root, and the pane at its session or its project page.
    fn arrive_at_active_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let worker = self.turn_to_lead();
        // The lead on screen changing takes the region's caret to the
        // composer of the session now shown, before the region moves on.
        let region_caret = self
            .workers_panel
            .focus_handle(cx)
            .contains_focused(window, cx);
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
        self.tell_workbench_git(cx);
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
            // Nor is there a host project for either dock to be refusing.
            self._host_files = None;
            self.workbench
                .update(cx, |panel, cx| panel.refuse(None, cx));
            self.terminal.update(cx, |panel, cx| panel.refuse(None, cx));
            return;
        };
        let label = SharedString::from(root.label.clone());
        let path = root.path.clone();
        // Read out of the tree before anything below borrows the shell mutably,
        // rather than reached for again further down.
        let session = root
            .active_session()
            .map(|session| (session.uid, session.spec.clone()));
        let host = root.on_host().cloned();
        let local = root.local_path().map(Path::to_path_buf);
        // Editor tabs and the file tree are per root, so the Workbench follows
        // the selection rather than mixing two projects' state. A folder on a
        // host has its files read over the host's attach, and no terminal
        // yet: that panel says so instead of keeping the last project's.
        match &host {
            None => {
                self._host_files = None;
                self.workbench.update(cx, |panel, cx| {
                    panel.set_root(path.clone(), Disk::Local, cx)
                });
                self.terminal
                    .update(cx, |panel, cx| panel.set_root(path.clone(), cx));
            }
            Some(host) => {
                self.show_host_files(host.clone(), path.clone(), cx);
                self.terminal.update(cx, |panel, cx| {
                    panel.refuse(Some(TERMINALS_ON_HOST_LATER), cx)
                });
            }
        }
        self.follow_terminal_dock(&path, window, cx);
        // Whether a shell is alive is a fact about the project being arrived at,
        // not about the window, and the title bar draws it. The
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
                pane.clear_active(Some((label, path.clone(), host)), window, cx)
            });
            // After, never before: `clear_active` builds the page's state fresh
            // for the project being arrived at, so anything pushed into the old
            // one is thrown away with it.
            self.sync_project_facts(cx);
            self.show_check_input(local.as_deref(), window, cx);
            // Nothing is running on this project and the page now on screen is
            // a list of past conversations over a *New session* button, so the
            // next thing asked of it is almost certainly a session. Start the
            // agent against this root now: bringing one up costs seconds that
            // are entirely the adapter's and the SDK's, and spending them while
            // the user reads the page is spending them for free.
            if let Some(local) = local {
                self.warm_default_agent(local, cx);
            }
            self.sync_agent_started(cx);
            return;
        };
        self.chat.update(cx, |pane, cx| {
            pane.show(uid, path.clone(), host, &spec, window, cx);
            pane.release_workers(cx);
            if region_caret {
                pane.reclaim_focus(window, cx);
            }
        });
        // A worker asked for is shown in its lead's region, picked, the caret
        // on the list.
        if let Some(worker) = worker {
            self.show_workers(Some(worker), true, window, cx);
        }
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

    /// Point the Workbench at `path` on `host`, read over the host's attach:
    /// at once when it is up, else once it lands, saying meanwhile what it
    /// waits on and, if it fails, why.
    ///
    /// A host found busy is waited on as everywhere else, and attached again
    /// once its sessions are done. What lands for a project no longer on
    /// screen is dropped.
    pub(super) fn show_host_files(
        &mut self,
        host: HostName,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if let Some(conn) = crate::hosts::live(cx, &host) {
            self._host_files = None;
            let disk = crate::hosts::disk_over(cx, conn);
            self.workbench
                .update(cx, |panel, cx| panel.set_root(path, disk, cx));
            return;
        }
        let attaching = Refusal::Waiting(format!("Attaching to {host}…").into());
        self.workbench
            .update(cx, |panel, cx| panel.refuse(Some(attaching), cx));
        self._host_files = Some(cx.spawn(async move |shell, cx| {
            let shown = |shell: &Shell| {
                shell
                    .window
                    .workspace
                    .active_root()
                    .is_some_and(|root| root.on_host() == Some(&host) && root.path == path)
            };
            loop {
                let attached = cx.update(|cx| crate::hosts::disk(cx, &host)).await;
                let why: String = match attached {
                    Ok(disk) => {
                        let path = path.clone();
                        let _ = shell.update(cx, |shell, cx| {
                            if shown(shell) {
                                shell
                                    .workbench
                                    .update(cx, |panel, cx| panel.set_root(path, disk, cx));
                            }
                        });
                        return;
                    }
                    Err(crate::hosts::HostFailure::Busy {
                        host: busy_on,
                        busy,
                    }) => {
                        let line = crate::hosts::waiting_line(&busy_on, busy.len());
                        let _ = shell.update(cx, |shell, cx| {
                            if shown(shell) {
                                shell.workbench.update(cx, |panel, cx| {
                                    panel.refuse(Some(Refusal::Waiting(line.into())), cx)
                                });
                            }
                        });
                        // None: an attach landed since, which the next turn takes.
                        let Some(idle) = cx.update(|cx| crate::hosts::idle(cx, &host)) else {
                            continue;
                        };
                        match idle.await {
                            Ok(()) => continue,
                            Err(why) => why,
                        }
                    }
                    Err(crate::hosts::HostFailure::Other(why)) => why,
                };
                let _ = shell.update(cx, |shell, cx| {
                    if shown(shell) {
                        shell.workbench.update(cx, |panel, cx| {
                            panel.refuse(Some(Refusal::Failed(why.into())), cx)
                        });
                    }
                });
                return;
            }
        }));
    }

    /// Attach again for the host project on screen, if it is one.
    pub(super) fn try_host_again(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.window.workspace.active_root()
            && let Some(host) = root.on_host().cloned()
        {
            let path = root.path.clone();
            self.show_host_files(host, path, cx);
        }
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
                if let Some(spec) = onehand_core::config::agent_or_first(&shared.agents, None) {
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
                    shell.open_root(dir, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The project session `uid` is on, when this window holds it.
    pub(crate) fn session_project(&self, uid: u64) -> Option<PathBuf> {
        self.window
            .workspace
            .roots
            .iter()
            .find(|root| root.sessions.iter().any(|s| s.uid == uid))
            .map(|root| root.path.clone())
    }

    /// The project on screen, its name and path, when it is on this machine.
    pub(crate) fn on_screen(&self) -> Option<(String, PathBuf)> {
        let root = self.window.workspace.active_root()?;
        Some((root.label.clone(), root.local_path()?.to_path_buf()))
    }

    /// Open `path` in the Workbench and show the Workbench, without moving
    /// focus: a tool card's link and a tool's call land here alike.
    pub(crate) fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        // Another panel maximized would hide the Workbench the file opens in:
        // it is put back first, so the file is seen where it was asked for.
        if self
            .app_maximized
            .is_some_and(|p| p != super::FocusedPanel::Workbench)
        {
            self.toggle_maximize_panel(super::FocusedPanel::Workbench, window, cx);
        }
        self.workbench
            .update(cx, |panel, cx| panel.open_file(path, window, cx));
        self.leave_workers(window, cx);
        // Opening a file is what makes the Workbench worth showing -- but only
        // if it is closed, since toggling an open dock would hide the file
        // just asked for.
        self.dock.update(cx, |dock, cx| {
            use gpui_component::dock::DockPlacement;
            if !dock.is_dock_open(DockPlacement::Right) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
    }

    /// Add `dir` as a project and select it, or select it if it is one
    /// already. The folder picker lands here, and so does a second launch.
    pub(crate) fn open_root(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let idx = self.window.workspace.add_root(dir);
        self.window.workspace.select_root(idx);
        self.show_active_session(window, cx);
        self.refresh_git(cx);
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Add `path` on `host` as a project and select it, or select it if it is
    /// one already. Its `git status` is asked over the attach adding it made.
    pub(crate) fn open_host_root(
        &mut self,
        host: onehand_core::workspace::HostName,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idx = self.window.workspace.add_host_root(host, path);
        self.window.workspace.select_root(idx);
        self.show_active_session(window, cx);
        self.refresh_git(cx);
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Remove a project root, and everything the window keeps for it.
    ///
    /// Asked about in a modal first while the root has sessions or unsaved
    /// editor buffers: removing it kills those agents, mid-turn or not, and the
    /// buffers have nowhere to go once the tab strip they belong to leaves with
    /// the root. A root with nothing running and nothing unsaved just goes.
    pub fn remove_root(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(idx) else {
            return;
        };
        let (host, path, label) = (root.host.clone(), root.path.clone(), root.label.clone());
        let losses = self.removal_losses(idx, cx);
        if losses.is_empty() {
            self.drop_root(host.as_ref(), &path, window, cx);
            return;
        }
        let ask = Confirm {
            id: "remove-project",
            title: format!("Remove {label} from the workspace?").into(),
            description: format!(
                "This {}. The folder itself stays on disk.",
                losses.join(" and ")
            )
            .into(),
            act: "Remove",
            ..Default::default()
        };
        // By host and path, not by index: the tree can be reordered while the
        // question is open, and the answer has to land on the project it was
        // about, not on a folder of the same path elsewhere.
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.drop_root(host.as_ref(), &path, window, cx)
        });
    }

    /// What removing the root at `idx` would take with it: its sessions, closed,
    /// and its unsaved files, discarded. Removing asks first only when there is
    /// any, which is also what puts an ellipsis on the word that starts it.
    pub(crate) fn removal_losses(&self, idx: usize, cx: &App) -> Vec<String> {
        let Some(root) = self.window.workspace.roots.get(idx) else {
            return Vec::new();
        };
        let live = root.sessions.len();
        // A host's folder has its files open in the editor as one here does.
        let on_host = root.on_host().is_some();
        let unsaved = self.workbench.read(cx).unsaved_in(&root.path, on_host, cx);
        let mut losses = Vec::new();
        if live > 0 {
            let s = if live == 1 { "session" } else { "sessions" };
            losses.push(format!("closes {live} {s}"));
        }
        if unsaved > 0 {
            let s = if unsaved == 1 { "file" } else { "files" };
            losses.push(format!("discards {unsaved} unsaved {s}"));
        }
        losses
    }

    /// Remove the root at `path` on `host`, the question already answered.
    fn drop_root(
        &mut self,
        host: Option<&onehand_core::workspace::HostName>,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(idx) = self
            .window
            .workspace
            .roots
            .iter()
            .position(|r| r.on_host() == host && r.path == path)
        else {
            return;
        };
        let label = self.window.workspace.roots[idx].label.clone();
        self.forget_root(idx, cx);
        window.push_notification(crate::toast::info(format!("Removed {label}")), cx);
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
        // The editor's buffers and the tree go with a host's folder too, or
        // they come back under a later project on the same path.
        let on_host = self.local_root(idx).is_none();
        self.workbench
            .update(cx, |panel, cx| panel.forget_root(&path, on_host, cx));
        // A folder on a host was never given the rest, and what is filed
        // under its path belongs to a folder of the same path here.
        if self.local_root(idx).is_some() {
            self.issues_page.forget_root(&path, cx);
            self.terminal
                .update(cx, |panel, cx| panel.forget_root(&path, cx));
            self.window.git.remove(&path);
            // Its field goes with it: kept, the same folder added back found the
            // old command in a field its project no longer had.
            self.check_inputs.remove(&path);
            self.terminal_homes.forget(&path);
        }
        self.mru.remove(&path);
        self.folds.remove(&path);
        // A cycle's frozen order can name sessions that no longer exist.
        self.tab_cycle = None;

        self.window.workspace.remove_root(idx);
    }

    /// Hold a root at the top of the rail, or let it go.
    ///
    /// Only the drawing order moves; `roots` and every index into it stay
    /// exactly as they were, so nothing keyed by position has to be told.
    pub fn toggle_pin(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let (at, pinned) = ((root.host.clone(), root.path.clone()), !root.pinned);
        self.window.workspace.toggle_pin(root_idx);
        // Put back, alone, if its write fails: pinning is its own thing, and
        // it goes back to the pin as last saved.
        self.save_change(
            Thing::Pin(at.0.clone(), at.1.clone()),
            WorkspaceChange {
                before: (at.clone(), !pinned),
                after: (at, pinned),
                put: |shell, ((host, path), pinned), _, cx| {
                    if let Some(root) = shell
                        .window
                        .workspace
                        .roots
                        .iter_mut()
                        .find(|root| root.host == host && root.path == path)
                    {
                        root.pinned = pinned;
                    }
                    shell.sync_project_facts(cx);
                },
            },
            window,
            cx,
        );
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
        let before = self.window.workspace.root_order();
        // A drop the pin line clamps back onto itself moved nothing to write.
        if self.window.workspace.move_root(from, to).is_none() {
            return;
        }
        // Put back, alone, if its write fails: every root to its place in the
        // order last saved, which wherever the pin clamp landed this one, and
        // however many moves came after, is what the file still holds.
        let after = self.window.workspace.root_order();
        self.save_change(
            Thing::Order,
            WorkspaceChange {
                before,
                after,
                put: |shell, order, _, _| shell.window.workspace.put_order(&order),
            },
            window,
            cx,
        );
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
        let path = onehand_core::workspace::host_path(root.on_host(), &root.path);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
        window.push_notification(crate::toast::info(format!("Copied {path}")), cx);
    }

    /// The folder of root `idx` on this machine; `None` for a root on a host,
    /// which nothing here may read, run in or scan.
    pub(crate) fn local_root(&self, idx: usize) -> Option<&Path> {
        self.window.workspace.roots.get(idx)?.local_path()
    }

    /// Where the project at `root`, a folder on this machine, sits in this
    /// window's list of roots. Never a root on a host of the same path: every
    /// caller names a folder here, which work here is about to read.
    pub(super) fn root_index(&self, root: &Path) -> Option<usize> {
        self.window
            .workspace
            .roots
            .iter()
            .position(|r| r.local_path() == Some(root))
    }

    /// Whether `root`, a folder on this machine, is one of this window's
    /// projects.
    pub fn holds_root(&self, root: &Path) -> bool {
        self.root_index(root).is_some()
    }

    /// The check command of the project at `root`, if it has one.
    pub fn check_of(&self, root: &Path) -> Option<String> {
        self.window.workspace.roots[self.root_index(root)?]
            .check
            .clone()
    }
}

impl Shell {
    /// Answer what the workspace page asked for.
    pub(super) fn on_workspace_page(
        &mut self,
        event: &crate::workspace_page::WorkspacePageEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::workspace_page::WorkspacePageEvent as E;
        match event {
            E::OpenIssue { root, number } => self.open_issue(root, *number, window, cx),
            E::ShowSession { uid, window: at } => self.show_session_in(*uid, *at, window, cx),
            E::ShowProject(host, root) => {
                let at = self
                    .window
                    .workspace
                    .roots
                    .iter()
                    .position(|r| r.on_host() == host.as_ref() && &r.path == root);
                if let Some(idx) = at {
                    self.select_root(idx, window, cx);
                }
            }
            // Its project made the active one first, since a session
            // is minted on that, but not shown: showing it would
            // connect the session it was last on, which nobody asked
            // for. The new session's own arrival shows the project.
            E::ResumeIn {
                root,
                agent,
                archive,
            } => {
                if let Some(idx) = self.root_index(root) {
                    self.window.workspace.select_root(idx);
                    let _ =
                        self.start_session(Some(agent.clone()), Some(archive.clone()), window, cx);
                }
            }
        }
        cx.notify();
    }
}
