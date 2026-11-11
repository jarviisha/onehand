//! Every task this process keeps, and the place each works in: what a start
//! asks for, what waits behind whom, and what happens when a run ends.
//!
//! **Decided here, carried out elsewhere.** A [`TasksDrive`] takes the
//! front end's commands as methods (a request for a place, a run that ended,
//! a Stop on a queued task, a dismiss, a retry) and is told what the world
//! found ([`Input`]); it answers with what to do ([`Effect`]). It reads no
//! disk and keeps no timer: where a place is, a write landing and a turn
//! being over all come back as inputs.
//!
//! **One task works in a place at a time.** Every start goes through
//! [`TasksDrive::request`]: a free place starts the task at once, a taken one
//! queues it, first in first out, and the place passes on only once the work
//! has stopped: a run that ended while its session's turn still goes holds
//! its place until that turn is over, or for [`DRAIN_LIMIT`] at most. Its
//! last mark is pinned first, and then the next task waiting starts in the
//! window it was asked from.
//!
//! **What a window is is the front end's** (`W`): the drive only keeps the
//! one each queued task was asked from, to start it there.
//!
//! The runs themselves are their own drives ([`super::drive::RunDrive`]),
//! held beside this one; [`link`] joins them.

use super::queue::Queue;
use super::{history, Launch, Task, Working};
use crate::workflow::{Run, Template, WithCurrent};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

mod begin;
mod ending;
pub mod link;
mod off_screen;
mod place;

pub use begin::{Begin, Start, SESSION_NOT_STARTED};
pub use off_screen::Cut;

/// How long a cancelled turn is waited for before its place is given up
/// anyway: a turn that never ends would hold the place, and a slot, for good.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(30);

/// Where a task asked for its place landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// It holds the place and is being driven.
    Started,
    /// It waits behind another, as said.
    Queued(String),
    /// It was not taken, and why.
    Refused(String),
}

/// What the world found, as the drive is told it.
#[derive(Debug, Clone, PartialEq)]
pub enum Input<W> {
    /// Where task `task`, asked for from `window`, would work: `place`, and
    /// whether that window is still open to start it in.
    PlaceFound {
        task: String,
        place: PathBuf,
        window: W,
        window_open: bool,
    },
    /// What [`Effect::Cut`] came to for task `task`'s `launch`, from
    /// `window`.
    Cut {
        task: String,
        launch: Box<Launch>,
        window: W,
        made: Cut,
    },
    /// What [`Effect::Keep`] came to: whether `window` was still `open` to
    /// keep task `task`'s project in.
    Kept {
        task: String,
        launch: Box<Launch>,
        window: W,
        open: bool,
    },
    /// Task `task`, holding its place, was driven in its window: begun, or
    /// refused there for `why` (its run's end is
    /// [`TasksDrive::not_started`]'s).
    Driven {
        task: String,
        started: Result<(), String>,
    },
    /// Task `task` could not be driven in the window it was to start in:
    /// that window has gone.
    NotDriven(String),
    /// The turn task `task`'s run left going is over, its session went, or
    /// [`DRAIN_LIMIT`] ran out.
    Drained(String),
    /// What was deferred for task `task`: its work has stopped, and its
    /// marks from the `from`th boundary on are still to be pinned.
    Free { task: String, from: usize },
    /// Task `task`'s last marks landed on this commit, or did not.
    Pinned {
        task: String,
        from: usize,
        pinned: Result<String, String>,
    },
    /// A task's write landed, or why it did not.
    Saved(Result<(), String>),
    /// Task `task`'s write of its dismissal landed, or why it did not.
    Dismissed {
        task: String,
        written: Result<(), String>,
    },
}

/// What a person, or the log, is told; the words for each and where they go
/// are the front end's, unless the drive says them (`said`).
#[derive(Debug, Clone, PartialEq)]
pub enum Signal<W> {
    /// A task asked for its place from `window` waits behind another.
    Queued { window: W, said: String },
    /// A start asked for from `window` was refused, for `why`.
    Refused { window: W, why: String },
    /// A check asked for from `window` ended in `project`: passed or not.
    CheckEnded {
        window: W,
        project: String,
        passed: bool,
    },
    /// A schedule's task ended with nobody necessarily watching: the
    /// schedule's `name`, and how it ended.
    ScheduleEnded { name: String, said: String },
    /// A dismiss asked for from `window` was not kept, for `why`, and the
    /// task is as it was.
    DismissFailed { window: W, why: String },
    /// A task's file could not be written; said once until a write lands.
    SaveFailed(String),
    /// A task's last mark could not be pinned.
    PinFailed(String),
}

