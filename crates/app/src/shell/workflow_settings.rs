//! Settings ▸ Workflows, as the shell keeps it: the template form, saving
//! and deleting templates, and each project's check command field.

use super::Shell;
use super::confirm::Ask;
use crate::settings::WorkflowDraft;
use gpui::{App, AppContext as _, Context, Entity, ParentElement as _, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::workflow::{self as core, StepKind, Template};
use std::path::PathBuf;

impl Shell {
    pub fn workflow_draft(&self) -> Option<&WorkflowDraft> {
        self.workflow_draft.as_ref()
    }

    /// Open the form on a new template, one agent step to start from.
    pub fn new_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut template = Template::blank("");
        template.steps.push(core::StepSpec {
            id: "plan".to_string(),
            label: "Plan".to_string(),
            kind: StepKind::Agent {
                prompt: "{brief}".to_string(),
                gates: vec![core::GateKind::Answered],
                keep_answer: true,
            },
        });
        self.open_workflow_form(template, None, window, cx);
    }

    /// Open the form on a copy of template `at`, to be saved as a new one.
    pub fn duplicate_workflow(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(mut template)) = crate::workflow::templates(cx)
            .get(at)
            .map(|entry| entry.template.clone())
        else {
            return;
        };
        template.name = format!("{} copy", template.name);
        self.open_workflow_form(template, None, window, cx);
    }

    /// Ask where to, then write template `at` to a file of the person's
    /// choosing, shipped ones included.
    pub fn export_workflow(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = crate::workflow::templates(cx).get(at).cloned() else {
            return;
        };
        let Ok(template) = entry.template else {
            return;
        };
        let name = core::store::export_name(&template);
        cx.spawn_in(window, async move |shell, cx| {
            // The native dialog blocks until it is answered, so it runs off
            // the UI thread, as does the write.
            let written = cx
                .background_executor()
                .spawn(async move {
                    let path = rfd::FileDialog::new().set_file_name(name).save_file()?;
                    Some(core::store::export_blocking(&path, &template))
                })
                .await;
            let Some(written) = written else {
                return;
            };
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                shell.report_write("Exported workflow", written, true, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask for a template file, then open the form on it as a new workflow:
    /// nothing is written until it is saved, which gives it an id of its own.
    pub fn import_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |shell, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let path = rfd::FileDialog::new()
                        .add_filter("Workflow", &["toml"])
                        .pick_file()?;
                    Some(
                        core::store::read_blocking(&path)
                            .map_err(|why| format!("{} cannot be read: {why}", path.display())),
                    )
                })
                .await;
            let Some(read) = read else {
                return;
            };
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                match read {
                    Ok(template) => shell.open_workflow_form(template, None, window, cx),
                    Err(why) => window.push_notification(
                        gpui_component::notification::Notification::error(format!(
                            "Workflow not imported — {why}"
                        )),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the form on the person's own template `at`.
    pub fn edit_workflow(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = crate::workflow::templates(cx).get(at).cloned() else {
            return;
        };
        let (Ok(template), Some(file)) = (entry.template, entry.file) else {
            return;
        };
        self.open_workflow_form(template, Some(file), window, cx);
    }

    /// Put `template`, kept in `file`, in the form, asking first when the
    /// form holds changes not saved: opening another template over them
    /// would throw them away as surely as closing Settings does.
    fn open_workflow_form(
        &mut self,
        template: Template,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workflow_draft.as_ref().is_some_and(|d| d.dirty(cx)) {
            self.workflow_draft = Some(WorkflowDraft::load(&template, file, window, cx));
            cx.notify();
            return;
        }
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (shell, template, file) = (shell.clone(), template.clone(), file.clone());
            alert
                .title("Discard the workflow being edited?")
                .description(
                    "Its changes are not saved. Opening another workflow throws them away.",
                )
                .footer(
                    gpui_component::dialog::DialogFooter::new()
                        .child(
                            gpui_component::dialog::DialogClose::new().child(
                                crate::controls::action("keep-workflow-edit")
                                    .ghost()
                                    .label("Keep editing"),
                            ),
                        )
                        .child(
                            crate::controls::action("discard-workflow-edit")
                                .danger()
                                .label("Discard")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    let (template, file) = (template.clone(), file.clone());
                                    shell.update(cx, |shell: &mut Self, cx| {
                                        shell.workflow_draft =
                                            Some(WorkflowDraft::load(&template, file, window, cx));
                                        cx.notify();
                                    });
                                }),
                        ),
                )
        });
    }

    /// Change the open form with `edit`.
    pub fn edit_workflow_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut WorkflowDraft, &mut Window, &mut gpui::App),
    ) {
        if let Some(draft) = self.workflow_draft.as_mut() {
            edit(draft, window, cx);
        }
        cx.notify();
    }

    pub fn clear_workflow_draft(&mut self, cx: &mut Context<Self>) {
        self.workflow_draft = None;
        cx.notify();
    }

    /// Write the form to its file, or to a new one, and read the templates
    /// again. Refused while the template has problems, or shares its name
    /// with another, since the launcher lists them by name.
    pub fn save_workflow_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.workflow_draft.as_ref() else {
            return;
        };
        let Ok(template) = draft.to_template(cx) else {
            return;
        };
        if !core::validate(&template).is_empty() {
            return;
        }
        let file = draft.file.clone();
        // Which form this save is for: each form is made anew, with inputs of
        // its own, so its name input says which one it is.
        let form = draft.name.entity_id();
        let same = |entry: &crate::workflow::Entry| file.is_some() && entry.file == file;
        let clash = crate::workflow::templates(cx)
            .iter()
            .any(|entry| !same(entry) && entry.name() == template.name);
        if clash {
            let why = format!("another workflow is already called {}", template.name);
            self.report_write("Workflow", Err(why), true, window, cx);
            cx.notify();
            return;
        }
        cx.spawn_in(window, async move |shell, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    core::store::save_blocking(&core::store::dir(), file.as_deref(), &template)
                })
                .await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                match saved {
                    Ok((path, written)) => {
                        // Only the form that was saved learns its file: one
                        // opened on another template meanwhile would otherwise
                        // take this file, and its next save overwrite it.
                        if let Some(draft) = shell
                            .workflow_draft
                            .as_mut()
                            .filter(|d| d.name.entity_id() == form)
                        {
                            draft.file = Some(path);
                            draft.original = written;
                        }
                        shell.report_write("Workflow", Ok::<(), String>(()), true, window, cx);
                        crate::workflow::reload_templates(cx);
                    }
                    Err(why) => shell.report_write("Workflow", Err(why), true, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask, then delete the person's own template `at`.
    pub fn confirm_delete_workflow(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = crate::workflow::templates(cx).get(at).cloned() else {
            return;
        };
        let Some(file) = entry.file.clone() else {
            return;
        };
        let ask = Ask {
            id: "delete-workflow",
            title: format!("Delete {}?", entry.name()).into(),
            description: "The template's file is deleted. Runs already started from it carry \
                          on with the copy they took."
                .into(),
            act: "Delete",
        };
        self.ask(ask, window, cx, move |shell, window, cx| {
            shell.delete_workflow_file(file.clone(), window, cx)
        });
    }

    /// Delete the template file `file`, off the UI thread, and read the
    /// templates again.
    fn delete_workflow_file(&mut self, file: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |shell, cx| {
            let gone = {
                let file = file.clone();
                cx.background_executor()
                    .spawn(async move { core::store::delete_blocking(&file) })
                    .await
            };
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                if gone.is_ok()
                    && shell
                        .workflow_draft
                        .as_ref()
                        .is_some_and(|d| d.file.as_ref() == Some(&file))
                {
                    shell.workflow_draft = None;
                }
                shell.report_write("Workflow", gone, true, window, cx);
                crate::workflow::reload_templates(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A check command field for every project that has none yet. Each edit
    /// is written to its project and saved, debounced, as the workspace name
    /// is: there is no Save button beside it.
    pub(super) fn make_check_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let roots: Vec<_> = self
            .window
            .workspace
            .roots
            .iter()
            .filter(|root| !root.transient && !self.check_inputs.contains_key(&root.path))
            .map(|root| (root.path.clone(), root.check.clone().unwrap_or_default()))
            .collect();
        for (path, check) in roots {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("None: a command step that names none cannot run here")
                    .default_value(check)
            });
            let at = path.clone();
            cx.subscribe_in(
                &input,
                window,
                move |shell: &mut Self, state, event: &InputEvent, _, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let typed = state.read(cx).value().trim().to_string();
                    let check = (!typed.is_empty()).then_some(typed);
                    let Some(root) = shell
                        .window
                        .workspace
                        .roots
                        .iter_mut()
                        .find(|r| r.path == at)
                    else {
                        return;
                    };
                    if root.check != check {
                        root.check = check;
                        shell.workspace_note_wanted = true;
                        shell.save_workspace_soon(cx);
                    }
                },
            )
            .detach();
            self.check_inputs.insert(path, input);
        }
    }

    /// Each project of this workspace beside its check command field, once
    /// Settings has made them.
    pub fn check_inputs(&self) -> Vec<(SharedString, Entity<InputState>)> {
        self.window
            .workspace
            .roots
            .iter()
            .filter_map(|root| {
                let input = self.check_inputs.get(&root.path)?.clone();
                Some((root.label.clone().into(), input))
            })
            .collect()
    }
}
