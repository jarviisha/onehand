//! The search, the claim and what is made for it: everything a tick or a
//! pick decides once it is off the UI loop, over the projects, the workflows
//! and the issue tasks the app read for it.

use super::{Issue, Tracker, TrackerRef};
use crate::connector::{Connector, PrState};
use crate::preflight::{self, Facts, Forge};
use crate::task::{Task, Working};
use crate::workflow::Template;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// An issue task that worked on a branch of its own, as a claim looks for
/// one still working on the issue, or whose pull request it answers.
pub struct Earlier {
    pub task: String,
    pub tracker: TrackerRef,
    pub number: u64,
    pub dir: PathBuf,
    pub branch: String,
    pub working: bool,
    /// Its last run's workflow can answer a review: it has a step its status
    /// checks send back to, and a forge to push the answer to.
    pub answers_reviews: bool,
}

/// What a search judges each new task by, gathered before it goes off the
/// UI loop: the configuration's own facts, every workflow an issue may be
/// given, and the issue tasks there are.
pub struct Judging {
    pub common: Facts,
    pub templates: Vec<(String, Result<Template, String>)>,
    pub tasks: Vec<(Task, Option<Working>)>,
}

impl Judging {
    /// A new run of workflow `id` on `issue`, as a window gathers it for a
    /// pick.
    pub(crate) fn facts(
        &self,
        id: &str,
        tracker: &Tracker,
        issue: &Issue,
        has_check: bool,
        checked_out: Option<String>,
        forge: Option<&'static dyn Connector>,
    ) -> Facts {
        let workflow = self.templates.iter().find(|(at, _)| at == id).map_or_else(
            || Err(format!("there is no workflow `{id}`")),
            |(_, t)| t.clone(),
        );
        let project = preflight::ProjectFacts {
            has_check,
            checked_out,
            // The look before the search asked the account; a project whose
            // account failed is not searched.
            forge: forge.map(|forge| Forge {
                name: forge.name().to_string(),
                account: None,
            }),
        };
        preflight::new_issue_run(
            self.common.clone(),
            workflow,
            tracker,
            issue,
            project,
            self.tasks.clone(),
        )
    }
}

/// How a search chooses each issue's workflow: the default, the workflow
/// labels, and which of the workflows those name run the project's check
/// command.
pub struct Choosing {
    pub default: String,
    pub by_label: BTreeMap<String, String>,
    pub need_check: HashSet<String>,
}

impl Choosing {
    /// Choose with `default` and `by_label`, knowing from `validated`, each
    /// workflow as a run would take it, which of them run the project's
    /// check command; one that cannot run is not counted as needing one.
    pub fn new(
        default: String,
        by_label: BTreeMap<String, String>,
        validated: &[(String, Result<Template, String>)],
    ) -> Self {
        let need_check = std::iter::once(&default)
            .chain(by_label.values())
            .filter(|id| {
                validated
                    .iter()
                    .any(|(at, t)| at == *id && t.as_ref().is_ok_and(Template::needs_check))
            })
            .cloned()
            .collect();
        Self {
            default,
            by_label,
            need_check,
        }
    }

    /// The id of the workflow an issue carrying `labels` is worked with, the
    /// trigger label being `trigger`.
    pub fn workflow_for(&self, labels: &[String], trigger: &str) -> &str {
        super::workflow_for(labels, &self.by_label, &self.default, trigger)
    }

    /// Whether that workflow runs the project's check command.
    pub fn needs_check(&self, labels: &[String], trigger: &str) -> bool {
        self.need_check.contains(self.workflow_for(labels, trigger))
    }
}

/// What claiming an issue comes to, given the tasks it had before.
pub enum Taking {
    /// A new task, on a branch of its own.
    Fresh,
    /// The last task's pull request is open: the label put back is a reviewer
    /// asking for changes, and that task answers them on its own branch.
    Review {
        task: String,
        /// Its worktree and branch.
        dir: PathBuf,
        branch: String,
        pr: String,
        note: String,
    },
    /// The issue is claimed only to be told why nothing follows: its last
    /// pull request was closed without being merged, so a second is never
    /// opened beside the one turned down, or its workflow cannot answer a
    /// review on the one open.
    Refused(String),
}

impl Taking {
    /// The pull request whose review the claim answers, if it does.
    pub fn answering(&self) -> Option<&str> {
        match self {
            Self::Review { pr, .. } => Some(pr),
            Self::Fresh | Self::Refused(_) => None,
        }
    }
}

/// Why an issue is not claimed now, its label left on.
pub enum Skip {
    /// A task is still working on it: it is never taken twice.
    Busy,
    /// The pull request on its last task's branch could not be looked up.
    Unread(String),
}

impl Skip {
    /// What a person who picked the issue `named` is told.
    pub fn said(&self, named: &str) -> String {
        match self {
            Self::Busy => preflight::already_working(named),
            Self::Unread(why) => format!("Could not start on issue {named}: {why}"),
        }
    }
}

/// What claiming issue `number` of `tracker` comes to, by the newest of
/// `earlier` on it and, with a forge, the pull request on its branch; one
/// merged, or none, is a fresh start. Blocking.
pub fn taking_blocking(
    earlier: &[Earlier],
    tracker: &Tracker,
    number: u64,
    repo: &Path,
    forge: Option<&'static dyn Connector>,
) -> Result<Taking, Skip> {
    let at = tracker.to_ref();
    let Some(last) = earlier
        .iter()
        .rfind(|e| e.tracker == at && e.number == number)
    else {
        return Ok(Taking::Fresh);
    };
    if last.working {
        return Err(Skip::Busy);
    }
    let Some(forge) = forge else {
        return Ok(Taking::Fresh);
    };
    let pr = match forge.pull_request_for_blocking(repo, &last.branch) {
        Ok(Some(pr)) => pr,
        Ok(None) => return Ok(Taking::Fresh),
        Err(why) => {
            return Err(Skip::Unread(format!(
                "the pull request on `{}` could not be looked up: {why}",
                last.branch
            )));
        }
    };
    Ok(match pr.state {
        PrState::Open if !last.answers_reviews => {
            Taking::Refused(super::review_unanswerable(&pr.url))
        }
        PrState::Open => Taking::Review {
            task: last.task.clone(),
            dir: last.dir.clone(),
            branch: last.branch.clone(),
            note: super::review_note(&pr.url, &forge.read_review_with(pr.number)),
            pr: pr.url,
        },
        PrState::Closed => Taking::Refused(super::review_closed(&pr.url)),
        PrState::Merged => Taking::Fresh,
    })
}

/// Where a project's issues are looked for, in the order they are searched.
///
/// A project kept in step with its forge is looked for **here only**, since
/// the forge's issues are already here and searching both would find each one
/// twice. Otherwise its own issues first, then the forge's. Blocking: it reads
/// the issue file to learn whether the project is synced.
pub fn trackers_blocking(
    issues: Option<PathBuf>,
    forge: Option<&'static dyn Connector>,
) -> Vec<Tracker> {
    if let (Some(file), Some(forge)) = (&issues, forge) {
        if crate::issues::load_blocking(file).is_ok_and(|kept| kept.in_step_with(forge.name())) {
            return vec![Tracker::Synced {
                file: file.clone(),
                forge,
            }];
        }
    }
    issues
        .map(Tracker::Local)
        .into_iter()
        .chain(forge.map(Tracker::Forge))
        .collect()
}

#[cfg(test)]
mod tests;
