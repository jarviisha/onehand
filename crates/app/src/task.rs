//! Tasks, as the app drives them: every task kept, the runs under way, and
//! the queue each start goes through so one task at a time works in a place.
//!
//! Everything a task or a run decides is `onehand_core`'s; what is here is
//! what needs a session, a timer or a window.

use crate::shell::Shell;
use gpui::{
    AnyWindowHandle, App, BorrowAppContext as _, Context, Global, SharedString, Subscription,
    WeakEntity, Window,
};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use onehand_core::task::{Group, Task, Working, files, history, marks, queue};
use onehand_core::workflow::{
    Action, ApprovalAt, Failure, Run, Stop, Template, run_command_blocking,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod driver;
mod listing;
pub(crate) use driver::{approve, revise, start, stop};
pub(crate) use listing::*;

/// Every task in this process and what each is doing.
pub(crate) struct Tasks {
    /// Every task kept, unfinished or history. Empty until the boot read lands.
    tasks: Vec<Task>,
    /// Runs under way, by the uid of the session they drive.
    live: HashMap<u64, driver::Driven>,
    queue: queue::Queue,
    /// Where each queued task was asked for, to start it there.
    asked_in: HashMap<String, (AnyWindowHandle, WeakEntity<Shell>)>,
    /// Tasks that ended while their session's turn was still going, holding
    /// their place until it is over.
    draining: HashMap<String, Vec<Subscription>>,
    /// Checks running, each with the flag that calls its command off.
    checks: HashMap<String, Arc<AtomicBool>>,
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
            checks: HashMap::new(),
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
                .is_none_or(|d| !d.run.awaiting_status_checks()),
            Some(Working::Queued) => true,
            Some(Working::Waiting) | None => false,
        }
    }

    /// The driver running task `id`'s run, if one is.
    fn driven(&self, id: &str) -> Option<&driver::Driven> {
        self.live.values().find(|d| d.task == id)
    }

    /// The session task `id`'s run is driven on, if one is.
    fn live_uid(&self, id: &str) -> Option<u64> {
        self.live
            .iter()
            .find(|(_, d)| d.task == id)
            .map(|(uid, _)| *uid)
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
        (self.queue.holds(id) || self.checks.contains_key(id)).then_some(Working::Running)
    }
}

