//! Tasks, as the app carries them: the tasks' drive, which keeps every task
//! and decides where each works, and the runs under way, each its own drive.
//!
//! Everything a task or a run decides is `onehand_core`'s
//! ([`TasksDrive`], `RunDrive`); what is here is what needs a session, a
//! timer, a window or the disk.

use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Global, SharedString, Window};
use onehand_core::task::tasks::link::Seen;
use onehand_core::task::tasks::{Begin, Effect, Start, TasksDrive};
use onehand_core::task::work::UnderReview;
use onehand_core::task::{Task, Working, files};
use onehand_core::unattended::drive::UnattendedDrive;
use onehand_core::unattended::link::{Joined, Out};
use onehand_core::workflow::{Run, Template};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) mod approval;
mod carrier;
mod listing;
mod place;
pub(crate) use carrier::{approve, revise, start, stop};
pub(crate) use listing::*;
pub(crate) use onehand_core::task::tasks::Placed;
#[cfg(test)]
pub(crate) use place::submit_told;
pub(crate) use place::{not_started, request, start_off_screen, submit};

/// Every task in this process and what each is doing.
pub(crate) struct Tasks {
    /// Every task kept, the queue, and what ended runs still hold. Empty
    /// until the boot read lands.
    drive: TasksDrive<AnyWindowHandle>,
    /// Unattended runs, once they are set up: one per process, beside the
    /// tasks they start, since two would be two agents on one issue.
    unattended: Option<UnattendedDrive>,
    /// Runs under way, by their task's id; a check's has no session.
    live: HashMap<String, carrier::Driven>,
    /// What watches each run that ended while its session's turn still goes.
    draining: HashMap<String, place::Drain>,
    /// Whoever asked to be told where a task's request landed.
    told: HashMap<String, futures::channel::oneshot::Sender<Placed>>,
    /// What a person pressed on each task that is still being read or done,
    /// so the button waits wherever the task is drawn.
    acting: HashMap<String, onehand_core::task::work::Act>,
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
        Self::in_dir(files::dir())
    }
}

impl Tasks {
    /// No task yet, with their files kept in `dir`.
    pub(crate) fn in_dir(dir: PathBuf) -> Self {
        Self {
            drive: TasksDrive::default(),
            unattended: None,
            live: HashMap::new(),
            draining: HashMap::new(),
            told: HashMap::new(),
            acting: HashMap::new(),
            commands: Arc::default(),
            writer: files::Writer::spawn(dir),
        }
    }
}

impl Global for Tasks {}

impl Tasks {
    fn tasks(&self) -> &[Task] {
        self.drive.tasks()
    }

    fn task(&self, id: &str) -> Option<&Task> {
        self.drive.task(id)
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
        self.drive.busy(id)
    }

    /// The carrier running task `id`'s run, if one is.
    fn driven(&self, id: &str) -> Option<&carrier::Driven> {
        self.live.get(id)
    }

    /// The task whose run is driven on session `uid`, if one is.
    fn task_on(&self, uid: u64) -> Option<String> {
        self.live
            .iter()
            .find(|(_, d)| d.uid() == Some(uid))
            .map(|(id, _)| id.clone())
    }

    /// What task `id` is doing, if anything: its place's word, and a run
    /// under way waiting on a person, its drive's.
    fn working(&self, id: &str) -> Option<Working> {
        self.seen().working(id)
    }

    /// Every task and the runs under way, to read.
    fn seen(&self) -> Seen<'_, AnyWindowHandle> {
        Seen::new(&self.drive, self.live.values().map(carrier::Driven::drive))
    }

    /// The drives joined for one call.
    fn joined(&mut self) -> Joined<'_, AnyWindowHandle> {
        Joined::new(
            &mut self.drive,
            self.live.values().map(carrier::Driven::drive),
            self.unattended.as_mut(),
        )
    }
}

