//! Starting a pipeline run from the launcher, and resuming one a previous
//! session left unfinished.

use super::Shell;
use crate::state::Shared;
use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::notification::Notification;
use onehand_core::pipeline::{self as core, Brief, PipelineRun, Place, Setup, StepKind, Template};
use onehand_core::worktree;
use std::path::PathBuf;

/// The launcher's fields, while it is on screen.
pub struct PipelineLauncher {
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
    pub fn pipeline_launcher(&self) -> Option<&PipelineLauncher> {
        self.pipeline_launcher.as_ref()
    }

    /// Put the launcher up for the project on screen.
    pub fn begin_pipeline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            window.push_notification(
                Notification::warning("Add a project before running a pipeline"),
                cx,
            );
            return;
        };
        let (root, project) = (root.path.clone(), root.label.clone());
        let template = crate::pipeline::templates(cx)
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
        self.pipeline_launcher = Some(PipelineLauncher {
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

    pub fn pick_pipeline_template(&mut self, template: usize, cx: &mut Context<Self>) {
        if let Some(launcher) = self.pipeline_launcher.as_mut() {
            launcher.template = template;
            launcher.error = None;
        }
        cx.notify();
    }

    /// Put the launcher away, unless a worktree is being made for its run:
    /// that cannot be called back, and a run with nowhere to report would be
    /// a folder made for nothing.
    pub fn cancel_pipeline(&mut self, cx: &mut Context<Self>) {
        if self.pipeline_launcher.as_ref().is_some_and(|l| !l.busy) {
            self.pipeline_launcher = None;
            cx.notify();
        }
    }

    /// Start the run the launcher describes, or say on it why not.
    pub fn commit_pipeline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(launcher) = self.pipeline_launcher.as_ref().filter(|l| !l.busy) else {
            return;
        };
        let started = self.launch_parts(launcher, cx);
        let (template, brief, check) = match started {
            Ok(parts) => parts,
            Err(why) => {
                if let Some(launcher) = self.pipeline_launcher.as_mut() {
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
                self.pipeline_launcher = None;
                self.start_pipeline(setup, template, brief, window, cx);
            }
            Place::Worktree => {
                if let Some(launcher) = self.pipeline_launcher.as_mut() {
                    launcher.busy = true;
                    launcher.error = None;
                }
                cx.notify();
                let title = brief.title.clone();
                cx.spawn_in(window, async move |shell, cx| {
                    let made = cx
                        .background_executor()
                        .spawn(async move {
                            let branch = onehand_core::unattended::free_branch_blocking(
                                &root,
                                &core::branch_for(&title),
                            );
                            let dir = worktree::worktree_dir(&root, &branch);
                            worktree::branch_off_blocking(&root, &branch, &dir, "HEAD")
                                .map(|made| (made, branch))
                        })
                        .await;
                    let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match made {
                        Ok((dir, branch)) => {
                            shell.pipeline_launcher = None;
                            shell.window.workspace.add_root(dir.clone());
                            shell.refresh_git(cx);
                            shell.save_workspace(window, cx);
                            let setup = Setup {
                                dir,
                                branch: Some(branch),
                                ..setup
                            };
                            shell.start_pipeline(setup, template, brief, window, cx);
                        }
                        Err(why) => {
                            if let Some(launcher) = shell.pipeline_launcher.as_mut() {
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
        launcher: &PipelineLauncher,
        cx: &Context<Self>,
    ) -> Result<(Template, Brief, Option<String>), String> {
        let entries = crate::pipeline::templates(cx);
        let entry = entries
            .get(launcher.template)
            .ok_or_else(|| "Pick a template".to_string())?;
        let template = entry
            .template
            .clone()
            .map_err(|why| format!("{} cannot be read: {why}", entry.name()))?;
        let problems = core::validate(&template);
        if !problems.is_empty() {
            let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return Err(format!("This template cannot run: {}", said.join("; ")));
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
        let wants_check = template.steps.iter().any(|step| match &step.kind {
            StepKind::Command { command, .. } => command.is_none(),
            StepKind::Agent { .. } | StepKind::Approval { .. } => false,
        });
        if wants_check && check.is_none() {
            return Err(format!(
                "This template runs the project's check command, and {} has none. Set one \
                 under Settings ▸ Pipelines.",
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

    /// Start a session on the project at `setup.dir` and drive a new run of
    /// `template` on it.
    fn start_pipeline(
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
        let id = core::files::new_id();
        let (run, first) = PipelineRun::begin(id, template, brief, setup);
        self.drive_pipeline(run, first, false, window, cx);
    }

    /// Resume the unfinished run `id`: its project is put on screen, added
    /// back if it left the workspace, and a new session carries on from the
    /// step it was at.
    pub fn resume_pipeline(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut run) = crate::pipeline::take_unfinished(id, cx) else {
            return;
        };
        if !run.setup.dir.is_dir() {
            window.push_notification(
                Notification::warning(format!(
                    "The folder that run worked in is gone: {}",
                    run.setup.dir.display()
                )),
                cx,
            );
            crate::pipeline::park(run, cx);
            return;
        }
        let first = run.resume();
        self.drive_pipeline(run, first, true, window, cx);
    }

    /// Show the run's project, start a session on it, and hand both to the
    /// driver. A `resumed` run that cannot start goes back on the unfinished
    /// list; a new one had nothing yet worth keeping.
    fn drive_pipeline(
        &mut self,
        run: PipelineRun,
        first: core::Action,
        resumed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = run.setup.dir.clone();
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
            window.push_notification(
                Notification::warning("The pipeline's session did not start"),
                cx,
            );
            if resumed {
                crate::pipeline::park(run, cx);
            }
            return;
        };
        // Deferred: the driver reaches into the session and the global the
        // shell is reading from while this runs.
        cx.defer(move |cx| crate::pipeline::start(uid, &session, run, first, cx));
    }
}
