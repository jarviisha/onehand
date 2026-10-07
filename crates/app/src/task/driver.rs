//! The one driver of every run: it watches the run's session, does
//! what the run's engine asks, and reports back what happened.
//!
//! **Every Stop goes through the engine the same way.** A person's Stop, a
//! prompt of their own, the timeout, the agent going and the session closing
//! each end the run with [`Run::stopped`], which never judges the turn
//! that was under way — a cut-short turn passing as finished work is the
//! mistake two drivers disagreeing made before.

use super::Tasks;
use crate::chat::session::{ChatEvent, ChatSession, note};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use onehand_core::chat::Link;
use onehand_core::preflight::{Check, found_late};
use onehand_core::task::Approval;
use onehand_core::task::marks;
use onehand_core::unattended::Budget;
use onehand_core::workflow::{
    Action, Facts, Failure, Mark, Outcome, Run, Stop, run_command_blocking,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod forge;

/// A call held until a mark is pinned, and whether it ends the run.
type Later = (bool, Box<dyn FnOnce(&mut App)>);

/// A run under way on one session.
pub(super) struct Driven {
    /// The task the run is of.
    pub(super) task: String,
    pub(super) run: Run,
    /// How many of the run's boundaries have had their mark pinned, or tried.
    pinned: usize,
    /// `Some` while a mark is being pinned and the action the engine asked
    /// for waits on it, holding what a person or an event asked meanwhile.
    /// Done at once, an approval or a Stop would move the run on under the
    /// waiting action, which would then be carried out stale; held, each
    /// runs once that action has. An ending held runs instead of the action,
    /// so a prompt is never sent to a run already said to be over. A turn
    /// ending meanwhile is not the one the action will send.
    held: Option<Vec<Later>>,
    session: WeakEntity<ChatSession>,
    /// The window the session is in.
    pub(super) window: AnyWindowHandle,
    /// How many prompts the run has sent its session: any more, and a person
    /// is driving.
    sent: usize,
    /// How many transcript items there were when the run's last prompt went:
    /// the turn's answer is what the agent said after that.
    answer_from: usize,
    /// A prompt waiting for the agent to come up.
    pending: Option<String>,
    /// The agent has been seen up in this session, and the run's mode set.
    up: bool,
    /// The step the transcript was last told of.
    noted: Option<usize>,
    /// How much of the timeout is left; it does not run while a card or an
    /// approval waits on a person.
    budget: Budget,
    /// Working time spent before this session took the run up.
    spent_before: u64,
    /// The agent parked a card nobody has answered yet.
    card: bool,
    /// Set to call off the step's command, while one is running.
    pub(super) command: Option<Arc<AtomicBool>>,
    /// How the run ends once its called-off command has exited.
    stopping: Option<Stop>,
    _watch: Subscription,
    _answered: Subscription,
    _release: Subscription,
    _clock: Task<()>,
    /// Looking at the pull request's status checks, while the run waits on
    /// them.
    _status_checks: Option<Task<()>>,
}

impl Driven {
    /// The run waits on a person: an approval, or a card the agent parked.
    pub(super) fn waits_on_person(&self) -> bool {
        self.run.awaiting_approval() || self.card
    }

    /// Working time spent on the run, sessions before this one included.
    fn spent_secs(&self) -> u64 {
        let used = self
            .budget
            .limit()
            .saturating_sub(self.budget.left(Instant::now()));
        self.spent_before + used.as_secs()
    }
}

/// How long a run may work when its template's timeout does not read, which
/// validation refuses, so only a run file edited by hand meets it.
const TIMEOUT_FALLBACK: Duration = Duration::from_secs(45 * 60);

/// The longest answer kept from a turn. An answer the length of a book is cut
/// rather than carried whole into the run's file and the next prompt.
const ANSWER_MAX: usize = 20_000;

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
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    // Fires on every notify the session makes, so it reads before it borrows.
    let answered = cx.observe(session, move |session, cx| {
        let card = read(uid, cx, |d| d.card).unwrap_or(false);
        if card && !session.read(cx).chat.awaiting_permission() {
            let task = with(uid, cx, |d| {
                d.card = false;
                d.budget.resume(Instant::now());
                d.task.clone()
            });
            if let Some(task) = task {
                cx.defer(move |cx| crate::unattended::answered(&task, cx));
            }
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended.
    let release = cx.observe_release(session, move |_, cx| {
        cx.defer(move |cx| end(uid, Stop::Closed, cx));
    });
    let limit = onehand_core::unattended::parse_every(&run.template.timeout)
        .unwrap_or(TIMEOUT_FALLBACK)
        .saturating_sub(Duration::from_secs(run.spent_secs));
    let driven = Driven {
        task,
        spent_before: run.spent_secs,
        run,
        pinned,
        held: None,
        session: session.downgrade(),
        window,
        sent: 0,
        answer_from: 0,
        pending: None,
        up: false,
        noted: None,
        budget: Budget::start(limit, Instant::now()),
        card: false,
        command: None,
        stopping: None,
        _watch: watch,
        _answered: answered,
        _release: release,
        _clock: clock(uid, limit, cx),
        _status_checks: None,
    };
    cx.default_global::<Tasks>().live.insert(uid, driven);
    act(uid, first, cx);
}

/// Hold `then`, which ends the run when `ends`, while a mark is being pinned
/// for the run on session `uid`: whether it was held.
fn held(uid: u64, cx: &mut App, ends: bool, then: impl FnOnce(&mut App) + 'static) -> bool {
    with(uid, cx, |d| match &mut d.held {
        Some(held) => {
            held.push((ends, Box::new(then)));
            true
        }
        None => false,
    })
    .unwrap_or(false)
}

/// A person approved what they read, as `approval` names it.
pub(crate) fn approve(approval: Approval, cx: &mut App) {
    if let Some(uid) = live_uid(&approval.task, cx) {
        let task = approval.task.clone();
        answer(uid, cx, move |run| run.approved(&approval.at));
        crate::unattended::answered(&task, cx);
    }
}

/// A person sent what they read, as `approval` names it, back with `note`.
pub(crate) fn revise(approval: Approval, note: String, cx: &mut App) {
    if let Some(uid) = live_uid(&approval.task, cx) {
        let task = approval.task.clone();
        answer(uid, cx, move |run| run.revised(&approval.at, note));
        crate::unattended::answered(&task, cx);
    }
}

/// The session task `task`'s run is driven on: a press is routed by the
/// task, so a window other than the session's can make it.
fn live_uid(task: &str, cx: &App) -> Option<u64> {
    cx.try_global::<Tasks>()?.live_uid(task)
}

/// A person's answer to the approval the run on session `uid` waits on: held
/// while a mark is pinned, then judged by the engine against the visit it
/// was drawn from. The clock goes on only once the engine took it, so a
/// refused press leaves the run waiting as it was.
fn answer(uid: u64, cx: &mut App, report: impl FnOnce(&mut Run) -> Action + 'static) {
    if read(uid, cx, |d| d.held.is_some()).unwrap_or(false) {
        held(uid, cx, false, move |cx| answer(uid, cx, report));
        return;
    }
    let Some(action) = with(uid, cx, |d| report(&mut d.run)) else {
        return;
    };
    if action != Action::Idle {
        with(uid, cx, |d| d.budget.resume(Instant::now()));
    }
    act(uid, action, cx);
}

/// A person pressed Stop: the turn is cancelled and the run ends.
pub(crate) fn stop(uid: u64, cx: &mut App) {
    cut(uid, Stop::ByPerson, cx);
}

/// Cancel the turn under way and end the run as `stop`, as one call: held
/// together while a mark is pinned, so the cancel never runs before a
/// prompt the pin is still holding back.
fn cut(uid: u64, stop: Stop, cx: &mut App) {
    if held(uid, cx, true, move |cx| cut(uid, stop, cx)) {
        return;
    }
    cancel_turn(uid, cx);
    end(uid, stop, cx);
}

/// Read the run on session `uid`. Apart from [`with`] because every event the
/// session sends asks, and a mutable borrow would tell every observer of the
/// global that it changed.
fn read<R>(uid: u64, cx: &App, look: impl FnOnce(&Driven) -> R) -> Option<R> {
    cx.try_global::<Tasks>()?.live.get(&uid).map(look)
}

fn with<R>(uid: u64, cx: &mut App, act: impl FnOnce(&mut Driven) -> R) -> Option<R> {
    if !cx.has_global::<Tasks>() {
        return None;
    }
    cx.update_global::<Tasks, _>(|t, _| t.live.get_mut(&uid).map(act))
}

/// End the run as `stop` — at once, or, while the step's command runs, once
/// that command and everything it started have been stopped and have exited.
/// A run said to be stopped never leaves a build or a test writing to the
/// work behind it. The first reason given is the one the run ends with.
fn end(uid: u64, stop: Stop, cx: &mut App) {
    if held(uid, cx, true, move |cx| end(uid, stop, cx)) {
        return;
    }
    let running = with(uid, cx, |d| {
        let cancel = d.command.as_ref()?;
        cancel.store(true, Ordering::SeqCst);
        d.stopping.get_or_insert(stop);
        Some(())
    })
    .flatten();
    if running.is_none() {
        advance(uid, cx, move |run| run.stopped(stop));
    }
}

/// Report to the run's engine, then do what it says.
fn advance(uid: u64, cx: &mut App, report: impl FnOnce(&mut Run) -> Action) {
    if let Some(action) = with(uid, cx, |d| report(&mut d.run)) {
        act(uid, action, cx);
    }
}

fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    // A turn ending while a mark is pinned is not the turn the run will send.
    let Some((sent, pending, judge_turn)) = read(uid, cx, |d| {
        (
            d.sent,
            d.pending.is_some(),
            d.run.awaiting_turn() && d.held.is_none(),
        )
    }) else {
        return;
    };
    if pending && session.read(cx).chat.link == Link::Connected {
        if let Some(text) = with(uid, cx, |d| d.pending.take()).flatten() {
            send(uid, session, text, cx);
        }
        return;
    }
    match event {
        ChatEvent::Appended => {
            if session.read(cx).chat.prompted_beyond(sent) {
                end(uid, Stop::TakenOver, cx);
            }
        }
        // A Stop pressed in the composer, or from the remote bridge: the
        // person's word that this is not to go on.
        ChatEvent::TurnEnded if session.read(cx).chat.cancelled => end(uid, Stop::ByPerson, cx),
        ChatEvent::TurnEnded if judge_turn => after_turn(uid, session, cx),
        ChatEvent::TurnEnded => {}
        // A person answers the card; the clock waits for them.
        ChatEvent::AwaitingUser(_) => {
            let task = with(uid, cx, |d| {
                d.card = true;
                d.budget.pause(Instant::now());
                d.task.clone()
            });
            if let Some(task) = task {
                crate::unattended::waiting(&task, cx);
            }
        }
        ChatEvent::Disconnected => end(uid, Stop::LinkLost, cx),
        ChatEvent::OpenFile(_) => {}
    }
}

/// Do what the engine said, once the work is pinned at every boundary the
/// run has crossed since the last pin: the mark of a step's start lands
/// before its prompt or command touches the work. A mark that cannot be
/// pinned is said and left out; it never stops the run.
fn act(uid: u64, action: Action, cx: &mut App) {
    if ends_run(&action) || action == Action::Idle {
        return carry_out(uid, action, cx);
    }
    let unpinned = with(uid, cx, |d| {
        let refs = marks::refs_from(&d.task, &d.run, d.pinned);
        if refs.is_empty() {
            return None;
        }
        d.held = Some(Vec::new());
        Some((d.run.setup.dir.clone(), refs, d.pinned))
    })
    .flatten();
    let Some((dir, refs, from)) = unpinned else {
        return carry_out(uid, action, cx);
    };
    let total = from + refs.len();
    cx.spawn(async move |cx| {
        let pinned = cx
            .background_executor()
            .spawn(async move { marks::pin_blocking(&dir, &refs) })
            .await;
        cx.update(|cx| {
            let held = with(uid, cx, |d| {
                match &pinned {
                    Ok(commit) => d.run.pinned(commit, from),
                    Err(why) => eprintln!("onehand: a step's mark was not pinned: {why}"),
                }
                d.pinned = total;
                d.held.take().unwrap_or_default()
            });
            let Some(held) = held else {
                return;
            };
            // An ending asked meanwhile wins over the action, which would only
            // start work on a run that is over; the first one asked is its
            // reason, and the rest find nothing left to do.
            let mut held = held;
            if let Some(at) = held.iter().position(|(ends, _)| *ends) {
                let (_, ending) = held.swap_remove(at);
                return ending(cx);
            }
            carry_out(uid, action, cx);
            // In the order asked; one that starts another pin holds the rest.
            for (_, then) in held {
                then(cx);
            }
        });
    })
    .detach();
}

/// Do what the engine said, now.
fn carry_out(uid: u64, action: Action, cx: &mut App) {
    let Some(session) = read(uid, cx, |d| d.session.upgrade()) else {
        return;
    };
    let Some(session) = session else {
        // The session is gone; only ending the run is left to do.
        if let Action::Finish(outcome) = action {
            finish(uid, None, outcome, cx);
        }
        return;
    };
    if !ends_run(&action) {
        announce_step(uid, &session, cx);
    }
    match action {
        Action::Measure => measure(uid, cx),
        Action::Prompt(text) => send(uid, &session, text, cx),
        Action::RunCommand(command) => run_command(uid, &session, command, cx),
        Action::AwaitApproval => {
            let task = with(uid, cx, |d| {
                d.budget.pause(Instant::now());
                d.task.clone()
            });
            note(
                &session,
                "Waiting for approval: Continue or Revise… under the header".to_string(),
                cx,
            );
            if let Some(task) = task {
                crate::unattended::waiting(&task, cx);
            }
        }
        Action::Push(commit) => {
            note(
                &session,
                format!("Pushing {}, which the check passed on", short(&commit)),
                cx,
            );
            forge::on_forge(uid, Some(commit), cx);
        }
        Action::OpenPullRequest => {
            note(&session, "Opening the pull request".to_string(), cx);
            forge::on_forge(uid, None, cx);
        }
        Action::AwaitStatusChecks { wait, pushed } => {
            let task = with(uid, cx, |d| {
                d.budget.pause(Instant::now());
                d.task.clone()
            });
            note(
                &session,
                "Waiting for the pull request's status checks".to_string(),
                cx,
            );
            let watching = forge::watch_status_checks(uid, pushed, Instant::now(), wait, cx);
            with(uid, cx, |d| d._status_checks = Some(watching));
            // Its slot is free now, for another issue to take up.
            if let Some(task) = task {
                crate::unattended::waiting(&task, cx);
            }
        }
        Action::Finish(outcome) => return finish(uid, Some(&session), outcome, cx),
        Action::Idle => return,
    }
    save(uid, cx);
    cx.refresh_windows();
}

/// Whether doing `action` ends the run, which is the one action after which
/// there is no step to announce and nothing to save.
fn ends_run(action: &Action) -> bool {
    match action {
        Action::Finish(_) => true,
        Action::Measure
        | Action::Prompt(_)
        | Action::RunCommand(_)
        | Action::AwaitApproval
        | Action::Push(_)
        | Action::OpenPullRequest
        | Action::AwaitStatusChecks { .. }
        | Action::Idle => false,
    }
}

/// Tell the transcript the step the run is now in, once per step entered.
fn announce_step(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let said = with(uid, cx, |d| {
        let fresh = d.noted != Some(d.run.step);
        d.noted = Some(d.run.step);
        let step = d.run.current()?;
        fresh.then(|| {
            format!(
                "Step {} of {}: {}",
                d.run.step + 1,
                d.run.template.steps.len(),
                step.label
            )
        })
    })
    .flatten();
    if let Some(said) = said {
        note(session, said, cx);
    }
}

/// Write the run into its task's file, in order with every other write.
fn save(uid: u64, cx: &mut App) {
    if !cx.has_global::<Tasks>() {
        return;
    }
    cx.update_global::<Tasks, _>(|t, _| {
        let Some(d) = t.live.get_mut(&uid) else {
            return;
        };
        d.run.spent_secs = d.spent_secs();
        let (task, run) = (d.task.clone(), d.run.clone());
        t.store_run(&task, run);
    });
}

/// Read where the step's work starts.
fn measure(uid: u64, cx: &mut App) {
    let Some(dir) = read(uid, cx, |d| d.run.setup.dir.clone()) else {
        return;
    };
    cx.spawn(async move |cx| {
        let mark = cx
            .background_executor()
            .spawn(async move { Mark::read_blocking(&dir) })
            .await;
        cx.update(|cx| match mark {
            Ok(mark) => advance(uid, cx, move |run| run.measured(mark)),
            Err(why) => advance(uid, cx, move |run| {
                run.failed(format!("the work could not be read: {why}"), Failure::Other)
            }),
        });
    })
    .detach();
}

/// Send `text`, or keep it until the agent is up.
fn send(uid: u64, session: &Entity<ChatSession>, text: String, cx: &mut App) {
    if session.read(cx).chat.link != Link::Connected {
        with(uid, cx, |d| d.pending = Some(text));
        return;
    }
    if session
        .read(cx)
        .chat
        .prompted_beyond(read(uid, cx, |d| d.sent).unwrap_or(0))
    {
        end(uid, Stop::TakenOver, cx);
        return;
    }
    if !came_up(uid, session, cx) {
        return;
    }
    let answer_from = session.read(cx).chat.items.len();
    with(uid, cx, |d| {
        d.sent += 1;
        d.answer_from = answer_from;
    });
    if !session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        advance(uid, cx, |run| {
            run.failed(
                "the agent did not take the prompt".to_string(),
                Failure::Other,
            )
        });
    }
}

/// The agent is up, the first time in this session: put it in the run's mode
/// before its first prompt, and say so to an issue the run works. Whether the
/// prompt may go: an agent that does not offer the mode fails the run, since
/// one left in the mode that asks before every edit would park at the first.
fn came_up(uid: u64, session: &Entity<ChatSession>, cx: &mut App) -> bool {
    let Some((mode, task)) = with(uid, cx, |d| {
        let first = !std::mem::replace(&mut d.up, true);
        first.then(|| (d.run.setup.mode.clone(), d.task.clone()))
    })
    .flatten() else {
        return true;
    };
    if let Some(task) = super::task(&task, cx) {
        crate::unattended::started(&task, session, cx);
    }
    let Some(mode) = mode else {
        return true;
    };
    let offered: Vec<String> = session
        .read(cx)
        .chat
        .modes
        .iter()
        .map(|m| m.id.clone())
        .collect();
    if let Some(why) = onehand_core::preflight::mode_refused(&mode, &offered) {
        crate::unattended::refuse_mode(&why, cx);
        advance(uid, cx, move |run| run.failed(why, found_late(Check::Mode)));
        return false;
    }
    session.update(cx, |session, _| session.chat.set_mode(&mode));
    note(session, format!("Mode {mode} set"), cx);
    true
}

/// A turn the run sent ended: read the work, then let the engine judge it.
fn after_turn(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some((dir, from, answer_from)) = read(uid, cx, |d| {
        (
            d.run.setup.dir.clone(),
            d.run.marks.step_from.clone(),
            d.answer_from,
        )
    }) else {
        return;
    };
    let Some(from) = from else {
        return;
    };
    let answer = turn_answer(session, answer_from, cx);
    cx.spawn(async move |cx| {
        let facts = cx
            .background_executor()
            .spawn(async move { Facts::read_blocking(&dir, &from) })
            .await;
        cx.update(|cx| match facts {
            Ok(facts) => advance(uid, cx, move |run| run.turn_ended(&facts, &answer)),
            Err(why) => advance(uid, cx, move |run| {
                run.failed(format!("the work could not be read: {why}"), Failure::Other)
            }),
        });
    })
    .detach();
}

/// The whole of the answer to the prompt sent at `from` items, cut at
/// [`ANSWER_MAX`] characters.
fn turn_answer(session: &Entity<ChatSession>, from: usize, cx: &App) -> String {
    let prose = session.read(cx).chat.prose_since(from);
    match prose.char_indices().nth(ANSWER_MAX) {
        Some((cut, _)) => format!("{}\n\n(cut at {ANSWER_MAX} characters)", &prose[..cut]),
        None => prose,
    }
}

/// Run the step's command in the work, then report how it went with the
/// commit it ran on.
fn run_command(uid: u64, session: &Entity<ChatSession>, command: String, cx: &mut App) {
    let cancel = Arc::new(AtomicBool::new(false));
    let Some(dir) = with(uid, cx, |d| {
        d.command = Some(cancel.clone());
        d.run.setup.dir.clone()
    }) else {
        return;
    };
    note(session, format!("Running: {command}"), cx);
    let weak = session.downgrade();
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
            // Called off: the command has exited by now, so the run can end.
            let stopping = with(uid, cx, |d| {
                d.command = None;
                d.stopping.take()
            })
            .flatten();
            if let Some(stop) = stopping {
                return advance(uid, cx, move |run| run.stopped(stop));
            }
            if let Some(session) = weak.upgrade().filter(|_| read(uid, cx, |_| ()).is_some()) {
                let said = match ran.passed {
                    true => "The command passed",
                    false => "The command failed",
                };
                note(&session, said.to_string(), cx);
            }
            advance(uid, cx, move |run| run.command_finished(ran));
        });
    })
    .detach();
}

