//! A run that ended, kept in its task: what its end tells, and the place it
//! holds given up once its work has stopped and its last mark is pinned.

use super::{Effect, Input, Signal, TasksDrive, DRAIN_LIMIT};
use crate::task::{marks, Origin, Source, Task};
use crate::workflow::{Failure, Outcome, Run, Stop};
use crate::Now;

impl<W: Copy> TasksDrive<W> {
    /// Task `id`'s run ended as `run`, which kept the marks of `from` of its
    /// boundaries, on sessions `uids` in `window`, if it had any: keep it, and
    /// let its place go once the work has stopped, which is at once unless
    /// its session's turn still goes (`turn_going`).
    pub fn run_ended(
        &mut self,
        id: &str,
        run: Run,
        from: usize,
        turn_going: bool,
        uids: Vec<u64>,
        window: W,
    ) -> Vec<Effect<W>> {
        // Whatever ended it, the run holds room nowhere now and waits for
        // none.
        let mut fx = super::give_room(self.rooms.leave(id));
        let Some(task) = self.task(id) else {
            return fx;
        };
        if task.source == Source::Check {
            if let Some(passed) = check_passed(&run) {
                fx.push(Effect::Signal(Signal::CheckEnded {
                    window,
                    project: project_of(&run),
                    passed,
                }));
            }
        }
        self.store_run(id, run, &mut fx);
        // A finished schedule's or board's session leaves the rail once the
        // run is let go, so nothing of the run is left listening to it.
        let closes = !uids.is_empty() && self.task(id).is_some_and(Task::closes_session);
        match turn_going {
            true => {
                self.draining.insert(id.to_string(), from);
                fx.push(Effect::WatchDrain {
                    task: id.to_string(),
                    limit: DRAIN_LIMIT,
                });
            }
            false => fx.push(Effect::Defer(Input::Free {
                task: id.to_string(),
                from,
            })),
        }
        if closes {
            fx.push(Effect::CloseSession { uids, window });
        }
        fx
    }

    /// `run` is where task `id`'s last run stands: keep it, and the task to
    /// write, as it now stands.
    pub fn keep_run(&mut self, id: &str, run: Run) -> Option<Task> {
        let task = self.task_mut(id)?;
        match task.runs.last_mut() {
            Some(last) => *last = run,
            None => task.runs.push(run),
        }
        Some(task.clone())
    }

    fn store_run(&mut self, id: &str, run: Run, fx: &mut Vec<Effect<W>>) {
        if let Some(task) = self.keep_run(id, run) {
            fx.push(Effect::Save(Box::new(task)));
        }
    }

    /// Task `id`, holding its place, could not start for `why`, of `kind`.
    /// A schedule's run ends failed, since nobody is watching it and failing
    /// is what says so. Any other's place only passes on, the task as it was.
    pub fn not_started(
        &mut self,
        id: &str,
        why: String,
        kind: Failure,
        now: Now,
    ) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        let unwatched = self.task(id).is_some_and(|task| match task.origin {
            Some(Origin::Schedule { .. } | Origin::Board { .. }) => true,
            Some(Origin::Tool) | None => false,
        });
        match unwatched {
            true => self.fail(id, why, kind, now, &mut fx),
            false => self.release(id, &mut fx),
        }
        fx
    }

    /// Task `id`, holding its place, could not start: its run ends failed,
    /// and its place passes on.
    pub(super) fn fail(
        &mut self,
        id: &str,
        why: String,
        kind: Failure,
        now: Now,
        fx: &mut Vec<Effect<W>>,
    ) {
        let Some(mut run) = self.resumable_run(id) else {
            return self.release(id, fx);
        };
        run.failed(why, kind, now.secs);
        self.store_run(id, run, fx);
        self.let_go(id, fx);
    }

    /// Task `id` has stopped working: pin where its last visit left the work,
    /// then give its place to whoever waits for it.
    pub(super) fn free(&mut self, id: String, from: usize, fx: &mut Vec<Effect<W>>) {
        let unpinned = self.task(&id).and_then(|task| {
            let run = task.runs.last()?;
            let refs = marks::refs_from(&task.id, run, from);
            (!refs.is_empty()).then(|| (run.setup.dir.clone(), refs))
        });
        match unpinned {
            Some((dir, refs)) => fx.push(Effect::Pin {
                task: id,
                dir,
                refs,
                from,
            }),
            None => self.let_go(&id, fx),
        }
    }

    /// Task `id` is over and its last mark pinned: tell whoever is owed it
    /// how it ended, then give its place up.
    pub(super) fn let_go(&mut self, id: &str, fx: &mut Vec<Effect<W>>) {
        if let Some(task) = self.task(id) {
            match &task.origin {
                Some(Origin::Schedule { name, .. }) => {
                    let said = task.ended_said().unwrap_or_else(|| "ended".to_string());
                    fx.push(Effect::Signal(Signal::ScheduleEnded {
                        name: name.clone(),
                        said,
                    }));
                }
                Some(Origin::Board { project, issue }) => {
                    let outcome = task.outcome();
                    fx.push(Effect::Signal(Signal::BoardEnded {
                        project: project.clone(),
                        issue: *issue,
                        done: matches!(outcome, Some(Outcome::Done)),
                        pull: pulls_next(outcome.as_ref()),
                    }));
                }
                Some(Origin::Tool) | None => {}
            }
        }
        self.release(id, fx);
    }
}

/// Whether a board task that ended as `outcome` lets the board start the
/// next of its To do: one done or that a person stopped left the board. One
/// that ended without finishing is left for a person to look at, and
/// pulling the next would most likely end it the same way, one after another
/// until To do is empty.
fn pulls_next(outcome: Option<&Outcome>) -> bool {
    match outcome {
        Some(Outcome::Done | Outcome::Stopped(Stop::ByPerson | Stop::TakenOver)) => true,
        Some(
            Outcome::Stopped(Stop::TimedOut | Stop::LinkLost | Stop::Closed)
            | Outcome::Exhausted { .. }
            | Outcome::Failed(_),
        )
        | None => false,
    }
}

/// Whether a check that ended as `run` passed; `None` when it was stopped
/// or never ended, which says nothing: a person who stopped it knows.
fn check_passed(run: &Run) -> Option<bool> {
    match run.outcome {
        Some(Outcome::Stopped(_)) | None => None,
        Some(Outcome::Done) => Some(true),
        Some(Outcome::Exhausted { .. } | Outcome::Failed(_)) => Some(false),
    }
}

/// The project a run was started from, as a person reads it: its folder.
fn project_of(run: &Run) -> String {
    run.setup.repo.file_name().map_or_else(
        || run.setup.repo.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
