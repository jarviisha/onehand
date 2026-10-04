//! Starting a task from the launcher, and driving one once its place is
//! free.

use super::Shell;
use crate::state::Shared;
use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::notification::Notification;
use onehand_core::task::Task;
use onehand_core::workflow::{self as core, Brief, Place, Setup, Template};
use onehand_core::worktree;
use std::path::PathBuf;

/// The launcher's fields, while it is on screen.
pub struct WorkflowLauncher {
    /// The project it runs on, and what the dialog calls it.
    pub root: PathBuf,
    pub project: String,
    /// The template picked, by position among those on offer.
    pub template: usize,
    pub title: Entity<InputState>,
    pub body: Entity<TextareaState>,
    pub instructions: Entity<TextareaState>,
    /// Why the last press of *Run* did not start anything.
    pub error: Option<String>,
    /// A worktree is being made for the run.
    pub busy: bool,
}

impl Shell {
    pub fn workflow_launcher(&self) -> Option<&WorkflowLauncher> {
        self.workflow_launcher.as_ref()
    }

    /// Put the launcher up for the project on screen.
    pub fn begin_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            window.push_notification(
                Notification::warning("Add a project before running a workflow"),
                cx,
            );
            return;
        };
        let (root, project) = (root.path.clone(), root.label.clone());
        let template = crate::workflow::templates(cx)
            .iter()
            .position(|entry| entry.template.is_ok())
            .unwrap_or(0);
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("What to do, in a line"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("The details: what is wrong, what is wanted")
        });
        let instructions = cx
            .new(|cx| TextareaState::new(window, cx).placeholder("Optional: asked of every step"));
        title.update(cx, |input, cx| input.focus(window, cx));
        self.workflow_launcher = Some(WorkflowLauncher {
            root,
            project: project.to_string(),
            template,
            title,
            body,
            instructions,
            error: None,
            busy: false,
        });
        cx.notify();
    }

    pub fn pick_workflow_template(&mut self, template: usize, cx: &mut Context<Self>) {
        if let Some(launcher) = self.workflow_launcher.as_mut() {
            launcher.template = template;
            launcher.error = None;
        }
        cx.notify();
    }

    /// Put the launcher away, unless a worktree is being made for its run:
    /// that cannot be called back, and a run with nowhere to report would be
    /// a folder made for nothing.
    pub fn cancel_workflow(&mut self, cx: &mut Context<Self>) {
        if self.workflow_launcher.as_ref().is_some_and(|l| !l.busy) {
            self.workflow_launcher = None;
            cx.notify();
        }
    }

    /// Start the run the launcher describes, or say on it why not.
    pub fn commit_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(launcher) = self.workflow_launcher.as_ref().filter(|l| !l.busy) else {
            return;
        };
        let started = self.launch_parts(launcher, cx);
        let (template, brief, check) = match started {
            Ok(parts) => parts,
            Err(why) => {
                if let Some(launcher) = self.workflow_launcher.as_mut() {
                    launcher.error = Some(why);
                }
                cx.notify();
                return;
            }
        };
        let root = launcher.root.clone();
        let setup = Setup {
            repo: root.clone(),
            dir: root.clone(),
            branch: None,
            agent: None,
            check,
        };
        match template.place {
            Place::Checkout => {
                self.workflow_launcher = None;
                self.start_workflow(setup, template, brief, window, cx);
            }
            Place::Worktree => {
                if let Some(launcher) = self.workflow_launcher.as_mut() {
                    launcher.busy = true;
                    launcher.error = None;
                }
                cx.notify();
                let title = brief.title.clone();
                cx.spawn_in(window, async move |shell, cx| {
                    let made = cx
                        .background_executor()
                        .spawn(async move {
                            // Git checks out repositories, not folders: the
                            // worktree is of the repository the project sits
                            // in, put beside it, and the run works in the same
                            // folder of it the project is.
                            let top = worktree::repo_top_blocking(&root).ok_or_else(|| {
                                format!("{} is not in a git repository", root.display())
                            })?;
                            let branch = onehand_core::unattended::free_branch_blocking(
                                &top,
                                &core::branch_for(&title),
                            );
                            let dir = worktree::worktree_dir(&top, &branch);
                            let made = worktree::branch_off_blocking(&top, &branch, &dir, "HEAD")?;
                            let subtree = worktree::subtree_in(&made, &top, &root);
                            let dir = if subtree.is_dir() { subtree } else { made };
                            Ok::<_, String>((dir, branch))
                        })
                        .await;
                    let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match made {
                        Ok((dir, branch)) => {
                            shell.workflow_launcher = None;
                            shell.window.workspace.add_root(dir.clone());
                            shell.refresh_git(cx);
                            shell.save_workspace(window, cx);
                            let setup = Setup {
                                dir,
                                branch: Some(branch),
                                ..setup
                            };
                            shell.start_workflow(setup, template, brief, window, cx);
                        }
                        Err(why) => {
                            if let Some(launcher) = shell.workflow_launcher.as_mut() {
                                launcher.busy = false;
                                launcher.error = Some(format!("The worktree was not made: {why}"));
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// The template, the brief and the project's check command the launcher
    /// describes, or why it cannot run.
    fn launch_parts(
        &self,
        launcher: &WorkflowLauncher,
        cx: &Context<Self>,
    ) -> Result<(Template, Brief, Option<String>), String> {
        let entries = crate::workflow::templates(cx);
        let entry = entries
            .get(launcher.template)
            .ok_or_else(|| "Pick a workflow".to_string())?;
        let template = entry
            .template
            .clone()
            .map_err(|why| format!("{} cannot be read: {why}", entry.name()))?;
        let problems = core::validate(&template);
        if !problems.is_empty() {
            let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return Err(format!("This workflow cannot run: {}", said.join("; ")));
        }
        let title = launcher.title.read(cx).value().trim().to_string();
        if title.is_empty() {
            return Err("Say what to do in the title".to_string());
        }
        let check = self
            .window
            .workspace
            .roots
            .iter()
            .find(|root| root.path == launcher.root)
            .and_then(|root| root.check.clone());
        if template.needs_check() && check.is_none() {
            return Err(format!(
                "This template runs the project's check command, and {} has none. Set one \
                 under Settings ▸ Workflows.",
                launcher.project
            ));
        }
        let instructions = launcher.instructions.read(cx).value().trim().to_string();
        let brief = Brief {
            title,
            body: launcher.body.read(cx).value().trim().to_string(),
            instructions: (!instructions.is_empty()).then_some(instructions),
        };
        Ok((template, brief, check))
    }

    /// Keep a new task of `template` on the project at `setup.dir`, then ask
    /// for its place: kept first, so one waiting for its place survives a
    /// restart, as interrupted.
    fn start_workflow(
        &mut self,
        mut setup: Setup,
        template: Template,
        brief: Brief,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        setup.agent = Shared::global(cx)
            .agents
            .first()
            .map(|spec| spec.name.clone());
        let id = onehand_core::task::new_id();
        crate::task::add(Task::new(id.clone(), template, brief, setup), cx);
        crate::task::request(id, window, cx);
    }

    /// Drive task `id`, whose place it now holds: show its project, adding
    /// it back if it left the workspace, start a session on it, and hand both
    /// to the driver, which starts the task's last run or carries it on from
    /// the step it was at. One that cannot start stays as it was, and its
    /// place passes on.
    pub fn drive_task(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let refused = |why: String, window: &mut Window, cx: &mut Context<Self>| {
            window.push_notification(Notification::warning(why), cx);
            // Deferred: handing the place on may start a task in this shell.
            let id = id.clone();
            cx.defer(move |cx| crate::task::release(id, cx));
        };
        let Some(mut run) = crate::task::resumable_run(&id, cx) else {
            return refused("That task has nothing left to run".to_string(), window, cx);
        };
        let dir = run.setup.dir.clone();
        if !dir.is_dir() {
            return refused(
                format!("The folder that task works in is gone: {}", dir.display()),
                window,
                cx,
            );
        }
        let idx = match self.root_index(&dir) {
            Some(idx) => idx,
            None => {
                let idx = self.window.workspace.add_root(dir);
                self.refresh_git(cx);
                self.save_workspace(window, cx);
                idx
            }
        };
        self.select_root(idx, window, cx);
        let agent = run.setup.agent.clone().map(SharedString::from);
        let session = self
            .start_session(agent, None, window, cx)
            .and_then(|uid| Some((uid, self.chat.read(cx).session_entity(uid)?)));
        let Some((uid, session)) = session else {
            return refused("The task's session did not start".to_string(), window, cx);
        };
        let pinned = run.boundaries().len();
        let first = run.resume();
        // Deferred: the driver reaches into the session and the global the
        // shell is reading from while this runs.
        cx.defer(move |cx| crate::task::start(uid, &session, id, run, first, pinned, cx));
    }
}
