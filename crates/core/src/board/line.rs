//! What a row of a board says under its title, and the one thing it offers
//! a person, worked out each time from where its issue stands.

use super::model::Column;
use super::{Board, MovesOn, Starts, Work};

/// What a row says of where its issue stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// In a person's column since this time, in seconds.
    Since(u64),
    /// In a column that starts on a press, not started.
    WaitingForStart,
    /// In a column that starts on entry, waiting for a place on the team.
    WaitingForTeam,
    /// Its start is on its way.
    Starting,
    /// Its task works here: the row says what the task says.
    Task,
    /// Its run here ended done, and a person moves it on.
    DoneHere,
    /// Its run here ended done, and the column it moves on to is full.
    WaitingForRoom(String),
    /// A start asked for it never landed.
    DidNotStart,
    /// Not started, and its column cannot start it, for this reason.
    CannotStart(String),
}

/// The one thing a row offers about its issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAct {
    Start,
    /// Move it to the column with this id, or to Done with none.
    MoveTo(Option<String>),
}

impl Line {
    /// The words a row says, but for [`Line::Since`], whose age the page
    /// draws, and [`Line::Task`], whose words are its task's.
    pub fn words(&self) -> String {
        match self {
            Line::Since(_) | Line::Task => String::new(),
            Line::WaitingForStart => "Waiting for Start".into(),
            Line::WaitingForTeam => "Waiting for a team place".into(),
            Line::Starting => "Starting".into(),
            Line::DoneHere => "Done here".into(),
            Line::WaitingForRoom(column) => format!("Waiting for room in {column}"),
            Line::DidNotStart => "Did not start".into(),
            Line::CannotStart(why) => why.clone(),
        }
    }
}

/// What the row of `issue` on `board` says and offers: `run_here` says
/// whether the run its entry asked for is one of its task's (no task kept,
/// or a next run refused, and it is not), `working` whether that task works,
/// `starting` whether a start of it is on its way, and `start` why its
/// column cannot start it, when it cannot.
pub fn line(
    board: &Board,
    issue: u64,
    run_here: bool,
    working: bool,
    starting: bool,
    start: Option<&str>,
) -> (Line, Option<EntryAct>) {
    let Some((at, entry)) = board.at(issue) else {
        return (Line::Task, None);
    };
    let column = &board.columns[at];
    let next = board.columns.get(at + 1);
    match column.work {
        Work::Person => (Line::Since(entry.since), None),
        Work::Workflow { .. } => {
            if starting {
                return (Line::Starting, None);
            }
            if entry.done {
                return done_here(column, next);
            }
            match (&entry.run, run_here || working, start) {
                (Some(_), true, _) => (Line::Task, None),
                (Some(_), false, _) => (Line::DidNotStart, Some(EntryAct::Start)),
                // Waiting would never end: it says why instead.
                (None, _, Some(why)) => (Line::CannotStart(why.into()), None),
                (None, _, None) => match column.starts {
                    Starts::OnPress => (Line::WaitingForStart, Some(EntryAct::Start)),
                    Starts::OnEntry => (Line::WaitingForTeam, None),
                },
            }
        }
    }
}

/// What a row done in `column` says, `next` the column it moves on to.
fn done_here(column: &Column, next: Option<&Column>) -> (Line, Option<EntryAct>) {
    match column.moves_on {
        MovesOn::ByPerson => (
            Line::DoneHere,
            Some(EntryAct::MoveTo(next.map(|c| c.id.clone()))),
        ),
        MovesOn::ByItself => match next {
            Some(next) => (Line::WaitingForRoom(next.name.clone()), None),
            None => (Line::DoneHere, None),
        },
    }
}

#[cfg(test)]
mod tests;
