use super::{IssuePicker, Shell};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use onehand_core::config::AgentSpec;
use std::path::{Path, PathBuf};

impl Shell {
    /// Turn unattended runs on or off for a project, from either of the places
    /// that offer it — the project's menu and Settings.
    pub fn toggle_unattended(
        &mut self,
        root_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A run's own worktree is not a project anybody chose, and the run
        // never searches it, so a switch on it would say `auto` over nothing.
        if self
            .window
            .workspace
            .roots
            .get(root_idx)
            .is_none_or(|root| root.transient)
        {
            return;
        }
        self.window.workspace.toggle_unattended(root_idx);
        // Switched on, it is looked at straight away, so a project that can
        // never be worked says so now and not at the next tick. Switched off,
        // nothing is cleared: a row that is off shows nothing, and the next
        // look over every switched-on project drops what is left.
        if let Some(root) = self
            .window
            .workspace
            .roots
            .get(root_idx)
            .filter(|root| root.unattended)
        {
            crate::unattended::check_now(self.project_for_runs(&root.path), cx);
        }
        if self.settings_open {
            self.workspace_note_wanted = true;
        }
        self.save_workspace(window, cx);
        // The project page's menu says whether this is on, in the entry that was
        // just used.
        self.sync_project_facts(cx);
        cx.notify();
    }

