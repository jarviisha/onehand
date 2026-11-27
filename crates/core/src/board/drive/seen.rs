//! The tasks read at boot, as the boards take them: a board written before
//! its columns named their tasks takes its tasks up once, every board lets
//! go of an entry whose task is gone, and the tasks the boards hold are said.

use super::{BoardDrive, Effect};
use crate::board::Entry;
use crate::project::ProjectId;
use crate::Now;

/// One board task as the boards see it, read by the front end.
#[derive(Debug, Clone)]
pub struct Seen {
    pub id: String,
    pub project: ProjectId,
    pub issue: u64,
    /// Its last run's id; none before it had one.
    pub run: Option<String>,
    /// Whether it is on its board: done (`Some(true)`), there to work or
    /// look at (`Some(false)`), or let go (`None`).
    pub standing: Option<bool>,
    /// When it last moved, to order what is taken up.
    pub recency: (u64, u128),
}

impl BoardDrive {
    /// What [`super::Input::TasksRead`] brought: each board written before
    /// its columns named their tasks takes its tasks up, and what the boards
    /// hold is said.
    pub(super) fn tasks_read(&mut self, tasks: &[Seen], now: Now) -> Vec<Effect> {
        let ids: Vec<ProjectId> = self.boards.boards.keys().cloned().collect();
        let mut fx = Vec::new();
        for id in &ids {
            let of: Vec<&Seen> = tasks.iter().filter(|seen| seen.project == *id).collect();
            // An entry whose task is not there stays, to read as a start that
            // did not land, for a person to start again.
            fx.extend(self.edit(id, false, |board| {
                let board = board?;
                Some(match board.adopts() {
                    true => adopted(&board, &of, now),
                    false => board,
                })
            }));
        }
        fx.push(self.say_held());
        fx
    }

    /// The tasks every board holds: those its work column's entries name.
    pub(super) fn say_held(&self) -> Effect {
        Effect::Held {
            tasks: self.boards.held(),
        }
    }
}

/// `board` with `tasks` taken up: those on it in its work column, oldest
/// first, and the done ones in Done, newest first.
fn adopted(board: &crate::board::Board, tasks: &[&Seen], now: Now) -> crate::board::Board {
    let mut on: Vec<&Seen> = tasks.to_vec();
    on.sort_by_key(|seen| seen.recency);
    let entry = |seen: &Seen, run: Option<String>| Entry {
        issue: seen.issue,
        task: Some(seen.id.clone()),
        run,
        since: now.secs,
        done: false,
    };
    let work = (on.iter())
        .filter(|seen| seen.standing == Some(false))
        .map(|seen| {
            entry(
                seen,
                Some(seen.run.clone().unwrap_or_else(|| seen.id.clone())),
            )
        })
        .collect();
    let done = (on.iter().rev())
        .filter(|seen| seen.standing == Some(true))
        .map(|seen| entry(seen, None))
        .collect();
    board.upgraded(work, done)
}

#[cfg(test)]
mod tests;
