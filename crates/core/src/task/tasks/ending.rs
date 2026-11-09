//! A run that ended, kept in its task: what its end tells, and the place it
//! holds given up once its work has stopped and its last mark is pinned.

use super::{Effect, Input, Signal, TasksDrive, DRAIN_LIMIT};
use crate::task::{marks, Origin, Source, Task};
use crate::workflow::{Failure, Outcome, Run};

impl<W: Copy> TasksDrive<W> {
    /// Task `id`'s run ended as `run`, which kept the marks of `from` of its
    /// boundaries, on session `uid` in `window`, if it had one: keep it, and
    /// let its place go once the work has stopped, which is at once unless
    /// its session's turn still goes (`turn_going`). `asked` is what a card
    /// the run waited on asked.
    #[allow(clippy::too_many_arguments)]
    pub fn run_ended(
        &mut self,
        id: &str,
        run: Run,
        from: usize,
        turn_going: bool,
        asked: Option<String>,
        uid: Option<u64>,
        window: W,
    ) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
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
        self.keep_report(id, &run, asked);
        self.store_run(id, run, &mut fx);
        // A finished schedule's session leaves the rail once the run is let
        // go, so nothing of the run is left listening to it.
        let closes = uid.filter(|_| self.task(id).is_some_and(Task::closes_session));
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
        if let Some(uid) = closes {
            fx.push(Effect::CloseSession { uid, window });
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

    /// Keep how `run` of task `id` ended for its issue, if it works one, in
    /// the task itself, so the task's next write holds it before anything is
    /// sent: `asked` is what a card the run waited on asked.
    pub(super) fn keep_report(&mut self, id: &str, run: &Run, asked: Option<String>) {
        if let Some(Source::Issue(issue)) = self.task_mut(id).map(|task| &mut task.source) {
            issue.keep_report(run, asked);
        }
    }

    fn store_run(&mut self, id: &str, run: Run, fx: &mut Vec<Effect<W>>) {
        if let Some(task) = self.keep_run(id, run) {
            fx.push(Effect::Save(Box::new(task)));
        }
    }

    /// Task `id`, holding its place, could not start for `why`, of `kind`.
    /// An issue's run ends failed, so its issue is told rather than left
    /// claimed with nothing after the claim; a schedule's too, since nobody
    /// is watching it either and failing is what says so. Any other's place
    /// only passes on, the task as it was.
    pub fn not_started(&mut self, id: &str, why: String, kind: Failure) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        let unwatched = self.task(id).is_some_and(|task| {
            task.issue().is_some() || matches!(task.origin, Some(Origin::Schedule { .. }))
        });
        match unwatched {
            true => self.fail(id, why, kind, &mut fx),
            false => self.release(id, &mut fx),
        }
        fx
    }

    /// Task `id`, holding its place, could not start: its run ends failed,
    /// its issue is told, and its place passes on.
    pub(super) fn fail(&mut self, id: &str, why: String, kind: Failure, fx: &mut Vec<Effect<W>>) {
        let Some(mut run) = self.resumable_run(id) else {
            return self.release(id, fx);
        };
        run.failed(why, kind);
        self.keep_report(id, &run, None);
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
            if let Some(Origin::Schedule { name, .. }) = &task.origin {
                let said = task.ended_said().unwrap_or_else(|| "ended".to_string());
                fx.push(Effect::Signal(Signal::ScheduleEnded {
                    name: name.clone(),
                    said,
                }));
            }
            if task.issue().is_some() {
                fx.push(Effect::IssueEnded(id.to_string()));
            }
        }
        self.release(id, fx);
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
