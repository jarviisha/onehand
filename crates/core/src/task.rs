//! Tasks: the work onehand does on a person's behalf, kept as history.
//!
//! A task wraps the runs of one brief on one place; the runs themselves stay
//! the workflow engine's. The parts:
//! - [`queue`] says which task may work in a place, one at a time;
//! - [`marks`] pins the work at each step visit's start and end as a commit
//!   the repository keeps;
//! - [`files`] keeps every task, one file each, written in order.

pub mod files;
pub mod marks;
pub mod queue;

use crate::workflow::{Brief, Outcome, Run, Setup, Stop, Template};
use serde::{Deserialize, Serialize};

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
        }
    }

    /// How it stands: its last run's outcome, `None` while that run has not
    /// ended, or stopped by a person when it was called off before any run.
    pub(crate) fn outcome(&self) -> Option<Outcome> {
        match self.runs.last() {
            Some(run) => run.outcome.clone(),
            None => Some(Outcome::Stopped(Stop::ByPerson)),
        }
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
