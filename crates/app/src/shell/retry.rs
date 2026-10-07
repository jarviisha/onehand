//! Running a task again. *Retry* runs it as the last run was configured, and
//! says before it starts where the new run starts, what it carries over and
//! what it keeps; *Retry with current settings* runs it with what Settings
//! say now, and says first what changes and where that makes it start.
//! Resume, judged first by the run's own setup, is here too.

use super::Shell;
use gpui::{App, Context, Entity, ParentElement as _, SharedString, Styled as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::notification::Notification;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use onehand_core::preflight;
use onehand_core::task::marks::{self, Against};
use onehand_core::task::work::Act;
use onehand_core::task::{Source, Task};
use onehand_core::workflow::{Changed, Run, Template, WithCurrent};
use std::cell::Cell;
use std::rc::Rc;

/// What happens once the work has been compared with where the last run
/// left it.
type Then = fn(&mut Shell, Task, Option<Against>, &mut Window, &mut Context<Shell>);

impl Shell {
    /// Ask whether to retry task `id`, saying where the new run starts, what
    /// it carries over, what it keeps of the last run's configuration, and
    /// whether the work moved since the last run left it. A check is retried
    /// at once: it has one step and keeps nothing.
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
        self.compare_work(task, Self::confirm_retry, window, cx);
    }

    /// Ask whether to retry task `id` with what Settings say now, saying
    /// first what changes, where it starts and what blocks it.
    pub fn begin_retry_current(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = crate::task::task(&id, cx) else {
            return;
        };
        if task.source == Source::Check || task.runs.is_empty() {
            return self.begin_retry(id, window, cx);
        }
        self.compare_work(task, Self::confirm_retry_current, window, cx);
    }

    /// Compare the work with where `task`'s last run left it, off the UI
    /// thread, then `then`.
    fn compare_work(
        &mut self,
        task: Task,
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(last) = task.runs.last() else {
            return;
        };
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
                then(shell, task, against, window, cx)
            });
        })
        .detach();
    }

    /// Task `task`'s next run with what Settings say now, or why there is
    /// none: its own workflow by id at its newest version, the agent, mode
    /// and timeout a new task of its kind takes, and its project's check
    /// command.
    ///
    /// With the work `changed` since the last run stopped, it starts no later
    /// than where that is checked again.
    pub(crate) fn plan_current(
        &self,
        task: &Task,
        changed: bool,
        cx: &App,
    ) -> Option<Result<WithCurrent, String>> {
        let last = task.runs.last()?;
        let now = crate::unattended::now_for(task, self.check_of(&last.setup.repo), cx);
        Some(Run::with_current(
            last,
            crate::workflow::newest(&last.template, cx),
            &now,
            changed,
        ))
    }

    fn confirm_retry(
        &mut self,
        task: Task,
        against: Option<Against>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if refused_elsewhere(&against, window, cx) {
            return;
        }
        let Some(last) = task.runs.last().cloned() else {
            return;
        };
        let same = last.template.clone();
        let steps = same.steps.clone();
        let changed = against == Some(Against::Changed);
        let (start, offered) = Run::retry_offer(&last, &same, changed);
        let picked = Rc::new(Cell::new(offered));
        let choices: Vec<SharedString> = steps
            .iter()
            .take(start + 1)
            .map(|step| step.label.clone().into())
            .collect();
        // Where a retry from step `from` starts, and how many answers it
        // carries.
        let starts = {
            let last = last.clone();
            move |template: &Template, from: Option<&str>| {
                let (at, carried) = Run::retry_plan(&last, template, from);
                let step = template.steps.get(at).map_or_else(
                    || "the end".to_string(),
                    |s| format!("the {} step", s.label),
                );
                (step, carried)
            }
        };
        // Judged by the run's own setup, which is what a retry runs: what
        // Settings says now neither blocks it nor clears a block.
        let facts = crate::unattended::task_facts(&task, same.clone(), cx);
        let found = preflight::preflight(preflight::Kind::Retry, &facts);
        let blocked = found.iter().any(|f| f.blocks);
        // What it keeps, one line each, and where Settings say otherwise now.
        let differs: Vec<Changed> = match self.plan_current(&task, false, cx) {
            Some(Ok(plan)) => plan.changes,
            Some(Err(_)) | None => Vec::new(),
        };
        let kept = kept_lines(&last, &differs);
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
            let mut said = match (carried, picked.get() < start) {
                (0, true) => format!("It starts at {step}, the step you picked."),
                (0, false) => format!("It starts at {step}, the first it cannot carry over."),
                (1, _) => format!("It starts at {step}, carrying over 1 answer."),
                (n, _) => format!("It starts at {step}, carrying over {n} answers."),
            };
            if changed {
                said.push_str(" The work changed since the last run stopped.");
            }
            let retry = {
                let (shell, id, same, from) =
                    (shell.clone(), id.clone(), same.clone(), from.clone());
                move |window: &mut Window, cx: &mut App| {
                    retry_now(&shell, id.clone(), same.clone(), from.clone(), window, cx);
                }
            };
            let with_current = {
                let (shell, id) = (shell.clone(), id.clone());
                crate::controls::action("retry-current")
                    .label(Act::RetryCurrent.label())
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        window.close_dialog(cx);
                        let id = id.clone();
                        shell.update(cx, |shell, cx| shell.begin_retry_current(id, window, cx));
                    })
            };
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
                                    .on_click(move |_, _, cx: &mut App| {
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
            let warning = crate::theme::status_ink(cx).warning;
            let kept_view =
                gpui::div()
                    .v_flex()
                    .gap_0p5()
                    .w_full()
                    .text_xs()
                    .children(kept.iter().map(|(line, differs)| {
                        gpui::div()
                            .text_color(if *differs { warning } else { muted })
                            .child(line.clone())
                    }));
            alert
                .title(format!("Retry {title}?"))
                .description(said)
                .children(menu)
                .child(kept_view)
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
                .footer(footer(
                    "retry",
                    Some(with_current),
                    "Retry",
                    blocked,
                    move |window, cx| retry(window, cx),
                ))
        });
    }

    fn confirm_retry_current(
        &mut self,
        task: Task,
        against: Option<Against>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if refused_elsewhere(&against, window, cx) {
            return;
        }
        let Some(last) = task.runs.last().cloned() else {
            return;
        };
        let changed = against == Some(Against::Changed);
        let Some(plan) = self.plan_current(&task, changed, cx) else {
            return;
        };
        // Judged on what it will run: the setup Settings give now, on its
        // workflow at its newest version, or why there is none.
        let setup = match &plan {
            Ok(plan) => plan.setup.clone(),
            Err(_) => last.setup.clone(),
        };
        let workflow = plan
            .as_ref()
            .map(|plan| plan.template.clone())
            .map_err(Clone::clone);
        let facts = crate::unattended::setup_facts(&task, &setup, workflow, cx);
        let found = preflight::preflight(preflight::Kind::RetryCurrent, &facts);
        let blocked = found.iter().any(|f| f.blocks);
        let (said, changes) = match &plan {
            Ok(plan) => {
                let step = plan.template.steps.get(plan.start).map_or_else(
                    || "the end".to_string(),
                    |s| format!("the {} step", s.label),
                );
                let lines: Vec<String> = match plan.changes.is_empty() {
                    true => vec!["Nothing differs from the last run's configuration.".into()],
                    false => plan
                        .changes
                        .iter()
                        .map(|c| format!("{}: {} → {}", c.what, c.old, c.new))
                        .collect(),
                };
                (format!("It starts at {step}, {}.", plan.why.said()), lines)
            }
            Err(_) => (String::new(), Vec::new()),
        };
        let plan = plan.ok();
        let blocked = blocked || plan.is_none();
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
            let retry = {
                let (shell, id, plan) = (shell.clone(), id.clone(), plan.clone());
                move |window: &mut Window, cx: &mut App| {
                    let Some(plan) = plan.clone() else {
                        return;
                    };
                    let id = id.clone();
                    shell.update(cx, |_, cx| {
                        if crate::task::retry_with(&id, plan, cx) {
                            crate::task::request(id, window, cx);
                        }
                    });
                }
            };
            alert
                .title(format!("Retry {title} with current settings?"))
                .description(said.clone())
                .child(
                    gpui::div()
                        .v_flex()
                        .gap_0p5()
                        .w_full()
                        .text_xs()
                        .children(changes.iter().map(|line| gpui::div().child(line.clone()))),
                )
                .child(gpui::div().v_flex().gap_1().w_full().children(lines))
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
                .footer(footer(
                    "retry-current",
                    None,
                    preflight::RETRY_CURRENT,
                    blocked,
                    move |window, cx| retry(window, cx),
                ))
        });
    }
}

