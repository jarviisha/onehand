//! What a project's board holds and which of its To do to start: the
//! decisions only, over what the caller read, so the board's page and the
//! drive that pulls agree.

use crate::connector::Connector;
use crate::issues::{Issues, LocalIssue};
use crate::project::{Board, Project, ProjectId};
use crate::task::{Task, Working};
use crate::workflow::{Outcome, Place, Stop, Template};
use std::collections::HashSet;
use std::path::PathBuf;

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
    issues
        .listed()
        .into_iter()
        .filter(|issue| issues.workable(issue, forge))
        .filter(|issue| !board.todo.contains(&issue.number()) && !taken.contains(&issue.number()))
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
    for &number in &board.todo {
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

/// How many of `project`'s board tasks work: those the app says are doing
/// something. One cut off by a restart is not.
pub fn running<'a>(
    tasks: impl Iterator<Item = &'a Task>,
    project: &ProjectId,
    working: &dyn Fn(&str) -> Option<Working>,
) -> usize {
    tasks
        .filter(|task| task.board_issue().is_some_and(|(of, _)| of == project))
        .filter(|task| working(task.id()).is_some())
        .count()
}

/// Whether `task` stands on the board, and whether in Done: one working,
/// ended needing attention or done does; one a person stopped or took over,
/// or let go, does not.
fn on_board(task: &Task, working: &dyn Fn(&str) -> Option<Working>) -> Option<bool> {
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

/// The issues `project`'s board tasks hold out of the Backlog: those of the
/// tasks on the board. A task that left it gives its issue back.
pub fn taken<'a>(
    tasks: impl Iterator<Item = &'a Task>,
    project: &ProjectId,
    working: &dyn Fn(&str) -> Option<Working>,
) -> HashSet<u64> {
    tasks
        .filter_map(|task| match task.board_issue() {
            Some((of, issue)) if of == project => on_board(task, working).map(|_| issue),
            Some(_) | None => None,
        })
        .collect()
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

/// Place `tasks` in the columns of `template`'s steps by where their last
/// run is. A task at a step the template lost gets a column after the
/// others, labelled by the step's id. Done ones go in `done`; one a person
/// stopped or took over, or dismissed, is in no column.
pub fn columns<'a>(
    template: &Template,
    tasks: impl Iterator<Item = &'a Task>,
    working: &dyn Fn(&str) -> Option<Working>,
) -> Columns<'a> {
    let mut steps: Vec<Column<'a>> = template
        .steps
        .iter()
        .map(|step| Column {
            step: step.id.clone(),
            label: step.label.clone(),
            tasks: Vec::new(),
        })
        .collect();
    let mut done = Vec::new();
    for task in tasks {
        match on_board(task, working) {
            None => continue,
            Some(true) => {
                done.push(task);
                continue;
            }
            Some(false) => {}
        }
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
    done.sort_by_key(|task| std::cmp::Reverse(task.recency()));
    Columns { steps, done }
}

#[cfg(test)]
mod tests;
