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
use onehand_core::task::{Task, files, marks, queue};
use onehand_core::workflow::Run;
use std::collections::HashMap;
use std::path::Path;

mod driver;
pub(crate) use driver::{approve, revise, start, stop};

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
    writer: files::Writer,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            live: HashMap::new(),
            queue: queue::Queue::default(),
            asked_in: HashMap::new(),
            draining: HashMap::new(),
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

    /// Whether task `id` is running or waiting for its place.
    fn busy(&self, id: &str) -> bool {
        self.live.values().any(|d| d.task == id) || self.queue.queued(id) || self.queue.holds(id)
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
    let writer = cx.global::<Tasks>().writer.clone();
    cx.on_app_quit(move |_| {
        let writer = writer.clone();
        async move {
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
    let Some(dir) = cx
        .try_global::<Tasks>()
        .and_then(|t| t.task(&id))
        .map(|task| task.setup.dir.clone())
    else {
        return;
    };
    cx.spawn_in(window, async move |shell, cx| {
        let place = cx
            .background_executor()
            .spawn(async move { queue::place_blocking(&dir) })
            .await;
        let _ = shell.update_in(cx, |shell: &mut Shell, window, cx| {
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
/// then give its place to whoever waits for it.
fn freed(id: String, from: usize, cx: &mut App) {
    let unpinned = cx.try_global::<Tasks>().and_then(|t| {
        let task = t.task(&id)?;
        let run = task.runs.last()?;
        let refs = marks::refs_from(&task.id, run, from);
        (!refs.is_empty()).then(|| (run.setup.dir.clone(), refs))
    });
    let Some((dir, refs)) = unpinned else {
        return release(id, cx);
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
            release(id, cx);
        });
    })
    .detach();
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
    cx.update_global::<Tasks, _>(|t, _| {
        if !t.queue.call_off(id) {
            return;
        }
        t.asked_in.remove(id);
        let Some(task) = t.task_mut(id) else {
            return;
        };
        if task.runs.last().is_some_and(|run| !run.begun()) {
            task.runs.pop();
        }
        t.save(id);
    });
    cx.refresh_windows();
}

/// A person let interrupted task `id` go: it is kept as history and never
/// offered again.
pub(crate) fn dismiss(id: &str, cx: &mut App) {
    cx.update_global::<Tasks, _>(|t, _| {
        if t.busy(id) {
            return;
        }
        if let Some(task) = t.task_mut(id) {
            task.dismissed = true;
        }
        t.save(id);
    });
    cx.refresh_windows();
}

/// Where the run on session `uid` stands, as its strip draws it.
pub(crate) struct Shown {
    pub(crate) name: SharedString,
    pub(crate) steps: Vec<SharedString>,
    pub(crate) at: usize,
    /// What it waits on *Continue* or *Revise…* for: the label of the step
    /// that answered, and its answer.
    pub(crate) review: Option<(SharedString, SharedString)>,
}

/// Where the run on session `uid` stands, if one drives it.
pub(crate) fn shown(uid: u64, cx: &App) -> Option<Shown> {
    let run = &cx.try_global::<Tasks>()?.live.get(&uid)?.run;
    Some(Shown {
        name: run.template.name.clone().into(),
        steps: run
            .template
            .steps
            .iter()
            .map(|step| step.label.clone().into())
            .collect(),
        at: run.step,
        review: run
            .under_review()
            .map(|(step, answer)| (step.label.clone().into(), answer.to_string().into())),
    })
}

/// A task a project page lists: one interrupted, or one queued.
#[derive(Clone, PartialEq)]
pub(crate) struct Listed {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) step: String,
    pub(crate) queued: bool,
    /// Its run took a step before: waiting, it waits to be resumed, and
    /// calling that off leaves it as it was.
    pub(crate) begun: bool,
}

/// The tasks started from, or working in, the project at `root` that wait
/// on a person or on their place, in the order they were started. One that
/// ended but still holds its place, its session's turn not yet over, is
/// left out until it lets go: nothing can be done with it before then.
pub(crate) fn listed_in(root: &Path, cx: &App) -> Vec<Listed> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    t.tasks
        .iter()
        .filter(|task| task.setup.repo == root || task.setup.dir == root)
        .filter(|task| task.resumable() && (t.queue.queued(&task.id) || !t.busy(&task.id)))
        .map(|task| {
            let run = task.runs.last();
            Listed {
                id: task.id.clone(),
                name: run.map_or_else(String::new, |run| run.template.name.clone()),
                title: task.brief.title.clone(),
                step: run
                    .and_then(Run::current)
                    .map_or_else(String::new, |step| step.label.clone()),
                queued: t.queue.queued(&task.id),
                begun: run.is_some_and(Run::begun),
            }
        })
        .collect()
}
