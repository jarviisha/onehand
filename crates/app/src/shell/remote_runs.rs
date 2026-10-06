use super::{IssuePicker, Shell};
use crate::state::Shared;
use gpui::{App, AppContext as _, BorrowAppContext, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputState, TextareaState};
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

    /// Start `spec` on `dir`, off screen: on the project already open there,
    /// or on `dir` added as a transient project.
    ///
    /// **Nothing the user is looking at moves.** No root is selected and the
    /// pane connects the session without showing it. A root added here is
    /// transient, so the workspace file never holds it; one the user already
    /// had keeps what it was, since marking it transient would quietly drop it
    /// from their workspace.
    pub fn run_unattended(
        &mut self,
        dir: PathBuf,
        spec: AgentSpec,
        cx: &mut Context<Self>,
    ) -> Option<(u64, Entity<crate::chat::session::ChatSession>)> {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let workspace = &mut self.window.workspace;
        let idx = match workspace.add_session_quietly(&dir, spec.clone(), uid) {
            Some(idx) => idx,
            None => workspace.add_transient_root(dir, spec.clone(), uid)?,
        };
        let root = self.window.workspace.roots[idx].path.clone();
        let session = self
            .chat
            .update(cx, |pane, cx| pane.open_unshown(uid, root, &spec, cx));
        self.refresh_git(cx);
        cx.notify();
        Some((uid, session?))
    }

    /// End a run's session and drop its project, if the run added it.
    ///
    /// Not `remove_root`, whose modal asks a person whether live
    /// sessions should be lost — the run has already decided — and which puts
    /// the active session back on screen afterwards, taking the caret with it.
    /// A run ending while somebody types elsewhere must not move either. Only
    /// when the run's own project was the one being looked at is there anything
    /// to show instead. The worktree stays on disk; only the row goes.
    pub fn end_unattended(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        // Only a project the run added itself: one a person has open stays.
        let Some(idx) = self
            .root_index(dir)
            .filter(|&idx| self.window.workspace.roots[idx].transient)
        else {
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

    /// Open the list of `root_idx`'s open issues, to pick one to work now, or
    /// only issue `only` of the project's own when it is asked for from there.
    ///
    /// The list is read off the UI loop and the dialog is up while it is:
    /// saying "reading" in the place the list will be is better than a menu
    /// entry that does nothing visible for the seconds `gh` takes.
    pub fn begin_pick(
        &mut self,
        root_idx: usize,
        only: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let (path, label) = (root.path.clone(), SharedString::from(root.label.clone()));
        let issues = self.issues_file(&path);
        let instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Optional: added to what every step is asked")
        });
        self.issue_picker = Some(IssuePicker {
            root: path.clone(),
            project: label,
            found: None,
            workflow: None,
            only,
            chosen: None,
            instructions,
            preview: false,
        });
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let found = {
                let path = path.clone();
                cx.background_executor()
                    .spawn(async move {
                        match only {
                            Some(number) => {
                                crate::unattended::pickable_one_blocking(&path, issues, number)
                            }
                            None => crate::unattended::pickable_blocking(&path, issues),
                        }
                    })
                    .await
            };
            shell
                .update(cx, |shell: &mut Self, cx| {
                    // Only the picker this read was for: one closed and opened
                    // on another project in the meantime is not its to fill.
                    if let Some(picker) = shell.issue_picker.as_mut().filter(|p| p.root == path) {
                        // Narrowed to one, the issue is chosen already.
                        if only.is_some() && found.as_ref().is_ok_and(|(rows, ..)| rows.len() == 1)
                        {
                            picker.chosen = Some(0);
                        }
                        picker.found = Some(std::rc::Rc::new(found));
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Let issue workflow labels choose the picker's workflow (`None`), or
    /// choose workflow `id` for whatever is picked.
    pub fn pick_issue_workflow(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(picker) = self.issue_picker.as_mut() {
            picker.workflow = id;
            cx.notify();
        }
    }

    /// Make `default` the workflow issues are worked with and `by_label` the
    /// workflow labels, from Settings, saying in the window if the config
    /// could not be written.
    fn set_issue_workflows(
        &mut self,
        default: String,
        by_label: std::collections::BTreeMap<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = crate::state::Shared::global(cx).config_path.clone();
        let saved = crate::unattended::set_workflows(default, by_label, &path, cx);
        self.report_write("Unattended runs", saved, true, window, cx);
        cx.notify();
    }

    /// Work issues with workflow `id` unless a workflow label names another.
    pub fn set_default_issue_workflow(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (_, by_label) = crate::unattended::workflows(cx);
        self.set_issue_workflows(id, by_label, window, cx);
    }

    /// The workflow Settings' new workflow label is to name, `None` for none
    /// picked yet.
    pub fn label_workflow(&self) -> (Entity<InputState>, Option<&str>, Option<&str>) {
        (
            self.label_input.clone(),
            self.label_workflow.as_deref(),
            self.label_refused.as_deref(),
        )
    }

    /// Pick workflow `id` for Settings' new workflow label.
    pub fn pick_label_workflow(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.label_workflow = id;
        self.label_refused = None;
        cx.notify();
    }

    /// Add the workflow label typed in Settings, naming the workflow picked
    /// for it, or say why not: what core refuses as a workflow label, or no
    /// workflow picked.
    pub fn add_workflow_label(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.label_input.read(cx).value().trim().to_string();
        let (default, mut by_label) = crate::unattended::workflows(cx);
        let trigger = crate::unattended::label(cx);
        let refused = onehand_core::unattended::workflow_label_refused(&label, &trigger, &by_label)
            .or_else(|| {
                self.label_workflow
                    .is_none()
                    .then(|| "Pick the workflow it chooses.".to_string())
            });
        let id = match (refused, self.label_workflow.clone()) {
            (None, Some(id)) => id,
            (refused, _) => {
                self.label_refused = refused;
                cx.notify();
                return;
            }
        };
        self.label_refused = None;
        by_label.insert(label, id);
        self.label_workflow = None;
        self.label_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.set_issue_workflows(default, by_label, window, cx);
    }

    /// Drop workflow label `label`: its issues go back to the default.
    pub fn remove_workflow_label(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (default, mut by_label) = crate::unattended::workflows(cx);
        if by_label.remove(label).is_some() {
            self.set_issue_workflows(default, by_label, window, cx);
        }
    }

    /// The picker on screen, if one is.
    pub fn issue_picker(&self) -> Option<&IssuePicker> {
        self.issue_picker.as_ref()
    }

    /// Choose the `index`th issue in the picker's list, for the form below it.
    pub fn choose_issue(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(picker) = self.issue_picker.as_mut() {
            picker.chosen = Some(index);
            cx.notify();
        }
    }

    pub fn toggle_pick_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.issue_picker.as_mut() {
            picker.preview = !picker.preview;
            cx.notify();
        }
    }

    /// Work the issue chosen in the picker, now, and close it. The person
    /// stays where they were: the issue says the run is starting, and leads
    /// to its session.
    pub fn commit_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.issue_picker.take() else {
            return;
        };
        cx.notify();
        let (Some(found), Some(index)) = (picker.found, picker.chosen) else {
            return;
        };
        let Ok((rows, _, _)) = &*found else {
            return;
        };
        let Some((tracker, row)) = rows.get(index).cloned() else {
            return;
        };
        let handle = window.window_handle();
        let has_check = self.check_of(&picker.root).is_some();
        let workflow = picker
            .workflow
            .unwrap_or_else(|| crate::unattended::workflow_for(&row.labels, cx));
        let instructions = picker.instructions.read(cx).value().to_string();
        if let Err(why) = crate::unattended::start_picked(
            picker.root,
            tracker,
            row,
            workflow,
            instructions,
            has_check,
            handle,
            cx,
        ) {
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
