//! The run's carrier: what the run's session says, and what a person
//! presses, goes to its [`RunDrive`], which decides, and what the drive says
//! to do is done here — the marks pinned, the command run, the forge
//! called, the clock armed — and what came of it goes back. What the run asks
//! of its session (a note, its prompt, a cancel) is made by their link, which
//! every call goes through; here only the session's effects are carried.
//!
//! Every rule is the drive's (`onehand_core::task::drive`); nothing here
//! decides what a run does next. A run is found by its task; a check's run
//! has no session at all.
//!
//! A run works in lanes, one session each: every call into the drive goes
//! with the session of the lane it works in now, and what another lane's
//! session says reaches it as that lane's, so the drive alone decides what
//! a lane it is not working in may still do to it.

use super::Tasks;
use crate::chat::session::{ChatEvent, ChatSession, Owed};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use gpui_component::WindowExt as _;
use onehand_core::Now;
use onehand_core::chat::Chat;
use onehand_core::task::drive::link::{self, Joined, Out};
use onehand_core::task::drive::{Effect, Input, Lane, RunDrive, Signal, Timer};
use onehand_core::task::tasks::{Start, link as tasks_link};
use onehand_core::task::{Approval, marks};
use onehand_core::workflow::{Facts, Mark, Run, run_command_blocking};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod forge;

// Where a lane's session is opened: on the run's window's shell, and in a
// test on whatever session that test hands it.
#[cfg(test)]
use crate::task::tests::open_lane as open;
#[cfg(not(test))]
use open_lane as open;

/// A run under way, on a session or, for a check, on none.
pub(super) struct Driven {
    drive: RunDrive,
    /// The session of each lane opened; none for a check.
    sessions: BTreeMap<Lane, Held>,
    /// The window the sessions are in, or the check was asked from.
    pub(super) window: AnyWindowHandle,
    /// Set to call off the step's command, while one is running.
    pub(super) command: Option<Arc<AtomicBool>>,
    /// The clock for the run's timeout, and the one for its next look at its
    /// status checks.
    _timeout: Option<Task<()>>,
    _checks: Option<Task<()>>,
}

/// A lane's session, its uid, and what listens to it for the run.
struct Held {
    uid: u64,
    session: WeakEntity<ChatSession>,
    _subscriptions: Vec<Subscription>,
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

    /// The uid of the session of the lane the run works in now.
    pub(super) fn uid(&self) -> Option<u64> {
        self.sessions
            .get(self.drive.current_lane())
            .map(|held| held.uid)
    }

    /// The uids of every lane's session still open.
    pub(super) fn uids(&self) -> Vec<u64> {
        let open = self.sessions.values();
        open.filter(|held| held.session.upgrade().is_some())
            .map(|held| held.uid)
            .collect()
    }

    /// Whether session `uid` is one of the run's lanes'.
    pub(super) fn on(&self, uid: u64) -> bool {
        self.sessions.values().any(|held| held.uid == uid)
    }

    /// The run waits on a person: an approval, or a card the agent parked.
    pub(super) fn waits_on_person(&self) -> bool {
        self.drive.waits_on_person()
    }

    /// Stop was pressed and waits on a command or forge call to land.
    pub(super) fn stopping(&self) -> bool {
        self.drive.stopping()
    }

    /// The session of the lane the run works in now.
    fn session(&self) -> Option<Entity<ChatSession>> {
        self.session_of(self.drive.current_lane())
    }

    fn session_of(&self, lane: &Lane) -> Option<Entity<ChatSession>> {
        self.sessions.get(lane)?.session.upgrade()
    }
}

