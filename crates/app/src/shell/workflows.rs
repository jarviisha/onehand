//! Starting a task from the launcher, and driving one once its place is
//! free.

use super::Shell;
use crate::state::Shared;
use gpui::{AppContext as _, Context, Entity, ParentElement as _, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::notification::Notification;
use gpui_component::{Icon, IconName, Sizable as _};
use onehand_core::task::marks::{self, Against};
use onehand_core::task::{Source, Task};
use onehand_core::workflow::{self as core, Brief, Place, Run, Setup, Template};
use onehand_core::worktree;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

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
        let brief = brief(&launcher.title, &launcher.body, &launcher.instructions, cx);
        if brief.title.is_empty() {
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
        let check = crate::task::task(&id, cx).is_some_and(|task| task.source == Source::Check);
        if check {
            let handle = window.window_handle();
            // Deferred, as the driver is below: the check reaches into the
            // global the shell may be reading from.
            cx.defer(move |cx| crate::task::drive_check(id, handle, cx));
            return;
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
        let check = self
            .window
            .workspace
            .roots
            .iter()
            .find(|r| r.path == root)
            .and_then(|r| r.check.clone());
        let Some(check) = check else {
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
        };
        let id = onehand_core::task::new_id();
        crate::task::add(Task::check(id.clone(), check, setup), cx);
        crate::task::request(id, window, cx);
    }

    /// Ask whether to retry task `id`, saying where the new run starts, what
    /// it carries over, and whether the work moved since the last run left
    /// it. A check is retried at once: it has one step and keeps nothing.
    pub fn begin_retry(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = crate::task::task(&id, cx) else {
            return;
        };
        let Some(last) = task.runs.last() else {
            return;
        };
        // From its one step: a check that passed would otherwise start past it.
        if task.source == Source::Check {
            let first = last.template.steps.first().map(|step| step.id.as_str());
            if crate::task::retry(&id, last.template.clone(), first, cx) {
                crate::task::request(id, window, cx);
            }
            return;
        }
        // A later save of the template the last run took, which reads and
        // may run.
        let newer = crate::workflow::templates(cx)
            .into_iter()
            .filter_map(|entry| entry.template.ok())
            .find(|t| t.newer_than(&last.template) && core::validate(t).is_empty());
        let (dir, end) = (last.setup.dir.clone(), last.last_mark().map(str::to_string));
        cx.spawn_in(window, async move |shell, cx| {
            let against = match end {
                Some(end) => cx
                    .background_executor()
                    .spawn(async move { marks::against_blocking(&dir, &end) })
                    .await
                    .inspect_err(|why| eprintln!("onehand: the work was not read: {why}"))
                    .ok(),
                None => None,
            };
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                shell.confirm_retry(task, newer, against, window, cx)
            });
        })
        .detach();
    }

    fn confirm_retry(
        &mut self,
        task: Task,
        newer: Option<Template>,
        against: Option<Against>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Against::OtherBranch(branch)) = &against {
            window.push_notification(
                Notification::warning(format!(
                    "Check out {branch} again to retry: the last run worked on it"
                )),
                cx,
            );
            return;
        }
        let Some(last) = task.runs.last().cloned() else {
            return;
        };
        let same = last.template.clone();
        let steps = same.steps.clone();
        let start = Run::retry_start(&last, &same);
        let picked = Rc::new(Cell::new(Run::retry_offered(&last, &same)));
        let choices: Vec<SharedString> = steps
            .iter()
            .take(start + 1)
            .map(|step| step.label.clone().into())
            .collect();
        // Where a retry on `template` from step `from` starts, and how many
        // answers it carries.
        let starts = move |template: &Template, from: Option<&str>| {
            let (at, carried) = Run::retry_plan(&last, template, from);
            let step = template.steps.get(at).map_or_else(
                || "the end".to_string(),
                |s| format!("the {} step", s.label),
            );
            (step, carried)
        };
        let changed = against == Some(Against::Changed);
        let (id, title) = (task.id.clone(), task.brief.title.clone());
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let from = steps.get(picked.get()).map(|step| step.id.clone());
            let (step, carried) = starts(&same, from.as_deref());
            let mut said = match carried {
                0 => format!("It starts at {step}."),
                1 => format!("It starts at {step}, carrying over 1 answer."),
                n => format!("It starts at {step}, carrying over {n} answers."),
            };
            if changed {
                said.push_str(" The work changed since the last run stopped.");
            }
            let retry = {
                let (shell, id, same, from) =
                    (shell.clone(), id.clone(), same.clone(), from.clone());
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    window.close_dialog(cx);
                    retry_now(&shell, id.clone(), same.clone(), from.clone(), window, cx);
                }
            };
            let with_newer = newer.clone().map(|template| {
                let at = starts(&template, from.as_deref()).0;
                let (shell, id, from) = (shell.clone(), id.clone(), from.clone());
                crate::controls::action("retry-newer")
                    .label(format!(
                        "Retry with the newer workflow (version {}), from {at}",
                        template.version
                    ))
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        window.close_dialog(cx);
                        retry_now(
                            &shell,
                            id.clone(),
                            template.clone(),
                            from.clone(),
                            window,
                            cx,
                        );
                    })
            });
            let menu = (choices.len() > 1).then(|| {
                let (choices, picked, shell) = (choices.clone(), picked.clone(), shell.clone());
                let at = choices.get(picked.get()).cloned().unwrap_or_default();
                crate::controls::menu_below(
                    "retry-from",
                    crate::controls::action("retry-from-trigger")
                        .small()
                        .label(format!("From {at}"))
                        .icon(Icon::new(IconName::ChevronDown)),
                    move |mut menu, _, _| {
                        for (i, label) in choices.iter().enumerate() {
                            let (picked, shell) = (picked.clone(), shell.clone());
                            menu = menu.item(
                                crate::controls::menu_item(label.clone())
                                    .checked(i == picked.get())
                                    .on_click(move |_, _, cx: &mut gpui::App| {
                                        picked.set(i);
                                        // The dialog is drawn by the shell.
                                        shell.update(cx, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        menu
                    },
                )
            });
            alert
                .title(format!("Retry {title}?"))
                .description(said)
                .children(menu)
                .footer(
                    gpui_component::dialog::DialogFooter::new()
                        .child(
                            gpui_component::dialog::DialogClose::new().child(
                                crate::controls::action("retry-cancel")
                                    .ghost()
                                    .label("Cancel"),
                            ),
                        )
                        .children(with_newer)
                        .child(
                            crate::controls::action("retry-confirm")
                                .primary()
                                .label("Retry")
                                .on_click(retry),
                        ),
                )
        });
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

/// Give task `id` a new run of `template`, from step `from` when that is
/// earlier than where it would start, and ask for its place.
fn retry_now(
    shell: &Entity<Shell>,
    id: String,
    template: Template,
    from: Option<String>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    shell.update(cx, |_, cx| {
        if crate::task::retry(&id, template, from.as_deref(), cx) {
            crate::task::request(id, window, cx);
        }
    });
}
