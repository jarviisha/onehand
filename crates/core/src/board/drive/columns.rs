//! What a person does to a board's columns and to where an issue stands:
//! a column added, edited, removed or moved; an issue placed in a column or
//! in Done; a run started on a press.

use super::look::workflow_of;
use super::{BoardDrive, Effect};
use crate::board::model::Column;
use crate::board::{usable, Work};
use crate::project::{ProjectId, Toward};
use crate::workflow::Template;
use crate::Now;

impl BoardDrive {
    /// Add `column`, with no issues, at place `at` of project `id`'s board:
    /// refused when `valid` refuses its work or the board would not keep.
    pub fn add_column(
        &mut self,
        id: &ProjectId,
        column: Column,
        at: usize,
        valid: &dyn Fn(&Work) -> Result<(), String>,
    ) -> Result<Vec<Effect>, String> {
        valid(&column.work)?;
        let column = Column {
            entries: Vec::new(),
            ..column
        };
        self.change(id, |board| Ok(board.with_column(at, column)))
    }

    /// Make column `column_id` of project `id`'s board what `edit` says, its
    /// issues kept. One that stops running a workflow forgets what each of
    /// its issues did there, refused while one of their tasks works
    /// (`working`).
    pub fn edit_column(
        &mut self,
        id: &ProjectId,
        column_id: &str,
        edit: Column,
        working: &dyn Fn(&str) -> bool,
        valid: &dyn Fn(&Work) -> Result<(), String>,
    ) -> Result<Vec<Effect>, String> {
        valid(&edit.work)?;
        self.change(id, |board| {
            let at = (board.columns.iter())
                .position(|c| c.id == column_id)
                .ok_or_else(|| format!("No column {column_id}"))?;
            let old = &board.columns[at];
            let mut entries = old.entries.clone();
            if workflow_of(old).is_some() && workflow_of(&edit).is_none() {
                let works = |e: &crate::board::Entry| e.task.as_deref().is_some_and(working);
                if entries.iter().any(works) {
                    return Err("Stop what works there first".to_string());
                }
                for entry in &mut entries {
                    entry.run = None;
                    entry.done = false;
                }
            }
            let mut board = board.clone();
            board.columns[at] = Column { entries, ..edit };
            Ok(board)
        })
    }

    /// Take column `column_id` off project `id`'s board: refused while it
    /// holds issues.
    pub fn remove_column(
        &mut self,
        id: &ProjectId,
        column_id: &str,
    ) -> Result<Vec<Effect>, String> {
        self.change(id, |board| match board.column(column_id) {
            Some(column) if !column.entries.is_empty() => Err(format!(
                "Move its {} issues out first",
                column.entries.len()
            )),
            Some(_) => Ok(board.without_column(column_id)),
            None => Err(format!("No column {column_id}")),
        })
    }

    /// Move column `column_id` of project `id`'s board one place up (left) or
    /// down.
    #[must_use = "its effects are to be carried"]
    pub fn move_column(&mut self, id: &ProjectId, column_id: &str, toward: Toward) -> Vec<Effect> {
        self.edit(id, false, |board| {
            board.map(|b| b.column_moved(column_id, toward))
        })
    }

    /// Put `issue` of project `id`'s board in column `to`, just before
    /// `before` when it stands there, or in Done with none: refused while its
    /// task works (`working`) and when the column is full. What moves there
    /// is looked at again, and what enters Done is closed once read.
    pub fn place(
        &mut self,
        id: &ProjectId,
        issue: u64,
        to: Option<&str>,
        before: Option<u64>,
        working: bool,
        now: Now,
    ) -> Result<Vec<Effect>, String> {
        if working {
            return Err("It is at work: Stop it first".to_string());
        }
        if self.starting.get(id).is_some_and(|s| s.contains(&issue)) {
            return Err("It is starting: wait for it to start".to_string());
        }
        let mut fx = self.change(id, |board| match to {
            None => Ok(board.to_done(issue, now.secs)),
            Some(to) => {
                let at = (board.columns.iter())
                    .position(|c| c.id == to)
                    .ok_or_else(|| format!("No column {to}"))?;
                let column = &board.columns[at];
                let others = column.entries.iter().filter(|e| e.issue != issue).count();
                if column.limit.is_some_and(|limit| others >= limit as usize) {
                    return Err(format!("{} is full", column.name));
                }
                Ok(board.placed(issue, at, before, now.secs))
            }
        })?;
        if to.is_none() {
            self.closing.entry(id.clone()).or_default().insert(issue);
        }
        fx.push(Effect::ReadIssues { id: id.clone() });
        Ok(fx)
    }

    /// Start `issue` of project `id`'s board in its column, on a press: past
    /// a full team, which caps only what the board starts by itself. A new
    /// task when no task works it or its task was never kept (`task_lost`),
    /// else a next run of its task; held for the write that names it.
    pub fn start(
        &mut self,
        id: &ProjectId,
        issue: u64,
        task_lost: bool,
        ids: Vec<String>,
        template: Result<Template, String>,
    ) -> Result<Vec<Effect>, String> {
        let template = template?;
        usable(Some(&template))?;
        if self.starting.get(id).is_some_and(|s| s.contains(&issue)) {
            return Err("It is starting already".to_string());
        }
        let board = self.boards.board(id).ok_or("The board is gone")?;
        let (at, entry) = board.at(issue).ok_or("It is on no column")?;
        let column = &board.columns[at];
        workflow_of(column).ok_or("Its column runs no workflow")?;
        let fresh = ids.into_iter().next().ok_or("No id to start it with")?;
        let (begun, start) = match entry.task.clone().filter(|_| !task_lost) {
            Some(task) => (
                board.begun(issue, &task, &fresh),
                Effect::NextRun {
                    id: id.clone(),
                    issue,
                    task,
                    run: fresh,
                    template,
                    column: column.name.clone(),
                },
            ),
            None => (
                board.begun(issue, &fresh, &fresh),
                Effect::Start {
                    id: id.clone(),
                    issue,
                    task: fresh,
                    template,
                    column: column.name.clone(),
                },
            ),
        };
        let mut fx = self.edit(id, false, |_| Some(begun));
        self.hold(id, vec![(issue, start)]);
        fx.push(self.say_held());
        Ok(fx)
    }

    /// Change project `id`'s board by `make`, refused when it refuses, when
    /// the board cannot be read, and when what it makes would not keep.
    fn change(
        &mut self,
        id: &ProjectId,
        make: impl FnOnce(&crate::board::Board) -> Result<crate::board::Board, String>,
    ) -> Result<Vec<Effect>, String> {
        if let Some(why) = self.boards.unreadable(id) {
            return Err(format!("Its board cannot be read: {why}"));
        }
        let board = self.boards.board(id).ok_or("The board is gone")?;
        let changed = make(board)?;
        changed.shape()?;
        let mut fx = self.edit(id, false, |_| Some(changed));
        fx.push(self.say_held());
        Ok(fx)
    }
}

#[cfg(test)]
mod tests;