/// A last run that worked on another branch is retried only once that branch
/// is checked out again: said, and nothing opened.
fn refused_elsewhere(against: &Option<Against>, window: &mut Window, cx: &mut App) -> bool {
    let Some(Against::OtherBranch(branch)) = against else {
        return false;
    };
    window.push_notification(
        Notification::warning(format!(
            "Check out {branch} again to retry: the last run worked on it"
        )),
        cx,
    );
    true
}

/// What a *Retry* keeps of `last`'s configuration, one line each, and
/// whether Settings say otherwise now, which the line says.
fn kept_lines(last: &Run, differs: &[Changed]) -> Vec<(String, bool)> {
    let setup = &last.setup;
    let shown = |value: &Option<String>, none: &str| value.clone().unwrap_or(none.to_string());
    [
        ("Agent", shown(&setup.agent, "the first configured")),
        ("Mode", shown(&setup.mode, "as the agent starts")),
        ("Check command", shown(&setup.check, "none")),
        ("Timeout", last.template.timeout.clone()),
        (
            "Workflow version",
            format!("version {}", last.template.version),
        ),
    ]
    .into_iter()
    .map(
        |(what, kept)| match differs.iter().find(|c| c.what == what) {
            Some(now) => (
                format!(
                    "Keeps {}: {kept}; Settings now say {}, which {} runs",
                    what.to_lowercase(),
                    now.new,
                    preflight::RETRY_CURRENT
                ),
                true,
            ),
            None => (format!("Keeps {}: {kept}", what.to_lowercase()), false),
        },
    )
    .collect()
}