/// How long quitting waits for the task files still being written.
const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

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
            for cancel in t.checks.values().chain(steps) {
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
            cx.update_global::<Tasks, _>(|t, _| t.tasks.extend(tasks));
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

/// Start task `id` from this window once its place is free: at once when
/// nobody works there, or after the task that does, saying so. A task
/// already running or waiting is left as it is.
pub(crate) fn request(id: String, window: &mut Window, cx: &mut Context<Shell>) {
    if let Some(why) = crate::unattended::over_cap(&id, cx) {
        // A retry's new run never began: dropped, the task is as it was.
        cx.update_global::<Tasks, _>(|t, _| {
            let Some(task) = t.task_mut(&id) else {
                return;
            };
            if task.runs.len() > 1 && task.runs.last().is_some_and(|run| !run.begun()) {
                task.runs.pop();
                t.save(&id);
            }
        });
        window.push_notification(Notification::warning(why), cx);
        cx.refresh_windows();
        return;
    }
    let Some(dir) = cx
        .try_global::<Tasks>()
        .and_then(|t| t.task(&id))
        .map(|task| task.setup.dir.clone())
    else {
        crate::unattended::placed(&id, cx);
        return;
    };
    cx.spawn_in(window, async move |shell, cx| {
        let place = cx
            .background_executor()
            .spawn(async move { queue::place_blocking(&dir) })
            .await;
        let asked = id.clone();
        let landed = shell.update_in(cx, |shell: &mut Shell, window, cx| {
            let handle = window.window_handle();
            let weak = cx.entity().downgrade();
            let free = cx.update_global::<Tasks, _>(|t, _| {
                if t.busy(&id) {
                    return None;
                }
                let free = t.queue.ask(place.clone(), id.clone());
                if !free {
                    t.asked_in.insert(id.clone(), (handle, weak));
                }
                Some(free)
            });
            // Asked for, or held already: an issue's run counts against the
            // cap from here as queued or running.
            crate::unattended::placed(&id, cx);
            match free {
                Some(true) => shell.drive_task(id, window, cx),
                Some(false) => {
                    let title = cx
                        .global::<Tasks>()
                        .task(&id)
                        .map_or_else(String::new, |task| task.brief.title.clone());
                    let at = place.file_name().map_or_else(
                        || place.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    window.push_notification(
                        Notification::info(format!(
                            "{title} is queued behind the task working in {at}"
                        )),
                        cx,
                    );
                    cx.refresh_windows();
                }
                None => {}
            }
        });
        // The window went while the place was looked up, so nothing asks for
        // it: the cap lets go here, and the task reads cut off, as after a
        // restart, rather than holding a slot until one.
        if landed.is_err() {
            gpui::AsyncApp::update(cx, |cx| crate::unattended::placed(&asked, cx));
        }
    })
    .detach();
}

/// Task `id` ended in `session`: its place is given up once the work has
/// stopped, which is at once unless the session's turn is still going.
/// Marks from the `from`th boundary on are still to be pinned.
fn ended(
    id: String,
    from: usize,
    session: Option<&gpui::Entity<crate::chat::session::ChatSession>>,
    cx: &mut App,
) {
    let Some(session) = session.filter(|s| s.read(cx).chat.busy) else {
        cx.defer(move |cx| freed(id, from, cx));
        return;
    };
    let drained = {
        let id = id.clone();
        move |cx: &mut App| {
            let id = id.clone();
            cx.defer(move |cx| {
                let waited = cx.update_global::<Tasks, _>(|t, _| t.draining.remove(&id));
                if waited.is_some() {
                    freed(id, from, cx);
                }
            });
        }
    };
    let on_event = drained.clone();
    let watch = cx.subscribe(
        session,
        move |_, event: &crate::chat::session::ChatEvent, cx| {
            use crate::chat::session::ChatEvent as E;
            match event {
                E::TurnEnded | E::Disconnected => on_event(cx),
                E::Appended | E::AwaitingUser(_) | E::OpenFile(_) => {}
            }
        },
    );
    let gone = cx.observe_release(session, move |_, cx| drained(cx));
    cx.update_global::<Tasks, _>(|t, _| {
        t.draining.insert(id, vec![watch, gone]);
    });
}

/// Task `id` has stopped working: pin where its last visit left the work,
/// then give its place to whoever waits for it. An issue it works is told
/// how the run ended.
fn freed(id: String, from: usize, cx: &mut App) {
    let unpinned = cx.try_global::<Tasks>().and_then(|t| {
        let task = t.task(&id)?;
        let run = task.runs.last()?;
        let refs = marks::refs_from(&task.id, run, from);
        (!refs.is_empty()).then(|| (run.setup.dir.clone(), refs))
    });
    let Some((dir, refs)) = unpinned else {
        return let_go(id, cx);
    };
    cx.spawn(async move |cx| {
        let pinned = cx
            .background_executor()
            .spawn(async move { marks::pin_blocking(&dir, &refs) })
            .await;
        cx.update(|cx| {
            match pinned {
                Ok(commit) => cx.update_global::<Tasks, _>(|t, _| {
                    if let Some(run) = t.task_mut(&id).and_then(|task| task.runs.last_mut()) {
                        run.pinned(&commit, from);
                    }
                    t.save(&id);
                }),
                Err(why) => eprintln!("onehand: a task's last mark was not pinned: {why}"),
            }
            let_go(id, cx);
        });
    })
    .detach();
}

/// Task `id`, holding its place, could not start for `why`, of `kind`: its
/// run ends failed, its issue is told, and its place passes on.
pub(crate) fn fail(id: String, why: String, kind: Failure, cx: &mut App) {
    let Some(mut run) = resumable_run(&id, cx) else {
        return release(id, cx);
    };
    run.failed(why, kind);
    crate::unattended::keep(&id, &run, false, None, cx);
    cx.update_global::<Tasks, _>(|t, _| t.store_run(&id, run));
    let_go(id, cx);
}

/// Task `id` is over and its last mark pinned: tell an issue it works how it
/// ended, then give its place up.
fn let_go(id: String, cx: &mut App) {
    crate::unattended::ended(&id, cx);
    release(id, cx);
}

/// Give task `id`'s place up, and start the next task waiting for it in the
/// window it was asked from. One that cannot start there stays interrupted,
/// and the place passes on.
pub(crate) fn release(id: String, cx: &mut App) {
    let next = cx.update_global::<Tasks, _>(|t, _| {
        let next = t.queue.release(&id)?;
        let asked = t.asked_in.remove(&next);
        Some((next, asked))
    });
    enforce_cap(cx);
    cx.refresh_windows();
    let Some((next, asked)) = next else {
        return;
    };
    let started = asked.and_then(|(handle, shell)| {
        let shell = shell.upgrade()?;
        let next = next.clone();
        handle
            .update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.drive_task(next, window, cx))
            })
            .ok()
    });
    if started.is_none() {
        release(next, cx);
    }
}

/// A person pressed Stop on queued task `id`: it no longer waits. One that
/// never ran a step is over, stopped by a person, with no empty run kept;
/// one resumed goes back to how it was.
pub(crate) fn stop_queued(id: &str, cx: &mut App) {
    let dropped = cx.update_global::<Tasks, _>(|t, _| {
        if !t.queue.call_off(id) {
            return None;
        }
        t.asked_in.remove(id);
        let task = t.task_mut(id)?;
        let dropped = match task.runs.last().is_some_and(|run| !run.begun()) {
            true => task.runs.pop(),
            false => None,
        };
        t.save(id);
        dropped
    });
    // An issue whose run never began is told it was stopped, or it would
    // stay claimed with nothing after the claim.
    if let Some(mut run) = dropped {
        run.outcome = Some(onehand_core::workflow::Outcome::Stopped(Stop::ByPerson));
        crate::unattended::keep(id, &run, false, None, cx);
        crate::unattended::deliver(id.to_string(), cx);
    }
    enforce_cap(cx);
    cx.refresh_windows();
}