/// What to do, in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect<W> {
    /// Write this task, in order with every other write, and report
    /// [`Input::Saved`]. Recorded: every write is of the whole task, so the
    /// next one holds what a failed one missed.
    Save(Box<Task>),
    /// Write this task, dismissed, and report [`Input::Dismissed`]. Recorded:
    /// a dismissal not written is put back.
    SaveDismissed(Box<Task>),
    /// Remove task `0`'s file, after every write sent before it, so a write
    /// still queued cannot bring it back. Not answered: the writer logs.
    Remove(String),
    /// Drop the marks task `id` pinned, in its work at `dir` or, once that is
    /// gone, in `repo`. Not answered: a mark left behind is only a ref.
    DropMarks {
        dir: PathBuf,
        repo: PathBuf,
        id: String,
    },
    /// Find the place task `task`'s work at `dir` is in, and report
    /// [`Input::PlaceFound`] with `window`. Dropped late: a read.
    FindPlace {
        task: String,
        dir: PathBuf,
        window: W,
    },
    /// Drive task `task`, which holds its place, in `window`; report
    /// [`Input::Driven`] when it was, [`Input::NotDriven`] when that window
    /// has gone, and [`TasksDrive::not_started`] when it cannot start there.
    Drive { task: String, window: W },
    /// Report [`Input::Drained`] once task `task`'s session's turn is over,
    /// the session goes, or `limit` runs out, whichever is first. Dropped
    /// late: only the first counts.
    WatchDrain { task: String, limit: Duration },
    /// Pin task `task`'s marks `refs`, from its `from`th boundary, in the
    /// work at `dir`, and report [`Input::Pinned`]. Awaited: the place is
    /// let go only once it lands.
    Pin {
        task: String,
        dir: PathBuf,
        refs: Vec<String>,
        from: usize,
    },
    /// Cut a worktree of `launch`'s repository for task `task`, named for
    /// its title, and report [`Input::Cut`]. Recorded: a worktree cut is
    /// where the task works, whatever the drive did since.
    Cut {
        task: String,
        launch: Box<Launch>,
        window: W,
    },
    /// Keep the project `launch` works in, in `window`, without putting it
    /// on screen, and report [`Input::Kept`] at once.
    Keep {
        task: String,
        launch: Box<Launch>,
        window: W,
    },
    /// Ask for task `task`'s place from `window`, held to the cap on
    /// unattended runs: the link's, which asks it.
    Ask { task: String, window: W },
    /// Report this input once the call being handled is over.
    Defer(Input<W>),
    /// Task `task`, of an issue, has let its place go: tell the issue.
    IssueEnded(String),
    /// Send task `task`'s issue the reports it has not been given yet.
    Deliver(String),
    /// Send every issue the reports it has not been given yet.
    DeliverAll,
    /// Where task `task`'s request landed, for whoever asked to be told.
    Told { task: String, placed: Placed },
    /// Task `task` asked for its place, or was refused it: an issue's run no
    /// longer counts as starting.
    Asked(String),
    /// Close session `uid`, in `window`: its finished schedule's task let go.
    CloseSession { uid: u64, window: W },
    /// Tell this.
    Signal(Signal<W>),
    /// What the tasks show changed: draw them again.
    Refresh,
}

/// A dismissal waiting for its write: the run it lets go of, when that run
/// was cut off, and the window it was asked from.
#[derive(Debug)]
struct Dismissing<W> {
    cut_off: Option<Run>,
    window: W,
}

/// Every task in this process and the place each holds or waits for.
#[derive(Debug)]
pub struct TasksDrive<W> {
    /// Every task kept, unfinished or history, oldest first.
    tasks: Vec<Task>,
    queue: Queue,
    /// Where each queued task was asked for, to start it there.
    asked_in: HashMap<String, W>,
    /// Tasks that ended while their session's turn was still going, by the
    /// boundary their marks are pinned to.
    draining: HashMap<String, usize>,
    dismissing: HashMap<String, Dismissing<W>>,
    /// How many finished tasks of each project were let go to keep the
    /// history bounded.
    // ponytail: counts since boot only; persist it if people ask.
    removed: HashMap<PathBuf, usize>,
    /// Tasks to be driven off screen the first time they are driven: ones a
    /// tool started, which must not move what the person is looking at.
    off_screen: HashSet<String>,
    /// A write failed and was said; nothing more is said until one lands.
    save_failed: bool,
}

impl<W> Default for TasksDrive<W> {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            queue: Queue::default(),
            asked_in: HashMap::new(),
            draining: HashMap::new(),
            dismissing: HashMap::new(),
            removed: HashMap::new(),
            off_screen: HashSet::new(),
            save_failed: false,
        }
    }
}

