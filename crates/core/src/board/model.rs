//! A project's board as it is kept: its columns between the Backlog, which
//! is the tracker's and not kept, and Done, each column's issues in the
//! person's order.
//!
//! This build knows one shape only, the one a board bound to a workflow has
//! always had: To do, a person's column whose issues move on by themselves
//! into one column that runs the workflow, and Done. A board in any other
//! shape was written by a newer onehand and is not read.

use serde::{Deserialize, Serialize};

/// The version of the board file this build writes and reads.
pub const SCHEMA_VERSION: u32 = 2;

/// The version a board was written at before its columns named their
/// tasks: read, and upgraded once the tasks are read.
const FIRST_VERSION: u32 = 1;

/// How many done issues a board keeps.
pub const DONE_KEPT: usize = crate::board::BOARD_SHOWN;

/// The most tasks a board's team may run at once.
pub const MOST_TEAM: u8 = 8;

/// The team a new board starts with.
pub(crate) const FIRST_TEAM: u8 = 2;

/// The id of To do, the column a person fills.
const TODO: &str = "todo";

/// The id of the column that runs the board's workflow.
const WORK: &str = "work";

/// A project's board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub schema_version: u32,
    /// How many of the board's tasks work at once, 1 to [`MOST_TEAM`].
    pub team: u8,
    /// Between the Backlog and Done, in order.
    pub columns: Vec<Column>,
    /// What is done, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done: Vec<Entry>,
}

/// One column of a board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Stable across a rename.
    pub id: String,
    pub name: String,
    pub work: Work,
    /// Read only when the column runs a workflow.
    pub starts: Starts,
    pub moves_on: MovesOn,
    /// How many issues the column holds at most; none for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// In the person's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<Entry>,
}

/// What a column does with the issues in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Work {
    /// A person works them and moves them on.
    Person,
    /// A run of the workflow with this id works each.
    Workflow { id: String },
}

/// When a column's run starts on an issue that entered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Starts {
    OnEntry,
    OnPress,
}

/// How an issue ready to leave a column leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovesOn {
    ByItself,
    ByPerson,
}

/// One issue's place on a board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub issue: u64,
    /// The task working the issue, once a column with work took it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The run the board last asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// When it entered its column, in seconds.
    pub since: u64,
}

impl Board {
    /// A board running `workflow` with a team of `team` and `todo` in To do,
    /// each entered at `since`.
    pub fn made(workflow: String, team: u8, todo: &[u64], since: u64) -> Board {
        let entry = |&issue: &u64| Entry {
            issue,
            task: None,
            run: None,
            since,
        };
        Board {
            schema_version: SCHEMA_VERSION,
            team: team.clamp(1, MOST_TEAM),
            columns: vec![
                Column {
                    id: TODO.to_string(),
                    name: "To do".to_string(),
                    work: Work::Person,
                    starts: Starts::OnEntry,
                    moves_on: MovesOn::ByItself,
                    limit: None,
                    entries: todo.iter().map(entry).collect(),
                },
                Column {
                    id: WORK.to_string(),
                    name: "Work".to_string(),
                    work: Work::Workflow { id: workflow },
                    starts: Starts::OnEntry,
                    moves_on: MovesOn::ByItself,
                    limit: None,
                    entries: Vec::new(),
                },
            ],
            done: Vec::new(),
        }
    }

    /// The id of the workflow the board's tasks run.
    pub fn workflow(&self) -> &str {
        match self.columns.get(1).map(|column| &column.work) {
            Some(Work::Workflow { id }) => id,
            Some(Work::Person) | None => "",
        }
    }

    /// The issues in To do, first first.
    pub fn todo(&self) -> Vec<u64> {
        (self.columns.first().into_iter())
            .flat_map(|column| column.entries.iter().map(|entry| entry.issue))
            .collect()
    }

    /// The board running `workflow` instead, all else kept.
    pub(crate) fn rebound(&self, workflow: String) -> Board {
        let mut board = self.clone();
        if let Some(column) = board.columns.get_mut(1) {
            column.work = Work::Workflow { id: workflow };
        }
        board
    }

    /// The board with a team of `team`, held within 1 to [`MOST_TEAM`].
    pub(crate) fn with_team(&self, team: u8) -> Board {
        Board {
            team: team.clamp(1, MOST_TEAM),
            ..self.clone()
        }
    }

    /// The board with `todo` in To do, in that order: an issue already there
    /// keeps when it entered, a new one entered at `now`.
    pub(crate) fn with_todo(&self, todo: &[u64], now: u64) -> Board {
        let mut board = self.clone();
        if let Some(column) = board.columns.first_mut() {
            let since = |issue: u64| {
                (column.entries.iter())
                    .find(|entry| entry.issue == issue)
                    .map_or(now, |entry| entry.since)
            };
            let entries = (todo.iter())
                .map(|&issue| Entry {
                    issue,
                    task: None,
                    run: None,
                    since: since(issue),
                })
                .collect();
            column.entries = entries;
        }
        board
    }

