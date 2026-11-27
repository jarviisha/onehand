//! A look at a board: its tracker read, the extends waiting for a place given
//! one, the issues of its To do that fit started, and the issues of tasks
//! that ended done closed.

use super::{BoardDrive, Effect, Signal};
use crate::board::model::Place;
use crate::board::{extends_given, pull, pulls_next, usable};
use crate::connector::Connector;
use crate::issues::Issues;
use crate::project::ProjectId;
use crate::task::rooms::Room;
use crate::workflow::{Outcome, Template};
use crate::Now;
use std::collections::{BTreeSet, HashSet};

/// What the front end read beside a board's issues, deciding nothing: the
/// board's workflow from the library, how the tasks stand, and the ids the
/// starts it decides take.
pub struct Facts {
    /// The board's workflow as the library has it, or why not.
    pub template: Result<Template, String>,
    /// How many of the board's tasks hold a place on its team.
    pub running: usize,
    /// The board's tasks whose extend waits for a place, first asked first.
    pub extends_waiting: Vec<String>,
    /// The room at the workflow's first step, when that step is limited.
    pub first: Option<Room>,
    /// The issues the board's tasks hold out of the Backlog.
    pub taken: HashSet<u64>,
    /// Whether a window holds the project, to start a task in.
    pub window: bool,
    /// Fresh task ids, one for each start the look may decide.
    pub ids: Vec<String>,
}

impl BoardDrive {
    /// Look at project `id`'s board: its tracker is read first.
    #[must_use = "its effects are to be carried"]
    pub fn look(&mut self, id: &ProjectId) -> Vec<Effect> {
        match self.boards.board(id) {
            Some(_) => vec![Effect::ReadIssues { id: id.clone() }],
            None => Vec::new(),
        }
    }

    /// The issues of project `id`'s To do taken out to start and not yet
    /// landed: in no list until their task is kept.
    pub fn starting(&self, id: &ProjectId) -> impl Iterator<Item = u64> + '_ {
        self.starting.get(id).into_iter().flatten().copied()
    }

    /// What [`super::Input::IssuesRead`] brought: places to the extends waiting,
    /// first; then, with the issues read, the issues closed whose task ended
    /// done, and, with a window to start in and a workflow the board can
    /// run, as many of To do as there is room for, the gone ones dropped in
    /// the same write.
    pub(super) fn issues_read(
        &mut self,
        id: &ProjectId,
        read: Result<(Issues, Option<&'static dyn Connector>), String>,
        facts: Facts,
        now: Now,
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        let starting = self.starting.get(id).map_or(0, BTreeSet::len);
        if let Some(board) = self.boards.board(id) {
            let given = extends_given(board, facts.running + starting, &facts.extends_waiting);
            fx.extend(given.into_iter().map(|task| Effect::GivePlace { task }));
        }
        let Ok((issues, forge)) = read else {
            return fx;
        };
        let synced = forge.is_some_and(|forge| issues.in_step_with(forge.name()));
        for number in self.closing.remove(id).unwrap_or_default() {
            let close = (issues.get(number))
                .is_some_and(|issue| crate::board::after_end(issue, synced, true));
            if close {
                fx.push(Effect::CloseIssue {
                    id: id.clone(),
                    issue: number,
                });
            }
        }
        let Some(board) = self.boards.board(id) else {
            return fx;
        };
        let Ok(template) = facts.template else {
            return fx;
        };
        // A board that has not taken its tasks up yet does not know what
        // works on it, and taking them up would write over what it started.
        if !facts.window || usable(Some(&template)).is_err() || board.adopts() {
            return fx;
        }
        let decided = pull(
            board,
            // An extend waiting for a place is first in line: To do gets
            // only what is left after it.
            facts.running + starting + facts.extends_waiting.len(),
            // ponytail: a start counts here only until it lands; a task kept
            // and not yet asking for room is counted nowhere at the first
            // step, so a pull then may start one too many. The team cap and
            // the room ledger still hold, so the cost is a task that waits;
            // count a landed task until its run first asks if that bites.
            facts
                .first
                .map(|room| (room.limit, room.holding + room.waiting + starting)),
            &issues,
            forge,
            &facts.taken,
        );
        // Each start paired with its task's id first, so an issue leaves To
        // do only with a start to stand for it.
        let starts: Vec<(u64, String)> = decided.start.iter().copied().zip(facts.ids).collect();
        let gone = decided.gone;
        fx.extend(self.edit(id, false, |board| {
            let board = board?;
            let mut todo = board.todo();
            todo.retain(|n| !gone.contains(n));
            let board = board.with_todo(&todo, now.secs);
            let started = (starts.iter()).fold(board, |board, (issue, task)| {
                board.started(*issue, task, now.secs)
            });
            Some(started)
        }));
        // Asked only once the write that names them lands.
        let seq = self.newest.get(id).copied().unwrap_or(0);
        let named = !starts.is_empty();
        for (issue, task) in starts {
            self.starting.entry(id.clone()).or_default().insert(issue);
            let start = Effect::Start {
                id: id.clone(),
                issue,
                task,
                template: template.clone(),
            };
            self.held.push(super::Held {
                id: id.clone(),
                seq,
                issue,
                start,
            });
        }
        if named {
            fx.push(self.say_held());
        }
        fx
    }

    /// What [`super::Input::Landed`] brought: the start is no longer on its way.
    /// One that went ahead lets the board look again, to fill a place left
    /// over from counting it twice meanwhile; one refused before its task was
    /// made is said, its issue already out of To do and so back in the
    /// Backlog, and nothing is pulled in its place, so a reason every issue
    /// shares is said once; one refused after is its task's to say.
    pub(super) fn landed(
        &mut self,
        id: &ProjectId,
        issue: u64,
        kept: bool,
        result: Result<(), String>,
    ) -> Vec<Effect> {
        if let Some(starting) = self.starting.get_mut(id) {
            starting.remove(&issue);
            if starting.is_empty() {
                self.starting.remove(id);
            }
        }
        match (result, kept) {
            (Ok(()), _) => self.look(id),
            (Err(_), true) => Vec::new(),
            (Err(why), false) => vec![Effect::Signal(Signal::NotStarted {
                id: id.clone(),
                issue,
                why,
            })],
        }
    }

    /// What [`super::Input::Ended`] brought: an end is the board's when its
    /// work column names the task. A done one moves to Done, its issue closed
    /// once its tracker is read; any other stays where it is; and an end that
    /// left room on the team lets the board look again. The end of a task no
    /// entry names, one a person took up off the board, is theirs.
    pub(super) fn ended(
        &mut self,
        id: &ProjectId,
        issue: u64,
        task: &str,
        outcome: Option<&Outcome>,
        now: Now,
    ) -> Vec<Effect> {
        let ours = (self.boards.board(id)).is_some_and(|b| b.names(task) == Some(Place::Work));
        if !ours {
            return Vec::new();
        }
        let done = match outcome {
            Some(Outcome::Done) => true,
            Some(Outcome::Stopped(_) | Outcome::Exhausted { .. } | Outcome::Failed(_)) | None => {
                false
            }
        };
        let mut fx = Vec::new();
        if done {
            fx.extend(self.edit(id, false, |board| board.map(|b| b.finished(task, now.secs))));
            fx.push(self.say_held());
            self.closing.entry(id.clone()).or_default().insert(issue);
        }
        if done || pulls_next(outcome) {
            fx.push(Effect::ReadIssues { id: id.clone() });
        }
        fx
    }
}

#[cfg(test)]
mod tests;