impl<W: Copy> TasksDrive<W> {
    /// Every task kept, oldest first.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// Task `id` as it is kept.
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    /// Who holds each place, and who waits.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// How many finished tasks of the project at `repo` were let go since
    /// boot.
    pub fn removed(&self, repo: &std::path::Path) -> usize {
        self.removed.get(repo).copied().unwrap_or(0)
    }

    /// What task `id` does as its place says: queued, or holding its place.
    /// A run holding it may be waiting on a person, which is its own drive's
    /// to say.
    pub fn working(&self, id: &str) -> Option<Working> {
        match (self.queue.queued(id), self.queue.holds(id)) {
            (true, _) => Some(Working::Queued),
            (false, true) => Some(Working::Running),
            (false, false) => None,
        }
    }

    /// Whether task `id` is running or waiting for its place.
    pub fn busy(&self, id: &str) -> bool {
        self.working(id).is_some()
    }

    /// The last run of task `id`, to drive, while the task may be picked up:
    /// not dismissed, and its run not ended on its own outcome.
    pub fn resumable_run(&self, id: &str) -> Option<Run> {
        self.task(id)
            .filter(|task| task.resumable())?
            .runs
            .last()
            .cloned()
    }

    /// The tasks read at boot, `read`, joined by those added while they were
    /// read. Oldest first, as everything that looks for the newest task of an
    /// issue reads them: what was read is older than anything added meanwhile,
    /// and a task added then may have been written in time to be read too.
    pub fn loaded(&mut self, read: Vec<Task>) -> Vec<Effect<W>> {
        let added = std::mem::replace(&mut self.tasks, read);
        self.tasks
            .retain(|task| added.iter().all(|a| a.id != task.id));
        self.tasks.extend(added);
        let mut fx = self.enforce_cap();
        // An issue not told how its run ended before the last quit is told now.
        fx.push(Effect::DeliverAll);
        fx.push(Effect::Refresh);
        fx
    }

    /// Keep `task`, new, and write it.
    pub fn add(&mut self, task: Task) -> Vec<Effect<W>> {
        let fx = vec![Effect::Save(Box::new(task.clone()))];
        self.tasks.push(task);
        fx
    }

    /// Drive task `id` off screen the first time it is driven.
    pub fn drive_off_screen(&mut self, id: &str) {
        self.off_screen.insert(id.to_string());
    }

    /// Whether task `id` is to be driven off screen this time, which only its
    /// first drive is: a Resume or Retry a person presses later shows as
    /// theirs.
    fn take_off_screen(&mut self, id: &str) -> bool {
        self.off_screen.remove(id)
    }

