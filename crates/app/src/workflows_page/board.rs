//! The Workflows page opened on a board's workflow, from the board or its
//! settings: the workflow picked by its id, a step of it open, and the board
//! it was opened for remembered while it stays open, so a duplicate of it
//! saved there can take its place on that board.

use super::WorkflowsPage;
use gpui::{Context, Window};
use onehand_core::project::ProjectId;

impl WorkflowsPage {
    /// Open the workflow of id `id`, asking first when the form holds
    /// changes not saved to another, and open its step `step` if named.
    /// `for_board` is the board it was opened from.
    pub(crate) fn open_workflow(
        &mut self,
        id: &str,
        step: Option<&str>,
        for_board: Option<ProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pick = crate::workflow::templates(cx)
            .iter()
            .filter(|entry| entry.template.as_ref().is_ok_and(|t| t.id == id))
            .find_map(super::pick::pick_of);
        let Some(pick) = pick else {
            return;
        };
        let step = step.map(str::to_string);
        self.pick_then(pick, window, cx, move |page, window, cx| {
            page.for_board = for_board.clone();
            let Some(step) = &step else {
                return;
            };
            let at = page.draft.as_ref().and_then(|draft| {
                (draft.steps.iter()).position(|s| s.id.read(cx).value() == step.as_str())
            });
            if at.is_some() {
                page.edit_open_step(window, cx, |draft| draft.open_step = at);
            }
        });
    }
}

#[cfg(test)]
impl WorkflowsPage {
    /// The id of the workflow open, and the step open in it.
    pub(crate) fn opened(&self) -> Option<(String, Option<usize>)> {
        let draft = self.draft.as_ref()?;
        Some((draft.original.id.clone(), draft.open_step))
    }

    /// Put `name` in the open form's name.
    pub(crate) fn rename_draft(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = &self.draft {
            draft.name.update(cx, |input, cx| {
                input.set_value(name.to_string(), window, cx)
            });
        }
    }

    /// The board the workflow open was opened from.
    pub(crate) fn for_board(&self) -> Option<&ProjectId> {
        self.for_board.as_ref()
    }

    /// Pick the workflow of id `id`, as its card does.
    pub(crate) fn pick_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.pick(super::pick::Pick::Shipped(id.to_string()), window, cx);
    }

    /// Open a duplicate of the workflow open.
    pub(crate) fn duplicate_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pick) = self.picked.clone() {
            self.duplicate(&pick, window, cx);
        }
    }
}
