//! A project's board: its settings and the issues waiting in To do.

use super::{Effect, ProjectsDrive, Thing, Value};
use crate::project::{Board, ProjectId};

impl ProjectsDrive {
    /// Give project `id` a board, or take it away with none. Written now;
    /// unchanged, nothing happens.
    #[must_use = "its effects are to be carried"]
    pub fn set_board(&mut self, id: &ProjectId, board: Option<Board>) -> Vec<Effect> {
        self.edit_board(id, false, |_| board)
    }

    /// Set the size of project `id`'s team, kept within 1 to 8; written once
    /// the changing stops.
    #[must_use = "its effects are to be carried"]
    pub fn set_team(&mut self, id: &ProjectId, team: u8) -> Vec<Effect> {
        self.edit_board(id, true, |b| {
            b.map(|b| Board {
                team: team.clamp(1, Board::MOST_TEAM),
                ..b
            })
        })
    }

    /// Put `issue` last in project `id`'s To do, unless it is there.
    #[must_use = "its effects are to be carried"]
    pub fn add_todo(&mut self, id: &ProjectId, issue: u64) -> Vec<Effect> {
        self.edit_todo(id, |todo| {
            if !todo.contains(&issue) {
                todo.push(issue);
            }
        })
    }

    /// Move `issue` one place up (or down) in project `id`'s To do.
    #[must_use = "its effects are to be carried"]
    pub fn move_todo(&mut self, id: &ProjectId, issue: u64, up: bool) -> Vec<Effect> {
        self.edit_todo(id, |todo| {
            let Some(at) = todo.iter().position(|&n| n == issue) else {
                return;
            };
            let to = if up { at.checked_sub(1) } else { Some(at + 1) };
            if let Some(to) = to.filter(|&to| to < todo.len()) {
                todo.swap(at, to);
            }
        })
    }

    /// Take `issue` out of project `id`'s To do.
    #[must_use = "its effects are to be carried"]
    pub fn drop_todo(&mut self, id: &ProjectId, issue: u64) -> Vec<Effect> {
        self.take_todo(id, &[issue])
    }

    /// Take each of `issues` out of project `id`'s To do.
    #[must_use = "its effects are to be carried"]
    pub fn take_todo(&mut self, id: &ProjectId, issues: &[u64]) -> Vec<Effect> {
        self.edit_todo(id, |todo| todo.retain(|n| !issues.contains(n)))
    }

    fn edit_todo(&mut self, id: &ProjectId, edit: impl FnOnce(&mut Vec<u64>)) -> Vec<Effect> {
        self.edit_board(id, false, |board| {
            board.map(|mut b| {
                edit(&mut b.todo);
                b
            })
        })
    }

    /// Replace project `id`'s board by what `make` answers of it. A project
    /// without a board stays so unless `make` gives it one (`set_board`).
    fn edit_board(
        &mut self,
        id: &ProjectId,
        soon: bool,
        make: impl FnOnce(Option<Board>) -> Option<Board>,
    ) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        let before = project.board.clone();
        let after = make(before.clone());
        if after == before {
            return Vec::new();
        }
        project.board = after.clone();
        self.change(
            id,
            Thing::Board,
            Value::Board(before),
            Value::Board(after),
            soon,
        )
    }
}
