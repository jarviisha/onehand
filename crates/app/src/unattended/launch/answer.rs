//! *Answer the pull request review*, pressed on an issue or its task: a
//! second door to the path a re-added trigger label runs, never a second
//! path. What would refuse it is read first, off the UI loop, and judged by
//! the preflight before anything is claimed; then the issue is picked as a
//! person picks one, which finds the open pull request and answers it.

use super::pick::start_picked;
use gpui::{AnyWindowHandle, App};
use onehand_core::connector::{self, PrState};
use onehand_core::preflight::{Facts, ReviewFacts};
use onehand_core::unattended::{self as core, IssueRow, Tracker};
use onehand_core::worktree;
use std::path::PathBuf;

/// What answering a task's review is judged on, and what starts it once
/// nothing blocks it.
pub struct ReviewRead {
    /// The task's own setup with the pull request, as the forge says now.
    pub facts: Facts,
    /// The step the review is answered from, by label.
    pub from: Option<String>,
    start: Option<(PathBuf, Tracker, IssueRow, String)>,
}

/// Read what answering issue task `id`'s review is judged on: its pull
/// request, whether its worktree still comes up to the branch on the forge,
/// and the issue among the open ones. Off the UI loop; `Err` says why there
/// is nothing to read at all.
pub fn read_review(id: &str, cx: &mut App) -> gpui::Task<Result<ReviewRead, String>> {
    let Some(task) = crate::task::task(id, cx) else {
        return gpui::Task::ready(Err("The task is gone".to_string()));
    };
    let (Some(issue), Some(last)) = (task.issue().cloned(), task.runs.last().cloned()) else {
        return gpui::Task::ready(Err("Only an issue's task has a pull request review".into()));
    };
    let connectors = crate::plugins::connectors();
    let forge = task
        .setup
        .forge
        .as_deref()
        .and_then(|name| connector::named(connectors, name));
    let Some(tracker) = issue.tracker.resolve(connectors) else {
        return gpui::Task::ready(Err("The issue's forge is not one this onehand has".into()));
    };
    // Judged by its own snapshot and setup, as a Retry is: a review is
    // answered by the task that opened the pull request.
    let mut facts = crate::unattended::task_facts(&task, last.template.clone(), cx);
    let answers = task.answers_reviews();
    let from = last.template.repair_step().and_then(|id| {
        let step = last.template.steps.iter().find(|step| step.id == id)?;
        Some(step.label.clone())
    });
    let (repo, dir, branch) = (
        task.setup.repo.clone(),
        task.setup.dir.clone(),
        task.setup.branch.clone().unwrap_or_default(),
    );
    let workflow = last.template.id.clone();
    let number = issue.number;
    cx.background_executor().spawn(async move {
        let pr = match forge {
            Some(forge) => forge
                .pull_request_for_blocking(&repo, &branch)
                .map(|pr| pr.map(|pr| (pr.state, pr.url))),
            None => Ok(None),
        };
        // Whether the worktree comes up to the branch on the forge, which a
        // reviewer may have pushed to: asked only of a review that would
        // otherwise be answered.
        let open = matches!(&pr, Ok(Some((PrState::Open, _))));
        let diverged = match (forge, open && answers) {
            (Some(forge), true) => forge
                .fetch_blocking(&repo, &branch)
                .and_then(|()| worktree::fast_forwards_blocking(&dir, &format!("origin/{branch}")))
                .map(|forwards| !forwards),
            _ => Ok(false),
        };
        let (pr, diverged) = match diverged {
            Ok(diverged) => (pr, diverged),
            Err(why) => (
                Err(format!(
                    "the branch could not be compared with the forge: {why}"
                )),
                false,
            ),
        };
        let row = core::open_issues_blocking(&tracker, &repo)
            .map(|(rows, _)| rows.into_iter().find(|row| row.issue.number == number));
        facts.review = Some(ReviewFacts {
            pr,
            answers,
            diverged,
            issue_open: row.as_ref().map(Option::is_some).map_err(Clone::clone),
        });
        Ok(ReviewRead {
            facts,
            from,
            start: row.ok().flatten().map(|row| (repo, tracker, row, workflow)),
        })
    })
}

/// Answer the review `read` found nothing blocking, the project's
/// `has_check` told by the window it was pressed in: the issue is picked as a
/// person picks one. Why it did not start, when it did not.
pub fn start_answer(
    read: ReviewRead,
    has_check: bool,
    window: AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let (repo, tracker, row, workflow) = read
        .start
        .ok_or("the issue is closed, or not among the open issues a pick reads")?;
    start_picked(
        repo,
        tracker,
        row,
        workflow,
        String::new(),
        has_check,
        window,
        cx,
    )
}
