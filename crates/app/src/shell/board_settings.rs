//! Binding a workflow as a project's board, and the size of its team, from
//! Settings' Projects page.

use super::Confirm;
use super::Shell;
use gpui::{BorrowAppContext as _, Context, Window};
use onehand_core::project::Board;
use std::path::{Path, PathBuf};

/// The team a board starts with.
const FIRST_TEAM: u8 = 2;

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
        // Its To do goes with it and cannot be had back: asked first.
        let name = crate::state::Shared::global(cx)
            .projects
            .get(&id)
            .filter(|project| project.board().is_some_and(|board| !board.todo.is_empty()))
            .map(|project| project.name.clone());
        if workflow.is_none()
            && let Some(name) = name
        {
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
        }
        let offered = workflow.map(|workflow| {
            let template = crate::workflow::templates(cx)
                .into_iter()
                .find_map(|entry| entry.template.ok().filter(|t| t.id == workflow));
            (workflow, template)
        });
        self.unbind_or_bind(root, id, offered, cx);
    }

    /// Bind `template`, a copy of the workflow of the board of `project`
    /// just saved, as that board, and say so.
    pub(super) fn board_uses_copy(
        &mut self,
        project: onehand_core::project::ProjectId,
        template: onehand_core::workflow::Template,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some((root, name)) = crate::state::Shared::global(cx)
            .projects
            .get(&project)
            .filter(|p| p.board().is_some())
            .map(|p| (p.path.clone(), p.name.clone()))
        else {
            return;
        };
        let said = now_uses(&name, &template.name);
        // Passed as written: the list on offer is read again only after.
        self.unbind_or_bind(
            root,
            project,
            Some((template.id.clone(), Some(template))),
            cx,
        );
        match &self.board_refused {
            None => gpui_component::WindowExt::push_notification(
                window,
                crate::toast::success(said),
                cx,
            ),
            Some((_, why)) => gpui_component::WindowExt::push_notification(
                window,
                crate::toast::failed(&format!("Binding the copy to {name}'s board"), why),
                cx,
            ),
        }
    }

    fn unbind_board(&mut self, root: PathBuf, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.project_of_row(&root) {
            self.unbind_or_bind(root, id, None, cx);
        }
    }

    /// Bind the workflow of id `workflow`, as `template` holds it when it is
    /// on offer, or none.
    fn unbind_or_bind(
        &mut self,
        root: PathBuf,
        id: onehand_core::project::ProjectId,
        workflow: Option<(String, Option<onehand_core::workflow::Template>)>,
        cx: &mut Context<Self>,
    ) {
        let board = match workflow {
            None => None,
            Some((workflow, template)) => {
                if let Err(why) = onehand_core::board::usable(template.as_ref()) {
                    self.board_refused = Some((root, why));
                    cx.notify();
                    return;
                }
                let kept = crate::state::Shared::global(cx)
                    .projects
                    .get(&id)
                    .and_then(|project| project.board().cloned());
                Some(match kept {
                    Some(board) => Board { workflow, ..board },
                    None => Board {
                        workflow,
                        team: FIRST_TEAM,
                        todo: Vec::new(),
                    },
                })
            }
        };
        let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.projects.set_board(&id, board)
        });
        crate::projects::carry(fx, cx);
        crate::board::pull_now(&id, cx);
        cx.notify();
    }

    /// Set the size of the team of the board of the project at `root`.
    pub(crate) fn set_board_team(&mut self, root: &Path, team: u8, cx: &mut Context<Self>) {
        let Some(id) = self.project_of_row(root) else {
            return;
        };
        let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.projects.set_team(&id, team)
        });
        crate::projects::carry(fx, cx);
        // A bigger team has room at once.
        crate::board::pull_now(&id, cx);
        cx.notify();
    }
}

/// What a board bound to a copy of its workflow says.
fn now_uses(project: &str, copy: &str) -> String {
    format!("{project}'s board now uses {copy}.")
}

#[cfg(test)]
mod tests;