/// A retry dialog's footer: *Cancel*, an action beside, and the confirm,
/// spent while something blocks it. Wrapped, for a narrow window. Cancel
/// closes through the library's close box, as every cancel here does; that
/// box is full width, so a box of its own sized to the button keeps it on
/// the row.
fn footer(
    id: &'static str,
    beside: Option<gpui_component::button::Button>,
    confirm: &'static str,
    blocked: bool,
    run: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui_component::dialog::DialogFooter {
    gpui_component::dialog::DialogFooter::new()
        .flex_wrap()
        .child(
            gpui::div().flex_none().child(
                gpui_component::dialog::DialogClose::new().child(
                    crate::controls::action(SharedString::from(format!("{id}-cancel")))
                        .ghost()
                        .label("Cancel"),
                ),
            ),
        )
        .children(beside)
        .child({
            let button = crate::controls::action(SharedString::from(format!("{id}-confirm")))
                .primary()
                .label(confirm);
            match blocked {
                true => crate::controls::resting(button).disabled(true),
                false => button.on_click(move |_, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    run(window, cx);
                }),
            }
        })
}

impl Shell {
    /// Ask whether to answer the review on issue task `id`'s open pull
    /// request, as putting the trigger label back does: what would refuse it
    /// is read first, off the UI loop, and listed as its preflight found it,
    /// before anything is claimed.
    pub(crate) fn answer_review(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = crate::task::task(&id, cx) else {
            return;
        };
        let has_check = self.check_of(&task.setup.repo).is_some();
        let reading = crate::unattended::read_review(&id, cx);
        cx.spawn_in(window, async move |shell, cx| {
            let read = reading.await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match read {
                Ok(read) => shell.confirm_answer(task, read, has_check, window, cx),
                Err(why) => window.push_notification(Notification::warning(why), cx),
            });
        })
        .detach();
    }

    fn confirm_answer(
        &mut self,
        task: Task,
        read: crate::unattended::ReviewRead,
        has_check: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = preflight::preflight(preflight::Kind::AnswerReview, &read.facts);
        let blocked = found.iter().any(|f| f.blocks);
        let said = match &read.from {
            Some(step) => format!(
                "A new run starts at the {step} step, told how to read the review, on the \
                 task's own setup, as putting the label back does."
            ),
            None => "Its workflow has no step to answer a review from.".to_string(),
        };
        let shell = cx.entity();
        let read = Rc::new(Cell::new(Some(read)));
        let handle = window.window_handle();
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
            let answer = {
                let read = read.clone();
                move |window: &mut Window, cx: &mut App| {
                    let Some(read) = read.take() else {
                        return;
                    };
                    if let Err(why) = crate::unattended::start_answer(read, has_check, handle, cx) {
                        window.push_notification(Notification::warning(why), cx);
                    }
                }
            };
            alert
                .title(format!("Answer the review on {}?", task.brief.title))
                .description(said.clone())
                .child(gpui::div().v_flex().gap_1().w_full().children(lines))
                .on_ok({
                    let answer = answer.clone();
                    move |_, window, cx| {
                        if blocked {
                            return false;
                        }
                        answer(window, cx);
                        true
                    }
                })
                .footer(footer(
                    "answer-review",
                    None,
                    "Answer the review",
                    blocked,
                    move |window, cx| answer(window, cx),
                ))
        });
    }

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

/// Give task `id` a new run of `template`, from step `from` when that is
/// earlier than where it would start, and ask for its place.
fn retry_now(
    shell: &Entity<Shell>,
    id: String,
    template: Template,
    from: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    shell.update(cx, |_, cx| {
        if crate::task::retry(&id, template, from.as_deref(), None, cx) {
            crate::task::request(id, window, cx);
        }
    });
}
