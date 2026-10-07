//! Starting a task from the launcher, and driving one once its place is
//! free.

use super::Shell;
use crate::state::Shared;
use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::notification::Notification;
use onehand_core::preflight::{self, Check, found_late};
use onehand_core::task::{Source, Task};
use onehand_core::workflow::{self as core, Brief, Failure, Place, Setup, Template};
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
    /// The preview of what the run starts with is open.
    pub preview: bool,
    /// Whether the project is in git and the forge serving it, `None` while
    /// they are being found out; the cached git status cannot say, since it
    /// is empty until its first read lands.
    pub ground: Option<(
        bool,
        Option<&'static dyn onehand_core::connector::Connector>,
    )>,
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
            preview: false,
            ground: None,
        });
        cx.notify();
        let root = self.workflow_launcher.as_ref().map(|l| l.root.clone());
        cx.spawn(async move |shell, cx| {
            let Some(root) = root else {
                return;
            };
            let ground = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move {
                        let in_git = worktree::repo_top_blocking(&root).is_some();
                        (in_git, crate::unattended::connector_for(&root).ok())
                    }
                })
                .await;
            shell
                .update(cx, |shell: &mut Self, cx| {
                    // Only the launcher this read was for.
                    if let Some(launcher) =
                        shell.workflow_launcher.as_mut().filter(|l| l.root == root)
                    {
                        launcher.ground = Some(ground);
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    pub fn pick_workflow_template(&mut self, template: usize, cx: &mut Context<Self>) {
        if let Some(launcher) = self.workflow_launcher.as_mut() {
            launcher.template = template;
            launcher.error = None;
        }
        cx.notify();
    }

    pub fn toggle_workflow_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(launcher) = self.workflow_launcher.as_mut() {
            launcher.preview = !launcher.preview;
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

    /// What the preflight finds of the run the launcher describes.
    pub fn launcher_preflight(&self, cx: &gpui::App) -> Option<Vec<preflight::Finding>> {
        let launcher = self.workflow_launcher.as_ref()?;
        let entries = crate::workflow::templates(cx);
        let workflow = match entries.get(launcher.template) {
            Some(entry) => entry
                .template
                .clone()
                .map_err(|why| format!("{} cannot be read: {why}", entry.name())),
            None => Err("pick a workflow".to_string()),
        };
        let shared = Shared::global(cx);
        let git = self.window.git.get(&launcher.root);
        // Not known yet blocks nothing: the cut says so itself if it fails.
        let (in_git, forge) = launcher.ground.unwrap_or((true, None));
        let facts = preflight::Facts {
            workflow,
            agent: shared.agents.first().map(|spec| spec.name.clone()),
            agent_configured: !shared.agents.is_empty(),
            mode: None,
            offered: None,
            has_check: self.check_of(&launcher.root).is_some(),
            in_git,
            checked_out: git.map(|git| git.branch.clone()),
            forge: forge.map(|forge| crate::unattended::forge_facts(forge, cx)),
            issue: None,
            slots: None,
            queued_behind: None,
            review: None,
            shared_checkout: self.own_session_in(&launcher.root, cx),
        };
        Some(preflight::preflight(preflight::Kind::NewRun, &facts))
    }

    /// A session of a person's own, not a run's, that has been prompted in
    /// the checkout at `root`, by its name: what a run working in the same
    /// checkout would edit beside.
    fn own_session_in(&self, root: &std::path::Path, cx: &gpui::App) -> Option<String> {
        let project = self
            .window
            .workspace
            .roots
            .iter()
            .find(|r| r.path == root)?;
        project
            .sessions
            .iter()
            .filter(|session| crate::task::shown(session.uid, cx).is_none())
            .find_map(|session| {
                self.rail_sessions
                    .iter()
                    .find(|(uid, _)| *uid == session.uid)
                    .and_then(|(_, row)| row.title.clone())
            })
            .map(|title| title.to_string())
    }

    /// Start the run the launcher describes, or say on it why not.
    pub fn commit_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let blocked = self
            .launcher_preflight(cx)
            .and_then(|found| found.into_iter().find(|f| f.blocks));
        if let (Some(block), Some(launcher)) = (blocked, self.workflow_launcher.as_mut()) {
            launcher.error = Some(block.text);
            cx.notify();
            return;
        }
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
            mode: None,
            forge: None,
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
                            // ponytail: only local branches count here; a
                            // `workflow/` branch whose worktree is gone but
                            // whose pull request is on the forge is reused.
                            // Ask the forge as an issue's claim does if that
                            // is seen.
                            let branch = onehand_core::unattended::free_branch_blocking(
                                &top,
                                &core::branch_for(&title),
                                |_| Ok(false),
                            )?;
                            let dir = worktree::worktree_dir(&top, &branch);
                            let made = worktree::branch_off_blocking(&top, &branch, &dir, "HEAD")?;
                            let subtree = worktree::subtree_in(&made, &top, &root);
                            let dir = if subtree.is_dir() { subtree } else { made };
                            // The forge the branch goes to, for a step on it;
                            // none, and the branch is the result.
                            let forge = crate::unattended::connector_for(&top)
                                .ok()
                                .map(|forge| forge.name().to_string());
                            Ok::<_, String>((dir, branch, forge))
                        })
                        .await;
                    let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match made {
                        Ok((dir, branch, forge)) => {
                            shell.workflow_launcher = None;
                            shell.window.workspace.add_root(dir.clone());
                            shell.refresh_git(cx);
                            shell.save_workspace(window, cx);
                            let setup = Setup {
                                dir,
                                branch: Some(branch),
                                forge,
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
        // What blocks the workflow itself was judged by the preflight first.
        let template = entry
            .template
            .clone()
            .map_err(|why| format!("{} cannot be read: {why}", entry.name()))?;
        let brief = brief(&launcher.title, &launcher.body, &launcher.instructions, cx);
        if brief.title.is_empty() {
            return Err("Say what to do in the title".to_string());
        }
        Ok((template, brief, self.check_of(&launcher.root)))
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
        let refused = |why: String, kind: Failure, window: &mut Window, cx: &mut Context<Self>| {
            // An issue's run that cannot start ends failed, so its issue is
            // told rather than left claimed with nothing after the claim.
            let issue = crate::task::task(&id, cx).is_some_and(|task| task.issue().is_some());
            window.push_notification(Notification::warning(why.clone()), cx);
            // Deferred: handing the place on may start a task in this shell.
            let id = id.clone();
            cx.defer(move |cx| match issue {
                true => crate::task::fail(id, why, kind, cx),
                false => crate::task::release(id, cx),
            });
        };
        let Some(mut run) = crate::task::resumable_run(&id, cx) else {
            return refused(
                "That task has nothing left to run".to_string(),
                Failure::Other,
                window,
                cx,
            );
        };
        let dir = run.setup.dir.clone();
        if !dir.is_dir() {
            return refused(
                format!("The folder that task works in is gone: {}", dir.display()),
                Failure::Other,
                window,
                cx,
            );
        }
        let Some(task) = crate::task::task(&id, cx) else {
            return refused(
                "That task has nothing left to run".to_string(),
                Failure::Other,
                window,
                cx,
            );
        };
        if task.source == Source::Check {
            let handle = window.window_handle();
            // Deferred, as the driver is below: the check reaches into the
            // global the shell may be reading from.
            cx.defer(move |cx| crate::task::drive_check(id, handle, cx));
            return;
        }
        // A snapshot that no longer validates (a later build's rules) is what
        // the preflight would have blocked, found late.
        let problems = core::validate(&run.template);
        if !problems.is_empty() {
            let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return refused(
                format!(
                    "The workflow `{}` no longer validates: {}",
                    run.template.name,
                    said.join("; ")
                ),
                found_late(Check::Workflow),
                window,
                cx,
            );
        }
        let mut unstarted = Failure::Other;
        let session = match task.issue() {
            // An issue's run comes up off screen, on a project of its own that
            // the workspace file never holds unless it was kept, so nothing the
            // person is looking at moves. One picked by hand too: the person
            // stays on the issue, which says the run is starting and leads to
            // its session.
            Some(_) => {
                let spec = crate::unattended::spec_for(run.setup.agent.as_deref(), cx);
                // No agent of that name configured any more is what the
                // preflight would have blocked.
                if spec.is_none() {
                    unstarted = found_late(Check::Agent);
                }
                let started = spec.and_then(|spec| self.run_unattended(dir, spec, cx));
                if let Some((_, session)) = &started {
                    crate::unattended::opening(&task, session, cx);
                }
                started
            }
            None => {
                let idx = self.root_index(&dir).unwrap_or_else(|| {
                    let idx = self.window.workspace.add_root(dir);
                    self.refresh_git(cx);
                    self.save_workspace(window, cx);
                    idx
                });
                self.select_root(idx, window, cx);
                let agent = run.setup.agent.clone().map(SharedString::from);
                self.start_session(agent, None, window, cx)
                    .and_then(|uid| Some((uid, self.chat.read(cx).session_entity(uid)?)))
            }
        };
        let Some((uid, session)) = session else {
            return refused(
                "The task's session did not start".to_string(),
                unstarted,
                window,
                cx,
            );
        };
        // Counted from what landed, so a mark the app quit before pinning is
        // pinned now rather than taken as made.
        let pinned = run.pinned_count();
        let first = run.resume();
        // Deferred: the driver reaches into the session and the global the
        // shell is reading from while this runs.
        let handle = window.window_handle();
        cx.defer(move |cx| crate::task::start(uid, &session, handle, id, run, first, pinned, cx));
    }

    /// Run the check command of the project at `root` as a task of its own,
    /// once its place is free.
    pub fn run_check(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(check) = self.check_of(&root) else {
            window.push_notification(
                Notification::warning("This project has no check command to run"),
                cx,
            );
            return;
        };
        let setup = Setup {
            repo: root.clone(),
            dir: root,
            branch: None,
            agent: None,
            check: Some(check.clone()),
            mode: None,
            forge: None,
        };
        let id = onehand_core::task::new_id();
        crate::task::add(Task::check(id.clone(), check, setup), cx);
        crate::task::request(id, window, cx);
    }
}

/// The brief the launcher's fields say, as typed so far.
pub(crate) fn brief(
    title: &Entity<InputState>,
    body: &Entity<TextareaState>,
    instructions: &Entity<TextareaState>,
    cx: &gpui::App,
) -> Brief {
    let instructions = instructions.read(cx).value().trim().to_string();
    Brief {
        title: title.read(cx).value().trim().to_string(),
        body: body.read(cx).value().trim().to_string(),
        instructions: (!instructions.is_empty()).then_some(instructions),
    }
}
