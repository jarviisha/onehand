//! Binding a workflow as a project's board, and the size of its team, from
//! Settings' Projects page.

use super::Shell;
use gpui::{BorrowAppContext as _, Context};
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
        cx: &mut Context<Self>,
    ) {
        self.board_refused = None;
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let board = match workflow {
            None => None,
            Some(workflow) => {
                let template = crate::workflow::templates(cx)
                    .into_iter()
                    .find_map(|entry| entry.template.ok().filter(|t| t.id == workflow));
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
