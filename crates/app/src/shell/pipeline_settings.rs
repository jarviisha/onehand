//! Settings ▸ Pipelines, as the shell keeps it: the template form, saving
//! and deleting templates, and each project's check command field.

use super::Shell;
use crate::settings::PipelineDraft;
use gpui::{App, AppContext as _, Context, Entity, ParentElement as _, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::pipeline::{self as core, StepKind, Template};
use std::path::PathBuf;

impl Shell {
    pub fn pipeline_draft(&self) -> Option<&PipelineDraft> {
        self.pipeline_draft.as_ref()
    }

    /// Open the form on a new template, one agent step to start from.
    pub fn new_pipeline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.open_pipeline_form(template, None, window, cx);
    }

    /// Open the form on a copy of template `at`, to be saved as a new one.
    pub fn duplicate_pipeline(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(mut template)) = crate::pipeline::templates(cx)
            .get(at)
            .map(|entry| entry.template.clone())
        else {
            return;
        };
        template.name = format!("{} copy", template.name);
        self.open_pipeline_form(template, None, window, cx);
    }

    /// Open the form on the person's own template `at`.
    pub fn edit_pipeline(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = crate::pipeline::templates(cx).get(at).cloned() else {
            return;
        };
        let (Ok(template), Some(file)) = (entry.template, entry.file) else {
            return;
        };
        self.open_pipeline_form(template, Some(file), window, cx);
    }

    /// Put `template`, kept in `file`, in the form, asking first when the
    /// form holds changes not saved: opening another template over them
    /// would throw them away as surely as closing Settings does.
    fn open_pipeline_form(
        &mut self,
        template: Template,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pipeline_draft.as_ref().is_some_and(|d| d.dirty(cx)) {
            self.pipeline_draft = Some(PipelineDraft::load(&template, file, window, cx));
            cx.notify();
            return;
        }
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (shell, template, file) = (shell.clone(), template.clone(), file.clone());
            alert
                .title("Discard the template being edited?")
                .description(
                    "Its changes are not saved. Opening another template throws them away.",
                )
                .footer(
                    gpui_component::dialog::DialogFooter::new()
                        .child(
                            gpui_component::dialog::DialogClose::new().child(
                                crate::controls::action("keep-pipeline-edit")
                                    .ghost()
                                    .label("Keep editing"),
                            ),
                        )
                        .child(
                            crate::controls::action("discard-pipeline-edit")
                                .danger()
                                .label("Discard")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    let (template, file) = (template.clone(), file.clone());
                                    shell.update(cx, |shell: &mut Self, cx| {
                                        shell.pipeline_draft =
                                            Some(PipelineDraft::load(&template, file, window, cx));
                                        cx.notify();
                                    });
                                }),
                        ),
                )
        });
    }

    /// Change the open form with `edit`.
    pub fn edit_pipeline_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut PipelineDraft, &mut Window, &mut gpui::App),
    ) {
        if let Some(draft) = self.pipeline_draft.as_mut() {
            edit(draft, window, cx);
        }
        cx.notify();
    }

    pub fn clear_pipeline_draft(&mut self, cx: &mut Context<Self>) {
        self.pipeline_draft = None;
        cx.notify();
    }

    /// Write the form to its file, or to a new one, and read the templates
    /// again. Refused while the template has problems, or shares its name
    /// with another, since the launcher lists them by name.
    pub fn save_pipeline_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.pipeline_draft.as_ref() else {
            return;
        };
        let Ok(template) = draft.to_template(cx) else {
            return;
        };
        if !core::validate(&template).is_empty() {
            return;
        }
        let file = draft.file.clone();
        let same = |entry: &crate::pipeline::Entry| file.is_some() && entry.file == file;
        let clash = crate::pipeline::templates(cx)
            .iter()
            .any(|entry| !same(entry) && entry.name() == template.name);
        if clash {
            let why = format!("another template is already called {}", template.name);
            self.report_write("Pipeline", Err(why), true, window, cx);
            cx.notify();
            return;
        }
        cx.spawn_in(window, async move |shell, cx| {
            let kept = template.clone();
            let saved = cx
                .background_executor()
                .spawn(async move {
                    core::store::save_blocking(&core::store::dir(), file.as_deref(), &kept)
                })
                .await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                match saved {
                    Ok(path) => {
                        if let Some(draft) = shell.pipeline_draft.as_mut() {
                            draft.file = Some(path);
                            draft.original = template;
                        }
                        shell.report_write("Pipeline", Ok::<(), String>(()), true, window, cx);
                        crate::pipeline::reload_templates(cx);
                    }
                    Err(why) => shell.report_write("Pipeline", Err(why), true, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask, then delete the person's own template `at`.
    pub fn confirm_delete_pipeline(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = crate::pipeline::templates(cx).get(at).cloned() else {
            return;
        };
        let Some(file) = entry.file.clone() else {
            return;
        };
        let name = entry.name();
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (shell, file) = (shell.clone(), file.clone());
            alert
                .title(format!("Delete {name}?"))
                .description(
                    "The template's file is deleted. Runs already started from it carry on \
                     with the copy they took.",
                )
                .footer(
                    gpui_component::dialog::DialogFooter::new()
                        .child(
                            gpui_component::dialog::DialogClose::new().child(
                                crate::controls::action("keep-pipeline")
                                    .ghost()
                                    .label("Keep"),
                            ),
                        )
                        .child(
                            crate::controls::action("delete-pipeline-confirm")
                                .danger()
                                .label("Delete")
                                .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                                    window.close_dialog(cx);
                                    let file = file.clone();
                                    shell.update(cx, |shell: &mut Self, cx| {
                                        shell.delete_pipeline_file(file, window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    /// Delete the template file `file`, off the UI thread, and read the
    /// templates again.
    fn delete_pipeline_file(&mut self, file: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
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
                        .pipeline_draft
                        .as_ref()
                        .is_some_and(|d| d.file.as_ref() == Some(&file))
                {
                    shell.pipeline_draft = None;
                }
                shell.report_write("Pipeline", gone, true, window, cx);
                crate::pipeline::reload_templates(cx);
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
