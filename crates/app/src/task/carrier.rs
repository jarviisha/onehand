//! The run's carrier: what the run's session says, and what a person
//! presses, goes to its [`RunDrive`], which decides, and what the drive says
//! to do is done here — the marks pinned, the prompt sent, the command
//! run, the forge called, the clock armed — and what came of it goes back.
//!
//! Every rule is the drive's (`onehand_core::task::drive`); nothing here
//! decides what a run does next. A run is found by its task; a check's run
//! has no session at all.

use super::Tasks;
use crate::chat::session::{ChatEvent, ChatSession, note};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use gpui_component::WindowExt as _;
use onehand_core::chat::Chat;
use onehand_core::task::drive::{Effect, Input, RunDrive, Signal, Timer, link};
use onehand_core::task::tasks::{Start, link as tasks_link};
use onehand_core::task::{Approval, marks};
use onehand_core::workflow::{Facts, Mark, Run, run_command_blocking};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod forge;

/// A run under way, on a session or, for a check, on none.
pub(super) struct Driven {
    drive: RunDrive,
    /// The session the run is driven on, and its uid; neither for a check.
    session: Option<(u64, WeakEntity<ChatSession>)>,
    /// The window the session is in, or the check was asked from.
    pub(super) window: AnyWindowHandle,
    /// Set to call off the step's command, while one is running.
    pub(super) command: Option<Arc<AtomicBool>>,
    _subscriptions: Vec<Subscription>,
    /// The clock for the run's timeout, and the one for its next look at its
    /// status checks.
    _timeout: Option<Task<()>>,
    _checks: Option<Task<()>>,
}

