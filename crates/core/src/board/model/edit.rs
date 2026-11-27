//! What a board of any columns allows, and the changes a column and an
//! entry go through, each answering the board as it then stands.

use super::{matches_workflow, Board, Column, Entry, DONE_KEPT};
use crate::project::Toward;
use std::collections::HashSet;

impl Board {
    /// Whether a board of any columns is one this build keeps: at least one
    /// column, ids unique and names said, limits of at least 1, an issue on
    /// one entry at most, and a run or a finished run only where a workflow
    /// runs, a run only beside its task.
    pub(super) fn any_shape(&self) -> Result<(), String> {
        if self.columns.is_empty() {
            return Err("it has no columns".into());
        }
        let mut ids = HashSet::new();
        for column in &self.columns {
            if column.id.trim().is_empty() || !ids.insert(column.id.as_str()) {
                return Err(format!(
                    "a column's id, `{}`, is empty or used twice",
                    column.id
                ));
            }
            if column.name.trim().is_empty() {
                return Err(format!("column `{}` has no name", column.id));
            }
            if column.limit == Some(0) {
                return Err(format!("column `{}` holds none", column.id));
            }
            let runs = matches_workflow(&column.work);
            let sound = |e: &Entry| {
                (runs || (e.run.is_none() && !e.done)) && (e.task.is_some() || e.run.is_none())
            };
            if !column.entries.iter().all(sound) {
                return Err(format!("column `{}` holds an entry it cannot", column.id));
            }
        }
        let mut issues = HashSet::new();
        let all = self
            .columns
            .iter()
            .flat_map(|c| c.entries.iter())
            .chain(&self.done);
        for entry in all {
            if !issues.insert(entry.issue) {
                return Err(format!("issue {} is on the board twice", entry.issue));
            }
        }
        match self.done.len() <= DONE_KEPT {
            true => Ok(()),
            false => Err("it keeps more done than it may".into()),
        }
    }

    /// The column with id `id`.
    pub fn column(&self, id: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.id == id)
    }

    /// Where `issue` stands, by its column's place, and its entry.
    pub fn at(&self, issue: u64) -> Option<(usize, &Entry)> {
        self.columns.iter().enumerate().find_map(|(at, column)| {
            (column.entries.iter())
                .find(|entry| entry.issue == issue)
                .map(|entry| (at, entry))
        })
    }

    /// The board with `issue` in the column at `to`, just before `before`
    /// when that stands there, else last, entered at `now`: taken from where
    /// it was, its task kept and what it did in its old column forgotten, or
    /// made new, with no task, from the Backlog or Done.
    pub(crate) fn placed(&self, issue: u64, to: usize, before: Option<u64>, now: u64) -> Board {
        if before == Some(issue) {
            return self.clone();
        }
        let mut board = self.clone();
        let task = board.take(issue).and_then(|entry| entry.task);
        let Some(column) = board.columns.get_mut(to) else {
            return self.clone();
        };
        let entry = Entry {
            issue,
            task,
            since: now,
            ..Entry::default()
        };
        let at = (before.and_then(|b| column.entries.iter().position(|e| e.issue == b)))
            .unwrap_or(column.entries.len());
        column.entries.insert(at, entry);
        board
    }

    /// The board with `issue` at the front of Done, entered at `now`, Done
    /// kept to [`DONE_KEPT`].
    pub(crate) fn to_done(&self, issue: u64, now: u64) -> Board {
        let mut board = self.clone();
        let task = board.take(issue).and_then(|entry| entry.task);
        board.done.insert(
            0,
            Entry {
                issue,
                task,
                since: now,
                ..Entry::default()
            },
        );
        board.done.truncate(DONE_KEPT);
        board
    }

    /// The board with `issue`'s entry worked by task `task`, its run `run`.
    pub(crate) fn begun(&self, issue: u64, task: &str, run: &str) -> Board {
        let mut board = self.clone();
        if let Some(entry) = board.entry_mut(issue) {
            entry.task = Some(task.to_string());
            entry.run = Some(run.to_string());
            entry.done = false;
        }
        board
    }

    /// The board with the entry task `task` works marked done in its column.
    pub(crate) fn ended_done(&self, task: &str) -> Board {
        let mut board = self.clone();
        let entry = (board.columns.iter_mut())
            .flat_map(|column| column.entries.iter_mut())
            .find(|entry| entry.task.as_deref() == Some(task));
        if let Some(entry) = entry {
            entry.done = true;
        }
        board
    }

    /// The board with `column` put at place `at`.
    pub(crate) fn with_column(&self, at: usize, column: Column) -> Board {
        let mut board = self.clone();
        let at = at.min(board.columns.len());
        board.columns.insert(at, column);
        board
    }

    /// The board without the column with id `id`.
    pub(crate) fn without_column(&self, id: &str) -> Board {
        let mut board = self.clone();
        board.columns.retain(|column| column.id != id);
        board
    }

    /// The board with the column with id `id` one place up (left) or down.
    pub(crate) fn column_moved(&self, id: &str, toward: Toward) -> Board {
        let mut board = self.clone();
        let Some(at) = board.columns.iter().position(|column| column.id == id) else {
            return board;
        };
        let to = match toward {
            Toward::Up => at.checked_sub(1),
            Toward::Down => Some(at + 1),
        };
        if let Some(to) = to.filter(|&to| to < board.columns.len()) {
            board.columns.swap(at, to);
        }
        board
    }

    /// Take `issue`'s entry out of wherever it stands.
    fn take(&mut self, issue: u64) -> Option<Entry> {
        for column in &mut self.columns {
            if let Some(at) = column.entries.iter().position(|e| e.issue == issue) {
                return Some(column.entries.remove(at));
            }
        }
        let at = self.done.iter().position(|e| e.issue == issue)?;
        Some(self.done.remove(at))
    }

    fn entry_mut(&mut self, issue: u64) -> Option<&mut Entry> {
        (self.columns.iter_mut())
            .flat_map(|column| column.entries.iter_mut())
            .find(|entry| entry.issue == issue)
    }
}

#[cfg(test)]
mod tests;
