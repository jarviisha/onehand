//! The one driver of every run, as the app carries it out: what the run's
//! session says goes to its [`Drive`], which decides, and what the drive
//! says to do is done here — the marks pinned, the prompt sent, the command
//! run, the forge called, the clock armed — and what came of it goes back.
//!
//! Every rule is the drive's (`onehand_core::task::drive`); nothing here
//! decides what a run does next.

use super::Tasks;
use crate::chat::session::{ChatEvent, ChatSession, note};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use onehand_core::task::Approval;
use onehand_core::task::drive::{Drive, Effect, Input};
use onehand_core::task::marks;
use onehand_core::workflow::{Action, Facts, Mark, Run, run_command_blocking};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod forge;

/// A run under way on one session.
pub(super) struct Driven {
    drive: Drive,
    session: WeakEntity<ChatSession>,
    /// The window the session is in.
    pub(super) window: AnyWindowHandle,
    /// Set to call off the step's command or forge call, while one is running.
    pub(super) command: Option<Arc<AtomicBool>>,
    _watch: Subscription,
    _answered: Subscription,
    _release: Subscription,
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

    /// The run waits on a person: an approval, or a card the agent parked.
    pub(super) fn waits_on_person(&self) -> bool {
        self.drive.waits_on_person()
    }

    /// Stop was pressed and waits on a command or forge call to land.
    pub(super) fn stopping(&self) -> bool {
        self.drive.stopping()
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
    let watch = cx.subscribe(session, move |_, event: &ChatEvent, cx| {
        let input = match event {
            ChatEvent::Appended => Input::Appended,
            ChatEvent::TurnEnded => Input::TurnEnded,
            ChatEvent::AwaitingUser(_) => Input::AwaitingUser,
            ChatEvent::Disconnected => Input::Disconnected,
            ChatEvent::PickRefused(why) => Input::PickRefused(why.clone()),
            ChatEvent::OpenFile(_) => return,
        };
        feed(uid, input, cx);
    });
    // Fires on every notify the session makes, so it reads before it borrows:
    // only a parked card has anything to learn from one.
    let answered = cx.observe(session, move |_, cx| {
        if read(uid, cx, |d| d.drive.card()).unwrap_or(false) {
            feed(uid, Input::Notified, cx);
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended.
    let release = cx.observe_release(session, move |_, cx| {
        cx.defer(move |cx| feed(uid, Input::SessionGone, cx));
    });
    let issue = super::task(&task, cx).is_some_and(|task| task.issue().is_some());
    let now = Instant::now();
    let (drive, effects) = session.update(cx, |session, _| {
        Drive::start(
            task,
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
        session: session.downgrade(),
        window,
        command: None,
        _watch: watch,
        _answered: answered,
        _release: release,
        _clock: None,
        _status_checks: None,
    };
    cx.default_global::<Tasks>().live.insert(uid, driven);
    perform(uid, effects, cx);
}

/// A person approved what they read, as `approval` names it.
pub(crate) fn approve(approval: Approval, cx: &mut App) {
    if let Some(uid) = live_uid(&approval.task, cx) {
        feed(uid, Input::Approved { at: approval.at }, cx);
    }
}

/// A person sent what they read, as `approval` names it, back with `note`.
pub(crate) fn revise(approval: Approval, note: String, cx: &mut App) {
    if let Some(uid) = live_uid(&approval.task, cx) {
        feed(
            uid,
            Input::Revised {
                at: approval.at,
                note,
            },
            cx,
        );
    }
}

/// A person pressed Stop: the turn is cancelled and the run ends.
pub(crate) fn stop(uid: u64, cx: &mut App) {
    feed(uid, Input::Stop, cx);
}

/// The session task `task`'s run is driven on: a press is routed by the
/// task, so a window other than the session's can make it.
fn live_uid(task: &str, cx: &App) -> Option<u64> {
    cx.try_global::<Tasks>()?.live_uid(task)
}

/// Read the run on session `uid`. Apart from [`with`] because every event the
/// session sends asks, and a mutable borrow would tell every observer of the
/// global that it changed.
pub(super) fn read<R>(uid: u64, cx: &App, look: impl FnOnce(&Driven) -> R) -> Option<R> {
    cx.try_global::<Tasks>()?.live.get(&uid).map(look)
}

pub(super) fn with<R>(uid: u64, cx: &mut App, act: impl FnOnce(&mut Driven) -> R) -> Option<R> {
    if !cx.has_global::<Tasks>() {
        return None;
    }
    cx.update_global::<Tasks, _>(|t, _| t.live.get_mut(&uid).map(act))
}

/// Tell the run on session `uid` what happened, with its session as it
/// stands, and do what it says.
pub(super) fn feed(uid: u64, input: Input, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    let now = Instant::now();
    let effects = cx.update_global::<Tasks, _>(|t, cx| {
        let d = t.live.get_mut(&uid)?;
        Some(match d.session.upgrade() {
            Some(session) => session.update(cx, |session, _| {
                d.drive.on(input, Some(&mut session.chat), now)
            }),
            None => d.drive.on(input, None, now),
        })
    });
    if let Some(effects) = effects {
        perform(uid, effects, cx);
    }
}

/// Do what the run on session `uid` said, in order.
fn perform(uid: u64, effects: Vec<Effect>, cx: &mut App) {
    for effect in effects {
        let session = read(uid, cx, |d| d.session.upgrade()).flatten();
        let task = read(uid, cx, |d| d.task().to_string());
        match effect {
            Effect::Pin { dir, refs } => spawn_feed(uid, cx, move || {
                Input::Pinned(marks::pin_blocking(&dir, &refs))
            }),
            Effect::Measure { dir } => {
                spawn_feed(uid, cx, move || Input::Measured(Mark::read_blocking(&dir)))
            }
            Effect::ReadFacts { dir, from } => spawn_feed(uid, cx, move || {
                Input::TurnRead(Facts::read_blocking(&dir, &from))
            }),
            Effect::Submit(text) => {
                let taken = session
                    .is_some_and(|s| s.update(cx, |session, cx| session.submit(&text, &[], cx)));
                if !taken {
                    feed(uid, Input::NotTaken, cx);
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
            Effect::RunCommand { dir, command } => run_command(uid, dir, command, cx),
            Effect::CallOffCommand => {
                if let Some(cancel) = read(uid, cx, |d| d.command.clone()).flatten() {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Effect::Forge { commit } => forge::on_forge(uid, commit, cx),
            Effect::WatchChecks { pushed, wait } => {
                let watching = forge::watch_status_checks(uid, pushed, Instant::now(), wait, cx);
                with(uid, cx, |d| d._status_checks = Some(watching));
            }
            Effect::ArmClock(left) => {
                let clock = cx.spawn(async move |cx| {
                    cx.background_executor().timer(left).await;
                    cx.update(|cx| feed(uid, Input::ClockFired, cx));
                });
                with(uid, cx, |d| d._clock = Some(clock));
            }
            Effect::Defer(input) => cx.defer(move |cx| feed(uid, input, cx)),
            Effect::Save => save(uid, cx),
            Effect::Waiting => {
                if let Some(task) = task {
                    crate::unattended::waiting(&task, cx);
                }
            }
            Effect::Answered => {
                if let Some(task) = task {
                    cx.defer(move |cx| crate::unattended::answered(&task, cx));
                }
            }
            Effect::CameUp => {
                let found = task.and_then(|task| super::task(&task, cx));
                if let (Some(task), Some(session)) = (found, session) {
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
            Effect::Finish { .. } => finish(uid, session.as_ref(), cx),
        }
    }
}

/// Do `work` off the UI thread, then tell the run on session `uid` what came
/// of it.
fn spawn_feed(uid: u64, cx: &mut App, work: impl FnOnce() -> Input + Send + 'static) {
    cx.spawn(async move |cx| {
        let input = cx.background_executor().spawn(async move { work() }).await;
        cx.update(|cx| feed(uid, input, cx));
    })
    .detach();
}

/// Write the run into its task's file, in order with every other write.
fn save(uid: u64, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    cx.update_global::<Tasks, _>(|t, _| {
        let Some(d) = t.live.get(&uid) else {
            return;
        };
        let (task, run) = (d.task().to_string(), d.run().clone());
        t.store_run(&task, run);
    });
}

/// Run the step's command in the work, then report how it went with the
/// commit it ran on. Its flag calls it off; a quit waits for it.
fn run_command(uid: u64, dir: std::path::PathBuf, command: String, cx: &mut App) {
    let cancel = Arc::new(AtomicBool::new(false));
    if with(uid, cx, |d| d.command = Some(cancel.clone())).is_none() {
        return;
    }
    let running = cx.global::<Tasks>().command_started();
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
            with(uid, cx, |d| d.command = None);
            feed(uid, Input::CommandExited(ran), cx);
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
fn finish(uid: u64, session: Option<&Entity<ChatSession>>, cx: &mut App) {
    let driven = cx
        .has_global::<Tasks>()
        .then(|| cx.update_global::<Tasks, _>(|t, _| t.live.remove(&uid)))
        .flatten();
    let Some(driven) = driven else {
        return;
    };
    let run = driven.run().clone();
    let task = driven.task().to_string();
    let asked = session
        .filter(|_| driven.drive.card())
        .and_then(|session| crate::unattended::card_question(session, cx));
    crate::unattended::keep(&task, &run, asked, cx);
    cx.update_global::<Tasks, _>(|t, _| t.store_run(&task, run));
    super::ended(task, driven.drive.pinned(), session, cx);
    cx.defer(move |cx| {
        drop(driven);
        cx.refresh_windows();
    });
}
