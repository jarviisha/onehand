//! What a project's board holds and which of its To do to start: the
//! decisions only, over what the caller read, so the board's page and the
//! drive that pulls agree.

use crate::connector::Connector;
use crate::issues::{Issues, LocalIssue};
use crate::project::{Project, ProjectId};
use crate::task::{Task, Working};
use crate::workflow::{Outcome, Place, Stop, Template};
use std::collections::HashSet;
use std::path::PathBuf;

pub mod drive;
pub mod file;
pub mod model;

pub use model::{Board, Entry, MovesOn, Starts, Work};

/// How many tasks one column of the board draws; the rest are counted.
pub const BOARD_SHOWN: usize = 50;

/// Whether the board can run `template`: it must exist and work on a
/// worktree, since several tasks work at once and a checkout holds one.
pub fn usable(template: Option<&Template>) -> Result<&Template, String> {
    let Some(template) = template else {
        return Err("The board's workflow is gone or cannot be read".to_string());
    };
    match template.place {
        Place::Worktree => Ok(template),
        Place::Checkout => Err(format!(
            "A board needs a workflow that works on a worktree: {} works in the checkout",
            template.name
        )),
    }
}

/// Where `project`'s board works: its folder and the file its issues are
/// kept in. One on a host has no board that works, its issues not kept on
/// this machine.
pub fn local(project: &Project) -> Result<(PathBuf, PathBuf), String> {
    match project.host {
        Some(_) => Err("A board works on a project on this machine".to_string()),
        None => Ok((
            project.path.clone(),
            crate::project::file::issues_file(&project.id),
        )),
    }
}

/// The issues a person may put in To do: open ones a brief can be made from,
/// not already in To do and not `taken` by a task of the board, in the order
/// a list of issues reads.
pub fn backlog<'a>(
    issues: &'a Issues,
    forge: Option<&'static dyn Connector>,
    board: &Board,
    taken: &HashSet<u64>,
) -> Vec<&'a LocalIssue> {
    let todo = board.todo();
    issues
        .listed()
        .into_iter()
        .filter(|issue| issues.workable(issue, forge))
        .filter(|issue| !todo.contains(&issue.number()) && !taken.contains(&issue.number()))
        .collect()
}

/// What a look at To do decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    /// Issues to start now, first first.
    pub start: Vec<u64>,
    /// Issues dropped from To do because they are closed or no longer there.
    pub gone: Vec<u64>,
}

/// Which of `board`'s To do to start while `running` of its tasks work: as
/// many as the team has room for, and none when the team is full or was
/// lowered below what runs. When its workflow's first step is limited,
/// `first` is that limit and how many tasks hold or wait for room there, and
/// no more start than the step has room for. An issue closed or no longer in `issues` is
/// dropped. One open but not workable now, as a forge's while the forge is
/// not found, or one a task of the board still holds (`taken`), stays in
/// To do for a later look.
pub(crate) fn pull(
    board: &Board,
    running: usize,
    first: Option<(u32, usize)>,
    issues: &Issues,
    forge: Option<&'static dyn Connector>,
    taken: &HashSet<u64>,
) -> Pull {
    let team = usize::from(board.team).saturating_sub(running);
    let room = first.map_or(team, |(limit, at)| {
        team.min((limit as usize).saturating_sub(at))
    });
    let mut pull = Pull {
        start: Vec::new(),
        gone: Vec::new(),
    };
    for number in board.todo() {
        let Some(issue) = issues.get(number).filter(|issue| issue.open()) else {
            pull.gone.push(number);
            continue;
        };
        let ready = issues.workable(issue, forge) && !taken.contains(&number);
        if ready && pull.start.len() < room {
            pull.start.push(number);
        }
    }
    pull
}

/// Whether a board task's end closes its issue: it finished, and the issue
/// is open and the project's own, since one kept in step with a forge is
/// closed there, by what merges its work.
pub fn after_end(issue: &LocalIssue, synced: bool, done: bool) -> bool {
    done && !synced && issue.open()
}

/// Whether a board task that ended as `outcome` lets the board start the
/// next of its To do: one done or that a person stopped left the board. One
/// that ended without finishing is left for a person to look at and pulls
/// nothing, deliberately: a cause the tasks share would most likely end the
/// next the same way, one after another until To do is empty.
pub(crate) fn pulls_next(outcome: Option<&Outcome>) -> bool {
    match outcome {
        Some(Outcome::Done | Outcome::Stopped(Stop::ByPerson | Stop::TakenOver)) => true,
        Some(
            Outcome::Stopped(Stop::TimedOut | Stop::LinkLost | Stop::Closed)
            | Outcome::Exhausted { .. }
            | Outcome::Failed(_),
        )
        | None => false,
    }
}

/// How many of the tasks `board`'s work column names hold a place on its
/// team: those `holds` says do. One cut off by a restart does not, nor one
/// out of time at rest or waiting for a place.
pub fn running<'a>(
    tasks: impl Iterator<Item = &'a Task>,
    board: &Board,
    holds: &dyn Fn(&str) -> bool,
) -> usize {
    tasks
        .filter(|task| board.names(task.id()) == Some(model::Place::Work))
        .filter(|task| holds(task.id()))
        .count()
}

/// How an Extend pressed on a task out of time goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtendWay {
    /// At once: it still holds its place, or it has no board's team.
    Now,
    /// Once this project's board has a place for it.
    WaitForPlace(ProjectId),
}