/// What task `id`'s run hears from `session`, its lane `lane`'s: what moves
/// the run, asked of the link as the lane is current or not.
fn listen(id: &str, lane: &Lane, session: &Entity<ChatSession>, cx: &mut App) -> Vec<Subscription> {
    let (task, at) = (id.to_string(), lane.clone());
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        let input = match event {
            // Which signal moves the run, and which streamed chunk is worth
            // asking it about, is the link's.
            ChatEvent::Session(signal) => {
                let chat = &session.read(cx).chat();
                let input = read(&task, cx, |d| match d.drive.current_lane() == &at {
                    true => link::input_of(signal, &d.drive, chat),
                    false => link::lane_input_of(signal, &at, &d.drive, chat),
                });
                match input.flatten() {
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
            | ChatEvent::Look(_)
            | ChatEvent::Unsent(_)
            | ChatEvent::OpenSubagent(_) => return,
        };
        feed(&task, input, cx);
    });
    // Fires on every notify the session makes, so it reads before it borrows:
    // only a parked card, or work a turn left running, has anything to learn
    // from one, and only in the lane the run works in.
    let (task, at) = (id.to_string(), lane.clone());
    let answered = cx.observe(session, move |session, cx| {
        let chat = &session.read(cx).chat();
        let input = read(&task, cx, |d| {
            (d.drive.current_lane() == &at)
                .then(|| link::notified(&d.drive, chat))
                .flatten()
        });
        if let Some(input) = input.flatten() {
            feed(&task, input, cx);
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended, when it was the
    // lane the run works in; another lane's is only forgotten.
    let (task, at) = (id.to_string(), lane.clone());
    let release = cx.observe_release(session, move |_, cx| {
        let (task, lane) = (task.clone(), at.clone());
        cx.defer(move |cx| {
            let current = read(&task, cx, |d| d.drive.current_lane() == &lane);
            let input = match current {
                Some(true) | None => Input::SessionGone,
                Some(false) => Input::LaneGone { lane },
            };
            feed(&task, input, cx);
        });
    });
    vec![watch, answered, release]
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
    let Start { run, first, pinned } = start;
    // The first session is the lane of the step the run starts at.
    let lane = Lane::at(&run);
    let held = Held {
        uid,
        session: session.downgrade(),
        _subscriptions: listen(&task, &lane, session, cx),
    };
    let ((drive, effects, told), owed) = session.update(cx, |session, _| {
        session.lend(|chat, now| {
            let (drive, out) = Joined::start(task.clone(), run, first, pinned, now, Some(chat));
            ((drive, out.run, out.told), out.session)
        })
    });
    let driven = Driven {
        drive,
        sessions: BTreeMap::from([(lane, held)]),
        window,
        command: None,
        _timeout: None,
        _checks: None,
    };
    cx.default_global::<Tasks>()
        .live
        .insert(task.clone(), driven);
    // Only once the run is listed: what the session says of its first prompt
    // has a run to reach.
    carry_session(session, owed, told, cx);
    perform(&task, effects, cx);
}

/// Carry out what a joined call said of the run's session, and tell
/// whatever watches the session when it was told something.
fn carry_session(session: &Entity<ChatSession>, owed: Owed, told: bool, cx: &mut App) {
    if owed.is_empty() && !told {
        // Nothing to carry out and nothing told.
        return;
    }
    session.update(cx, |session, cx| session.settle(owed, cx));
}

/// Drive check task `task`'s run with no session, from `start`; what it
/// found is said in `window`.
pub(crate) fn start_check(task: String, start: Start, window: AnyWindowHandle, cx: &mut App) {
    let Start {
        run, first, pinned, ..
    } = start;
    let (drive, out) = Joined::start(task.clone(), run, first, pinned, Now::read(), None);
    let effects = out.run;
    let driven = Driven {
        drive,
        sessions: BTreeMap::new(),
        window,
        command: None,
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
    give(&approval.task, cx, move |run, now| run.approve(at, now));
}

/// A person sent what they read, as `approval` names it, back with `note`.
pub(crate) fn revise(approval: Approval, note: String, cx: &mut App) {
    let at = approval.at;
    give(&approval.task, cx, move |run, now| {
        run.revise(at, note, now)
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
    give(task, cx, |run, now| run.stop(now));
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
    // A person's prompt in another lane is read with that lane's session.
    let lane = match &input {
        Input::LaneAppended { lane } => Some(lane.clone()),
        _ => None,
    };
    // Another lane's session that went is forgotten, so nothing the run
    // asks before the lane opens again reaches it, and what it says no
    // longer moves the run.
    let forget = match &input {
        Input::LaneGone { lane } => Some(lane.clone()),
        _ => None,
    };
    give_in(id, lane, cx, move |run, now| run.handle(input, now));
    if let Some(lane) = forget {
        forget_lane(id, lane, cx);
    }
}

/// Let task `id`'s lane `lane` drop its session, unless the run works in
/// it now. Deferred: the session's own signal may be what is delivering
/// this, and its subscription must not be dropped inside it.
fn forget_lane(id: &str, lane: Lane, cx: &mut App) {
    let Some(uid) = read(id, cx, |d| d.sessions.get(&lane).map(|held| held.uid)).flatten() else {
        return;
    };
    let task = id.to_string();
    cx.defer(move |cx| {
        let gone = with(&task, cx, |d| {
            let still = d.drive.current_lane() != &lane
                && d.sessions.get(&lane).is_some_and(|held| held.uid == uid);
            still.then(|| d.sessions.remove(&lane)).flatten()
        });
        drop(gone);
    });
}

/// Give task `id`'s run `command`, joined with its session as it stands, and
/// do what comes back: the session's effects, then the run's.
///
/// The two drives are joined inside the tasks' update, which is where the
/// run's is held; what they said is carried out once that update is over, so
/// nothing the session's carrier does reads the tasks while they are lent.
fn give(id: &str, cx: &mut App, command: impl FnOnce(&mut Joined<'_>, Now) -> Out) {
    give_in(id, None, cx, command)
}

/// [`give`], joined with `lane`'s session, or, `None`, the current lane's.
fn give_in(
    id: &str,
    lane: Option<Lane>,
    cx: &mut App,
    command: impl FnOnce(&mut Joined<'_>, Now) -> Out,
) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    let now = Now::read();
    let given = cx.update_global::<Tasks, _>(|t, cx| {
        let d = t.live.get_mut(id)?;
        let session = match &lane {
            Some(lane) => d.session_of(lane),
            None => d.session(),
        };
        Some(match session {
            Some(session) => {
                let ((effects, told), owed) = session.update(cx, |s, _| {
                    s.lend(|chat, _| {
                        let out = command(&mut Joined::new(&mut d.drive, Some(chat)), now);
                        ((out.run, out.told), out.session)
                    })
                });
                (effects, Some((session, owed, told)))
            }
            None => (command(&mut Joined::new(&mut d.drive, None), now).run, None),
        })
    });
    let Some((effects, session)) = given else {
        return;
    };
    if let Some((session, owed, told)) = session {
        carry_session(&session, owed, told, cx);
    }
    perform(id, effects, cx);
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
            Effect::Refresh => cx.refresh_windows(),
            Effect::SessionChanged => {
                if let Some(session) = session {
                    session.update(cx, |_, cx| cx.notify());
                }
            }
            Effect::Finish { .. } => finish(id, session.as_ref(), cx),
            // The lane names its session itself once it is told it is open.
            Effect::OpenSession {
                lane,
                agent,
                name: _,
            } => open_session(id, lane, agent, cx),
        }
    }
}

/// Open the session of task `id`'s lane `lane`, on agent `agent`, in the
/// run's window, give it what the task's sessions may use of onehand's
/// tools, and tell the run how that went.
fn open_session(id: &str, lane: Lane, agent: String, cx: &mut App) {
    let Some((window, dir)) = read(id, cx, |d| (d.window, d.run().setup.dir.clone())) else {
        return;
    };
    let grant = cx
        .global::<Tasks>()
        .task(id)
        .map(|task| task.grant().clone());
    let task = id.to_string();
    // Deferred: the shell opening it may be the one whose update fed the run.
    cx.defer(move |cx| {
        // A run that ended meanwhile has nothing to open a session for.
        if read(&task, cx, |_| ()).is_none() {
            return;
        }
        let result = open(window, dir, &agent, cx).map(|(uid, session)| {
            if let Some(grant) = &grant {
                crate::control::grant(uid, grant, cx);
            }
            let held = Held {
                uid,
                session: session.downgrade(),
                _subscriptions: listen(&task, &lane, &session, cx),
            };
            // A session still held for the lane is not left in the rail
            // with nothing driving it: it is closed.
            let old = with(&task, cx, |d| d.sessions.insert(lane.clone(), held)).flatten();
            if let Some(old) = old.filter(|old| old.session.upgrade().is_some()) {
                crate::windows::with_shell(window, cx, |shell, window, cx| {
                    shell.close_finished(old.uid, window, cx)
                });
            }
        });
        feed(&task, Input::LaneOpened { lane, result }, cx);
    });
}

/// Open a lane's session on the shell of `window`, which says why it did
/// not.
#[cfg(not(test))]
fn open_lane(
    window: AnyWindowHandle,
    dir: std::path::PathBuf,
    agent: &str,
    cx: &mut App,
) -> Result<(u64, Entity<ChatSession>), String> {
    crate::windows::with_shell(window, cx, |shell, _, cx| {
        crate::shell::open_step_session(shell, dir, agent, cx)
    })
    .unwrap_or_else(|| Err("its window is closed".to_string()))
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
    let fx = cx.update_global::<Tasks, _>(|t, cx| {
        let chat = session.map(|session| session.read(cx).chat() as &Chat);
        tasks_link::finished(
            &mut t.drive,
            &driven.drive,
            chat,
            driven.uids(),
            driven.window,
        )
    });
    cx.defer(move |cx| {
        drop(driven);
        cx.refresh_windows();
    });
    super::place::perform(fx, session, cx);
}
