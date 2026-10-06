//! The tasks as the pages and the rail read them: the Tasks page's rows,
//! each issue's work, what needs a person, and the unattended runs under way.

use super::Tasks;
use gpui::{AnyWindowHandle, App};
use onehand_core::issues::IssueKey;
use onehand_core::task::work::{IssueWork, issue_work, waiting_said};
use onehand_core::task::{Group, Task, sort_listed};
use onehand_core::unattended::TrackerRef;
use onehand_core::workflow::Run;
use std::path::PathBuf;

/// A task as the Tasks page lists it.
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Its workflow's name, after the issue it works if it has one.
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
}

/// Every task of the projects at `roots`: finished ones newest first, every
/// other group oldest first.
pub(crate) fn rows(roots: &[PathBuf], cx: &App) -> Vec<Row> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    let mut rows: Vec<(Option<&Task>, Row)> = Vec::new();
    let tasks = t
        .tasks
        .iter()
        .filter(|task| roots.contains(&task.setup.repo) || roots.contains(&task.setup.dir));
    for task in tasks {
        let working = t.working(&task.id);
        let run = task.runs.last();
        let step = run
            .and_then(Run::current)
            .map_or_else(String::new, |step| step.label.clone());
        let at = match (working, task.ended_said()) {
            (Some(_), _) => step,
            (None, None) => format!("cut off at {step}"),
            (None, Some(said)) => said,
        };
        let live = t.live.iter().find(|(_, d)| d.task == task.id);
        let row = Row {
            id: task.id.clone(),
            title: task.brief.title.clone(),
            name: match (task.issue(), run) {
                (Some(issue), Some(run)) => format!("{} · {}", issue.shown(), run.template.name),
                (_, run) => run.map_or_else(String::new, |run| run.template.name.clone()),
            },
            at,
            project: task.setup.repo.clone(),
            group: task.group(working),
            session: live.map(|(uid, d)| (*uid, d.window)),
            resumable: task.resumable(),
            stoppable: live.is_some()
                || t.queue.queued(&task.id)
                || t.checks.contains_key(&task.id),
        };
        rows.push((Some(task), row));
    }
    sort_listed(&mut rows, |(task, row)| (row.group, *task));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The work on every issue kept in one of `kept`'s files that a task works,
/// one summary per issue: its newest task and what came before.
///
/// **Matched by the file, not only the project and the number.** A forge's
/// issue numbers are its own, and two workspaces holding one repository keep
/// their issues apart, each numbered from one: either way the number alone
/// would show a run on somebody else's issue.
pub(crate) fn issue_works(kept: &[(PathBuf, PathBuf)], cx: &App) -> Vec<IssueWork> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    let mut by_issue: Vec<(IssueKey, Vec<&Task>)> = Vec::new();
    for task in &t.tasks {
        let Some(issue) = task.issue() else {
            continue;
        };
        let file = match &issue.tracker {
            TrackerRef::Local { file } | TrackerRef::Synced { file, .. } => file,
            TrackerRef::Forge { .. } => continue,
        };
        if !kept.contains(&(task.setup.repo.clone(), file.clone())) {
            continue;
        }
        let key = IssueKey {
            file: file.clone(),
            number: issue.number,
        };
        match by_issue.iter_mut().find(|(at, _)| *at == key) {
            Some((_, tasks)) => tasks.push(task),
            None => by_issue.push((key, vec![task])),
        }
    }
    by_issue
        .into_iter()
        .filter_map(|(key, tasks)| {
            let tasks = tasks.into_iter().map(|task| {
                let behind = t
                    .queue
                    .holder_of(&task.id)
                    .and_then(|holder| t.task(holder))
                    .map(|holder| holder.brief.title.clone());
                (task, t.working(&task.id), behind)
            });
            issue_work(key, tasks)
        })
        .collect()
}

/// Every issue task whose issue `of` takes, each with what it is doing,
/// oldest first: what a start on an issue reads its earlier tasks from.
pub(crate) fn issue_tasks(
    cx: &App,
    of: impl Fn(&onehand_core::unattended::IssueSource) -> bool,
) -> Vec<(Task, Option<onehand_core::task::Working>)> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    t.tasks
        .iter()
        .filter(|task| task.issue().is_some_and(&of))
        .map(|task| (task.clone(), t.working(&task.id)))
        .collect()
}

/// The session task `id`'s run is driven in, and the window holding it.
pub(crate) fn session_of(id: &str, cx: &App) -> Option<(u64, AnyWindowHandle)> {
    let t = cx.try_global::<Tasks>()?;
    t.live
        .iter()
        .find(|(_, d)| d.task == id)
        .map(|(uid, d)| (*uid, d.window))
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
    cx.try_global::<Tasks>().map_or(0, |t| {
        roots.iter().filter_map(|root| t.removed.get(root)).sum()
    })
}

/// Whether task `id` is queued, running or waiting on a person.
pub(crate) fn is_working(id: &str, cx: &App) -> bool {
    cx.try_global::<Tasks>().is_some_and(|t| t.busy(id))
}

/// How each issue task working or queued is shown, oldest first: what the
/// cap on unattended runs counts. One waiting on a person, or on its pull
/// request's status checks, is not working.
pub(crate) fn issues_working(cx: &App) -> Vec<String> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    t.tasks
        .iter()
        .filter(|task| t.holds_slot(&task.id))
        .filter_map(|task| task.issue().map(|issue| issue.named(&task.brief.title)))
        .collect()
}

/// An unattended run with a session under way, as the rail and the workspace
/// page read it.
pub(crate) struct LiveRun {
    /// The project the issue was found in.
    pub(crate) repo: PathBuf,
    /// How its issue is shown: the forge's number, or *Draft*.
    pub(crate) name: String,
    /// Its number in the issues file its project keeps, for one kept there:
    /// what opens it on the Issues page.
    pub(crate) kept: Option<u64>,
    pub(crate) title: String,
    /// The run's own session.
    pub(crate) uid: u64,
    /// The window the session is in.
    pub(crate) window: AnyWindowHandle,
    /// What it waits on a person for, while it does.
    pub(crate) waiting: Option<String>,
}

/// Every issue task with a session under way, oldest first. The rail says on
/// a project's row that a run works one of its issues, since the run's own
/// session is on a worktree's row of its own; the workspace page lists them.
pub(crate) fn live_issues(cx: &App) -> Vec<LiveRun> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    let mut live: Vec<_> = t
        .live
        .iter()
        .filter_map(|(uid, d)| {
            let task = t.task(&d.task)?;
            let issue = task.issue()?;
            let waiting = d
                .waits_on_person()
                .then(|| waiting_said(d.run.awaiting_approval()).to_string());
            let run = LiveRun {
                repo: task.setup.repo.clone(),
                name: issue.shown(),
                kept: match issue.tracker {
                    TrackerRef::Local { .. } | TrackerRef::Synced { .. } => Some(issue.number),
                    TrackerRef::Forge { .. } => None,
                },
                title: task.brief.title.clone(),
                uid: *uid,
                window: d.window,
                waiting,
            };
            Some((task.id.clone(), run))
        })
        .collect();
    live.sort_by(|(a, _), (b, _)| a.cmp(b));
    live.into_iter().map(|(_, run)| run).collect()
}