/// How an Extend on `task` goes: a board's task at rest (not `holds_team`)
/// that its board still holds (`on_board`) waits for a place; any other goes
/// on at once.
pub fn extend_way(task: &Task, on_board: bool, holds_team: bool) -> ExtendWay {
    match task.board_issue() {
        Some((project, _)) if !holds_team && on_board => ExtendWay::WaitForPlace(project.clone()),
        Some(_) | None => ExtendWay::Now,
    }
}

/// Which of the extends `waiting` for a place on `board`'s team, oldest first,
/// are given one while `running` hold places: as many as there is room for.
pub(crate) fn extends_given(board: &Board, running: usize, waiting: &[String]) -> Vec<String> {
    let room = usize::from(board.team).saturating_sub(running);
    waiting.iter().take(room).cloned().collect()
}

/// The tasks `board`'s work column names whose extend waits for a place, by
/// when each asked (`asked`), first first.
pub fn extends_waiting<'a>(
    tasks: impl Iterator<Item = &'a Task>,
    board: &Board,
    asked: &dyn Fn(&str) -> Option<u64>,
) -> Vec<String> {
    let mut waiting: Vec<(u64, String)> = tasks
        .filter(|task| board.names(task.id()) == Some(model::Place::Work))
        .filter_map(|task| Some((asked(task.id())?, task.id().to_string())))
        .collect();
    waiting.sort();
    waiting.into_iter().map(|(_, id)| id).collect()
}

/// Whether `task` stands on the board, and whether in Done: one working,
/// ended needing attention or done does; one a person stopped or took over,
/// or let go, does not.
pub(crate) fn on_board(task: &Task, working: &dyn Fn(&str) -> Option<Working>) -> Option<bool> {
    let ended = match working(task.id()) {
        Some(Working::Queued | Working::Running | Working::Waiting) => None,
        None => task.outcome(),
    };
    if task.dismissed() {
        return None;
    }
    match ended {
        Some(Outcome::Done) => Some(true),
        Some(Outcome::Stopped(Stop::ByPerson | Stop::TakenOver)) => None,
        Some(
            Outcome::Stopped(Stop::TimedOut | Stop::LinkLost | Stop::Closed)
            | Outcome::Exhausted { .. }
            | Outcome::Failed(_),
        )
        | None => Some(false),
    }
}

/// `task` as the boards see it, while it is a board's: `working` says what
/// the app knows of whether it works.
pub fn seen(task: &Task, working: Option<Working>) -> Option<drive::Seen> {
    let (project, issue) = task.board_issue()?;
    Some(drive::Seen {
        id: task.id().to_string(),
        project: project.clone(),
        issue,
        run: task.runs().last().map(|run| run.id.clone()),
        standing: on_board(task, &|_| working),
        recency: task.recency(),
    })
}

/// The issues `board` holds out of the Backlog: those its columns and Done
/// name, and those of `project`'s board tasks that finished and are not let
/// go, so an issue whose task fell off Done does not come back.
pub fn taken<'a>(
    board: &Board,
    tasks: impl Iterator<Item = &'a Task>,
    project: &ProjectId,
) -> HashSet<u64> {
    let named = (board.columns.iter())
        .skip(1)
        .flat_map(|column| column.entries.iter())
        .chain(board.done())
        .map(|entry| entry.issue);
    let done = tasks.filter_map(|task| match task.board_issue() {
        Some((of, issue)) if of == project && !task.dismissed() => match task.outcome() {
            Some(Outcome::Done) => Some(issue),
            Some(Outcome::Stopped(_) | Outcome::Exhausted { .. } | Outcome::Failed(_)) | None => {
                None
            }
        },
        Some(_) | None => None,
    });
    named.chain(done).collect()
}

/// The board's columns: a step each, then what is done.
#[derive(Debug)]
pub struct Columns<'a> {
    pub steps: Vec<Column<'a>>,
    /// Newest first.
    pub done: Vec<&'a Task>,
}

/// One column: the tasks at a step.
#[derive(Debug)]
pub struct Column<'a> {
    pub step: String,
    pub label: String,
    pub tasks: Vec<&'a Task>,
}

/// Place the tasks `board`'s work column names in the columns of
/// `template`'s steps, by where their last run is, whatever it came to; a
/// task at a step the template lost gets a column after the others, labelled
/// by the step's id. Done is the tasks Done names, as it orders them.
pub fn columns<'a>(
    template: &Template,
    board: &Board,
    tasks: impl Iterator<Item = &'a Task>,
) -> Columns<'a> {
    let by_id: std::collections::HashMap<&str, &'a Task> =
        tasks.map(|task| (task.id(), task)).collect();
    let of = |entry: &Entry| entry.task.as_deref().and_then(|id| by_id.get(id).copied());
    let mut steps: Vec<Column<'a>> = template
        .steps
        .iter()
        .map(|step| Column {
            step: step.id.clone(),
            label: step.label.clone(),
            tasks: Vec::new(),
        })
        .collect();
    for task in board.work().iter().filter_map(of) {
        let Some(at) = task.runs.last().and_then(|run| run.current()) else {
            continue;
        };
        match steps.iter_mut().find(|column| column.step == at.id) {
            Some(column) => column.tasks.push(task),
            None => steps.push(Column {
                step: at.id.clone(),
                label: at.id.clone(),
                tasks: vec![task],
            }),
        }
    }
    let done = board.done().iter().filter_map(of).collect();
    Columns { steps, done }
}

#[cfg(test)]
mod tests;
