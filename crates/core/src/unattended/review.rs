//! What answering an issue task's pull request review is judged on: its
//! pull request as the forge says now, whether its worktree still comes up
//! to the branch on the forge, which a reviewer may have pushed to, and the
//! issue among the open ones; and what a pick of it starts once nothing
//! blocks it. Read off the UI loop, judged by the preflight.

use super::{open_issues_blocking, IssueRow, Tracker};
use crate::connector::{self, Connector, PrState};
use crate::preflight::{Facts, ReviewFacts};
use crate::task::Task;
use crate::workflow::Template;
use std::path::PathBuf;

/// What reading a task's review needs, gathered before the read goes off
/// the UI loop.
pub struct ReviewAsk {
    facts: Facts,
    answers: bool,
    from: Option<String>,
    repo: PathBuf,
    dir: PathBuf,
    branch: String,
    workflow: String,
    number: u64,
    forge: Option<&'static dyn Connector>,
    tracker: Tracker,
}

/// What answering a task's review is judged on, and what starts it once
/// nothing blocks it.
pub struct ReviewRead {
    /// The task's own setup with the pull request, as the forge says now.
    pub facts: Facts,
    /// The step the review is answered from, by label.
    pub from: Option<String>,
    /// The project, its tracker, the issue's row among the open ones and
    /// the workflow a pick of it runs; `None` when the issue is not open.
    pub start: Option<(PathBuf, Tracker, IssueRow, String)>,
}

impl ReviewAsk {
    /// Reading issue task `task`'s review, judged by the facts `facts` makes
    /// of it and its last run's workflow, its own snapshot and setup, as a
    /// Retry is: a review is answered by the task that opened the pull
    /// request. Why there is nothing to read, when there is not: the task
    /// gone, never run, or of no issue.
    pub fn of(
        task: Option<&Task>,
        facts: impl FnOnce(&Task, Template) -> Facts,
        connectors: super::drive::Connectors,
    ) -> Result<Self, String> {
        let task = task.ok_or("The task is gone")?;
        let last = task.runs.last().ok_or("The task has never run")?;
        let issue = task
            .issue()
            .ok_or("Only an issue's task has a pull request review")?;
        let facts = facts(task, last.template.clone());
        let tracker = issue
            .tracker
            .resolve(connectors)
            .ok_or("The issue's forge is not one this onehand has")?;
        let from = last.template.repair_step().and_then(|id| {
            let step = last.template.steps.iter().find(|step| step.id == id)?;
            Some(step.label.clone())
        });
        Ok(Self {
            facts,
            answers: task.answers_reviews(),
            from,
            repo: task.setup.repo.clone(),
            dir: task.setup.dir.clone(),
            branch: task.setup.branch.clone().unwrap_or_default(),
            workflow: last.template.id.clone(),
            number: issue.number,
            forge: task
                .setup
                .forge
                .as_deref()
                .and_then(|name| connector::named(connectors, name)),
            tracker,
        })
    }

    /// Read it. Blocking.
    pub fn read_blocking(self) -> ReviewRead {
        let Self {
            mut facts,
            answers,
            from,
            repo,
            dir,
            branch,
            workflow,
            number,
            forge,
            tracker,
        } = self;
        let pr = match forge {
            Some(forge) => forge
                .pull_request_for_blocking(&repo, &branch)
                .map(|pr| pr.map(|pr| (pr.state, pr.url))),
            None => Ok(None),
        };
        // Whether the worktree comes up to the branch on the forge: asked
        // only of a review that would otherwise be answered.
        let open = matches!(&pr, Ok(Some((PrState::Open, _))));
        let diverged = match (forge, open && answers) {
            (Some(forge), true) => forge.fetch_blocking(&repo, &branch).and_then(|()| {
                crate::worktree::went_its_own_way_blocking(&dir, &format!("origin/{branch}"))
            }),
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
        let row = open_issues_blocking(&tracker, &repo)
            .map(|(rows, _)| rows.into_iter().find(|row| row.issue.number == number));
        facts.review = Some(ReviewFacts {
            pr,
            answers,
            diverged,
            issue_open: row.as_ref().map(Option::is_some).map_err(Clone::clone),
        });
        ReviewRead {
            facts,
            from,
            start: row.ok().flatten().map(|row| (repo, tracker, row, workflow)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::flows::harness::repo;
    use crate::unattended::{IssueSource, TrackerRef};
    use crate::workflow::{builtin, Brief, Setup};

    fn facts() -> Facts {
        Facts {
            workflow: Err("unread".into()),
            agent: None,
            agent_configured: false,
            mode: None,
            offered: None,
            has_check: false,
            in_git: true,
            checked_out: None,
            forge: None,
            forge_read: true,
            issue: None,
            slots: None,
            queued_behind: None,
            review: None,
            shared_checkout: None,
        }
    }

    /// An issue kept here, on no forge: no pull request and nothing fetched,
    /// and the issue found among the open ones, to be picked from.
    #[test]
    fn a_review_on_no_forge_reads_no_pull_request_and_finds_its_issue() {
        let project = repo("review-read", &[("Fix it", &[])]);
        let setup = Setup {
            repo: project.root.clone(),
            dir: project.root.clone(),
            branch: Some("onehand/local-1-fix".into()),
            agent: None,
            check: None,
            mode: None,
            forge: None,
        };
        let brief = Brief {
            title: "#1".into(),
            body: String::new(),
            instructions: None,
        };
        let issue = IssueSource {
            tracker: TrackerRef::Local {
                file: project.issues.clone().unwrap(),
            },
            number: 1,
            forge_ref: None,
            forge: None,
            base: "main".into(),
            picked: false,
            unsent: Vec::new(),
            notes: Vec::new(),
        };
        let template = builtin::all().remove(0);
        let task = Task::of_issue("t".into(), template, brief, setup, issue);
        let read = ReviewAsk::of(Some(&task), |_, _| facts(), &[])
            .unwrap()
            .read_blocking();
        let review = read.facts.review.unwrap();
        assert_eq!(review.pr, Ok(None));
        assert!(!review.diverged);
        assert_eq!(review.issue_open, Ok(true));
        assert!(read
            .start
            .is_some_and(|(_, _, row, _)| row.issue.number == 1));
        let _ = std::fs::remove_dir_all(project.root);
    }

    /// Only an issue's task has a review to read.
    #[test]
    fn a_task_of_no_issue_has_no_review() {
        let task = Task::check(
            "c".into(),
            "true".into(),
            Setup {
                repo: "/r".into(),
                dir: "/r".into(),
                branch: None,
                agent: None,
                check: None,
                mode: None,
                forge: None,
            },
        );
        assert!(ReviewAsk::of(Some(&task), |_, _| facts(), &[]).is_err());
        let gone = ReviewAsk::of(None, |_, _| facts(), &[]).err();
        assert_eq!(gone.as_deref(), Some("The task is gone"));
    }
}
