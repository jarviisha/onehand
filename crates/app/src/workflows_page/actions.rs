//! What the Workflows page does to workflows: the form opened on a new,
//! duplicated, imported or own workflow, saving and deleting, and exporting.

use super::{WorkflowDraft, WorkflowsPage};
use gpui::{App, Context, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::notification::Notification;
use onehand_core::workflow::{self as core, StepKind, Template};
use std::path::PathBuf;

/// What a save says, done or not.
const SAVED: (&str, &str) = ("Workflow saved", "Workflow not saved");

impl WorkflowsPage {
    /// Open the form on a new template, one agent step to start from.
    pub(crate) fn new_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(crate) fn duplicate_workflow(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
    pub(crate) fn export_workflow(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = crate::workflow::templates(cx).get(at).cloned() else {
            return;
        };
        let Ok(template) = entry.template else {
            return;
        };
        let name = core::store::export_name(&template);
        cx.spawn_in(window, async move |page, cx| {
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
            let _ = page.update_in(cx, |page: &mut Self, window, cx| {
                page.report(
                    ("Workflow exported", "Workflow not exported"),
                    written,
                    window,
                    cx,
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask for a template file, then open the form on it as a new workflow:
    /// nothing is written until it is saved, which gives it an id of its own.
    pub(crate) fn import_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |page, cx| {
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
            let _ = page.update_in(cx, |page: &mut Self, window, cx| {
                match read {
                    Ok(template) => page.open_workflow_form(template, None, window, cx),
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
    pub(crate) fn edit_workflow(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        if !self.draft.as_ref().is_some_and(|d| d.dirty(cx)) {
            self.draft = Some(WorkflowDraft::load(&template, file, window, cx));
            cx.notify();
            return;
        }
        let page = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (page, template, file) = (page.clone(), template.clone(), file.clone());
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
                                    page.update(cx, |page: &mut Self, cx| {
                                        page.draft =
                                            Some(WorkflowDraft::load(&template, file, window, cx));
                                        cx.notify();
                                    });
                                }),
                        ),
                )
        });
    }

    /// Change the open form with `edit`.
    pub(crate) fn edit_workflow_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut WorkflowDraft, &mut Window, &mut gpui::App),
    ) {
        if let Some(draft) = self.draft.as_mut() {
            edit(draft, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn clear_workflow_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        cx.notify();
    }

    /// Write the form to its file, or to a new one, and read the templates
    /// again. Refused while the template has problems, or shares its name
    /// with another, since the launcher lists them by name.
    pub(crate) fn save_workflow_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_ref() else {
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
            self.report(SAVED, Err(why), window, cx);
            cx.notify();
            return;
        }
        cx.spawn_in(window, async move |page, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    core::store::save_blocking(&core::store::dir(), file.as_deref(), &template)
                })
                .await;
            let _ = page.update_in(cx, |page: &mut Self, window, cx| {
                match saved {
                    Ok((path, written)) => {
                        // Only the form that was saved learns its file: one
                        // opened on another template meanwhile would otherwise
                        // take this file, and its next save overwrite it.
                        if let Some(draft) =
                            page.draft.as_mut().filter(|d| d.name.entity_id() == form)
                        {
                            draft.file = Some(path);
                            draft.original = written;
                        }
                        page.report(SAVED, Ok::<(), String>(()), window, cx);
                        crate::workflow::reload_templates(cx);
                    }
                    Err(why) => page.report(SAVED, Err(why), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask, then delete the person's own template `at`.
    pub(crate) fn confirm_delete_workflow(
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
        let ask = crate::shell::Ask {
            id: "delete-workflow",
            title: format!("Delete {}?", entry.name()).into(),
            description: "The template's file is deleted. Runs already started from it carry \
                          on with the copy they took."
                .into(),
            act: "Delete",
            ..Default::default()
        };
        crate::shell::ask_on(cx.entity(), ask, window, cx, move |page, window, cx| {
            page.delete_workflow_file(file.clone(), window, cx)
        });
    }

    /// Delete the template file `file`, off the UI thread, and read the
    /// templates again.
    fn delete_workflow_file(&mut self, file: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |page, cx| {
            let gone = {
                let file = file.clone();
                cx.background_executor()
                    .spawn(async move { core::store::delete_blocking(&file) })
                    .await
            };
            let _ = page.update_in(cx, |page: &mut Self, window, cx| {
                if gone.is_ok()
                    && page
                        .draft
                        .as_ref()
                        .is_some_and(|d| d.file.as_ref() == Some(&file))
                {
                    page.draft = None;
                }
                page.report(
                    ("Workflow deleted", "Workflow not deleted"),
                    gone,
                    window,
                    cx,
                );
                crate::workflow::reload_templates(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Say how a write went, in a toast, by `said`'s two words: done, or
    /// not done and why. The page has no line of its own to say it on.
    fn report<E: std::fmt::Display>(
        &mut self,
        (done, not): (&str, &str),
        written: Result<(), E>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = match written {
            Ok(()) => Notification::success(done.to_string()),
            Err(why) => Notification::error(format!("{not} — {why}")),
        };
        window.push_notification(note, cx);
    }
}