/// A commit as a person reads it.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

/// The run's timeout: when `left` has run out, look again, since time spent
/// waiting on a person did not count; end the run once nothing is left.
fn clock(uid: u64, left: Duration, cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(left).await;
        cx.update(|cx| {
            let Some(left) = read(uid, cx, |d| d.budget.left(Instant::now())) else {
                return;
            };
            if left.is_zero() {
                cut(uid, Stop::TimedOut, cx);
            } else {
                let next = clock(uid, left, cx);
                with(uid, cx, |d| d._clock = next);
            }
        });
    })
}

/// Cancel the turn the run's session has under way, if any.
fn cancel_turn(uid: u64, cx: &mut App) {
    let Some(session) = read(uid, cx, |d| d.session.upgrade()).flatten() else {
        return;
    };
    session.update(cx, |session, cx| {
        if session.chat.busy {
            session.chat.cancel_turn();
            cx.notify();
        }
    });
}

/// End the run: say how in its transcript and keep it in its task, then let
/// its place go once its session's turn is over.
///
/// Taken off the list at once, so an event the ending causes finds nothing
/// to end again; dropped only once the event delivering it is over, since
/// dropping the run drops the subscription it came through.
fn finish(uid: u64, session: Option<&Entity<ChatSession>>, outcome: Outcome, cx: &mut App) {
    let driven = cx
        .has_global::<Tasks>()
        .then(|| cx.update_global::<Tasks, _>(|t, _| t.live.remove(&uid)))
        .flatten();
    let Some(driven) = driven else {
        return;
    };
    if let Some(session) = session {
        let said = match outcome.resumable() {
            true => format!("{}; resume it from the Tasks page", outcome.said()),
            false => outcome.said(),
        };
        note(session, said, cx);
    }
    let mut run = driven.run.clone();
    run.spent_secs = driven.spent_secs();
    let task = driven.task.clone();
    let asked = session
        .filter(|_| driven.card)
        .and_then(|session| crate::unattended::card_question(session, cx));
    crate::unattended::keep(&task, &run, driven.sent > 0, asked, cx);
    cx.update_global::<Tasks, _>(|t, _| t.store_run(&task, run));
    super::ended(task, driven.pinned, session, cx);
    cx.defer(move |cx| {
        drop(driven);
        cx.refresh_windows();
    });
}