    /// Something the world found: what to do about it.
    pub fn handle(&mut self, input: Input<W>) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        match input {
            Input::PlaceFound {
                task,
                place,
                window,
                window_open,
            } => self.place_found(task, place, window, window_open, &mut fx),
            Input::Cut {
                task,
                launch,
                window,
                made,
            } => fx.push(self.cut(task, *launch, window, made)),
            Input::Kept {
                task,
                launch,
                window,
                open,
            } => self.kept(task, *launch, window, open, &mut fx),
            Input::Driven { task, started } => fx.push(Effect::Told {
                task,
                placed: match started {
                    Ok(()) => Placed::Started,
                    Err(why) => Placed::Refused(why),
                },
            }),
            Input::NotDriven(task) => {
                fx.push(Effect::Told {
                    task: task.clone(),
                    placed: Placed::Refused("the window closed".to_string()),
                });
                self.release(&task, &mut fx);
            }
            Input::Drained(task) => {
                if let Some(from) = self.draining.remove(&task) {
                    self.free(task, from, &mut fx);
                }
            }
            Input::Free { task, from } => self.free(task, from, &mut fx),
            Input::Pinned { task, from, pinned } => {
                match pinned {
                    Ok(commit) => {
                        if let Some(t) = self.task_mut(&task) {
                            if let Some(run) = t.runs.last_mut() {
                                run.pinned(&commit, from);
                            }
                            fx.push(Effect::Save(Box::new(t.clone())));
                        }
                    }
                    Err(why) => fx.push(Effect::Signal(Signal::PinFailed(why))),
                }
                self.let_go(&task, &mut fx);
            }
            Input::Saved(Ok(())) => self.save_failed = false,
            Input::Saved(Err(why)) => {
                if !std::mem::replace(&mut self.save_failed, true) {
                    fx.push(Effect::Signal(Signal::SaveFailed(why)));
                }
            }
            Input::Dismissed { task, written } => self.dismissed(task, written, &mut fx),
        }
        fx
    }

    /// A person let task `id` go, from `window`: kept as history and never
    /// offered again, once that is written. A task at work is left alone.
    pub fn dismiss(&mut self, id: &str, window: W) -> Vec<Effect<W>> {
        if self.busy(id) {
            return Vec::new();
        }
        let Some(task) = self.task_mut(id) else {
            return Vec::new();
        };
        task.dismissed = true;
        let cut_off = task
            .runs
            .last()
            .filter(|run| run.outcome.is_none())
            .cloned();
        let task = Box::new(task.clone());
        self.dismissing
            .insert(id.to_string(), Dismissing { cut_off, window });
        vec![Effect::SaveDismissed(task), Effect::Refresh]
    }

    fn dismissed(&mut self, id: String, written: Result<(), String>, fx: &mut Vec<Effect<W>>) {
        let Some(Dismissing { cut_off, window }) = self.dismissing.remove(&id) else {
            return;
        };
        match written {
            // A run cut off by a quit was never reported; letting it go, once
            // that is kept, is its end.
            Ok(()) => {
                let issue = self.task(&id).is_some_and(|task| task.issue().is_some());
                if let Some(run) = cut_off.filter(|_| issue) {
                    self.keep_report(&id, &run, None);
                    if let Some(task) = self.task(&id) {
                        fx.push(Effect::Save(Box::new(task.clone())));
                    }
                    fx.push(Effect::Deliver(id));
                }
                fx.extend(self.enforce_cap());
            }
            // Not kept, so not let go: the task goes back to where it was.
            Err(why) => {
                if let Some(task) = self.task_mut(&id) {
                    task.dismissed = false;
                }
                fx.push(Effect::Signal(Signal::DismissFailed { window, why }));
            }
        }
        fx.push(Effect::Refresh);
    }

    /// Give task `id` a new run `run_id` of `template`, from step `from`
    /// when that is earlier than where it would start, carrying `note` as a
    /// revision; kept but not started: the caller asks for its place.
    /// Whether there was one to give: none while the task is at work.
    pub fn retry(
        &mut self,
        id: &str,
        run_id: String,
        template: Template,
        from: Option<&str>,
        note: Option<String>,
    ) -> (bool, Vec<Effect<W>>) {
        self.renew(id, |task| {
            task.retry(run_id, template, from, note).is_some()
        })
    }

    /// Give task `id` a new run `run_id` with what Settings say now, as
    /// `plan` worked it out; as [`Self::retry`].
    pub fn retry_with(
        &mut self,
        id: &str,
        run_id: String,
        plan: WithCurrent,
    ) -> (bool, Vec<Effect<W>>) {
        self.renew(id, |task| task.retry_with(run_id, plan).is_some())
    }

    fn renew(&mut self, id: &str, make: impl FnOnce(&mut Task) -> bool) -> (bool, Vec<Effect<W>>) {
        if self.busy(id) {
            return (false, Vec::new());
        }
        let Some(task) = self.task_mut(id) else {
            return (false, Vec::new());
        };
        match make(task) {
            true => (true, vec![Effect::Save(Box::new(task.clone()))]),
            false => (false, Vec::new()),
        }
    }

    /// Task `id`'s worktree and branch were removed: kept with the task, so
    /// nothing offers to remove them again.
    pub fn worktree_removed(&mut self, id: &str) -> Vec<Effect<W>> {
        let Some(task) = self.task_mut(id) else {
            return vec![Effect::Refresh];
        };
        task.worktree_removed = true;
        vec![Effect::Save(Box::new(task.clone())), Effect::Refresh]
    }

    /// Change the issue task `id` works, and write the task.
    pub fn change_issue(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut crate::unattended::IssueSource),
    ) -> Vec<Effect<W>> {
        let Some(task) = self.task_mut(id) else {
            return Vec::new();
        };
        let super::Source::Issue(issue) = &mut task.source else {
            return Vec::new();
        };
        change(issue);
        vec![Effect::Save(Box::new(task.clone()))]
    }

    /// Let go of the finished tasks past what each project keeps: their
    /// files, then the marks they pinned.
    fn enforce_cap(&mut self) -> Vec<Effect<W>> {
        let listed: Vec<(&Task, super::Group)> = self
            .tasks
            .iter()
            .map(|task| (task, task.group(self.working(&task.id))))
            .collect();
        let gone = history::over_cap(&listed);
        let mut fx = Vec::new();
        if gone.is_empty() {
            return fx;
        }
        let mut dropped = Vec::new();
        let removed = &mut self.removed;
        self.tasks.retain(|task| {
            if !gone.contains(&task.id) {
                return true;
            }
            fx.push(Effect::Remove(task.id.clone()));
            *removed.entry(task.setup.repo.clone()).or_default() += 1;
            dropped.push(Effect::DropMarks {
                dir: task.setup.dir.clone(),
                repo: task.setup.repo.clone(),
                id: task.id.clone(),
            });
            false
        });
        fx.extend(dropped);
        fx
    }
}

#[cfg(test)]
mod tests;