/// Carry out what the joined drives said: the tasks', with `session` the
/// session of the run that just ended if that is what this answers, then
/// unattended runs'.
fn carry(
    out: Out<AnyWindowHandle>,
    session: Option<&gpui::Entity<crate::chat::session::ChatSession>>,
    cx: &mut App,
) {
    place::perform(out.tasks, session, cx);
    crate::unattended::perform(out.unattended, cx);
}

/// Join the drives for `command`, and carry out what they say.
pub(crate) fn with_joined<R>(
    cx: &mut App,
    command: impl FnOnce(&mut Joined<'_, AnyWindowHandle>) -> (R, Out<AnyWindowHandle>),
) -> Option<R> {
    if !cx.has_global::<Tasks>() {
        return None;
    }
    let (answer, out) = cx.update_global::<Tasks, _>(|t, _| command(&mut t.joined()));
    carry(out, None, cx);
    Some(answer)
}

/// Give unattended runs `command`, reading the tasks, and carry out what it
/// says. `R::default()` before they are set up.
pub(crate) fn with_unattended<R: Default>(
    cx: &mut App,
    command: impl FnOnce(
        &mut UnattendedDrive,
        &Seen<'_, AnyWindowHandle>,
    ) -> (
        R,
        Vec<onehand_core::unattended::drive::Effect<AnyWindowHandle>>,
    ),
) -> R {
    with_joined(cx, |j| j.unattended(command)).unwrap_or_default()
}

/// Set unattended runs up with `drive`.
pub(crate) fn set_unattended(drive: UnattendedDrive, cx: &mut App) {
    cx.default_global::<Tasks>().unattended = Some(drive);
}

/// What `read` makes of unattended runs and the tasks, once they are set up.
pub(crate) fn unattended<R>(
    cx: &App,
    read: impl FnOnce(&UnattendedDrive, &Seen<'_, AnyWindowHandle>) -> R,
) -> Option<R> {
    let t = cx.try_global::<Tasks>()?;
    Some(read(t.unattended.as_ref()?, &t.seen()))
}

/// Give the tasks' drive `command`, and carry out what it says.
fn with_drive<R>(
    cx: &mut App,
    command: impl FnOnce(&mut TasksDrive<AnyWindowHandle>) -> (R, Vec<Effect<AnyWindowHandle>>),
) -> Option<R> {
    if !cx.has_global::<Tasks>() {
        return None;
    }
    with_joined(cx, |j| j.tasks(command))
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
            with_drive(cx, |d| ((), d.loaded(tasks)));
        });
    })
    .detach();
}

/// Keep `task`, new, and write it.
pub(crate) fn add(task: Task, cx: &mut App) {
    with_drive(cx, |d| ((), d.add(task)));
}

/// The last run of task `id`, to drive, while the task may be picked up:
/// not dismissed, and its run not ended on its own outcome.
pub(crate) fn resumable_run(id: &str, cx: &App) -> Option<Run> {
    cx.try_global::<Tasks>()?.drive.resumable_run(id)
}

/// A person let interrupted task `id` go: it is kept as history and never
/// offered again.
pub(crate) fn dismiss(id: &str, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    with_drive(cx, |d| ((), d.dismiss(id, handle)));
}

/// Task `id`'s worktree and branch were removed: kept with the task, so
/// nothing offers to remove them again.
pub(crate) fn worktree_removed(id: &str, cx: &mut App) {
    with_drive(cx, |d| ((), d.worktree_removed(id)));
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
        // A prompt's one step is the run itself, already named.
        steps: match run.template.is_prompt() {
            true => Vec::new(),
            false => run
                .template
                .steps
                .iter()
                .map(|step| step.label.clone().into())
                .collect(),
        },
        at: run.step,
        review: UnderReview::of(run),
        stopping: driven.stopping(),
    })
}

/// Whether the run on session `uid` waits on a person: an approval, which
/// the session's own chat cannot tell, its agent's turn being over, or a card.
pub(crate) fn waits_on_person(uid: u64, cx: &App) -> bool {
    let Some(t) = cx.try_global::<Tasks>() else {
        return false;
    };
    t.task_on(uid)
        .and_then(|id| t.driven(&id))
        .is_some_and(|d| d.waits_on_person())
}

