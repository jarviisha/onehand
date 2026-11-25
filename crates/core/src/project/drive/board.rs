//! A project's board: its settings and the issues waiting in To do.

use super::{Effect, ProjectsDrive, Thing, Value};
use crate::connector::Connector;
use crate::issues::Issues;
use crate::project::{Board, ProjectId};
use std::collections::{BTreeSet, HashSet};

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

    /// Take each of `issues` out of project `id`'s To do to start them:
    /// counted as running until each [`Self::landed`].
    #[must_use = "its effects are to be carried"]
    pub fn start_todo(&mut self, id: &ProjectId, issues: &[u64]) -> Vec<Effect> {
        let fx = self.take_todo(id, issues);
        if issues.is_empty() {
            return fx;
        }
        self.starting
            .entry(id.clone())
            .or_default()
            .extend(issues.iter().copied());
        fx
    }

    /// The start of `issue` of project `id`'s board landed, as a task or a
    /// refusal: it is no longer counted as starting.
    pub fn landed(&mut self, id: &ProjectId, issue: u64) {
        if let Some(starting) = self.starting.get_mut(id) {
            starting.remove(&issue);
            if starting.is_empty() {
                self.starting.remove(id);
            }
        }
    }

    /// The issues of project `id`'s To do taken out to start and not yet
    /// landed: in no list until their task is kept.
    pub fn starting(&self, id: &ProjectId) -> impl Iterator<Item = u64> + '_ {
        self.starting.get(id).into_iter().flatten().copied()
    }

    /// Which of project `id`'s To do to start while `running` of its board
    /// tasks work, its starts on their way counted with them, over its
    /// `issues` and the issues its tasks hold (`taken`); none without a
    /// board. `first` is its workflow's first step's limit and how many hold
    /// or wait for room there, when that step is limited; the starts on
    /// their way are counted there too.
    pub fn pull(
        &self,
        id: &ProjectId,
        running: usize,
        first: Option<(u32, usize)>,
        issues: &Issues,
        forge: Option<&'static dyn Connector>,
        taken: &HashSet<u64>,
    ) -> Option<crate::board::Pull> {
        let board = self.projects.get(id)?.board()?;
        let starting = self.starting.get(id).map_or(0, BTreeSet::len);
        Some(crate::board::pull(
            board,
            running + starting,
            first.map(|(limit, at)| (limit, at + starting)),
            issues,
            forge,
            taken,
        ))
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
