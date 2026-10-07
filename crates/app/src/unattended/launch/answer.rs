//! *Answer the pull request review*, pressed on an issue or its task: a
//! second door to the path a re-added trigger label runs, never a second
//! path. What would refuse it is read first, off the UI loop, and judged by
//! the preflight before anything is claimed; then the issue is picked as a
//! person picks one, which finds the open pull request and answers it.

use super::pick::{start_picked, warn};
use gpui::{AnyWindowHandle, App};
use onehand_core::connector::{self, PrState};
use onehand_core::preflight::{self, Kind, ReviewFacts};
use onehand_core::unattended as core;
use onehand_core::worktree;

/// Answer the review on the open pull request of issue task `id`, the
/// project's `has_check` told by the window it was pressed in, and say in
/// `window` what stops it.
pub fn answer_review_by_hand(id: String, has_check: bool, window: AnyWindowHandle, cx: &mut App) {
    let Some(task) = crate::task::task(&id, cx) else {
        return;
    };
    let (Some(issue), Some(last)) = (task.issue().cloned(), task.runs.last().cloned()) else {
        return warn(
            window,
            "Only an issue's task has a pull request review".into(),
            cx,
        );
    };
    let connectors = crate::plugins::connectors();
    let forge = task
        .setup
        .forge
        .as_deref()
        .and_then(|name| connector::named(connectors, name));
    let Some(tracker) = issue.tracker.resolve(connectors) else {
        return warn(
            window,
            "The issue's forge is not one this onehand has".into(),
            cx,
        );
    };
    // Judged by its own snapshot and setup, as a Retry is: a review is
    // answered by the task that opened the pull request.
    let mut facts = crate::unattended::task_facts(&task, last.template.clone(), cx);
    let answers = task.answers_reviews();
    let (repo, dir, branch) = (
        task.setup.repo.clone(),
        task.setup.dir.clone(),
        task.setup.branch.clone().unwrap_or_default(),
    );
    let workflow = last.template.id.clone();
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn({
                let tracker = tracker.clone();
                async move {
                    let pr = match forge {
                        Some(forge) => forge
                            .pull_request_for_blocking(&repo, &branch)
                            .map(|pr| pr.map(|pr| (pr.state, pr.url))),
                        None => Ok(None),
                    };
                    // Whether the worktree comes up to the branch on the
                    // forge, which a reviewer may have pushed to: asked only
                    // of a review that would otherwise be answered.
                    let open = matches!(&pr, Ok(Some((PrState::Open, _))));
                    let diverged = match (forge, open && answers) {
                        (Some(forge), true) => forge
                            .fetch_blocking(&repo, &branch)
                            .and_then(|()| {
                                worktree::fast_forwards_blocking(&dir, &format!("origin/{branch}"))
                            })
                            .map(|forwards| !forwards),
                        _ => Ok(false),
                    };
                    let row = core::open_issues_blocking(&tracker, &repo).map(|(rows, _)| {
                        rows.into_iter()
                            .find(|row| row.issue.number == issue.number)
                    });
                    (pr, diverged, row, repo)
                }
            })
            .await;
        cx.update(|cx| {
            let (pr, diverged, row, repo) = read;
            let (pr, diverged) = match diverged {
                Ok(diverged) => (pr, diverged),
                Err(why) => (
                    Err(format!(
                        "the branch could not be compared with the forge: {why}"
                    )),
                    false,
                ),
            };
            facts.review = Some(ReviewFacts {
                pr,
                answers,
                diverged,
            });
            // Every block is said, not only the first: each is its own thing
            // to put right before pressing again.
            let blocks: Vec<String> = preflight::preflight(Kind::AnswerReview, &facts)
                .into_iter()
                .filter(|f| f.blocks)
                .map(|f| f.text)
                .collect();
            if !blocks.is_empty() {
                return warn(window, format!("Not answered: {}", blocks.join(" ")), cx);
            }
            let row = match row {
                Ok(Some(row)) => row,
                Ok(None) => {
                    return warn(
                        window,
                        "Not answered: the issue is closed, or not among the open issues read"
                            .into(),
                        cx,
                    );
                }
                Err(why) => return warn(window, format!("Not answered: {why}"), cx),
            };
            if let Err(why) = start_picked(
                repo,
                tracker,
                row,
                workflow,
                String::new(),
                has_check,
                window,
                cx,
            ) {
                warn(window, why, cx);
            }
        });
    })
    .detach();
}