    /// Whether this build can keep the board: its version, and the one shape
    /// it knows.
    pub(crate) fn shape(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION && self.schema_version != FIRST_VERSION {
            return Err(format!(
                "it was written as version {}, and this build of onehand reads version \
                 {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let known = match self.columns.as_slice() {
            [todo, work] => {
                todo.id == TODO
                    && todo.work == Work::Person
                    && work.id == WORK
                    && matches_workflow(&work.work)
                    && todo
                        .entries
                        .iter()
                        .all(|e| e.task.is_none() && e.run.is_none())
                    && self.entries_known(work)
            }
            _ => false,
        };
        match known {
            true => Ok(()),
            false => Err("its columns are of a shape this build of onehand does not know".into()),
        }
    }
}

/// Where on a board an entry naming a task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Work,
    Done,
}

impl Board {
    /// Whether the work column's and Done's entries are as this version
    /// keeps them: none before the tasks were named, and after, each naming
    /// its task, the work column's its run too, and Done within its keep.
    fn entries_known(&self, work: &Column) -> bool {
        match self.schema_version {
            FIRST_VERSION => work.entries.is_empty() && self.done.is_empty(),
            _ => {
                work.entries
                    .iter()
                    .all(|e| e.task.is_some() && e.run.is_some())
                    && self.done.iter().all(|e| e.task.is_some())
                    && self.done.len() <= DONE_KEPT
            }
        }
    }

    /// The board as it stands before its tasks are taken up: what a board
    /// moved out of its project is, its tasks there already.
    pub(crate) fn unadopted(mut self) -> Board {
        self.schema_version = FIRST_VERSION;
        self
    }

    /// Whether the board was written before its columns named their tasks,
    /// and takes them up once they are read.
    pub(crate) fn adopts(&self) -> bool {
        self.schema_version == FIRST_VERSION
    }

    /// The work column's entries, first first.
    pub fn work(&self) -> &[Entry] {
        self.columns.get(1).map_or(&[], |column| &column.entries)
    }

    /// What is done, newest first.
    pub fn done(&self) -> &[Entry] {
        &self.done
    }

    /// Where an entry naming `task` stands, if one does.
    pub fn names(&self, task: &str) -> Option<Place> {
        let named = |entries: &[Entry]| entries.iter().any(|e| e.task.as_deref() == Some(task));
        match (named(self.work()), named(&self.done)) {
            (true, _) => Some(Place::Work),
            (false, true) => Some(Place::Done),
            (false, false) => None,
        }
    }

    /// The board at this version, its work column holding `work` and Done
    /// `done` (newest first, kept to [`DONE_KEPT`]), none of their issues
    /// left in To do.
    pub(crate) fn upgraded(&self, work: Vec<Entry>, mut done: Vec<Entry>) -> Board {
        let mut board = self.clone();
        board.schema_version = SCHEMA_VERSION;
        let named: Vec<u64> = work.iter().chain(&done).map(|e| e.issue).collect();
        if let Some(todo) = board.columns.first_mut() {
            todo.entries.retain(|e| !named.contains(&e.issue));
        }
        if let Some(column) = board.columns.get_mut(1) {
            column.entries = work;
        }
        done.truncate(DONE_KEPT);
        board.done = done;
        board
    }

    /// The board with `issue` taken out of To do into the work column, as
    /// task `task`'s first run, entered at `now`.
    pub(crate) fn started(&self, issue: u64, task: &str, now: u64) -> Board {
        let mut board = self.clone();
        if let Some(todo) = board.columns.first_mut() {
            todo.entries.retain(|e| e.issue != issue);
        }
        if let Some(column) = board.columns.get_mut(1) {
            column.entries.push(Entry {
                issue,
                task: Some(task.to_string()),
                run: Some(task.to_string()),
                since: now,
            });
        }
        board
    }

    /// The board with task `task`'s work entry moved to the front of Done,
    /// entered at `now`.
    pub(crate) fn finished(&self, task: &str, now: u64) -> Board {
        let mut board = self.clone();
        let Some(column) = board.columns.get_mut(1) else {
            return board;
        };
        let Some(at) = (column.entries.iter()).position(|e| e.task.as_deref() == Some(task)) else {
            return board;
        };
        let entry = column.entries.remove(at);
        board.done.insert(
            0,
            Entry {
                run: None,
                since: now,
                ..entry
            },
        );
        board.done.truncate(DONE_KEPT);
        board
    }

    /// The board with `issue` in no column and not in Done.
    pub(crate) fn removed(&self, issue: u64) -> Board {
        let mut board = self.clone();
        for column in &mut board.columns {
            column.entries.retain(|e| e.issue != issue);
        }
        board.done.retain(|e| e.issue != issue);
        board
    }
}

fn matches_workflow(work: &Work) -> bool {
    match work {
        Work::Workflow { .. } => true,
        Work::Person => false,
    }
}

#[cfg(test)]
mod tests;
