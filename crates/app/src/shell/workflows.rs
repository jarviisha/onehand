//! Starting a task from the launcher, and driving one once its place is
//! free.

use super::Shell;
use crate::state::Shared;
use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputState, TextareaState};
use onehand_core::preflight;
use onehand_core::task::tasks::{Begin, SESSION_NOT_STARTED};
use onehand_core::task::{Launch, Task};
use onehand_core::workflow::{self as core, Brief, Failure, Setup, Template};
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
    /// Whether the project is in git, the forge serving it and its place,
    /// `None` while they are being found out; the cached git status cannot
    /// say, since it is empty until its first read lands.
    pub ground: Option<Ground>,
}

/// What the launcher learns of its project off the UI loop.
pub struct Ground {
    pub in_git: bool,
    pub forge: Option<&'static dyn onehand_core::connector::Connector>,
    /// The place a run working on the checkout would wait for.
    pub place: std::path::PathBuf,
}

impl Shell {
    pub fn workflow_launcher(&self) -> Option<&WorkflowLauncher> {
        self.workflow_launcher.as_ref()
    }

    /// Put the launcher up for the project on screen.
    pub fn begin_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            window.push_notification(
                crate::toast::warning("Add a project before running a workflow"),
                cx,
            );
            return;
        };
        let Some(root) = root
            .local_path()
            .filter(|_| root.runs_here())
            .map(std::path::Path::to_path_buf)
        else {
            window.push_notification(
                crate::toast::warning(format!(
                    "{} runs its sessions on a host; workflows do not run there yet",
                    root.label
                )),
                cx,
            );
            return;
        };
        self.begin_workflow_on(&root, None, window, cx);
    }

    /// Put the launcher up for the project at `root`, the workflow of id
    /// `workflow` picked, or the first that reads.
    pub(crate) fn begin_workflow_on(
        &mut self,
        root: &std::path::Path,
        workflow: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self
            .root_index(root)
            .map(|i| &self.window.workspace.roots[i])
        else {
            return;
        };
        let (root, project) = (root.path.clone(), root.label.clone());
        self.remember_focus(window, cx);
        let entries = crate::workflow::templates(cx);
        let named = |entry: &crate::workflow::Entry| {
            entry
                .template
                .as_ref()
                .is_ok_and(|t| workflow.is_none_or(|id| t.id == id))
        };
        let template = entries.iter().position(named).unwrap_or(0);
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
                        Ground {
                            in_git: worktree::repo_top_blocking(&root).is_some(),
                            forge: crate::unattended::connector_for(&root).ok(),
                            place: onehand_core::task::queue::place_blocking(&root),
                        }
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
        let workflow = launcher_workflow(launcher, cx);
        let ground = launcher.ground.as_ref();
        Some(self.run_preflight(&launcher.root, workflow, ground, None, None, cx))
    }

    /// What the preflight finds of a new task of `workflow` on `root`, with
    /// `ground` once it is known: the launcher's question, asked by whatever
    /// starts one.
    pub(crate) fn run_preflight(
        &self,
        root: &std::path::Path,
        workflow: Result<Template, String>,
        ground: Option<&Ground>,
        agent: Option<&str>,
        mode: Option<&str>,
        cx: &gpui::App,
    ) -> Vec<preflight::Finding> {
        let shared = Shared::global(cx);
        let git = self.window.workspace.git().get(root);
        // Not known yet blocks nothing: the cut says so itself if it fails.
        let (in_git, forge) = ground.map_or((true, None), |g| (g.in_git, g.forge));
        // Who holds the place, said only for a workflow that works there.
        let queued_behind = match (&workflow, ground) {
            (Ok(template), Some(ground)) => preflight::queued_behind_for(
                template.place,
                crate::task::holder_at(&ground.place, None, cx),
            ),
            _ => None,
        };
        let named = agent;
        let agent = onehand_core::config::agent_or_first(&shared.settings.agents, named);
        let facts = preflight::Facts {
            workflow,
            // One named and gone is said by its name, as a Retry's is.
            agent: agent
                .map(|spec| spec.name.clone())
                .or(named.map(str::to_string)),
            agent_configured: agent.is_some(),
            mode: mode.map(str::to_string),
            offered: agent.and_then(|spec| crate::unattended::offered(spec, cx)),
            has_check: self.check_of(root).is_some(),
            in_git,
            checked_out: git.map(|git| git.branch.clone()),
            forge: forge.map(|forge| crate::unattended::forge_facts(forge, cx)),
            issue: None,
            slots: None,
            queued_behind,
            review: None,
            shared_checkout: self.own_session_in(root, cx),
            forge_read: ground.is_some(),
        };
        preflight::preflight(preflight::Kind::NewRun, &facts)
    }

    /// A session of a person's own, not a run's, that has been prompted in
    /// the checkout at `root`, by its name: what a run working in the same
    /// checkout would edit beside.
    fn own_session_in(&self, root: &std::path::Path, cx: &gpui::App) -> Option<String> {
        let project = &self.window.workspace.roots[self.root_index(root)?];
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
        let Some(launcher) = self.workflow_launcher.as_ref() else {
            return;
        };
        let found = self.launcher_preflight(cx).unwrap_or_default();
        let launch = Launch::new(
            &found,
            launcher_workflow(launcher, cx),
            brief(&launcher.title, &launcher.body, &launcher.instructions, cx),
            &launcher.root,
            self.check_of(&launcher.root),
            first_agent(cx),
        );
        let (busy, root) = (launcher.busy, launcher.root.clone());
        let launch = match launch {
            Ok(launch) => launch,
            Err(why) => {
                if let Some(launcher) = self.workflow_launcher.as_mut() {
                    launcher.error = Some(why);
                }
                cx.notify();
                return;
            }
        };
        if busy {
            return;
        }
        if !launch.cuts() {
            self.workflow_launcher = None;
            return self.start_launch(launch, window, cx);
        }
        if let Some(launcher) = self.workflow_launcher.as_mut() {
            launcher.busy = true;
            launcher.error = None;
        }
        cx.notify();
        let title = launch.title().to_string();
        cx.spawn_in(window, async move |shell, cx| {
            let made = cx
                .background_executor()
                .spawn(async move { cut_worktree_blocking(&root, &title) })
                .await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match made {
                Ok((dir, branch, forge)) => {
                    shell.workflow_launcher = None;
                    let (_, fx) = shell.window.workspace.keep_project(dir.clone());
                    shell.carry_workspace(fx, window, cx);
                    let launch = launch.on_worktree(dir, branch, forge);
                    shell.start_launch(launch, window, cx);
                }
                // The launcher stays up with what was written in it, and the
                // failure is said once.
                Err(why) => {
                    if let Some(launcher) = shell.workflow_launcher.as_mut() {
                        launcher.busy = false;
                    }
                    window.push_notification(crate::toast::failed("Making the worktree", why), cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Keep the new task `launch` describes, then ask for its place: kept
    /// first, so one waiting for its place survives a restart, as
    /// interrupted.
    fn start_launch(&mut self, launch: Launch, window: &mut Window, cx: &mut Context<Self>) {
        let task = launch.task(onehand_core::task::new_id());
        crate::task::submit(task, window.window_handle(), cx);
    }

    /// A new task of `template` on `setup`, as the launcher starts one, for a
    /// task started from outside the window:
    /// driven off screen, so nothing the person is looking at moves, and
    /// answering where it landed. Returns its id.
    pub(crate) fn start_workflow_told(
        &mut self,
        setup: Setup,
        template: Template,
        brief: Brief,
        given: (onehand_core::task::Origin, onehand_core::task::Grant),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        String,
        futures::channel::oneshot::Receiver<crate::task::Placed>,
    ) {
        // A worktree cut for it is a project of the workspace, as the
        // launcher's is: kept, listed, and there to come back to; but not
        // selected, as nothing the person is looking at moves for a task
        // they did not start.
        let fx = self
            .window
            .workspace
            .keep_project_quietly(setup.dir.clone());
        self.carry_workspace(fx, window, cx);
        let (origin, grant) = given;
        let task = Task {
            origin: Some(origin),
            grant,
            ..new_task(setup, template, brief, cx)
        };
        let id = task.id.clone();
        crate::task::drive_off_screen(&id, cx);
        (
            id,
            crate::task::submit_told(task, window.window_handle(), cx),
        )
    }

    /// Drive task `id`, whose place it now holds, as the tasks' drive begins
    /// it: show its project, adding it back if it left the workspace, start a
    /// session on it, and hand both to the run's carrier, which starts the
    /// task's last run or carries it on from the step it was at. One that
    /// cannot start is said here, and the drive decides what comes of it.
    /// Says why when it did not start.
    pub fn drive_task(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let refused =
            |why: String, then: Unstarted, window: &mut Window, cx: &mut Context<Self>| {
                window.push_notification(crate::toast::warning(why.clone()), cx);
                // Deferred: handing the place on may start a task in this shell.
                let id = id.clone();
                cx.defer(move |cx| match then {
                    Unstarted::Refused(why, kind) => crate::task::not_started(id, why, kind, cx),
                    Unstarted::NoSession { agent_gone } => {
                        crate::task::session_not_started(id, agent_gone, cx)
                    }
                });
                Err(why)
            };
        let there = crate::task::resumable_run(&id, cx).is_some_and(|run| run.setup.dir.is_dir());
        let (start, off_screen, issue) = match crate::task::begin(&id, there, cx) {
            Begin::Refused { why, kind } => {
                return refused(why.clone(), Unstarted::Refused(why, kind), window, cx);
            }
            Begin::Check(start) => {
                let handle = window.window_handle();
                // Deferred, as the run's carrier is below: the check reaches
                // into the global the shell may be reading from.
                cx.defer(move |cx| crate::task::start_check(id, start, handle, cx));
                return Ok(());
            }
            Begin::Session {
                start,
                off_screen,
                issue,
            } => (start, off_screen, issue),
        };
        let Some(task) = crate::task::task(&id, cx) else {
            return Err("That task is gone".to_string());
        };
        let dir = start.run.setup.dir.clone();
        let mut agent_gone = false;
        let session = match off_screen {
            // Nothing the person is looking at moves: an issue's run stays
            // where the person is, the issue saying the run is starting and
            // leading to its session; a tool's task is on the Tasks page and
            // its session in the rail, and the caret stays where it was.
            true => {
                let spec = crate::unattended::spec_for(start.run.setup.agent.as_deref(), cx);
                agent_gone = spec.is_none();
                let started = spec.and_then(|spec| self.run_unattended(dir, spec, Vec::new(), cx));
                if let (Some((_, session)), true) = (&started, issue) {
                    crate::unattended::opening(&task, session, cx);
                }
                started
            }
            false => {
                let (idx, fx) = self.window.workspace.keep_project(dir);
                self.carry_workspace(fx, window, cx);
                self.select_root(idx, window, cx);
                let agent = start.run.setup.agent.clone().map(SharedString::from);
                self.start_session(agent, None, window, cx)
                    .and_then(|uid| Some((uid, self.chat.read(cx).session_entity(uid)?)))
            }
        };
        let Some((uid, session)) = session else {
            let why = SESSION_NOT_STARTED.to_string();
            return refused(why, Unstarted::NoSession { agent_gone }, window, cx);
        };
        // Every drive, a Resume's new session included: what the task was
        // given when it started is what its session may use.
        crate::control::grant(uid, &task.grant, cx);
        // Deferred: the run's carrier reaches into the session and the global
        // the shell is reading from while this runs.
        let handle = window.window_handle();
        cx.defer(move |cx| crate::task::start(uid, &session, handle, id, start, cx));
        Ok(())
    }

    /// Run the check command of the project at `root` as a task of its own,
    /// once its place is free.
    pub fn run_check(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(check) = self.check_of(&root) else {
            window.push_notification(
                crate::toast::warning("This project has no check command to run"),
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
        crate::task::submit(Task::check(id, check, setup), window.window_handle(), cx);
    }
}

/// What the tasks' drive is told of a task that did not start.
enum Unstarted {
    /// It was refused before a session was asked for.
    Refused(String, Failure),
    /// Its session did not start, its agent gone from Settings or not.
    NoSession { agent_gone: bool },
}

/// A new task of `template` on `setup`, with the agent `setup` names, or the
/// one a new session takes.
fn new_task(setup: Setup, template: Template, brief: Brief, cx: &gpui::App) -> Task {
    let setup = onehand_core::task::on_agent(setup, first_agent(cx));
    Task::new(onehand_core::task::new_id(), template, brief, setup)
}

/// The agent a new session takes: the first configured.
fn first_agent(cx: &gpui::App) -> Option<String> {
    Shared::global(cx)
        .settings
        .agents
        .first()
        .map(|spec| spec.name.clone())
}

/// The workflow the launcher has picked, or why it cannot run.
fn launcher_workflow(launcher: &WorkflowLauncher, cx: &gpui::App) -> Result<Template, String> {
    let entries = crate::workflow::templates(cx);
    match entries.get(launcher.template) {
        Some(entry) => entry
            .template
            .clone()
            .map_err(|why| format!("{} cannot be read: {why}", entry.name())),
        None => Err("pick a workflow".to_string()),
    }
}

/// A worktree of the repository `root` sits in, cut for a task titled
/// `title`: the folder the task works in, its branch, and the forge the
/// branch goes to. Blocking.
pub(crate) fn cut_worktree_blocking(
    root: &std::path::Path,
    title: &str,
) -> Result<(PathBuf, String, Option<String>), String> {
    // Git checks out repositories, not folders: the worktree is of the
    // repository the project sits in, put beside it, and the run works in the
    // same folder of it the project is.
    let top = worktree::repo_top_blocking(root)
        .ok_or_else(|| format!("{} is not in a git repository", root.display()))?;
    // ponytail: only local branches count here; a `workflow/` branch whose
    // worktree is gone but whose pull request is on the forge is reused. Ask
    // the forge as an issue's claim does if that is seen.
    let (branch, made) =
        worktree::cut_blocking(&top, &core::branch_for(title), "HEAD", |_| Ok(false))?;
    let dir = worktree::subtree_or(made, &top, root);
    // The forge the branch goes to, for a step on it; none, and the branch is
    // the result.
    let forge = crate::unattended::connector_for(&top)
        .ok()
        .map(|forge| forge.name().to_string());
    Ok((dir, branch, forge))
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
