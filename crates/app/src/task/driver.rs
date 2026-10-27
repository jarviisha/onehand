//! The one driver of every run, as the app carries it out: what the run's
//! session says goes to its [`Drive`], which decides, and what the drive
//! says to do is done here — the marks pinned, the prompt sent, the command
//! run, the forge called, the clock armed — and what came of it goes back.
//!
//! Every rule is the drive's (`onehand_core::task::drive`); nothing here
//! decides what a run does next. A run is found by its task; a check's run
//! has no session at all.

use super::Tasks;
use crate::chat::session::{ChatEvent, ChatSession, note};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use gpui_component::WindowExt as _;
use onehand_core::task::drive::{Drive, Effect, Input};
use onehand_core::task::{Approval, Source, marks};
use onehand_core::workflow::{Action, Facts, Mark, Outcome, Run, run_command_blocking};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod forge;

/// A run under way, on a session or, for a check, on none.
pub(super) struct Driven {
    drive: Drive,
    /// The session the run is driven on, and its uid; neither for a check.
    session: Option<(u64, WeakEntity<ChatSession>)>,
    /// The window the session is in, or the check was asked from.
    pub(super) window: AnyWindowHandle,
    /// Set to call off the step's command or forge call, while one is running.
    pub(super) command: Option<Arc<AtomicBool>>,
    _subscriptions: Vec<Subscription>,
    _clock: Option<Task<()>>,
    /// Looking at the pull request's status checks, while the run waits on
    /// them.
    _status_checks: Option<Task<()>>,
}

impl Driven {
    /// The task the run is of.
    pub(super) fn task(&self) -> &str {
        self.drive.task()
    }

    pub(super) fn run(&self) -> &Run {
        self.drive.run()
    }

    /// The uid of the session the run is driven on.
    pub(super) fn uid(&self) -> Option<u64> {
        self.session.as_ref().map(|(uid, _)| *uid)
    }

    /// The run waits on a person: an approval, or a card the agent parked.
    pub(super) fn waits_on_person(&self) -> bool {
        self.drive.waits_on_person()
    }

    /// Stop was pressed and waits on a command or forge call to land.
    pub(super) fn stopping(&self) -> bool {
        self.drive.stopping()
    }

    fn session(&self) -> Option<Entity<ChatSession>> {
        self.session.as_ref()?.1.upgrade()
    }
}

