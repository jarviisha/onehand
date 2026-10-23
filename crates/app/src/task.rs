//! Tasks, as the app drives them: every task kept, the runs under way, and
//! the queue each start goes through so one task at a time works in a place.
//!
//! Everything a task or a run decides is `onehand_core`'s; what is here is
//! what needs a session, a timer or a window.

use crate::shell::Shell;
use gpui::{
    AnyWindowHandle, App, BorrowAppContext as _, Global, SharedString, Subscription, WeakEntity,
    Window,
};
use gpui_component::WindowExt as _;
use onehand_core::task::work::UnderReview;
use onehand_core::task::{Group, Task, Working, files, history, marks, queue};
use onehand_core::workflow::{Run, Template};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

mod driver;
mod listing;
mod place;
pub(crate) use driver::{approve, revise, start, stop};
pub(crate) use listing::*;
use place::ended;
pub(crate) use place::{fail, release, request, stop_queued};

/// Every task in this process and what each is doing.
pub(crate) struct Tasks {
    /// Every task kept, unfinished or history. Empty until the boot read lands.
    tasks: Vec<Task>,
    /// Runs under way, by their task's id; a check's has no session.
    live: HashMap<String, driver::Driven>,
    queue: queue::Queue,
    /// Where each queued task was asked for, to start it there.
    asked_in: HashMap<String, (AnyWindowHandle, WeakEntity<Shell>)>,
    /// Tasks that ended while their session's turn was still going, holding
    /// their place until it is over.
    draining: HashMap<String, Vec<Subscription>>,
    /// What a person pressed on each task that is still being read or done,
    /// so the button waits wherever the task is drawn.
    acting: HashMap<String, onehand_core::task::work::Act>,
    /// How many finished tasks of each project were let go to keep the
    /// history bounded.
    // ponytail: counts since boot only; persist it if people ask.
    removed: HashMap<PathBuf, usize>,
    /// How many commands, a check's or a step's, have not exited yet.
    commands: Arc<AtomicUsize>,
    writer: files::Writer,
}

/// A command counted as running until this drops, which its background work
/// does the moment the command has exited.
pub(super) struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            live: HashMap::new(),
            queue: queue::Queue::default(),
            asked_in: HashMap::new(),
            draining: HashMap::new(),
            acting: HashMap::new(),
            removed: HashMap::new(),
            commands: Arc::default(),
            writer: files::Writer::spawn(files::dir()),
        }
    }
}

impl Global for Tasks {}

impl Tasks {
    fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    /// `run` is where task `id`'s last run stands: keep it, and write the task.
    fn store_run(&mut self, id: &str, run: Run) {
        let Some(task) = self.task_mut(id) else {
            return;
        };
        match task.runs.last_mut() {
            Some(last) => *last = run,
            None => task.runs.push(run),
        }
        let task = task.clone();
        self.writer.save(task);
    }

    fn save(&self, id: &str) {
        if let Some(task) = self.task(id) {
            self.writer.save(task.clone());
        }
    }

    /// Count a command as running until the guard drops.
    pub(super) fn command_started(&self) -> Running {
        self.commands.fetch_add(1, Ordering::SeqCst);
        Running(self.commands.clone())
    }

    /// Stop was pressed on task `id` and it has not given its place up
    /// yet: its step's command, or its check, is still being stopped.
    fn stopping(&self, id: &str) -> bool {
        self.live.get(id).is_some_and(|d| d.stopping())
    }

    /// Whether task `id` is running or waiting for its place.
    fn busy(&self, id: &str) -> bool {
        self.working(id).is_some()
    }

    /// Whether task `id` holds a slot of the cap on unattended runs: queued
    /// or running, and not waiting on its pull request's status checks.
    fn holds_slot(&self, id: &str) -> bool {
        match self.working(id) {
            Some(Working::Running) => self
                .driven(id)
                .is_none_or(|d| !d.run().awaiting_status_checks()),
            Some(Working::Queued) => true,
            Some(Working::Waiting) | None => false,
        }
    }

    /// The driver running task `id`'s run, if one is.
    fn driven(&self, id: &str) -> Option<&driver::Driven> {
        self.live.get(id)
    }

    /// The task whose run is driven on session `uid`, if one is.
    fn task_on(&self, uid: u64) -> Option<String> {
        self.live
            .iter()
            .find(|(_, d)| d.uid() == Some(uid))
            .map(|(id, _)| id.clone())
    }

