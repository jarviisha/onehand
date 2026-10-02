//! The one driver of every pipeline run: it watches the run's session, does
//! what the run's engine asks, and reports back what happened.
//!
//! **Every Stop goes through the engine the same way.** A person's Stop, a
//! prompt of their own, the timeout, the agent going and the session closing
//! each end the run with [`PipelineRun::stopped`], which never judges the turn
//! that was under way — a cut-short turn passing as finished work is the
//! mistake two drivers disagreeing made before.

use super::Pipelines;
use crate::chat::session::{ChatEvent, ChatSession, note};
use gpui::{App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use onehand_core::chat::{Link, TranscriptItemId};
use onehand_core::pipeline::{
    Action, Facts, Mark, Outcome, PipelineRun, Stop, files, run_command_blocking,
};
use onehand_core::unattended::Budget;
use onehand_core::worktree;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A run under way on one session.
pub(super) struct Driven {
    pub(super) run: PipelineRun,
    file: PathBuf,
    session: WeakEntity<ChatSession>,
    /// How many prompts the run has sent its session: any more, and a person
    /// is driving.
    sent: usize,
    /// A prompt waiting for the agent to come up.
    pending: Option<String>,
    /// The step the transcript was last told of.
    noted: Option<usize>,
    /// How much of the timeout is left; it does not run while a card or an
    /// approval waits on a person.
    budget: Budget,
    /// Working time spent before this session took the run up.
    spent_before: u64,
    /// The agent parked a card nobody has answered yet.
    card: bool,
    _watch: Subscription,
    _answered: Subscription,
    _release: Subscription,
    _clock: Task<()>,
}

impl Driven {
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

/// Drive `run` on session `uid`, starting with `first`: what
/// [`PipelineRun::begin`] or [`PipelineRun::resume`] said to do.
pub(crate) fn start(
    uid: u64,
    session: &Entity<ChatSession>,
    run: PipelineRun,
    first: Action,
    cx: &mut App,
) {
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    // Fires on every notify the session makes, so it reads before it borrows.
    let answered = cx.observe(session, move |session, cx| {
        let card = read(uid, cx, |d| d.card).unwrap_or(false);
        if card && !session.read(cx).chat.awaiting_permission() {
            with(uid, cx, |d| {
                d.card = false;
                d.budget.resume(Instant::now());
            });
        }
    });
    // The session went with the run still going: its window or the session
    // was closed. The run is kept to be resumed, not ended.
    let release = cx.observe_release(session, move |_, cx| {
        cx.defer(move |cx| advance(uid, cx, |run| run.stopped(Stop::Closed)));
    });
    let limit = onehand_core::unattended::parse_every(&run.template.timeout)
        .unwrap_or(TIMEOUT_FALLBACK)
        .saturating_sub(Duration::from_secs(run.spent_secs));
    let driven = Driven {
        file: files::run_file(&files::runs_dir(), &run.id),
        spent_before: run.spent_secs,
        run,
        session: session.downgrade(),
        sent: 0,
        pending: None,
        noted: None,
        budget: Budget::start(limit, Instant::now()),
        card: false,
        _watch: watch,
        _answered: answered,
        _release: release,
        _clock: clock(uid, limit, cx),
    };
    cx.default_global::<Pipelines>().runs.insert(uid, driven);
    act(uid, first, cx);
}

/// A person approved what the run waits on.
pub(crate) fn approve(uid: u64, cx: &mut App) {
    with(uid, cx, |d| d.budget.resume(Instant::now()));
    advance(uid, cx, PipelineRun::approved);
}

/// A person sent it back with `note`.
pub(crate) fn revise(uid: u64, note: String, cx: &mut App) {
    with(uid, cx, |d| d.budget.resume(Instant::now()));
    advance(uid, cx, move |run| run.revised(note));
}

/// A person pressed Stop: the turn is cancelled and the run ends.
pub(crate) fn stop(uid: u64, cx: &mut App) {
    cancel_turn(uid, cx);
    advance(uid, cx, |run| run.stopped(Stop::ByPerson));
}

/// Read the run on session `uid`. Apart from [`with`] because every event the
/// session sends asks, and a mutable borrow would tell every observer of the
/// global that it changed.
fn read<R>(uid: u64, cx: &App, look: impl FnOnce(&Driven) -> R) -> Option<R> {
    cx.try_global::<Pipelines>()?.runs.get(&uid).map(look)
}

fn with<R>(uid: u64, cx: &mut App, act: impl FnOnce(&mut Driven) -> R) -> Option<R> {
    if !cx.has_global::<Pipelines>() {
        return None;
    }
    cx.update_global::<Pipelines, _>(|p, _| p.runs.get_mut(&uid).map(act))
}

/// Report to the run's engine, then do what it says.
fn advance(uid: u64, cx: &mut App, report: impl FnOnce(&mut PipelineRun) -> Action) {
    if let Some(action) = with(uid, cx, |d| report(&mut d.run)) {
        act(uid, action, cx);
    }
}

fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    let Some((sent, pending, awaiting_turn)) = read(uid, cx, |d| {
        (d.sent, d.pending.is_some(), d.run.awaiting_turn())
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
                advance(uid, cx, |run| run.stopped(Stop::TakenOver));
            }
        }
        // A Stop pressed in the composer, or from the remote bridge: the
        // person's word that this is not to go on.
        ChatEvent::TurnEnded if session.read(cx).chat.cancelled => {
            advance(uid, cx, |run| run.stopped(Stop::ByPerson))
        }
        ChatEvent::TurnEnded if awaiting_turn => after_turn(uid, session, cx),
        ChatEvent::TurnEnded => {}
        // A person answers the card; the clock waits for them.
        ChatEvent::AwaitingUser(_) => {
            with(uid, cx, |d| {
                d.card = true;
                d.budget.pause(Instant::now());
            });
        }
        ChatEvent::Disconnected => advance(uid, cx, |run| run.stopped(Stop::LinkLost)),
        ChatEvent::OpenFile(_) => {}
    }
}

