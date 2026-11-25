//! The tasks as the pages and the rail read them: the Tasks page's rows and
//! what needs a person.

use super::Tasks;
use gpui::{AnyWindowHandle, App};
use onehand_core::task::{Group, Origin, Source, Task, sort_listed};
use onehand_core::workflow::Run;
use std::path::PathBuf;

/// A task as the Tasks page lists it.
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Its workflow's name, after what started it if that was not a person.
    pub(crate) name: String,
    /// The step it is at while it works, or how it ended.
    pub(crate) at: String,
    /// The project it was started from.
    pub(crate) project: PathBuf,
    pub(crate) group: Group,
    /// The session it runs in, and the window that holds it.
    pub(crate) session: Option<(u64, AnyWindowHandle)>,
    pub(crate) resumable: bool,
    /// Something answers Stop: a place asked for, a run driven or a check
    /// running. A task that ended but still holds its place, its session's
    /// turn not yet over, has nothing left to stop.
    pub(crate) stoppable: bool,
    /// Stop was pressed and the task has not given its place up yet: its
    /// step's command, or its check, is still being stopped.
    pub(crate) stopping: bool,
}

/// Whether a check is running on the project at `root`, so its *Run check*
/// says it is busy.
pub(crate) fn checking(root: &std::path::Path, cx: &App) -> bool {
    cx.try_global::<Tasks>().is_some_and(|t| {
        t.tasks().iter().any(|task| {
            task.setup().repo == root
                && *task.source() == Source::Check
                && t.live.contains_key(task.id())
        })
    })
}

/// Every task of the projects at `roots`: finished ones newest first, every
/// other group oldest first.
pub(crate) fn rows(roots: &[PathBuf], cx: &App) -> Vec<Row> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    let mut rows: Vec<(Option<&Task>, Row)> = Vec::new();
    let tasks = t
        .tasks()
        .iter()
        .filter(|task| roots.contains(&task.setup().repo) || roots.contains(&task.setup().dir));
    for task in tasks {
        rows.push((Some(task), row_of(t, task)));
    }
    sort_listed(&mut rows, |(task, row)| (row.group, *task));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Task `task` as the pages list it.
fn row_of(t: &Tasks, task: &Task) -> Row {
    let working = t.working(task.id());
    let run = task.runs().last();
    // A prompt's one step is the task itself: naming it would only say
    // *Prompt* twice.
    let step = run
        .filter(|run| !run.template.is_prompt())
        .and_then(Run::current)
        .map_or_else(String::new, |step| step.label.clone());
    let at = match (working, task.ended_said()) {
        (Some(_), _) => step,
        (None, None) if step.is_empty() => "cut off".to_string(),
        (None, None) => format!("cut off at {step}"),
        (None, Some(said)) => said,
    };
    let live = t.live.get(task.id());
    Row {
        id: task.id().to_string(),
        title: task.brief().title.clone(),
        name: {
            let workflow = run.map_or_else(String::new, |run| run.template.name.clone());
            match &task.origin() {
                Some(Origin::Schedule { name, .. }) => format!("{name} · {workflow}"),
                Some(Origin::Board { .. }) => format!("from the board · {workflow}"),
                Some(Origin::Tool) => format!("by a tool · {workflow}"),
                None => workflow,
            }
        },
        at,
        project: task.setup().repo.clone(),
        group: task.group(working),
        session: live.and_then(|d| Some((d.uid()?, d.window))),
        resumable: task.resumable(),
        stoppable: live.is_some() || t.drive.queue().queued(task.id()),
        stopping: t.stopping(task.id()),
    }
}

/// How many tasks of the projects at `roots` need a person: waiting on one,
/// or ended on something nobody chose. Counted from [`rows`], so the rail
/// and the page cannot disagree.
pub(crate) fn attention(roots: &[PathBuf], cx: &App) -> usize {
    rows(roots, cx)
        .iter()
        .filter(|row| row.group.needs_attention())
        .count()
}

/// How many finished tasks of the projects at `roots` were let go since
/// the app started, to keep the history bounded.
pub(crate) fn removed(roots: &[PathBuf], cx: &App) -> usize {
    cx.try_global::<Tasks>()
        .map_or(0, |t| roots.iter().map(|root| t.drive.removed(root)).sum())
}

/// Whether task `id` is queued, running or waiting on a person.
pub(crate) fn is_working(id: &str, cx: &App) -> bool {
    cx.try_global::<Tasks>().is_some_and(|t| t.busy(id))
}

/// How many of `project`'s board tasks are queued, running or waiting on a
/// person.
pub(crate) fn board_running(project: &onehand_core::project::ProjectId, cx: &App) -> usize {
    cx.try_global::<Tasks>().map_or(0, |t| {
        onehand_core::board::running(t.tasks().iter(), project, &|id| t.working(id))
    })
}

/// Project `project`'s board tasks laid over `template`'s steps, each as
/// the pages list it.
pub(crate) struct BoardTasks {
    /// A column per step, by its label, then any step the workflow lost.
    pub(crate) steps: Vec<(String, Vec<Row>)>,
    /// Newest first.
    pub(crate) done: Vec<Row>,
    /// The issues its tasks hold out of the Backlog.
    pub(crate) taken: std::collections::HashSet<u64>,
}

/// Project `project`'s board tasks in the columns of `template`, as core
/// places them.
pub(crate) fn board(
    project: &onehand_core::project::ProjectId,
    template: &onehand_core::workflow::Template,
    cx: &App,
) -> BoardTasks {
    let Some(t) = cx.try_global::<Tasks>() else {
        return BoardTasks {
            steps: Vec::new(),
            done: Vec::new(),
            taken: Default::default(),
        };
    };
    let working = |id: &str| t.working(id);
    let of_board = || {
        t.tasks()
            .iter()
            .filter(|task| task.board_issue().is_some_and(|(of, _)| of == project))
    };
    let columns = onehand_core::board::columns(template, of_board(), &working);
    let rows = |tasks: Vec<&Task>| tasks.into_iter().map(|task| row_of(t, task)).collect();
    BoardTasks {
        steps: columns
            .steps
            .into_iter()
            .map(|column| (column.label, rows(column.tasks)))
            .collect(),
        done: rows(columns.done),
        taken: onehand_core::board::taken(of_board(), project, &working),
    }
}
