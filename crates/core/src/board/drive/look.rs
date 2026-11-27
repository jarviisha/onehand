//! A look at a board: its tracker read, the extends waiting for a place given
//! one, the issues of its To do that fit started, and the issues of tasks
//! that ended done closed.

use super::{BoardDrive, Effect, Signal};
use crate::board::{extends_given, flow, pulls_next, usable, Work};
use crate::connector::Connector;
use crate::issues::Issues;
use crate::project::ProjectId;
use crate::task::rooms::Room;
use crate::workflow::{Outcome, Template};
use crate::Now;
use std::collections::{BTreeSet, HashMap};

/// What the front end read beside a board's issues, deciding nothing: the
/// workflows its columns run, from the library, how the tasks stand, and
/// the ids the starts it decides take.
pub struct Facts {
    /// Each workflow a column runs, by its id, as the library has it, or why
    /// not.
    pub templates: HashMap<String, Result<Template, String>>,
    /// How many of the board's tasks hold a place on its team.
    pub running: usize,
    /// The board's tasks whose extend waits for a place, first asked first.
    pub extends_waiting: Vec<String>,
    /// The room at each workflow's first step, by its id, when that step is
    /// limited.
    pub first: HashMap<String, Option<Room>>,
    /// Whether a window holds the project, to start a task in.
    pub window: bool,
    /// Fresh ids, one for each start the look may decide.
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

    /// The issues of project `id`'s board whose start is on its way: in no
    /// list until their task is kept.
    pub fn starting(&self, id: &ProjectId) -> impl Iterator<Item = u64> + '_ {
        self.starting.get(id).into_iter().flatten().copied()
    }

    /// What [`super::Input::IssuesRead`] brought: places to the extends
    /// waiting, first; then, with the issues read, the issues closed that
    /// entered Done, and, with a window to start in, what a look decides
    /// (`flow::look`), written whole and its starts held for that write.
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
        let mut closing: Vec<u64> = self
            .closing
            .remove(id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        // A board that has not taken its tasks up yet does not know what
        // works on it, and taking them up would write over what it started.
        let looks = self.boards.board(id).is_some_and(|b| !b.adopts());
        if looks {
            fx.extend(self.decide(id, &issues, forge, facts, starting, now, &mut closing));
        }
        let synced = forge.is_some_and(|forge| issues.in_step_with(forge.name()));
        for number in closing {
            let close = (issues.get(number))
                .is_some_and(|issue| crate::board::after_end(issue, synced, true));
            if close {
                fx.push(Effect::CloseIssue {
                    id: id.clone(),
                    issue: number,
                });
            }
        }
        fx
    }

    /// What a look at project `id`'s board decides, written, its starts held
    /// for that write, what entered Done added to `closing`.
    #[allow(clippy::too_many_arguments)]
    fn decide(
        &mut self,
        id: &ProjectId,
        issues: &Issues,
        forge: Option<&'static dyn Connector>,
        facts: Facts,
        starting: usize,
        now: Now,
        closing: &mut Vec<u64>,
    ) -> Vec<Effect> {
        let Some(board) = self.boards.board(id) else {
            return Vec::new();
        };
        let usable = |workflow: &str| {
            (facts.templates.get(workflow))
                .is_some_and(|t| t.as_ref().is_ok_and(|t| usable(Some(t)).is_ok()))
        };
        // An extend waiting for a place is first in line: the look gets only
        // what is left after it.
        let held = facts.running + starting + facts.extends_waiting.len();
        let room = flow::Room {
            // With no window to start a task in, nothing starts: moves into
            // a person's column or into Done still go.
            team: match facts.window {
                true => usize::from(board.team).saturating_sub(held),
                false => 0,
            },
            // ponytail: a start counts here only until it lands; a task kept
            // and not yet asking for room is counted nowhere at the first
            // step, so a look then may start one too many. The team cap and
            // the room ledger still hold, so the cost is a task that waits;
            // count a landed task until its run first asks if that bites.
            first: (facts.first.iter())
                .map(|(w, room)| {
                    (
                        w.clone(),
                        room.map(|r| (r.limit, r.holding + r.waiting + starting)),
                    )
                })
                .collect(),
        };
        let decided = flow::look(board, issues, forge, room, &usable, now.secs);
        closing.extend(decided.closing);
        // Each start paired with its id first, so an entry is begun only with
        // a start to stand for it; one left without waits for the next look.
        let mut ids = facts.ids.into_iter();
        let mut begun = decided.board;
        let mut starts = Vec::new();
        for start in decided.starts {
            let Some(fresh) = ids.next() else {
                break;
            };
            let (issue, column) = match &start {
                flow::Begin::New { issue, column } | flow::Begin::Next { issue, column, .. } => {
                    (*issue, *column)
                }
            };
            let Some(column) = begun.columns.get(column).cloned() else {
                continue;
            };
            let Some(template) =
                workflow_of(&column).and_then(|w| facts.templates.get(w)?.clone().ok())
            else {
                continue;
            };
            let effect = match start {
                flow::Begin::New { .. } => {
                    begun = begun.begun(issue, &fresh, &fresh);
                    Effect::Start {
                        id: id.clone(),
                        issue,
                        task: fresh,
                        template,
                        column: column.name.clone(),
                    }
                }
                flow::Begin::Next { task, .. } => {
                    begun = begun.begun(issue, &task, &fresh);
                    Effect::NextRun {
                        id: id.clone(),
                        issue,
                        task,
                        run: fresh,
                        template,
                        column: column.name.clone(),
                    }
                }
            };
            starts.push((issue, effect));
        }
        let mut fx = self.edit(id, false, |_| Some(begun));
        self.hold(id, starts);
        if !fx.is_empty() {
            fx.push(self.say_held());
        }
        fx
    }

    /// Hold `starts` of project `id` for the newest write, which names them.
    pub(super) fn hold(&mut self, id: &ProjectId, starts: Vec<(u64, Effect)>) {
        let seq = self.newest.get(id).copied().unwrap_or(0);
        for (issue, start) in starts {
            self.starting.entry(id.clone()).or_default().insert(issue);
            self.held.push(super::Held {
                id: id.clone(),
                seq,
                issue,
                start,
            });
        }
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

    /// What [`super::Input::Ended`] brought: an end is the board's when a
    /// column running a workflow names the task. A done one is marked done in
    /// its column, which it then leaves as that column says; any other stays
    /// where it is; and an end that left room on the team lets the board look
    /// again. The end of a task no such entry names, one a person moved to a
    /// person's column or took up off the board, is not the board's.
    pub(super) fn ended(
        &mut self,
        id: &ProjectId,
        _issue: u64,
        task: &str,
        outcome: Option<&Outcome>,
    ) -> Vec<Effect> {
        let ours = self.boards.board(id).is_some_and(|b| {
            (b.columns.iter()).any(|c| {
                // Begun in this column: one moved here has no run until it is.
                workflow_of(c).is_some()
                    && (c.entries.iter())
                        .any(|e| e.task.as_deref() == Some(task) && e.run.is_some())
            })
        });
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
            fx.extend(self.edit(id, false, |board| board.map(|b| b.ended_done(task))));
        }
        if done || pulls_next(outcome) {
            fx.push(Effect::ReadIssues { id: id.clone() });
        }
        fx
    }
}

/// The workflow `column` runs, if it runs one.
pub(super) fn workflow_of(column: &crate::board::model::Column) -> Option<&str> {
    match &column.work {
        Work::Workflow { id } => Some(id),
        Work::Person => None,
    }
}

#[cfg(test)]
mod tests;