/// Do what the engine said.
fn act(uid: u64, action: Action, cx: &mut App) {
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
            with(uid, cx, |d| d.budget.pause(Instant::now()));
            note(
                &session,
                "Waiting for approval: Continue or Revise… under the header".to_string(),
                cx,
            );
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

/// Write the run's snapshot, in order with every other write.
fn save(uid: u64, cx: &mut App) {
    if !cx.has_global::<Pipelines>() {
        return;
    }
    cx.update_global::<Pipelines, _>(|p, _| {
        let Some(d) = p.runs.get_mut(&uid) else {
            return;
        };
        d.run.spent_secs = d.spent_secs();
        p.writer
            .send(files::FileOp::Save(d.file.clone(), Box::new(d.run.clone())));
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
                run.failed(format!("the work could not be read: {why}"))
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
        advance(uid, cx, |run| run.stopped(Stop::TakenOver));
        return;
    }
    with(uid, cx, |d| d.sent += 1);
    if !session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        advance(uid, cx, |run| {
            run.failed("the agent did not take the prompt".to_string())
        });
    }
}

/// A turn the run sent ended: read the work, then let the engine judge it.
fn after_turn(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some((dir, from)) = read(uid, cx, |d| {
        (d.run.setup.dir.clone(), d.run.marks.step_from.clone())
    }) else {
        return;
    };
    let Some(from) = from else {
        return;
    };
    let answer = turn_answer(session, cx);
    cx.spawn(async move |cx| {
        let facts = cx
            .background_executor()
            .spawn(async move { Facts::read_blocking(&dir, &from) })
            .await;
        cx.update(|cx| match facts {
            Ok(facts) => advance(uid, cx, move |run| run.turn_ended(&facts, &answer)),
            Err(why) => advance(uid, cx, move |run| {
                run.failed(format!("the work could not be read: {why}"))
            }),
        });
    })
    .detach();
}

/// The whole of the last turn's answer, cut at [`ANSWER_MAX`] characters.
fn turn_answer(session: &Entity<ChatSession>, cx: &App) -> String {
    let chat = &session.read(cx).chat;
    let last = chat.items.len().saturating_sub(1);
    let prose = chat.turn_prose(TranscriptItemId::Live(last));
    match prose.char_indices().nth(ANSWER_MAX) {
        Some((cut, _)) => format!("{}\n\n(cut at {ANSWER_MAX} characters)", &prose[..cut]),
        None => prose,
    }
}

/// Run the step's command in the work, then report how it went with the
/// commit it ran on.
fn run_command(uid: u64, session: &Entity<ChatSession>, command: String, cx: &mut App) {
    let Some(dir) = read(uid, cx, |d| d.run.setup.dir.clone()) else {
        return;
    };
    note(session, format!("Running: {command}"), cx);
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let ran = cx
            .background_executor()
            .spawn(async move {
                run_command_blocking(&dir, &command)?;
                worktree::head_blocking(&dir)
            })
            .await;
        cx.update(|cx| {
            if let Some(session) = weak.upgrade().filter(|_| read(uid, cx, |_| ()).is_some()) {
                let said = match &ran {
                    Ok(_) => "The command passed",
                    Err(_) => "The command failed",
                };
                note(&session, said.to_string(), cx);
            }
            advance(uid, cx, move |run| run.command_finished(ran));
        });
    })
    .detach();
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
                cancel_turn(uid, cx);
                advance(uid, cx, |run| run.stopped(Stop::TimedOut));
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

/// End the run: say how in its transcript, and keep its file only when it
/// can be resumed — its agent stopped or its session went, neither of which
/// is the run's own outcome.
///
/// Taken off the list at once, so an event the ending causes finds nothing
/// to end again; dropped only once the event delivering it is over, since
/// dropping the run drops the subscription it came through.
fn finish(uid: u64, session: Option<&Entity<ChatSession>>, outcome: Outcome, cx: &mut App) {
    let driven = cx
        .has_global::<Pipelines>()
        .then(|| cx.update_global::<Pipelines, _>(|p, _| p.runs.remove(&uid)))
        .flatten();
    let Some(driven) = driven else {
        return;
    };
    let resumable = outcome.resumable();
    if let Some(session) = session {
        let said = match resumable {
            true => format!("{}; resume it from the project's page", outcome.said()),
            false => outcome.said(),
        };
        note(session, said, cx);
    }
    let file = driven.file.clone();
    let mut run = driven.run.clone();
    run.spent_secs = driven.spent_secs();
    if resumable {
        super::park(run, cx);
    } else {
        cx.update_global::<Pipelines, _>(|p, _| p.writer.send(files::FileOp::Remove(file)));
    }
    cx.defer(move |cx| {
        drop(driven);
        cx.refresh_windows();
    });
}