    /// What task `id` is doing, if anything.
    fn working(&self, id: &str) -> Option<Working> {
        if self.queue.queued(id) {
            return Some(Working::Queued);
        }
        if let Some(d) = self.driven(id) {
            return Some(match d.waits_on_person() {
                true => Working::Waiting,
                false => Working::Running,
            });
        }
        self.queue.holds(id).then_some(Working::Running)
    }
}

/// How long quitting waits for the task files still being written.
pub(crate) const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Read every task kept, off the UI thread. Nothing starts by itself: a task
/// that was running or queued when the last process ended reads as
/// interrupted, and waits for a person to resume it.
pub(crate) fn boot(cx: &mut App) {
    cx.default_global::<Tasks>();
    // The process exits right after its last window closes, and a run's last
    // save still queued would die with it. The wait is in the future, which
    // runs once the windows are gone and their sessions have let go of their
    // runs, so those last writes are already queued ahead of it.
    //
    // A command still running is called off and waited for as well: left
    // alone, it would go on writing to the work after the app is gone, beside
    // whatever task starts there next.
    let writer = cx.global::<Tasks>().writer.clone();
    let commands = cx.global::<Tasks>().commands.clone();
    cx.on_app_quit(move |cx| {
        if let Some(t) = cx.try_global::<Tasks>() {
            let steps = t.live.values().filter_map(|d| d.command.as_ref());
            for cancel in steps {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        let (writer, commands) = (writer.clone(), commands.clone());
        async move {
            let until = std::time::Instant::now() + QUIT_WAIT;
            while commands.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if commands.load(Ordering::SeqCst) > 0 {
                eprintln!("onehand: a command was still running at quit");
            }
            if !writer.flush(QUIT_WAIT) {
                eprintln!("onehand: a task's last write may not have landed");
            }
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        let found = cx
            .background_executor()
            .spawn(async { files::load_all_blocking(&files::dir()) })
            .await;
        cx.update(|cx| {
            let tasks: Vec<Task> = found
                .into_iter()
                .filter_map(|(path, read)| {
                    read.inspect_err(|why| {
                        eprintln!("onehand: a task in {} was not read: {why}", path.display())
                    })
                    .ok()
                })
                .collect();
            // Oldest first, as everything that looks for the newest task of an
            // issue reads them: what was read is older than anything added
            // while it was being read, and a task added then may have been
            // written in time to be read too.
            cx.update_global::<Tasks, _>(|t, _| {
                let added = std::mem::replace(&mut t.tasks, tasks);
                t.tasks.retain(|task| added.iter().all(|a| a.id != task.id));
                t.tasks.extend(added);
            });
            enforce_cap(cx);
            // An issue not told how its run ended before the last quit is
            // told now.
            crate::unattended::deliver_all(cx);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Keep `task`, new, and write it.
pub(crate) fn add(task: Task, cx: &mut App) {
    cx.update_global::<Tasks, _>(|t, _| {
        t.writer.save(task.clone());
        t.tasks.push(task);
    });
}

/// The last run of task `id`, to drive, while the task may be picked up:
/// not dismissed, and its run not ended on its own outcome.
pub(crate) fn resumable_run(id: &str, cx: &App) -> Option<Run> {
    cx.try_global::<Tasks>()?
        .task(id)
        .filter(|task| task.resumable())?
        .runs
        .last()
        .cloned()
}

/// A person let interrupted task `id` go: it is kept as history and never
/// offered again.
pub(crate) fn dismiss(id: &str, window: &mut Window, cx: &mut App) {
    let dismissed = cx.update_global::<Tasks, _>(|t, _| {
        if t.busy(id) {
            return None;
        }
        let task = t.task_mut(id)?;
        task.dismissed = true;
        let cut_off = task
            .runs
            .last()
            .filter(|run| run.outcome.is_none())
            .cloned();
        let task = task.clone();
        Some((t.writer.save_said(task), cut_off))
    });
    let Some((written, cut_off)) = dismissed else {
        return;
    };
    let id = id.to_string();
    window
        .spawn(cx, async move |cx| {
            let written = written.await;
            let _ = cx.update(|window, cx| match written {
                // A run cut off by a quit was never reported; letting it go,
                // once that is kept, is its end.
                Ok(()) => {
                    if let Some(run) = cut_off {
                        crate::unattended::keep(&id, &run, None, cx);
                        crate::unattended::deliver(id, cx);
                    }
                    enforce_cap(cx);
                    cx.refresh_windows();
                }
                // Not kept, so not let go: the task goes back to where it was.
                Err(why) => {
                    cx.update_global::<Tasks, _>(|t, _| {
                        if let Some(task) = t.task_mut(&id) {
                            task.dismissed = false;
                        }
                    });
                    window.push_notification(crate::toast::failed("Dismissing the task", why), cx);
                    cx.refresh_windows();
                }
            });
        })
        .detach();
    cx.refresh_windows();
}

/// Task `id`'s worktree and branch were removed: kept with the task, so
/// nothing offers to remove them again.
pub(crate) fn worktree_removed(id: &str, cx: &mut App) {
    cx.update_global::<Tasks, _>(|t, _| {
        if let Some(task) = t.task_mut(id) {
            task.worktree_removed = true;
            t.save(id);
        }
    });
    cx.refresh_windows();
}

/// Where the run on session `uid` stands, as its strip draws it.
pub(crate) struct Shown {
    pub(crate) task: SharedString,
    pub(crate) name: SharedString,
    pub(crate) steps: Vec<SharedString>,
    pub(crate) at: usize,
    /// What it waits on *Continue* or *Revise…* for.
    pub(crate) review: Option<UnderReview>,
    /// Stop was pressed and its step's command is still being stopped.
    pub(crate) stopping: bool,
}

/// What task `id`'s run, under way, waits for approval on now.
pub(crate) fn review_of(id: &str, cx: &App) -> Option<UnderReview> {
    UnderReview::of(cx.try_global::<Tasks>()?.driven(id)?.run())
}

/// Where the run on session `uid` stands, if one drives it.
pub(crate) fn shown(uid: u64, cx: &App) -> Option<Shown> {
    let t = cx.try_global::<Tasks>()?;
    let driven = t.live.get(&t.task_on(uid)?)?;
    let run = driven.run();
    Some(Shown {
        task: driven.task().to_string().into(),
        name: run.template.name.clone().into(),
        steps: run
            .template
            .steps
            .iter()
            .map(|step| step.label.clone().into())
            .collect(),
        at: run.step,
        review: UnderReview::of(run),
        stopping: driven.stopping(),
    })
}

/// What `look` makes of each task, told whether it is working, oldest first.
pub(crate) fn each<R>(cx: &App, look: impl Fn(&Task, bool) -> Option<R>) -> Vec<R> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    t.tasks
        .iter()
        .filter_map(|task| look(task, t.busy(&task.id)))
        .collect()
}

/// What a person pressed on task `id` that is still being done, Stop
/// included, so its button waits.
pub(crate) fn pressed(id: &str, cx: &App) -> Option<onehand_core::task::work::Act> {
    let t = cx.try_global::<Tasks>()?;
    t.acting.get(id).copied().or_else(|| {
        t.stopping(id)
            .then_some(onehand_core::task::work::Act::Stop)
    })
}

/// Say that `act`, pressed on task `id`, is being done, or with `None` that
/// it is over.
pub(crate) fn set_acting(id: &str, act: Option<onehand_core::task::work::Act>, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    cx.update_global::<Tasks, _>(|t, _| match act {
        Some(act) => t.acting.insert(id.to_string(), act),
        None => t.acting.remove(id),
    });
    cx.refresh_windows();
}

/// Task `id` as it is kept.
pub(crate) fn task(id: &str, cx: &App) -> Option<Task> {
    cx.try_global::<Tasks>()?.task(id).cloned()
}

/// Why Resume cannot carry `task` on now: the first thing the preflight of
/// its last run's own setup finds in the way. Pure, so the buttons that offer
/// Resume ask it as they are drawn and refuse with it.
// ponytail: asked per resumable row per frame, and an issue task's facts walk
// every task for its slots, so the cost is resumable rows times tasks, held
// down by the cap on kept tasks; keep the answers beside the issue works if a
// long Tasks page ever stutters.
pub(crate) fn resume_refused(task: &Task, cx: &App) -> Option<String> {
    let template = task.runs.last()?.template.clone();
    let facts = crate::unattended::task_facts(task, template, cx);
    onehand_core::preflight::preflight(onehand_core::preflight::Kind::Resume, &facts)
        .into_iter()
        .find(|f| f.blocks)
        .map(|f| f.text)
}

/// The title of the task holding `place`, unless that task is `except`: a
/// task's own next run never waits behind itself.
pub(crate) fn holder_at(place: &std::path::Path, except: Option<&str>, cx: &App) -> Option<String> {
    let t = cx.try_global::<Tasks>()?;
    let holder = t
        .queue
        .holder_at(place)
        .filter(|holder| Some(*holder) != except)?;
    t.task(holder).map(|task| task.brief.title.clone())
}

/// A person pressed Stop on task `id`, whatever it is doing.
pub(crate) fn stop_task(id: &str, cx: &mut App) {
    let Some(t) = cx.try_global::<Tasks>() else {
        return;
    };
    if t.queue.queued(id) {
        return stop_queued(id, cx);
    }
    if t.live.contains_key(id) {
        driver::stop_task(id, cx);
    }
}

/// Give task `id` a new run of `template`, from step `from` when that is
/// earlier than where it would start, kept but not started: the caller asks
/// for its place, carrying `note` as a revision. Whether there was one to give.
pub(crate) fn retry(
    id: &str,
    template: Template,
    from: Option<&str>,
    note: Option<String>,
    cx: &mut App,
) -> bool {
    cx.update_global::<Tasks, _>(|t, _| {
        if t.busy(id) {
            return false;
        }
        let made = t.task_mut(id).is_some_and(|task| {
            task.retry(onehand_core::task::new_id(), template, from, note)
                .is_some()
        });
        if made {
            t.save(id);
        }
        made
    })
}

/// Give task `id` a new run with what Settings say now, as `plan` worked it
/// out, kept but not started: the caller asks for its place. Whether there
/// was one to give.
pub(crate) fn retry_with(
    id: &str,
    plan: onehand_core::workflow::WithCurrent,
    cx: &mut App,
) -> bool {
    cx.update_global::<Tasks, _>(|t, _| {
        if t.busy(id) {
            return false;
        }
        let made = t.task_mut(id).is_some_and(|task| {
            task.retry_with(onehand_core::task::new_id(), plan)
                .is_some()
        });
        if made {
            t.save(id);
        }
        made
    })
}

/// Run check task `id`, whose place it now holds: pin the work, run the
/// command, and say in `window` how it went. No session: there is no agent
/// to watch.
pub(crate) fn drive_check(id: String, window: AnyWindowHandle, cx: &mut App) {
    let Some(mut run) = resumable_run(&id, cx) else {
        return release(id, cx);
    };
    // Counted from what landed, as a session's run is.
    let from = run.pinned_count();
    let first = run.resume();
    driver::start_check(id, run, first, from, window, cx);
}

/// Let go of the finished tasks past what each project keeps: their files,
/// then the marks they pinned, off the UI thread.
fn enforce_cap(cx: &mut App) {
    let Some(t) = cx.try_global::<Tasks>() else {
        return;
    };
    let listed: Vec<(&Task, Group)> = t
        .tasks
        .iter()
        .map(|task| (task, task.group(t.working(&task.id))))
        .collect();
    let gone = history::over_cap(&listed);
    if gone.is_empty() {
        return;
    }
    let dropped = cx.update_global::<Tasks, _>(|t, _| {
        let mut dropped = Vec::new();
        t.tasks.retain(|task| {
            if !gone.contains(&task.id) {
                return true;
            }
            t.writer.remove(task.id.clone());
            *t.removed.entry(task.setup.repo.clone()).or_default() += 1;
            dropped.push((
                task.setup.dir.clone(),
                task.setup.repo.clone(),
                task.id.clone(),
            ));
            false
        });
        dropped
    });
    cx.background_executor()
        .spawn(async move {
            for (dir, repo, id) in dropped {
                let at = if dir.is_dir() { dir } else { repo };
                if let Err(why) = marks::drop_blocking(&at, &id) {
                    eprintln!("onehand: the marks of an old task were not dropped: {why}");
                }
            }
        })
        .detach();
}

/// Change the issue task `id` works, and write the task.
pub(crate) fn update_issue(
    id: &str,
    cx: &mut App,
    change: impl FnOnce(&mut onehand_core::unattended::IssueSource),
) {
    cx.update_global::<Tasks, _>(|t, _| {
        let Some(task) = t.task_mut(id) else {
            return;
        };
        let onehand_core::task::Source::Issue(issue) = &mut task.source else {
            return;
        };
        change(issue);
        t.save(id);
    });
}

/// Every issue task whose issue has not been told how a run ended.
pub(crate) fn undelivered(cx: &App) -> Vec<String> {
    cx.try_global::<Tasks>().map_or_else(Vec::new, |t| {
        t.tasks
            .iter()
            .filter(|task| task.issue().is_some_and(|issue| !issue.unsent.is_empty()))
            .map(|task| task.id.clone())
            .collect()
    })
}