/// What `look` makes of each task, told whether it is working, oldest first.
pub(crate) fn each<R>(cx: &App, look: impl Fn(&Task, bool) -> Option<R>) -> Vec<R> {
    let Some(t) = cx.try_global::<Tasks>() else {
        return Vec::new();
    };
    t.tasks()
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
// ponytail: asked per drawn Resume per frame, and an issue task's facts walk
// every task for its slots, so the cost is drawn Resumes times tasks, held
// down by the cap on rows drawn; keep the answers beside the issue works if a
// long Tasks page ever stutters.
pub(crate) fn resume_refused(task: &Task, cx: &App) -> Option<String> {
    let template = task.runs.last()?.template.clone();
    let facts = crate::unattended::task_facts(task, template, cx);
    onehand_core::preflight::refusal(onehand_core::preflight::Kind::Resume, &facts)
}

/// The title of the task holding `place`, unless that task is `except`: a
/// task's own next run never waits behind itself.
pub(crate) fn holder_at(place: &std::path::Path, except: Option<&str>, cx: &App) -> Option<String> {
    let t = cx.try_global::<Tasks>()?;
    let holder = t
        .drive
        .queue()
        .holder_at(place)
        .filter(|holder| Some(*holder) != except)?;
    t.task(holder).map(|task| task.brief.title.clone())
}

/// A person pressed Stop on task `id`, whatever it is doing: a queued one is
/// called off, and a run under way is stopped.
pub(crate) fn stop_task(id: &str, cx: &mut App) {
    if with_drive(cx, |d| d.stop_queued(id)) == Some(false) {
        carrier::stop_task(id, cx);
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
    let run = onehand_core::task::new_id();
    with_drive(cx, |d| d.retry(id, run, template, from, note)).unwrap_or(false)
}

/// Retry task `id` at once when it is a check, which has one step and keeps
/// nothing: whether it was retried, or `None` for a task a person retries
/// through what the retry would keep.
pub(crate) fn retry_at_once(id: &str, cx: &mut App) -> Option<bool> {
    let run = onehand_core::task::new_id();
    with_drive(cx, |d| match d.retry_at_once(id, run) {
        Some((made, fx)) => (Some(made), fx),
        None => (None, Vec::new()),
    })
    .flatten()
}

/// Give task `id` a new run with what Settings say now, as `plan` worked it
/// out, kept but not started: the caller asks for its place. Whether there
/// was one to give.
pub(crate) fn retry_with(
    id: &str,
    plan: onehand_core::workflow::WithCurrent,
    cx: &mut App,
) -> bool {
    let run = onehand_core::task::new_id();
    with_drive(cx, |d| d.retry_with(id, run, plan)).unwrap_or(false)
}

/// Begin task `id`, whose place it now holds, its work's folder being there
/// or not: how it is driven, or why it cannot be.
pub(crate) fn begin(id: &str, folder_there: bool, cx: &mut App) -> Begin {
    match cx.has_global::<Tasks>() {
        true => cx.update_global::<Tasks, _>(|t, _| t.drive.begin(id, folder_there)),
        false => Begin::Refused {
            why: "That task has nothing left to run".to_string(),
            kind: onehand_core::workflow::Failure::Other,
        },
    }
}

/// Task `id`'s session did not start, its agent gone from Settings or not.
pub(crate) fn session_not_started(id: String, agent_gone: bool, cx: &mut App) {
    with_drive(cx, |d| ((), d.session_not_started(&id, agent_gone)));
}

/// Run check task `id` from `start`, whose place it now holds: pin the work,
/// run the command, and say in `window` how it went. No session: there is no
/// agent to watch.
pub(crate) fn start_check(id: String, start: Start, window: AnyWindowHandle, cx: &mut App) {
    carrier::start_check(id, start, window, cx);
}

#[cfg(test)]
mod tests;
