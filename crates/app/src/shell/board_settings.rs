//! Binding a workflow as a project's board, and the size of its team, from
//! Settings' Projects page.

use super::Confirm;
use super::Shell;
use gpui::{BorrowAppContext as _, Context, Window};
use onehand_core::project::ProjectId;
use std::path::{Path, PathBuf};

impl Shell {
    /// Why the board of the project at `root` was not bound, once it was
    /// refused.
    pub(crate) fn board_refusal(&self, root: &Path) -> Option<&str> {
        self.board_refused
            .as_ref()
            .filter(|(at, _)| at == root)
            .map(|(_, why)| why.as_str())
    }

    /// Bind the workflow `workflow` as the board of the project at `root`,
    /// or take its board away with none. A workflow a board cannot run binds
    /// nothing and leaves its reason on the page.
    pub(crate) fn bind_board(
        &mut self,
        root: PathBuf,
        workflow: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let Some(workflow) = workflow else {
            let shared = crate::state::Shared::global(cx);
            if !shared.boards.unbinding_asks(&id) {
                return self.unbind_board(root, window, cx);
            }
            let name = shared
                .projects
                .get(&id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let ask = Confirm {
                id: "remove-board",
                title: format!("Remove the board from {}?", name).into(),
                description: "Its To do list goes; the issues stay open, in no list. Tasks \
                              already started run on."
                    .into(),
                act: "Remove",
                ..Default::default()
            };
            self.confirm(ask, window, cx, move |shell, window, cx| {
                shell.unbind_board(root.clone(), window, cx)
            });
            return;
        };
        let template = crate::control::template(&workflow, cx).ok();
        let bound = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared
                .boards
                .bind(&id, template.as_ref(), onehand_core::Now::read())
        });
        self.bound(root, id, bound, cx);
    }

    /// Bind `template`, a copy of workflow `from` the board of `project`
    /// runs, just saved, where the board runs `from`, and say so.
    pub(super) fn board_uses_copy(
        &mut self,
        project: ProjectId,
        from: String,
        template: onehand_core::workflow::Template,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some((root, name)) = crate::state::Shared::global(cx)
            .projects
            .get(&project)
            .map(|p| (p.path.clone(), p.name.clone()))
        else {
            return;
        };
        // Passed as written: the list on offer is read again only after.
        let Some(bound) = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared
                .boards
                .bind_copy(&project, &from, &template, onehand_core::Now::read())
        }) else {
            return;
        };
        let said = match &bound {
            Ok(_) => crate::toast::success(now_uses(&name, &template.name)),
            Err(why) => crate::toast::failed(&format!("Binding the copy to {name}'s board"), why),
        };
        self.bound(root, project, bound, cx);
        gpui_component::WindowExt::push_notification(window, said, cx);
    }

    /// *Remove the board…*: ask first, then take the board of the project at
    /// `root` away.
    pub(crate) fn remove_board(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let name = (crate::state::Shared::global(cx).projects.get(&id))
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let ask = Confirm {
            id: "remove-board",
            title: format!("Remove the board from {}?", name).into(),
            description: "Its columns and its To do go; the issues stay open, in no list. Tasks \
                          already started run on, as yours."
                .into(),
            act: "Remove",
            ..Default::default()
        };
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.unbind_board(root.clone(), window, cx)
        });
    }

    fn unbind_board(&mut self, root: PathBuf, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.project_of_row(&root) {
            let fx =
                cx.update_global::<crate::state::Shared, _>(|shared, _| shared.boards.unbind(&id));
            self.bound(root, id, Ok(fx), cx);
        }
    }

    /// Carry out what binding the board of project `id`, at `root`, came to:
    /// written and its To do looked at, or its refusal left on the page.
    fn bound(
        &mut self,
        root: PathBuf,
        id: ProjectId,
        bound: Result<Vec<onehand_core::board::drive::Effect>, String>,
        cx: &mut Context<Self>,
    ) {
        match bound {
            Ok(fx) => {
                crate::board::carry(fx, cx);
                crate::board::look(&id, cx);
            }
            Err(why) => self.board_refused = Some((root, why)),
        }
        cx.notify();
    }

    /// Set the size of the team of the board of the project at `root`.
    pub(crate) fn set_board_team(&mut self, root: &Path, team: u8, cx: &mut Context<Self>) {
        let Some(id) = self.project_of_row(root) else {
            return;
        };
        let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.boards.set_team(&id, team)
        });
        crate::board::carry(fx, cx);
        // A bigger team has room at once.
        crate::board::look(&id, cx);
        cx.notify();
    }
}

/// What a board bound to a copy of its workflow says.
fn now_uses(project: &str, copy: &str) -> String {
    format!("{project}'s board now uses {copy}")
}

#[cfg(test)]
mod tests;
