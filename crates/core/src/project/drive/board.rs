//! A project's board: its settings and the issues waiting in To do.

use super::{Effect, ProjectsDrive, Thing, Value};
use crate::connector::Connector;
use crate::issues::Issues;
use crate::project::{Board, ProjectId, Toward};
use crate::task::rooms::Room;
use crate::workflow::Template;
use std::collections::{BTreeSet, HashSet};

/// The team a board starts with.
const FIRST_TEAM: u8 = 2;

impl ProjectsDrive {
    /// Give project `id` a board, or take it away with none. Written now;
    /// unchanged, nothing happens.
    #[must_use = "its effects are to be carried"]
    pub fn set_board(&mut self, id: &ProjectId, board: Option<Board>) -> Vec<Effect> {
        self.edit_board(id, false, |_| board)
    }

    /// Bind `template` as project `id`'s board, as read when it is on offer,
    /// or none when it is gone or cannot be read: refused, with why, when a
    /// board cannot run it. A board bound again keeps its team and its To
    /// do; a new one starts with a small team and nothing to do.
    pub fn bind_board(
        &mut self,
        id: &ProjectId,
        template: Option<&Template>,
    ) -> Result<Vec<Effect>, String> {
        let workflow = crate::board::usable(template)?.id.clone();
        let board = match self.projects.get(id).and_then(|p| p.board()) {
            Some(kept) => Board {
                workflow,
                ..kept.clone()
            },
            None => Board {
                workflow,
                team: FIRST_TEAM,
                todo: Vec::new(),
            },
        };
        Ok(self.set_board(id, Some(board)))
    }

    /// Bind `copy`, a copy of the workflow of project `id`'s board, as that
    /// board, as [`Self::bind_board`] does; none when it has no board.
    pub fn bind_copy(
        &mut self,
        id: &ProjectId,
        copy: &Template,
    ) -> Option<Result<Vec<Effect>, String>> {
        self.projects.get(id)?.board()?;
        Some(self.bind_board(id, Some(copy)))
    }

    /// Whether taking project `id`'s board away asks a person first: its To
    /// do goes with it and cannot be had back.
    pub fn unbinding_asks(&self, id: &ProjectId) -> bool {
        (self.projects.get(id)).is_some_and(|p| p.board().is_some_and(|b| !b.todo.is_empty()))
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
    pub fn move_todo(&mut self, id: &ProjectId, issue: u64, toward: Toward) -> Vec<Effect> {
        self.edit_todo(id, |todo| {
            let Some(at) = todo.iter().position(|&n| n == issue) else {
                return;
            };
            let to = match toward {
                Toward::Up => at.checked_sub(1),
                Toward::Down => Some(at + 1),
            };
            if let Some(to) = to.filter(|&to| to < todo.len()) {
                todo.swap(at, to);
            }
        })
    }

    /// Put `issue` at place `at` in project `id`'s To do, taking it from where
    /// it was; `at` counts after that and is held within the list.
    #[must_use = "its effects are to be carried"]
    pub fn place_todo(&mut self, id: &ProjectId, issue: u64, at: usize) -> Vec<Effect> {
        self.edit_todo(id, |todo| {
            todo.retain(|&n| n != issue);
            todo.insert(at.min(todo.len()), issue);
        })
    }

    /// Put `issue` just before `target` in project `id`'s To do, taking it
    /// from where it was; dropped on itself, or on an issue not in To do, it
    /// moves nothing.
    #[must_use = "its effects are to be carried"]
    pub fn place_todo_before(&mut self, id: &ProjectId, issue: u64, target: u64) -> Vec<Effect> {
        let todo = (self.projects.get(id))
            .and_then(|p| p.board())
            .map(|b| b.todo.as_slice());
        match todo.and_then(|todo| place_before(todo, issue, target)) {
            Some(at) => self.place_todo(id, issue, at),
            None => Vec::new(),
        }
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
    /// board, or when its workflow, `template`, is one a board cannot run,
    /// which its page says. `first` is the room at the workflow's first
    /// step, when that step is limited: those holding or waiting for it, and
    /// the starts on their way, leave the rest.
    #[allow(clippy::too_many_arguments)]
    pub fn pull(
        &self,
        id: &ProjectId,
        template: &Template,
        running: usize,
        first: Option<Room>,
        issues: &Issues,
        forge: Option<&'static dyn Connector>,
        taken: &HashSet<u64>,
    ) -> Option<crate::board::Pull> {
        let board = self.projects.get(id)?.board()?;
        crate::board::usable(Some(template)).ok()?;
        let starting = self.starting.get(id).map_or(0, BTreeSet::len);
        Some(crate::board::pull(
            board,
            running + starting,
            // ponytail: a start counts here only until it lands; a task kept
            // and not yet asking for room is counted nowhere at the first
            // step, so a pull then may start one too many. The team cap and
            // the room ledger still hold, so the cost is a task that waits;
            // count a landed task until its run first asks if that bites.
            first.map(|room| (room.limit, room.holding + room.waiting + starting)),
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

/// Where in `todo` issue `dragged` goes to stand just before `target`: the
/// target's place once the dragged issue is taken out. None when dropped on
/// itself, which moves nothing.
fn place_before(todo: &[u64], dragged: u64, target: u64) -> Option<usize> {
    if dragged == target {
        return None;
    }
    todo.iter()
        .filter(|&&n| n != dragged)
        .position(|&n| n == target)
}

#[cfg(test)]
mod tests {
    use super::place_before;

    #[test]
    fn a_drop_on_a_row_lands_just_before_it() {
        let todo = [1, 2, 3, 4];
        // From below: the target keeps its place.
        assert_eq!(place_before(&todo, 4, 2), Some(1));
        // From above: the target moves up one once the issue is out.
        assert_eq!(place_before(&todo, 1, 3), Some(1));
        // Onto the row after itself: its own place, which writes nothing.
        assert_eq!(place_before(&todo, 2, 3), Some(1));
        // From the Backlog.
        assert_eq!(place_before(&todo, 9, 1), Some(0));
        assert_eq!(place_before(&todo, 3, 3), None);
    }
}