/// Drive `run`, the last run of `task`, on session `uid`, starting with
/// `first`: what [`Run::resume`] said to do. `pinned` boundaries of it had
/// their mark pinned before.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    uid: u64,
    session: &Entity<ChatSession>,
    window: AnyWindowHandle,
    task: String,
    run: Run,
    first: Action,
    pinned: usize,
    cx: &mut App,
) {
    let id = task.clone();
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        let input = match event {
            // Most of what a session says is its reply streaming in. Asked
            // first, read-only, so a chunk that cannot move the run does not
            // tell every observer of the tasks that they changed.
            ChatEvent::Appended => {
                let chat = &session.read(cx).chat;
                if !read(&id, cx, |d| d.drive.heeds_appended(chat)).unwrap_or(false) {
                    return;
                }
                Input::Appended
            }
            ChatEvent::TurnEnded => Input::TurnEnded,
            ChatEvent::AwaitingUser(_) => Input::AwaitingUser,
            ChatEvent::Disconnected => Input::Disconnected,
            ChatEvent::PickRefused(why) => Input::PickRefused(why.clone()),
            // Between-turn work and a refused stop move no run.
            ChatEvent::OpenFile(_) | ChatEvent::CycleEnded | ChatEvent::StopRefused(_) => return,
        };
        feed(&id, input, cx);
    });
    // Fires on every notify the session makes, so it reads before it borrows:
    // only a parked card has anything to learn from one.
    let id = task.clone();
    let answered = cx.observe(session, move |_, cx| {
        if read(&id, cx, |d| d.drive.card()).unwrap_or(false) {
            feed(&id, Input::Notified, cx);
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended.
    let id = task.clone();
    let release = cx.observe_release(session, move |_, cx| {
        let id = id.clone();
        cx.defer(move |cx| feed(&id, Input::SessionGone, cx));
    });
    let issue = super::task(&task, cx).is_some_and(|task| task.issue().is_some());
    let now = Instant::now();
    let (drive, effects) = session.update(cx, |session, _| {
        Drive::start(
            task.clone(),
            run,
            first,
            pinned,
            issue,
            now,
            Some(&mut session.chat),
        )
    });
    let driven = Driven {
        drive,
        session: Some((uid, session.downgrade())),
        window,
        command: None,
        _subscriptions: vec![watch, answered, release],
        _clock: None,
        _status_checks: None,
    };
    cx.default_global::<Tasks>()
        .live
        .insert(task.clone(), driven);
    perform(&task, effects, cx);
}

/// Drive the check `run`, the last run of task `task`, with no session,
/// starting with `first`; what it found is said in `window`.
pub(crate) fn start_check(
    task: String,
    run: Run,
    first: Action,
    pinned: usize,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    let (drive, effects) = Drive::start(
        task.clone(),
        run,
        first,
        pinned,
        false,
        Instant::now(),
        None,
    );
    let driven = Driven {
        drive,
        session: None,
        window,
        command: None,
        _subscriptions: Vec::new(),
        _clock: None,
        _status_checks: None,
    };
    cx.default_global::<Tasks>()
        .live
        .insert(task.clone(), driven);
    perform(&task, effects, cx);
    // Busy from the press, not from when its first mark lands.
    cx.refresh_windows();
}

/// A person approved what they read, as `approval` names it.
pub(crate) fn approve(approval: Approval, cx: &mut App) {
    feed(&approval.task, Input::Approved { at: approval.at }, cx);
}

/// A person sent what they read, as `approval` names it, back with `note`.
pub(crate) fn revise(approval: Approval, note: String, cx: &mut App) {
    let input = Input::Revised {
        at: approval.at,
        note,
    };
    feed(&approval.task, input, cx);
}

/// A person pressed Stop on the run driven on session `uid`: the turn is
/// cancelled and the run ends.
pub(crate) fn stop(uid: u64, cx: &mut App) {
    if let Some(task) = cx.try_global::<Tasks>().and_then(|t| t.task_on(uid)) {
        feed(&task, Input::Stop, cx);
    }
}

/// A person pressed Stop on task `task`'s run, a check's included.
pub(super) fn stop_task(task: &str, cx: &mut App) {
    feed(task, Input::Stop, cx);
}

/// Read task `id`'s run. Apart from [`with`] because every event the session
/// sends asks, and a mutable borrow would tell every observer of the global
/// that it changed.
pub(super) fn read<R>(id: &str, cx: &App, look: impl FnOnce(&Driven) -> R) -> Option<R> {
    cx.try_global::<Tasks>()?.live.get(id).map(look)
}

pub(super) fn with<R>(id: &str, cx: &mut App, act: impl FnOnce(&mut Driven) -> R) -> Option<R> {
    if !cx.has_global::<Tasks>() {
        return None;
    }
    cx.update_global::<Tasks, _>(|t, _| t.live.get_mut(id).map(act))
}

/// Tell task `id`'s run what happened, with its session as it stands, and do
/// what it says.
pub(super) fn feed(id: &str, input: Input, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    let now = Instant::now();
    let effects = cx.update_global::<Tasks, _>(|t, cx| {
        let d = t.live.get_mut(id)?;
        Some(match d.session() {
            Some(session) => session.update(cx, |session, _| {
                d.drive.on(input, Some(&mut session.chat), now)
            }),
            None => d.drive.on(input, None, now),
        })
    });
    if let Some(effects) = effects {
        perform(id, effects, cx);
    }
}

/// Do what task `id`'s run said, in order.
fn perform(id: &str, effects: Vec<Effect>, cx: &mut App) {
    for effect in effects {
        let session = read(id, cx, Driven::session).flatten();
        match effect {
            Effect::Pin { dir, refs } => spawn_feed(id, cx, move || {
                Input::Pinned(marks::pin_blocking(&dir, &refs))
            }),
            Effect::Measure { dir } => {
                spawn_feed(id, cx, move || Input::Measured(Mark::read_blocking(&dir)))
            }
            Effect::ReadFacts { dir, from } => spawn_feed(id, cx, move || {
                Input::TurnRead(Facts::read_blocking(&dir, &from))
            }),
            Effect::Submit(text) => {
                let taken = session
                    .is_some_and(|s| s.update(cx, |session, cx| session.submit(&text, &[], cx)));
                if !taken {
                    feed(id, Input::NotTaken, cx);
                }
            }
            Effect::CancelTurn => {
                if let Some(session) = session {
                    session.update(cx, |session, cx| {
                        if session.chat.busy {
                            session.chat.cancel_turn();
                            cx.notify();
                        }
                    });
                }
            }
            Effect::Note(said) => {
                if let Some(session) = session {
                    note(&session, said, cx);
                }
            }
            Effect::RunCommand { dir, command } => run_command(id, dir, command, cx),
            Effect::CallOffCommand => {
                if let Some(cancel) = read(id, cx, |d| d.command.clone()).flatten() {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Effect::Forge { commit } => forge::on_forge(id, commit, cx),
            Effect::WatchChecks { pushed, wait } => {
                let watching = forge::watch_status_checks(id, pushed, Instant::now(), wait, cx);
                with(id, cx, |d| d._status_checks = Some(watching));
            }
            Effect::ArmClock(left) => {
                let task = id.to_string();
                let clock = cx.spawn(async move |cx| {
                    cx.background_executor().timer(left).await;
                    cx.update(|cx| feed(&task, Input::ClockFired, cx));
                });
                with(id, cx, |d| d._clock = Some(clock));
            }
            Effect::Defer(input) => {
                let task = id.to_string();
                cx.defer(move |cx| feed(&task, input, cx));
            }
            Effect::Save => save(id, cx),
            Effect::Waiting => crate::unattended::waiting(id, cx),
            Effect::Answered => {
                let task = id.to_string();
                cx.defer(move |cx| crate::unattended::answered(&task, cx));
            }
            Effect::CameUp => {
                if let (Some(task), Some(session)) = (super::task(id, cx), session) {
                    crate::unattended::started(&task, &session, cx);
                }
            }
            Effect::Pause { why, fix } => crate::unattended::pause(&why, &fix, cx),
            Effect::Refresh => cx.refresh_windows(),
            Effect::SessionChanged => {
                if let Some(session) = session {
                    session.update(cx, |_, cx| cx.notify());
                }
            }
            Effect::Finish { .. } => finish(id, session.as_ref(), cx),
        }
    }
}

/// Do `work` off the UI thread, then tell task `id`'s run what came of it.
fn spawn_feed(id: &str, cx: &mut App, work: impl FnOnce() -> Input + Send + 'static) {
    let task = id.to_string();
    cx.spawn(async move |cx| {
        let input = cx.background_executor().spawn(async move { work() }).await;
        cx.update(|cx| feed(&task, input, cx));
    })
    .detach();
}

/// Write the run into its task's file, in order with every other write.
fn save(id: &str, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    cx.update_global::<Tasks, _>(|t, _| {
        let Some(run) = t.live.get(id).map(|d| d.run().clone()) else {
            return;
        };
        t.store_run(id, run);
    });
}

/// Run the step's command in the work, then report how it went with the
/// commit it ran on. Its flag calls it off; a quit waits for it.
fn run_command(id: &str, dir: std::path::PathBuf, command: String, cx: &mut App) {
    let cancel = Arc::new(AtomicBool::new(false));
    if with(id, cx, |d| d.command = Some(cancel.clone())).is_none() {
        return;
    }
    let running = cx.global::<Tasks>().command_started();
    let task = id.to_string();
    cx.spawn(async move |cx| {
        let ran = cx
            .background_executor()
            .spawn(async move {
                let ran = run_command_blocking(&dir, &command, &cancel);
                drop(running);
                ran
            })
            .await;
        cx.update(|cx| {
            with(&task, cx, |d| d.command = None);
            feed(&task, Input::CommandExited(ran), cx);
        });
    })
    .detach();
}

/// End the run: keep it in its task, then let its place go once its
/// session's turn is over.
///
/// Taken off the list at once, so an event the ending causes finds nothing
/// to end again; dropped only once the event delivering it is over, since
/// dropping the run drops the subscription it came through.
fn finish(id: &str, session: Option<&Entity<ChatSession>>, cx: &mut App) {
    let driven = cx
        .has_global::<Tasks>()
        .then(|| cx.update_global::<Tasks, _>(|t, _| t.live.remove(id)))
        .flatten();
    let Some(driven) = driven else {
        return;
    };
    let run = driven.run().clone();
    let task = id.to_string();
    let asked = session
        .filter(|_| driven.drive.card())
        .and_then(|session| crate::unattended::card_question(session, cx));
    let check = super::task(&task, cx).is_some_and(|task| task.source == Source::Check);
    if check {
        say_check(&run, driven.window, cx);
    }
    crate::unattended::keep(&task, &run, asked, cx);
    cx.update_global::<Tasks, _>(|t, _| t.store_run(&task, run));
    // A finished schedule's session leaves the rail once the run is let go,
    // so nothing of the run is left listening to it.
    let closes = driven
        .uid()
        .filter(|_| super::task(&task, cx).is_some_and(|task| task.closes_session()));
    super::ended(task, driven.drive.pinned(), session, cx);
    cx.defer(move |cx| {
        let window = driven.window;
        drop(driven);
        if let Some(uid) = closes {
            crate::state::with_shell(window, cx, |shell, _, cx| shell.close_finished(uid, cx));
        }
        cx.refresh_windows();
    });
}

/// Say in `window` how a check came out. One a person stopped says nothing:
/// they know.
fn say_check(run: &Run, window: AnyWindowHandle, cx: &mut App) {
    let project = run.setup.repo.file_name().map_or_else(
        || run.setup.repo.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let note = match run.outcome {
        Some(Outcome::Stopped(_)) | None => return,
        Some(Outcome::Done) => crate::toast::success(format!("Check passed in {project}")),
        Some(Outcome::Exhausted { .. } | Outcome::Failed(_)) => {
            crate::toast::warning(format!("Check failed in {project}"))
        }
    };
    let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
}
