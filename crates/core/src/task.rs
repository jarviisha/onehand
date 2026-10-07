//! Tasks: the work onehand does on a person's behalf, kept as history.
//!
//! A task wraps the runs of one brief on one place; the runs themselves stay
//! the workflow engine's. The parts:
//! - [`queue`] says which task may work in a place, one at a time;
//! - [`marks`] pins the work at each step visit's start and end as a commit
//!   the repository keeps;
//! - [`files`] keeps every task, one file each, written in order;
//! - [`history`] says which finished tasks are old enough to let go;
//! - [`work`] says where an issue's work stands and what to do next.

pub mod files;
pub mod history;
pub mod marks;
pub mod queue;
pub mod work;

use crate::unattended::IssueSource;
use crate::workflow::{
    ApprovalAt, Brief, Outcome, Run, Setup, StepKind, StepSpec, Stop, Template, WithCurrent,
};
use serde::{Deserialize, Serialize};

/// What a task runs: a workflow a person picked, the project's check command
/// on its own, which needs no session, or an issue worked unattended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    #[default]
    Workflow,
    Check,
    Issue(IssueSource),
}

/// A person's answer to the approval task `task`'s run waits on, drawn from
/// what they read at `at`: what every press of *Continue* or *Revise…*
/// carries to the run, whichever window it is made in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub task: String,
    pub at: ApprovalAt,
}

/// What a task is doing right now, which only the app driving it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Working {
    /// Waiting for its place.
    Queued,
    Running,
    /// Waiting on a person: an approval or a card.
    Waiting,
}

/// Where a task is listed, in the order a list of them reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Waiting on a person.
    Waiting,
    /// Over, and a person should look: cut off, or ended on something nobody
    /// chose.
    Ended,
    Running,
    Queued,
    /// History: done, stopped by a person, or let go.
    Finished,
}

impl Group {
    /// Whether a person should act on a task listed here: one waiting on
    /// them, or one that ended on something nobody chose.
    pub fn needs_attention(self) -> bool {
        match self {
            Self::Waiting | Self::Ended => true,
            Self::Running | Self::Queued | Self::Finished => false,
        }
    }
}

/// Sort `rows` as a list of tasks reads: by group, finished ones newest
/// first, every other group oldest first. `of` gives a row's group and its
/// task, if it has one; a row with none comes first in its group.
pub fn sort_listed<T>(rows: &mut [T], of: impl Fn(&T) -> (Group, Option<&Task>)) {
    rows.sort_by(|a, b| {
        let ((group, a), (other, b)) = (of(a), of(b));
        group.cmp(&other).then_with(|| match group {
            Group::Finished => b.map(Task::recency).cmp(&a.map(Task::recency)),
            Group::Waiting | Group::Ended | Group::Running | Group::Queued => {
                a.map(Task::created).cmp(&b.map(Task::created))
            }
        })
    });
}

/// One piece of work and every run of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Its first run's id.
    pub id: String,
    /// Kept beside the runs, so a task called off before it ran still says
    /// what it was asked and where.
    pub brief: Brief,
    pub setup: Setup,
    /// Oldest first.
    pub runs: Vec<Run>,
    /// A person let it go: it is history, never offered again.
    #[serde(default)]
    pub dismissed: bool,
    /// A file from before checks were tasks reads as a workflow's.
    #[serde(default)]
    pub source: Source,
}

impl Task {
    /// A task of `template` on `brief`, its one run not started.
    pub fn new(id: String, template: Template, brief: Brief, setup: Setup) -> Self {
        Self {
            runs: vec![Run::new(id.clone(), template, brief.clone(), setup.clone())],
            id,
            brief,
            setup,
            dismissed: false,
            source: Source::Workflow,
        }
    }

    /// A task that runs the project's check `command` once: one command step
    /// that ends the run failed when it fails. Its template is made here,
    /// never saved, and needs no validating.
    pub fn check(id: String, command: String, setup: Setup) -> Self {
        let mut template = Template::blank("Check");
        template.steps.push(StepSpec {
            id: "check".to_string(),
            label: "Check".to_string(),
            kind: StepKind::Command {
                command: Some(command.clone()),
                on_fail: String::new(),
            },
        });
        let brief = Brief {
            title: "Check".to_string(),
            body: command,
            instructions: None,
        };
        Self {
            source: Source::Check,
            ..Self::new(id, template, brief, setup)
        }
    }

