//! A task that holds its place, begun: how it is driven, or why it cannot
//! be, and the run it carries on.

use super::{Effect, TasksDrive};
use crate::preflight::{found_late, About};
use crate::task::Grant;
use crate::workflow::{validate, Action, Failure, Run};

/// What a Retry pressed on a task comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// It was dealt with at once: a check run again, or nothing to retry.
    AtOnce,
    /// The person is asked first, told what the new run keeps and where it
    /// starts.
    Confirm,
}

/// What a task whose session did not start is said to have come to.
pub const SESSION_NOT_STARTED: &str = "The task's session did not start";

/// The run a task begins with: its last run, carried on from `first`, with
/// `pinned` of its boundaries' marks landed before.
#[derive(Debug)]
pub struct Start {
    pub run: Run,
    pub first: Action,
    pub pinned: usize,
}

/// How a task that holds its place is driven.
#[derive(Debug)]
pub enum Begin {
    /// A check: its command runs with no session, since there is no agent
    /// to watch.
    Check(Start),
    /// A run on a session of its own, on the agent its setup names. One
    /// `off_screen` comes up where nothing the person is looking at moves: an
    /// issue's run, on a project the workspace file never holds unless it
    /// was kept, and one a tool started, the first time it is driven.
    Session {
        start: Start,
        off_screen: bool,
        /// What its session may use, given to every session it is driven
        /// on, a Resume's new one included.
        grant: Grant,
        /// The lines its session opens with, before its first prompt: an
        /// issue's run says which issue and where it works.
        opening: Vec<String>,
    },
    /// It cannot start, for `why`, of `kind`: said where it was to start,
    /// then handed to [`TasksDrive::not_started`].
    Refused { why: String, kind: Failure },
}

impl<W: Copy> TasksDrive<W> {
    /// Begin task `id`, whose place it now holds, its work's folder being
    /// there or not (`folder_there`).
    pub fn begin(&mut self, id: &str, folder_there: bool) -> Begin {
        // Taken first, so a first drive that is refused uses it up as well.
        let off_screen = self.take_off_screen(id);
        let refused = |why: String, kind| Begin::Refused { why, kind };
        let (Some(mut run), Some(task)) = (self.resumable_run(id), self.task(id)) else {
            return refused(
                "That task has nothing left to run".to_string(),
                Failure::Other,
            );
        };
        if !folder_there {
            return refused(
                format!(
                    "The folder that task works in is gone: {}",
                    run.setup.dir.display()
                ),
                Failure::Other,
            );
        }
        let check = task.source == super::super::Source::Check;
        let issue = task.issue().is_some();
        let (grant, opening) = (task.grant.clone(), crate::unattended::drive::opening(task));
        // A snapshot that no longer validates (a later build's rules) is what
        // the preflight would have blocked, found late.
        let problems = validate(&run.template);
        if !check && !problems.is_empty() {
            let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return refused(
                format!(
                    "The workflow `{}` no longer validates: {}",
                    run.template.name,
                    said.join("; ")
                ),
                found_late(About::Workflow),
            );
        }
        // Counted from what landed, so a mark the app quit before pinning is
        // pinned now rather than taken as made.
        let pinned = run.pinned_count();
        let first = run.resume();
        let start = Start { run, first, pinned };
        match check {
            true => Begin::Check(start),
            false => Begin::Session {
                start,
                off_screen: issue || off_screen,
                grant,
                opening,
            },
        }
    }

    /// Task `id`'s session did not start. With no agent of the name its
    /// setup keeps configured any more (`agent_gone`), that is what the
    /// preflight would have blocked, found late.
    pub fn session_not_started(&mut self, id: &str, agent_gone: bool) -> Vec<Effect<W>> {
        let kind = match agent_gone {
            true => found_late(About::Agent),
            false => Failure::Other,
        };
        self.not_started(id, SESSION_NOT_STARTED.to_string(), kind)
    }

    /// Retry task `id`, from `window`: at once, as run `run_id`, when it is
    /// a check, a fresh run of its one step from that step, since one that
    /// passed would otherwise start past it, asking for its place; anything
    /// else is asked about first, as a person retries through what the
    /// retry would keep.
    #[must_use]
    pub fn retry_asked(&mut self, id: &str, run_id: String, window: W) -> (Retry, Vec<Effect<W>>) {
        let Some(task) = self.task(id) else {
            return (Retry::AtOnce, Vec::new());
        };
        if task.source != super::super::Source::Check {
            return (Retry::Confirm, Vec::new());
        }
        let Some(template) = task.runs.last().map(|run| run.template.clone()) else {
            return (Retry::AtOnce, Vec::new());
        };
        let first = template.steps.first().map(|step| step.id.clone());
        let (_, fx) = self.retry(id, run_id, template, first.as_deref(), None, window);
        (Retry::AtOnce, fx)
    }
}
