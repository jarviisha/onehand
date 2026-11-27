//! The tasks as the pages and the rail read them: the Tasks page's rows and
//! what needs a person.

use super::Tasks;
use gpui::{AnyWindowHandle, App};
use onehand_core::task::work::{Around, PrSeen, Work, next_action};
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
    /// What it waits on a person for, or why it ended, in the words its
    /// detail has; none for a task working or finished.
    pub(crate) waits: Option<String>,
    /// It works, but waits for room at a full step before going on there.
    pub(crate) waits_for_room: bool,
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
    /// Its run ran out of time and waits to be extended.
    pub(crate) extendable: bool,
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
    let waits_for_room = working.is_some() && t.drive.waits_for_room(task.id()).is_some();
    let at = match (working, task.ended_said()) {
        (Some(_), _) if t.extend_asked(task.id()).is_some() => {
            "Waiting for a team place".to_string()
        }
        (Some(_), _) if waits_for_room => format!("Waiting for room at {step}"),
        (Some(_), _) => step,
        (None, None) if step.is_empty() => "cut off".to_string(),
        (None, None) => format!("cut off at {step}"),
        (None, Some(said)) => said,
    };
    let group = task.group(working);
    let waits = match group {
        Group::Waiting | Group::Ended => {
            let work = Work::of(task, working, None);
            let around = Around {
                open: true,
                session: false,
                pr: PrSeen::Unread,
                now: onehand_core::Now::read().secs,
            };
            next_action(Some(&work), around).said
        }
        Group::Running | Group::Queued | Group::Finished => None,
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
        waits,
        waits_for_room,
        project: task.setup().repo.clone(),
        group,
        session: live.and_then(|d| Some((d.uid()?, d.window))),
        resumable: task.resumable(),
        stoppable: live.is_some() || t.drive.queue().queued(task.id()),
        stopping: t.stopping(task.id()),
        extendable: live.is_some_and(|d| d.run().awaiting_extend())
            && t.extend_asked(task.id()).is_none(),
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

/// Every board task as the boards see it.
pub(crate) fn board_seen(cx: &App) -> Vec<onehand_core::board::drive::Seen> {
    cx.try_global::<Tasks>().map_or(Vec::new(), |t| {
        (t.tasks().iter())
            .filter_map(|task| onehand_core::board::seen(task, t.working(task.id())))
            .collect()
    })
}

/// How many of `project`'s board tasks hold a place on its team.
pub(crate) fn board_running(project: &onehand_core::project::ProjectId, cx: &App) -> usize {
    let Some(board) = crate::state::Shared::global(cx).boards.board(project) else {
        return 0;
    };
    cx.try_global::<Tasks>().map_or(0, |t| {
        onehand_core::board::running(t.tasks().iter(), board, &|id| t.holds_team(id))
    })
}

/// `project`'s board tasks whose extend waits for a team place, first
/// asked first.
pub(crate) fn board_extends_waiting(
    project: &onehand_core::project::ProjectId,
    cx: &App,
) -> Vec<String> {
    let Some(board) = crate::state::Shared::global(cx).boards.board(project) else {
        return Vec::new();
    };
    cx.try_global::<Tasks>().map_or(Vec::new(), |t| {
        onehand_core::board::extends_waiting(t.tasks().iter(), board, &|id| t.extend_asked(id))
    })
}

/// The limit of `template`'s step at `at`, for runs of the project at
/// `root`, and how many hold room there and wait for it; none when the step
/// is not limited.
pub(crate) fn room_at(
    root: &std::path::Path,
    template: &onehand_core::workflow::Template,
    at: usize,
    cx: &App,
) -> Option<onehand_core::task::rooms::Room> {
    // With no tasks kept yet there is no room to count: no limit and no
    // first-step cap apply before the tasks' drive exists.
    cx.try_global::<Tasks>()?.drive.room(root, template, at)
}

/// Project `project`'s board tasks laid over `template`'s steps, each as
/// the pages list it.
pub(crate) struct BoardTasks {
    /// A column per step; then any step the workflow lost.
    pub(crate) steps: Vec<StepColumn>,
    /// Newest first.
    pub(crate) done: Vec<Row>,
    /// The issues its tasks hold out of the Backlog.
    pub(crate) taken: std::collections::HashSet<u64>,
}

/// The column of one step of a board's workflow.
pub(crate) struct StepColumn {
    /// The step's label, which tells its rows apart.
    pub(crate) label: String,
    /// What its head says: a limited step's says how much of its room is
    /// held.
    pub(crate) head: String,
    pub(crate) rows: Vec<Row>,
    /// The step's id while the workflow still has the step.
    pub(crate) step: Option<String>,
}

/// Project `project`'s board tasks in the columns of `template`, as core
/// places them.
pub(crate) fn board(
    project: &onehand_core::project::ProjectId,
    root: &std::path::Path,
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
    let Some(board) = crate::state::Shared::global(cx).boards.board(project) else {
        return BoardTasks {
            steps: Vec::new(),
            done: Vec::new(),
            taken: Default::default(),
        };
    };
    let columns = onehand_core::board::columns(template, board, t.tasks().iter());
    let rows = |tasks: Vec<&Task>| tasks.into_iter().map(|task| row_of(t, task)).collect();
    BoardTasks {
        steps: columns
            .steps
            .into_iter()
            .map(|column| {
                let at = template.steps.iter().position(|s| s.id == column.step);
                let room = at.and_then(|at| room_at(root, template, at, cx));
                let head = match room {
                    Some(room) => format!("{} · {} of {}", column.label, room.holding, room.limit),
                    None => column.label.clone(),
                };
                let step = at.map(|_| column.step.clone());
                StepColumn {
                    label: column.label,
                    head,
                    rows: rows(column.tasks),
                    step,
                }
            })
            .collect(),
        done: rows(columns.done),
        taken: onehand_core::board::taken(board, t.tasks().iter(), project),
    }
}