    /// The issue it works, if it was started from one.
    pub fn issue(&self) -> Option<&IssueSource> {
        match &self.source {
            Source::Issue(issue) => Some(issue),
            Source::Workflow | Source::Check => None,
        }
    }

    /// Where it is listed, given what the app says it is doing.
    pub fn group(&self, working: Option<Working>) -> Group {
        match working {
            Some(Working::Queued) => Group::Queued,
            Some(Working::Running) => Group::Running,
            Some(Working::Waiting) => Group::Waiting,
            None if !self.dismissed
                && self
                    .outcome()
                    .is_none_or(|outcome| outcome.needs_attention()) =>
            {
                Group::Ended
            }
            None => Group::Finished,
        }
    }

    /// When it was made: its id is the nanos it was made at.
    fn created(&self) -> u128 {
        self.id.parse().unwrap_or(0)
    }

    /// When it last moved, then when it was made. One stopped before it ran
    /// last moved when it was made.
    pub(crate) fn recency(&self) -> (u64, u128) {
        let moved = self
            .runs
            .iter()
            .filter_map(|run| run.history.last().map(|t| t.at))
            .max()
            .unwrap_or((self.created() / 1_000_000_000) as u64);
        (moved, self.created())
    }

    /// Run it again on `template`, as run `id`, from the first step its last
    /// run cannot carry over, or from step `from` when that is earlier. The
    /// new run, not started; `None` with no run to retry. A task let go and
    /// retried is live again: its new run may need a person like any other.
    /// A `note` is what a person asked to change, which the step it starts
    /// at is told as a revision is.
    pub fn retry(
        &mut self,
        id: String,
        template: Template,
        from: Option<&str>,
        note: Option<String>,
    ) -> Option<&Run> {
        let mut next = Run::retry_of(self.runs.last()?, id, template, from);
        next.revise = note;
        self.dismissed = false;
        self.runs.push(next);
        self.runs.last()
    }

    /// Run it again as run `id` with what Settings say now, as `plan` worked
    /// it out from its last run: its own workflow at its newest version, the
    /// setup a new task of its kind takes, from the step `plan` starts at.
    pub fn retry_with(&mut self, id: String, plan: WithCurrent) -> Option<&Run> {
        let from = plan
            .template
            .steps
            .get(plan.start)
            .map(|step| step.id.clone());
        let mut next = Run::retry_of(self.runs.last()?, id, plan.template, from.as_deref());
        next.setup = plan.setup;
        self.dismissed = false;
        self.runs.push(next);
        self.runs.last()
    }

    /// Whether it can have a pull request review at all: an issue's task on a
    /// branch a forge serves.
    pub fn reviewable(&self) -> bool {
        self.issue().is_some() && self.setup.forge.is_some() && self.setup.branch.is_some()
    }

    /// Whether a review on its pull request can be answered: one it can have
    /// ([`Task::reviewable`]), its last run's workflow repairing what the
    /// status checks find, which is where a review is answered from.
    pub fn answers_reviews(&self) -> bool {
        self.reviewable()
            && self
                .runs
                .last()
                .is_some_and(|run| run.template.repair_step().is_some())
    }

    /// How it stands: its last run's outcome, `None` while that run has not
    /// ended, or stopped by a person when it was called off before any run.
    pub fn outcome(&self) -> Option<Outcome> {
        match self.runs.last() {
            Some(run) => run.outcome.clone(),
            None => Some(Outcome::Stopped(Stop::ByPerson)),
        }
    }

    /// How it ended, as its line in a list; `None` while it has not. A check
    /// says whether it passed, any other task how its workflow ended.
    pub fn ended_said(&self) -> Option<String> {
        let outcome = self.outcome()?;
        Some(match self.source {
            Source::Check => match outcome {
                Outcome::Done => "Check passed".to_string(),
                Outcome::Failed(why) => format!("Check failed: {why}"),
                Outcome::Exhausted { .. } => "Check failed".to_string(),
                Outcome::Stopped(stop) => {
                    let mut said = stop.said().to_string();
                    said[..1].make_ascii_uppercase();
                    said
                }
            },
            Source::Workflow | Source::Issue(_) => outcome.said(),
        })
    }

    /// Whether it may be picked up again where it was: not let go, and its
    /// last run cut off or ended by something that was not its own doing.
    pub fn resumable(&self) -> bool {
        !self.dismissed && self.outcome().is_none_or(|outcome| outcome.resumable())
    }
}

/// An id no other task has.
pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    nanos.to_string()
}

#[cfg(test)]
mod tests;