    /// Every project the Settings list offers the switch for: name, whether it
    /// is on, and its index. A run's own worktree is left out — it is not a
    /// project anybody chose, and it goes when the run does.
    pub fn unattended_choices(&self) -> Vec<(usize, SharedString, bool)> {
        self.window
            .workspace
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| !root.transient)
            .map(|(i, root)| (i, SharedString::from(root.label.clone()), root.unattended))
            .collect()
    }

    /// Every project root this window holds, as `(path, label)`.
    ///
    /// For the bridge, which has to know what there is before it can go looking
    /// on disk for what was said in it.
    pub fn remote_roots(&self) -> Vec<(PathBuf, String)> {
        self.window
            .workspace
            .roots
            .iter()
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
        let idx = self
            .window
            .workspace
            .roots
            .iter()
            .position(|candidate| candidate.path == root)?;
        self.select_root(idx, window, cx);
        self.start_session(agent, Some(archive), window, cx)
    }

    /// The project roots an unattended run may look for issues in, in the order
    /// the rail draws them — so pinning a project is also how it is worked
    /// first. Only the ones the user opted in, and never a run's own worktree.
    pub fn unattended_roots(&self) -> Vec<crate::unattended::Project> {
        let roots = &self.window.workspace.roots;
        self.window
            .workspace
            .display_order()
            .into_iter()
            .filter(|&i| roots[i].unattended && !roots[i].transient)
            .map(|i| self.project_for_runs(&roots[i].path))
            .collect()
    }

    /// `root` as unattended runs see it: the project, and the file its own
    /// issues are kept in, if this workspace keeps any.
    pub fn project_for_runs(&self, root: &Path) -> crate::unattended::Project {
        crate::unattended::Project {
            root: root.to_path_buf(),
            issues: self.issues_file(root),
        }
    }

    /// Where `root`'s own issues are kept, if this workspace keeps anything.
    pub fn issues_file(&self, root: &Path) -> Option<PathBuf> {
        let storage = self.window.workspace.storage_dir.as_deref()?;
        Some(onehand_core::issues::file_for(storage, root))
    }

    /// Add `dir` as a transient project and start `spec` on it, off screen,
    /// answering with the session and whether the run added the project.
    ///
    /// **Nothing the user is looking at moves.** The root is added without
    /// being selected and the pane connects the session without showing it.
    /// The root is transient, so the workspace file never holds it.
    ///
    /// When `dir` is already a project here — a run's worktree a person kept,
    /// coming back for a repair — the session is added to it and the project
    /// is left as it is: marking a root somebody kept as transient would
    /// quietly drop it from their workspace.
    pub fn run_unattended(
        &mut self,
        dir: PathBuf,
        spec: AgentSpec,
        cx: &mut Context<Self>,
    ) -> Option<(u64, Entity<crate::chat::session::ChatSession>, bool)> {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let workspace = &mut self.window.workspace;
        let (idx, owns_root) = match workspace.add_transient_root(dir.clone(), spec.clone(), uid) {
            Some(idx) => (idx, true),
            None => (
                workspace.add_unshown_session(dir, spec.clone(), uid)?,
                false,
            ),
        };
        let root = self.window.workspace.roots[idx].path.clone();
        let session = self
            .chat
            .update(cx, |pane, cx| pane.open_unshown(uid, root, &spec, cx));
        self.refresh_git(cx);
        cx.notify();
        Some((uid, session?, owns_root))
    }

    /// End a run's session and drop its project.
    ///
    /// Not `remove_root`, whose two-click guard asks a person whether live
    /// sessions should be lost — the run has already decided — and which puts
    /// the active session back on screen afterwards, taking the caret with it.
    /// A run ending while somebody types elsewhere must not move either. Only
    /// when the run's own project was the one being looked at is there anything
    /// to show instead. The worktree stays on disk; only the row goes.
    pub fn end_unattended(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.root_index(dir) else {
            return;
        };
        let was_active = self.window.workspace.active_root == idx;
        self.forget_root(idx, cx);
        if was_active {
            self.show_active_session(window, cx);
        }
        cx.notify();
    }

    /// Keep a run's project for good: a person has taken the run over, so it
    /// is their project now, and it is written into the workspace.
    pub fn adopt_unattended(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self
            .window
            .workspace
            .roots
            .iter_mut()
            .find(|r| r.path == dir)
        else {
            return;
        };
        root.transient = false;
        self.save_workspace(window, cx);
    }

    /// Open the list of `root_idx`'s open issues, to pick one to work now.
    ///
    /// The list is read off the UI loop and the dialog is up while it is:
    /// saying "reading" in the place the list will be is better than a menu
    /// entry that does nothing visible for the seconds `gh` takes.
    pub fn begin_pick(&mut self, root_idx: usize, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let (path, label) = (root.path.clone(), SharedString::from(root.label.clone()));
        let issues = self.issues_file(&path);
        self.issue_picker = Some(IssuePicker {
            root: path.clone(),
            project: label,
            found: None,
        });
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let found = {
                let path = path.clone();
                cx.background_executor()
                    .spawn(async move { crate::unattended::pickable_blocking(&path, issues) })
                    .await
            };
            shell
                .update(cx, |shell: &mut Self, cx| {
                    // Only the picker this read was for: one closed and opened
                    // on another project in the meantime is not its to fill.
                    if let Some(picker) = shell.issue_picker.as_mut().filter(|p| p.root == path) {
                        picker.found = Some(std::rc::Rc::new(found));
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// The picker on screen, if one is.
    pub fn issue_picker(&self) -> Option<&IssuePicker> {
        self.issue_picker.as_ref()
    }

    /// Work the `index`th issue in the picker's list, now, and close it.
    pub fn pick_issue(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.issue_picker.take() else {
            return;
        };
        cx.notify();
        let Some(found) = picker.found else {
            return;
        };
        let Ok((rows, _, _)) = &*found else {
            return;
        };
        let Some((tracker, row)) = rows.get(index).cloned() else {
            return;
        };
        let handle = window.window_handle();
        if let Err(why) = crate::unattended::start_picked(picker.root, tracker, row, handle, cx) {
            window.push_notification(Notification::warning(why), cx);
        }
    }

    /// Close the picker without working anything.
    pub fn cancel_pick(&mut self, cx: &mut Context<Self>) {
        if self.issue_picker.take().is_some() {
            cx.notify();
        }
    }

    /// Every session this window is running, for the bridge that has to describe
    /// them to somebody who is not looking at the window.
    ///
    /// A pass-through rather than a second walk of the workspace tree: the pane
    /// is where a session's running state actually lives, and the tree's
    /// `Session` is its description rather than its condition.
    pub fn remote_sessions(&self, cx: &App) -> Vec<crate::remote::RemoteSession> {
        self.chat.read(cx).remote_sessions(cx)
    }

    /// Send a prompt that arrived from outside the app to `uid`.
    ///
    /// `None` means this window does not hold that session, which is how the
    /// bridge finds the window that does without keeping a map of its own — a
    /// map that would have to be corrected every time a session is opened,
    /// closed or restarted, and would be wrong in between.
    pub fn remote_prompt(
        &mut self,
        uid: u64,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Option<crate::remote::Handled> {
        self.chat
            .update(cx, |pane, cx| pane.remote_prompt(uid, text, cx))
    }

    /// The pickers `uid`'s agent offers, for a chat that wants to change one.
    ///
    /// `None` for a session this window does not hold, the same handshake the
    /// other remote paths use.
    pub fn remote_options(
        &self,
        uid: u64,
        cx: &App,
    ) -> Option<(String, Vec<Vec<onehand_core::remote::types::Button>>)> {
        self.chat.read(cx).remote_options(uid, cx)
    }

    /// Cancel the turn running on `uid`, from outside the app.
    ///
    /// `None` for a session this window does not hold, the same handshake the
    /// other remote paths use.
    pub fn remote_stop(&mut self, uid: u64, cx: &mut Context<Self>) -> Option<String> {
        self.chat.update(cx, |pane, cx| pane.remote_stop(uid, cx))
    }

    /// Answer a permission or a question from outside the app.
    ///
    /// `None` for a session this window does not hold, the same handshake the
    /// prompt path uses to find the right window.
    pub fn remote_answer(
        &mut self,
        press: onehand_core::remote::Press,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.chat
            .update(cx, |pane, cx| pane.remote_answer(press, cx))
    }
}