impl Driven {
    /// The run's drive, to read.
    pub(super) fn drive(&self) -> &RunDrive {
        &self.drive
    }

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

/// Drive task `task`'s run on session `uid`, from `start`, as the tasks'
/// drive began it.
pub(crate) fn start(
    uid: u64,
    session: &Entity<ChatSession>,
    window: AnyWindowHandle,
    task: String,
    start: Start,
    cx: &mut App,
) {
    let Start {
        run,
        first,
        pinned,
        issue,
    } = start;
    let id = task.clone();
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        let input = match event {
            // Which signal moves the run, and which streamed chunk is worth
            // asking it about, is the link's.
            ChatEvent::Session(signal) => {
                let chat = &session.read(cx).chat;
                match read(&id, cx, |d| link::input_of(signal, &d.drive, chat)).flatten() {
                    Some(input) => input,
                    None => return,
                }
            }
            // A fork asked for, what a host says of its session and news for a
            // person move no run.
            ChatEvent::OpenFile(_)
            | ChatEvent::Fork(_)
            | ChatEvent::HostSession(_)
            | ChatEvent::HostBusy
            | ChatEvent::RestartHost(_)
            | ChatEvent::Look(_) => return,
        };
        feed(&id, input, cx);
    });
    // Fires on every notify the session makes, so it reads before it borrows:
    // only a parked card, or work a turn left running, has anything to learn
    // from one.
    let id = task.clone();
    let answered = cx.observe(session, move |session, cx| {
        let chat = &session.read(cx).chat;
        if let Some(input) = read(&id, cx, |d| link::notified(&d.drive, chat)).flatten() {
            feed(&id, input, cx);
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended.
    let id = task.clone();
    let release = cx.observe_release(session, move |_, cx| {
        let id = id.clone();
        cx.defer(move |cx| feed(&id, Input::SessionGone, cx));
    });
    let now = Instant::now();
    let (drive, effects) = session.update(cx, |session, cx| {
        let (drive, effects, asked) = link::start(
            task.clone(),
            run,
            first,
            pinned,
            issue,
            now,
            Some(&mut session.chat),
        );
        session.perform(asked, cx);
        // Named for the task: what shows it is drawn again.
        cx.notify();
        (drive, effects)
    });
    let driven = Driven {
        drive,
        session: Some((uid, session.downgrade())),
        window,
        command: None,
        _subscriptions: vec![watch, answered, release],
        _timeout: None,
        _checks: None,
    };
    cx.default_global::<Tasks>()
        .live
        .insert(task.clone(), driven);
    perform(&task, effects, cx);
}

/// Drive check task `task`'s run with no session, from `start`; what it
/// found is said in `window`.
pub(crate) fn start_check(task: String, start: Start, window: AnyWindowHandle, cx: &mut App) {
    let Start {
        run, first, pinned, ..
    } = start;
    let (drive, effects) = RunDrive::start(
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
        _timeout: None,
        _checks: None,
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
    let at = approval.at;
    give(&approval.task, cx, move |d, chat, now| {
        d.approve(at, chat, now)
    });
}

/// A person sent what they read, as `approval` names it, back with `note`.
pub(crate) fn revise(approval: Approval, note: String, cx: &mut App) {
    let at = approval.at;
    give(&approval.task, cx, move |d, chat, now| {
        d.revise(at, note, chat, now)
    });
}

/// A person pressed Stop on the run driven on session `uid`: the turn is
/// cancelled and the run ends.
pub(crate) fn stop(uid: u64, cx: &mut App) {
    if let Some(task) = cx.try_global::<Tasks>().and_then(|t| t.task_on(uid)) {
        stop_task(&task, cx);
    }
}

/// A person pressed Stop on task `task`'s run, a check's included.
pub(super) fn stop_task(task: &str, cx: &mut App) {
    give(task, cx, |d, chat, now| d.stop(chat, now));
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
    give(id, cx, move |d, chat, now| d.handle(input, chat, now));
}

/// Give task `id`'s run `command`, with its session as it stands, and do
/// what it says.
fn give(
    id: &str,
    cx: &mut App,
    command: impl FnOnce(&mut RunDrive, Option<&Chat>, Instant) -> Vec<Effect>,
) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    let now = Instant::now();
    let effects = cx.update_global::<Tasks, _>(|t, cx| {
        let d = t.live.get_mut(id)?;
        Some(match d.session() {
            Some(session) => session.update(cx, |session, cx| {
                let (effects, asked) =
                    link::command(&mut d.drive, Some(&mut session.chat), |drive, chat| {
                        command(drive, chat, now)
                    });
                session.perform(asked, cx);
                effects
            }),
            None => command(&mut d.drive, None, now),
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
                let answer = match session {
                    Some(session) => session.update(cx, |session, cx| {
                        let (answer, fx) =
                            link::submit(&mut session.chat, &text, crate::chat::session::now());
                        session.perform(fx, cx);
                        cx.notify();
                        answer
                    }),
                    None => Some(Input::NotTaken),
                };
                if let Some(input) = answer {
                    feed(id, input, cx);
                }
            }
            Effect::CancelTurn => {
                if let Some(session) = session {
                    session.update(cx, |session, cx| {
                        let fx = link::cancel(&mut session.chat);
                        session.perform(fx, cx);
                        cx.notify();
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
            Effect::Push { commit } => forge::push(id, commit, cx),
            Effect::FindPullRequest => forge::find(id, cx),
            Effect::OpenPullRequest => forge::open(id, cx),
            Effect::ReadLogs { checks } => forge::read_logs(id, checks, cx),
            Effect::MarkReady { number } => forge::mark_ready(id, number, cx),
            Effect::Arm { timer, after } => {
                let task = id.to_string();
                let clock = cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| feed(&task, Input::Elapsed(timer), cx));
                });
                // Armed again, the clock before it is dropped.
                with(id, cx, |d| match timer {
                    Timer::Timeout => d._timeout = Some(clock),
                    Timer::Checks => d._checks = Some(clock),
                });
            }
            Effect::Defer(input) => {
                let task = id.to_string();
                cx.defer(move |cx| feed(&task, input, cx));
            }
            Effect::Save => save(id, cx),
            Effect::Signal(Signal::SaveFailed(why)) => {
                if let Some(window) = read(id, cx, |d| d.window) {
                    let note = crate::toast::failed("Saving the task", why);
                    let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
                }
            }
            Effect::Signal(Signal::PinFailed(why)) => {
                eprintln!("onehand: a step's mark was not pinned: {why}")
            }
            Effect::Unattended(said) => crate::unattended::run_said(id, said, cx),
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

/// Write the run into its task's file, in order with every other write,
/// and tell the run how the write went.
fn save(id: &str, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    let written = cx.update_global::<Tasks, _>(|t, _| {
        let task = tasks_link::save_run(&mut t.drive, &t.live.get(id)?.drive)?;
        Some(t.writer.save_said(task))
    });
    let Some(written) = written else {
        return;
    };
    let task = id.to_string();
    cx.spawn(async move |cx| {
        let written = written.await;
        cx.update(|cx| feed(&task, Input::Saved(written), cx));
    })
    .detach();
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

/// End the run: keep it in its task, and let the tasks' drive decide when
/// its place goes.
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
    let out = cx.update_global::<Tasks, _>(|t, cx| {
        let on = session.zip(driven.uid());
        let chat = on.map(|(session, uid)| (&session.read(cx).chat as &Chat, uid));
        t.joined()
            .tasks(|d| {
                (
                    (),
                    tasks_link::finished(d, &driven.drive, chat, driven.window),
                )
            })
            .1
    });
    cx.defer(move |cx| {
        drop(driven);
        cx.refresh_windows();
    });
    super::carry(out, session, cx);
}
