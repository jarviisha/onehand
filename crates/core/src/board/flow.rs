//! A look at a board of any columns, decided whole: the closed issues no
//! task works dropped, the entries ready to move on that move on by
//! themselves moved, and the entries waiting in a column that starts on
//! entry started, within the team's places and each workflow's first-step
//! room. What the look decides, the board's drive writes and carries out.

use super::model::{Board, Column};
use super::{Entry, MovesOn, Starts, Work};
use crate::connector::Connector;
use crate::issues::Issues;
use std::collections::{HashMap, HashSet};

/// The room a look has to start in: the places left on the team, and, for
/// each workflow whose first step is limited, its limit and how many hold or
/// wait for room there.
pub(crate) struct Room {
    pub(crate) team: usize,
    pub(crate) first: HashMap<String, Option<(u32, usize)>>,
}

/// What a look decided.
pub(crate) struct Decided {
    /// The board as it now stands, its starts not yet begun.
    pub(crate) board: Board,
    /// What to start, in order.
    pub(crate) starts: Vec<Begin>,
    /// The issues that entered Done.
    pub(crate) closing: Vec<u64>,
}

/// A start a look decided, in the column at `column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Begin {
    /// Issue `issue`, which no task works yet.
    New { issue: u64, column: usize },
    /// Issue `issue`, which task `task` works: a next run of it.
    Next {
        issue: u64,
        task: String,
        column: usize,
    },
}

/// Look at `board` over its issues as just read: see the module header.
/// `usable` says whether a column's workflow can run now.
pub(crate) fn look(
    board: &Board,
    issues: &Issues,
    forge: Option<&'static dyn Connector>,
    room: Room,
    usable: &dyn Fn(&str) -> bool,
    now: u64,
) -> Decided {
    let mut room = room;
    let open = |issue: u64| issues.get(issue).is_some_and(|i| i.open());
    let workable =
        |issue: u64| (issues.get(issue)).is_some_and(|i| i.open() && issues.workable(i, forge));
    let mut board = board.clone();
    for column in &mut board.columns {
        column.entries.retain(|e| e.task.is_some() || open(e.issue));
    }
    let mut starts = Vec::new();
    let mut closing = Vec::new();
    let mut started: HashSet<u64> = HashSet::new();
    // Last first, so the room a move leaves downstream is seen by the column
    // before, and an entry moves at most once.
    for at in (0..board.columns.len()).rev() {
        let from = &board.columns[at];
        if from.moves_on != MovesOn::ByItself {
            continue;
        }
        let ready: Vec<Entry> = (from.entries.iter())
            .filter(|e| ready(from, e))
            .cloned()
            .collect();
        for entry in ready {
            let Some(next) = board.columns.get(at + 1) else {
                board = board.to_done(entry.issue, now);
                closing.push(entry.issue);
                continue;
            };
            if next
                .limit
                .is_some_and(|limit| next.entries.len() >= limit as usize)
            {
                break;
            }
            if let Some(workflow) = starting(next) {
                if !usable(workflow) || !room.fits(workflow) {
                    break;
                }
                if entry.task.is_none() && !workable(entry.issue) {
                    continue;
                }
                room.take(workflow);
                starts.push(begin(&entry, at + 1));
                started.insert(entry.issue);
            }
            board = board.placed(entry.issue, at + 1, None, now);
        }
    }
    for (at, column) in board.columns.iter().enumerate() {
        let Some(workflow) = starting(column) else {
            continue;
        };
        if !usable(workflow) {
            continue;
        }
        for entry in &column.entries {
            if entry.run.is_some() || entry.done || started.contains(&entry.issue) {
                continue;
            }
            if !room.fits(workflow) {
                break;
            }
            if entry.task.is_none() && !workable(entry.issue) {
                continue;
            }
            room.take(workflow);
            starts.push(begin(entry, at));
        }
    }
    Decided {
        board,
        starts,
        closing,
    }
}

/// Whether `entry` is ready to leave `column`: a person's column's always,
/// a workflow column's once its run there ended done.
fn ready(column: &Column, entry: &Entry) -> bool {
    match column.work {
        Work::Person => true,
        Work::Workflow { .. } => entry.done,
    }
}

/// The workflow `column` starts on an issue entering it, if it starts one so.
fn starting(column: &Column) -> Option<&str> {
    match (&column.work, column.starts) {
        (Work::Workflow { id }, Starts::OnEntry) => Some(id),
        (Work::Workflow { .. }, Starts::OnPress) | (Work::Person, _) => None,
    }
}

/// The start of `entry` in the column at `column`.
fn begin(entry: &Entry, column: usize) -> Begin {
    match &entry.task {
        Some(task) => Begin::Next {
            issue: entry.issue,
            task: task.clone(),
            column,
        },
        None => Begin::New {
            issue: entry.issue,
            column,
        },
    }
}

impl Room {
    /// Whether one more run of `workflow` fits: a place on the team, and room
    /// at its first step when that is limited.
    fn fits(&self, workflow: &str) -> bool {
        let first = match self.first.get(workflow) {
            Some(Some((limit, at))) => *at < *limit as usize,
            Some(None) | None => true,
        };
        self.team > 0 && first
    }

    /// One more run of `workflow` takes its place and its room.
    fn take(&mut self, workflow: &str) {
        self.team = self.team.saturating_sub(1);
        if let Some(Some((_, at))) = self.first.get_mut(workflow) {
            *at += 1;
        }
    }
}

#[cfg(test)]
mod tests;