/// A person let interrupted task `id` go: it is kept as history and never
/// offered again.
pub(crate) fn dismiss(id: &str, cx: &mut App) {
    let cut_off = cx.update_global::<Tasks, _>(|t, _| {
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
        t.save(id);
        cut_off
    });
    // A run cut off by a quit was never reported; letting it go is its end.
    if let Some(run) = cut_off {
        crate::unattended::keep(id, &run, run.begun(), None, cx);
        crate::unattended::deliver(id.to_string(), cx);
    }
    enforce_cap(cx);
    cx.refresh_windows();
}

/// Where the run on session `uid` stands, as its strip draws it.
pub(crate) struct Shown {
    pub(crate) task: SharedString,
    pub(crate) name: SharedString,
    pub(crate) steps: Vec<SharedString>,
    pub(crate) at: usize,
    /// What it waits on *Continue* or *Revise…* for.
    pub(crate) review: Option<Review>,
}

/// What a run waits for approval on, as a press is drawn from it.
#[derive(Clone)]
pub(crate) struct Review {
    /// The label of the step that answered.
    pub(crate) of: SharedString,
    pub(crate) answer: SharedString,
    /// The visit it is read at, which the press carries.
    pub(crate) at: ApprovalAt,
}

impl Review {
    fn of(run: &Run) -> Option<Self> {
        let (step, answer) = run.under_review()?;
        Some(Self {
            of: step.label.clone().into(),
            answer: answer.to_string().into(),
            at: run.approval_at()?,
        })
    }
}

/// What task `id`'s run, under way, waits for approval on now.
pub(crate) fn review_of(id: &str, cx: &App) -> Option<Review> {
    Review::of(&cx.try_global::<Tasks>()?.driven(id)?.run)
}

/// Where the run on session `uid` stands, if one drives it.
pub(crate) fn shown(uid: u64, cx: &App) -> Option<Shown> {
    let driven = cx.try_global::<Tasks>()?.live.get(&uid)?;
    let run = &driven.run;
    Some(Shown {
        task: driven.task.clone().into(),
        name: run.template.name.clone().into(),
        steps: run
            .template
            .steps
            .iter()
            .map(|step| step.label.clone().into())
            .collect(),
        at: run.step,
        review: Review::of(run),
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

/// Task `id` as it is kept.
pub(crate) fn task(id: &str, cx: &App) -> Option<Task> {
    cx.try_global::<Tasks>()?.task(id).cloned()
}

/// A person pressed Stop on task `id`, whatever it is doing.
pub(crate) fn stop_task(id: &str, cx: &mut App) {
    let Some(t) = cx.try_global::<Tasks>() else {
        return;
    };
    if t.queue.queued(id) {
        return stop_queued(id, cx);
    }
    if let Some(uid) = t.live_uid(id) {
        return stop(uid, cx);
    }
    if let Some(cancel) = t.checks.get(id) {
        cancel.store(true, Ordering::SeqCst);
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
    let Action::RunCommand(command) = first else {
        // Nothing to run: the run ended before it began.
        cx.update_global::<Tasks, _>(|t, _| t.store_run(&id, run));
        return freed(id, from, cx);
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let refs = marks::refs_from(&id, &run, from);
    // Counted from before the pin: a quit meanwhile waits for the command
    // too, which never starts once called off.
    let running = cx.update_global::<Tasks, _>(|t, _| {
        t.checks.insert(id.clone(), cancel.clone());
        t.store_run(&id, run.clone());
        t.command_started()
    });
    cx.refresh_windows();
    let dir = run.setup.dir.clone();
    cx.spawn(async move |cx| {
        let pinned = {
            let (dir, refs) = (dir.clone(), refs.clone());
            cx.background_executor()
                .spawn(async move { marks::pin_blocking(&dir, &refs) })
                .await
        };
        let ran = {
            let cancel = cancel.clone();
            cx.background_executor()
                .spawn(async move {
                    let ran = run_command_blocking(&dir, &command, &cancel);
                    drop(running);
                    ran
                })
                .await
        };
        cx.update(|cx| {
            match &pinned {
                Ok(commit) => run.pinned(commit, from),
                Err(why) => eprintln!("onehand: a check's mark was not pinned: {why}"),
            }
            cx.update_global::<Tasks, _>(|t, _| t.checks.remove(&id));
            let said = match cancel.load(Ordering::SeqCst) {
                true => {
                    run.stopped(Stop::ByPerson);
                    None
                }
                false => {
                    let passed = ran.passed;
                    run.command_finished(ran);
                    Some(passed)
                }
            };
            let project = run.setup.repo.file_name().map_or_else(
                || run.setup.repo.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            cx.update_global::<Tasks, _>(|t, _| t.store_run(&id, run));
            if let Some(passed) = said {
                let note = match passed {
                    true => Notification::success(format!("Check passed in {project}")),
                    false => Notification::warning(format!("Check failed in {project}")),
                };
                let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
            }
            freed(id, from + refs.len(), cx);
        });
    })
    .detach();
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
