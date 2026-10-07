//! Starting a task from the launcher, and driving one once its place is
//! free.

use super::Shell;
use crate::state::Shared;
use gpui::{
    AppContext as _, Context, Entity, ParentElement as _, SharedString, Styled as _, Window,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::notification::Notification;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use onehand_core::preflight::{self, Check, found_late};
use onehand_core::task::marks::{self, Against};
use onehand_core::task::{Source, Task};
use onehand_core::workflow::{self as core, Brief, Failure, Place, Run, Setup, Template};
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
        };
        Some(preflight::preflight(preflight::Kind::NewRun, &facts))
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
            if crate::task::retry(&id, last.template.clone(), first, None, cx) {
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
        let changed = against == Some(Against::Changed);
        // Work that changed since the last run stopped is checked again before
        // anything past the check: what it passed on is not what is there now,
        // and a push past it would send the old commit.
        let mut start = Run::retry_start(&last, &same);
        if changed {
            start = Run::recheck(&same, start);
        }
        let picked = Rc::new(Cell::new(Run::retry_offered(&last, &same).min(start)));
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
        // Judged by the run's own setup, which is what a retry runs: what
        // Settings says now neither blocks it nor clears a block.
        let judge = |template: &Template, newer: bool| {
            let facts = crate::unattended::task_facts(&task, template.clone(), cx);
            preflight::preflight(preflight::Kind::Retry { newer }, &facts)
        };
        let mut found = judge(&same, false);
        let blocked = found.iter().any(|f| f.blocks);
        let newer_blocked = newer.as_ref().is_some_and(|template| {
            // What blocks the newer version alone, said once and named.
            let extra: Vec<_> = judge(template, true)
                .into_iter()
                .filter(|f| f.blocks && !found.contains(f))
                .map(|f| preflight::Finding {
                    text: format!("Version {}: {}", template.version, f.text),
                    ..f
                })
                .collect();
            let any = !extra.is_empty() || blocked;
            found.extend(extra);
            any
        });
        let (id, title) = (task.id.clone(), task.brief.title.clone());
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let (danger, muted) = (
                crate::theme::status_ink(cx).danger,
                cx.theme().muted_foreground,
            );
            let lines: Vec<_> = found
                .iter()
                .enumerate()
                .map(|(at, finding)| {
                    crate::dialogs::finding_line(at, finding, danger, muted, &shell)
                })
                .collect();
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
            // Said here rather than on its button, which would outgrow the
            // dialog with a long step name.
            if let Some(template) = &newer {
                let at = starts(template, from.as_deref()).0;
                said.push_str(&format!(
                    " The newer workflow, version {}, starts at {at}.",
                    template.version
                ));
            }
            let retry = {
                let (shell, id, same, from) =
                    (shell.clone(), id.clone(), same.clone(), from.clone());
                move |window: &mut Window, cx: &mut gpui::App| {
                    retry_now(&shell, id.clone(), same.clone(), from.clone(), window, cx);
                }
            };
            let with_newer = newer.clone().map(|template| {
                let (shell, id, from) = (shell.clone(), id.clone(), from.clone());
                let button = crate::controls::action("retry-newer")
                    .label(format!("Retry with version {}", template.version));
                if newer_blocked {
                    return crate::controls::resting(button).disabled(true);
                }
                button.on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
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
                .child(gpui::div().v_flex().gap_1().w_full().children(lines))
                // Enter is the dialog's confirm: it retries as the primary
                // button does, rather than closing with nothing done; while
                // something blocks it, the dialog stays.
                .on_ok({
                    let retry = retry.clone();
                    move |_, window, cx| {
                        if blocked {
                            return false;
                        }
                        retry(window, cx);
                        true
                    }
                })
                .footer(
                    // Wrapped, for a narrow window. Cancel closes through the
                    // library's close box, as every cancel here does; that box
                    // is full width, so a box of its own sized to the button
                    // keeps it on the row.
                    gpui_component::dialog::DialogFooter::new()
                        .flex_wrap()
                        .child(
                            gpui::div().flex_none().child(
                                gpui_component::dialog::DialogClose::new().child(
                                    crate::controls::action("retry-cancel")
                                        .ghost()
                                        .label("Cancel"),
                                ),
                            ),
                        )
                        .children(with_newer)
                        .child({
                            let confirm = crate::controls::action("retry-confirm")
                                .primary()
                                .label("Retry");
                            match blocked {
                                true => crate::controls::resting(confirm).disabled(true),
                                false => confirm.on_click(
                                    move |_, window: &mut Window, cx: &mut gpui::App| {
                                        window.close_dialog(cx);
                                        retry(window, cx);
                                    },
                                ),
                            }
                        }),
                )
        });
    }
}

impl Shell {
    /// Carry task `id` on where it stopped, once the preflight finds nothing
    /// in the way of its own setup; what blocks it is said in the window.
    pub(crate) fn resume_task(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let blocked = crate::task::task(&id, cx).and_then(|task| {
            let template = task.runs.last()?.template.clone();
            let facts = crate::unattended::task_facts(&task, template, cx);
            preflight::preflight(preflight::Kind::Resume, &facts)
                .into_iter()
                .find(|f| f.blocks)
        });
        match blocked {
            Some(block) => window.push_notification(
                Notification::warning(format!("Not resumed: {}", block.text)),
                cx,
            ),
            None => crate::task::request(id, window, cx),
        }
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
        if crate::task::retry(&id, template, from.as_deref(), None, cx) {
            crate::task::request(id, window, cx);
        }
    });
}
