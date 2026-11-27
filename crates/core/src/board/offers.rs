//! What a row of a board offers a person about its issue, in its menu's
//! order, each with why it refuses when it does: the one thing its line
//! offers first, then the moves, then taking it off the board.

use super::line::EntryAct;
use super::Board;

/// What the front end knows of where a row's issue stands beside the board:
/// whether its task works, whether its start is on its way, and why a start
/// of it would be refused (its column's workflow gone or not able to run).
#[derive(Debug, Clone, Default)]
pub struct Standing {
    pub working: bool,
    pub starting: bool,
    pub start: Option<String>,
}

/// One thing a row offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Start,
    /// Move the issue to the column with id `column`, named `name`, or to
    /// Done with none; `back` when that column comes just before its own.
    MoveTo {
        column: Option<String>,
        name: String,
        back: bool,
    },
    /// One place up, or down, in its column.
    Up,
    Down,
    /// Back to the Backlog: off the board, its task kept as history.
    Back,
}

impl Offer {
    /// What a row's menu says of it, `has_task` when a task works the issue.
    pub fn words(&self, has_task: bool) -> String {
        match self {
            Offer::Start => "Start".into(),
            Offer::MoveTo { column: None, .. } => "Move to Done".into(),
            Offer::MoveTo {
                column: Some(_),
                name,
                back: true,
            } => format!("Move back to {name}"),
            Offer::MoveTo {
                column: Some(_),
                name,
                back: false,
            } => format!("Move to {name}"),
            Offer::Up => "Move up".into(),
            Offer::Down => "Move down".into(),
            Offer::Back => match has_task {
                true => "Remove from board".into(),
                false => "Back to Backlog".into(),
            },
        }
    }
}

/// What the row of `issue` on `board` offers, its line's `act` first, each
/// with why it refuses: everything while its task works or its start is on
/// its way, a start its column cannot make, a move into a full column, and
/// a move past its column's end.
pub fn offers(
    board: &Board,
    issue: u64,
    act: Option<&EntryAct>,
    standing: &Standing,
) -> Vec<(Offer, Option<String>)> {
    let Some((at, _)) = board.at(issue) else {
        return Vec::new();
    };
    let to = |index: Option<usize>, back: bool| -> Offer {
        match index.and_then(|i| board.columns.get(i)) {
            Some(column) => Offer::MoveTo {
                column: Some(column.id.clone()),
                name: column.name.clone(),
                back,
            },
            None => Offer::MoveTo {
                column: None,
                name: "Done".into(),
                back: false,
            },
        }
    };
    let mut wanted: Vec<Offer> = Vec::new();
    match act {
        Some(EntryAct::Start) => wanted.push(Offer::Start),
        Some(EntryAct::MoveTo(column)) => {
            let index = column
                .as_ref()
                .and_then(|id| board.columns.iter().position(|c| &c.id == id));
            wanted.push(to(index, index.is_some_and(|i| i + 1 == at)));
        }
        None => {}
    }
    wanted.push(to(Some(at + 1), false));
    if let Some(before) = at.checked_sub(1) {
        wanted.push(to(Some(before), true));
    }
    wanted.push(to(None, false));
    for other in 0..board.columns.len() {
        if other != at && other + 1 != at && other != at + 1 {
            wanted.push(to(Some(other), false));
        }
    }
    wanted.extend([Offer::Up, Offer::Down, Offer::Back]);
    let mut seen = Vec::new();
    wanted.retain(|offer| {
        let key = offer.words(false);
        let fresh = !seen.contains(&key);
        seen.push(key);
        fresh
    });
    let column = &board.columns[at];
    let place = column
        .entries
        .iter()
        .position(|e| e.issue == issue)
        .unwrap_or(0);
    let last = column.entries.len().saturating_sub(1);
    wanted
        .into_iter()
        .map(|offer| {
            let why = match &offer {
                Offer::Start | Offer::MoveTo { .. } | Offer::Up | Offer::Down | Offer::Back
                    if standing.starting =>
                {
                    Some("It is starting: wait for it to start".to_string())
                }
                Offer::Start | Offer::MoveTo { .. } | Offer::Up | Offer::Down | Offer::Back
                    if standing.working =>
                {
                    Some("It is at work: Stop it first".to_string())
                }
                Offer::Start => standing.start.clone(),
                Offer::MoveTo {
                    column: Some(id),
                    name,
                    ..
                } => board.column(id).and_then(|c| {
                    let others = c.entries.iter().filter(|e| e.issue != issue).count();
                    c.limit
                        .is_some_and(|limit| others >= limit as usize)
                        .then(|| format!("{name} is full"))
                }),
                Offer::MoveTo { column: None, .. } | Offer::Back => None,
                Offer::Up => (place == 0).then(|| "It is at the top already".to_string()),
                Offer::Down => (place >= last).then(|| "It is at the bottom already".to_string()),
            };
            (offer, why)
        })
        .collect()
}

#[cfg(test)]
mod tests;
