//! What the Workflows page does to workflows: picking one, which opens it;
//! the form opened on a new, duplicated or imported one; saving, deleting
//! and exporting the one open.

use super::pick::{self, Pick};
use super::{WorkflowDraft, WorkflowsPage};
use gpui::{Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::workflow::{self as core, StepKind, Template};
use std::path::PathBuf;

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

    /// The entry `pick` names, as last read.
    fn entry_of(&self, pick: &Pick, cx: &gpui::App) -> Option<crate::workflow::Entry> {
        let entries = crate::workflow::templates(cx);
        let at = pick::repick(&entries, pick)?;
        entries.get(at).cloned()
    }

    /// Open the form on a copy of the workflow `pick` names, as last saved,
    /// to be saved as a new one; the new one being written is copied as it
    /// was opened.
    pub(super) fn duplicate(&mut self, pick: &Pick, window: &mut Window, cx: &mut Context<Self>) {
        let template = match pick {
            Pick::New => self.draft.as_ref().map(|d| d.original.clone()),
            _ => self
                .entry_of(pick, cx)
                .and_then(|entry| entry.template.ok()),
        };
        let Some(mut template) = template else {
            return;
        };
        template.name = format!("{} copy", template.name);
        self.open_workflow_form(template, None, window, cx);
    }

    /// Ask where to, then write the workflow `pick` names, as saved, to a
    /// file of the person's choosing, shipped ones included.
    pub(super) fn export(&mut self, pick: &Pick, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_of(pick, cx) else {
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
            let _ = page.update_in(cx, |_: &mut Self, window, cx| {
                crate::toast::report(
                    window,
                    cx,
                    "Exporting the workflow",
                    Some("Workflow exported"),
                    written,
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
                    Err(why) => window
                        .push_notification(crate::toast::failed("Importing the workflow", why), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The entry the page has picked, as last read.
    pub(super) fn picked_entry(&self, cx: &gpui::App) -> Option<crate::workflow::Entry> {
        self.entry_of(self.picked.as_ref()?, cx)
    }

    /// Open `pick` on the page, asking first when the form holds changes
    /// not saved. A shipped workflow opens read-only; one of the person's
    /// own that cannot be read opens no form, only why.
    pub(super) fn pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if self.picked.as_ref() == Some(&pick) {
            return;
        }
        self.unless_editing(
            "Opening another workflow",
            window,
            cx,
            move |page, window, cx| {
                let entries = crate::workflow::templates(cx);
                let entry = pick::repick(&entries, &pick).and_then(|at| entries.get(at).cloned());
                page.draft = entry.and_then(|entry| {
                    let shipped = entry.file.is_none();
                    let template = entry.template.ok()?;
                    Some(WorkflowDraft::load(
                        &template, entry.file, shipped, window, cx,
                    ))
                });
                page.picked = Some(pick.clone());
                page.hold_focus(window, cx);
                cx.notify();
            },
        );
    }

    /// Open the step `by` steps from the one open, held at the
    /// first and the last; nothing while no step is open.
    pub(super) fn step_open(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_open_step(window, cx, |d| {
            let last = d.steps.len().saturating_sub(1);
            d.open_step = d.open_step.map(|at| at.saturating_add_signed(by).min(last));
        });
    }

    /// Put `template`, a new workflow, in the form, asking first when the
    /// form holds changes not saved: opening another template over them
    /// would throw them away.
    fn open_workflow_form(
        &mut self,
        template: Template,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unless_editing(
            "Opening another workflow",
            window,
            cx,
            move |page, window, cx| {
                page.draft = Some(WorkflowDraft::load(
                    &template,
                    file.clone(),
                    false,
                    window,
                    cx,
                ));
                page.picked = Some(Pick::New);
                page.hold_focus(window, cx);
                cx.notify();
            },
        );
    }

    /// Put the keyboard on the page itself: the row that took the click
    /// leaves the frame when the editor shows alone, and a focus left on a
    /// handle no longer drawn takes the page's keys with it.
    fn hold_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }

    /// Esc: the innermost thing open goes, the open step before the editor.
    pub(super) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = self.draft.as_mut().filter(|d| d.open_step.is_some()) {
            draft.open_step = None;
            cx.notify();
            return;
        }
        self.close_workflow_form(window, cx);
    }

    /// Close the form, asking first when it holds changes.
    pub(crate) fn close_workflow_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unless_editing("Closing the form", window, cx, |page, _, cx| {
            page.clear_workflow_draft(cx)
        });
    }

    /// Do `then`, asking first when the form holds changes not saved, which
    /// `doing` would throw away.
    fn unless_editing(
        &mut self,
        doing: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.dirty(cx) {
            then(self, window, cx);
            return;
        }
        crate::shell::confirm_on(
            cx.entity(),
            crate::shell::Confirm {
                id: "discard-workflow-edit",
                title: "Discard the workflow being edited?".into(),
                description: format!("Its changes are not saved. {doing} throws them away.").into(),
                act: "Discard",
                ..Default::default()
            },
            window,
            cx,
            then,
        );
    }

    /// Change the open form with `edit`.
    pub(crate) fn edit_workflow_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut WorkflowDraft, &mut Window, &mut gpui::App),
    ) {
        // A shipped workflow is read, never changed: duplicating it is how.
        if let Some(draft) = self.draft.as_mut().filter(|d| !d.shipped) {
            edit(draft, window, cx);
        }
        cx.notify();
    }

    /// Open or close a step of the form with `edit`: reading a step is no
    /// change to it, so a shipped workflow's open too.
    pub(super) fn edit_open_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut WorkflowDraft),
    ) {
        if let Some(draft) = self.draft.as_mut() {
            let was = draft.open_step;
            edit(draft);
            let now = draft.open_step;
            if now != was {
                // A field of the step that was open may hold the keyboard,
                // and it leaves the frame with its step.
                self.hold_focus(window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn clear_workflow_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.picked = None;
        cx.notify();
    }

    /// Write the form to its file, or to a new one, and read the templates
    /// again. Refused while the template has problems, or shares its name
    /// with another, since the launcher lists them by name.
    pub(crate) fn save_workflow_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // One write at a time: *Save* spins until this one lands.
        if self.writing {
            return;
        }
        let Some(draft) = self.draft.as_ref() else {
            return;
        };
        let Ok(template) = draft.to_template(cx) else {
            return;
        };
        if !core::validate(&template).is_empty() {
            return;
        }
        // The form says a clash above *Save*, which refuses while it stands.
        if name_clash(draft, cx).is_some() {
            return;
        }
        let file = draft.file.clone();
        // Which form this save is for: each form is made anew, with inputs of
        // its own, so its name input says which one it is.
        let form = draft.name.entity_id();
        self.writing = true;
        cx.notify();
        cx.spawn_in(window, async move |page, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    core::store::save_blocking(&core::store::dir(), file.as_deref(), &template)
                })
                .await;
            let _ = page.update_in(cx, |page: &mut Self, window, cx| {
                page.writing = false;
                match saved {
                    Ok((path, written)) => {
                        // Only the form that was saved learns its file: one
                        // opened on another template meanwhile would otherwise
                        // take this file, and its next save overwrite it.
                        if let Some(draft) =
                            page.draft.as_mut().filter(|d| d.name.entity_id() == form)
                        {
                            draft.file = Some(path.clone());
                            draft.original = written;
                            page.picked = Some(Pick::Own(path));
                        }
                        crate::toast::report(
                            window,
                            cx,
                            "Saving the workflow",
                            Some("Workflow saved"),
                            Ok::<(), String>(()),
                        );
                        crate::workflow::reload_templates(cx);
                    }
                    Err(why) => {
                        crate::toast::report(window, cx, "Saving the workflow", None, Err(why))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask, then delete the person's own workflow `pick` names, read or not.
    pub(super) fn confirm_delete(
        &mut self,
        pick: &Pick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entry_of(pick, cx) else {
            return;
        };
        let Some(file) = entry.file.clone() else {
            return;
        };
        let ask = crate::shell::Confirm {
            id: "delete-workflow",
            title: format!("Delete {}?", entry.name()).into(),
            description: "The template's file is deleted. Runs already started from it carry \
                          on with the copy they took."
                .into(),
            act: "Delete",
            ..Default::default()
        };
        crate::shell::confirm_on(cx.entity(), ask, window, cx, move |page, window, cx| {
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
                if gone.is_ok() && page.picked == Some(Pick::Own(file.clone())) {
                    page.draft = None;
                    page.picked = None;
                }
                crate::toast::report(
                    window,
                    cx,
                    "Deleting the workflow",
                    Some("Workflow deleted"),
                    gone,
                );
                crate::workflow::reload_templates(cx);
                cx.notify();
            });
        })
        .detach();
    }
}

/// That another workflow already has `draft`'s name, said as a problem: the
/// launcher lists them by name, so two of one name are one too many.
pub(super) fn name_clash(draft: &super::WorkflowDraft, cx: &gpui::App) -> Option<String> {
    let template = draft.to_template(cx).ok()?;
    let file = draft.file.clone();
    let same = |entry: &crate::workflow::Entry| file.is_some() && entry.file == file;
    crate::workflow::templates(cx)
        .iter()
        .any(|entry| !same(entry) && entry.name() == template.name)
        .then(|| format!("Another workflow is already called {}", template.name))
}
