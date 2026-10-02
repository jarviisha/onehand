//! An issue worked by hand in steps, in the checkout its project is open on:
//! a plan, the change, the project's check, each judged by onehand before the
//! next, with the change left uncommitted for the person who asked.
//!
//! **Not a run.** Nothing is claimed, timed out or carried across a restart,
//! and the session stays the person's: a prompt of their own takes it over,
//! and the steps stop. What is shared with a run is the judging, which is
//! core's, and the prompts.

use super::Shell;
use crate::chat::session::{ChatEvent, ChatSession};
use crate::unattended::{note, plan_passed, prompted_by_someone_else, turn_answer};
use gpui::{App, BorrowAppContext as _, Entity, Global, Subscription, WeakEntity};
use onehand_core::unattended::{
    self as core, Facts, Gate, Issue, Missing, Next, Place, Progress, Start, Step, Tracker,
};
use onehand_core::worktree;
use std::collections::HashMap;
use std::path::PathBuf;

/// Every session working an issue in steps in its checkout, by session uid.
#[derive(Default)]
pub(crate) struct HandSteps(HashMap<u64, Hand>);

impl Global for HandSteps {}

/// One session working an issue in steps.
struct Hand {
    /// The checkout, which is the project the session is on.
    root: PathBuf,
    /// Where the issue is kept, for the notes about its steps and its plan.
    tracker: Tracker,
    issue: Issue,
    /// The step, the plan, a person's note on it and their instructions.
    progress: Progress,
    /// Where the step's work is measured from: the head and the digest of
    /// the uncommitted work as the plan or the change started.
    from: Option<(String, String)>,
    /// Turns that failed their step's gate.
    missed: u32,
    /// How many prompts the steps have sent the session: any more, and the
    /// person is driving.
    sent: usize,
    /// The plan is written and waits for the person to approve it.
    awaiting: bool,
    session: WeakEntity<ChatSession>,
    shell: WeakEntity<Shell>,
    _watch: Subscription,
    _release: Subscription,
}

/// Where a session working in steps stands, as its header strip draws it.
pub(crate) struct Shown {
    /// How its issue is shown: the forge's number, or *Draft*.
    pub(crate) name: String,
    pub(crate) step: Step,
    /// Its plan waits for *Continue* or *Revise…*.
    pub(crate) awaiting: bool,
}

/// The steps a checkout takes: no pull request, since nothing is pushed.
pub(crate) const STEPS: [Step; 3] = [Step::Plan, Step::Implement, Step::Verify];

/// Act on the session `uid`'s steps, if it is working in steps.
fn with<R>(uid: u64, cx: &mut App, act: impl FnOnce(&mut Hand) -> R) -> Option<R> {
    if !cx.has_global::<HandSteps>() {
        return None;
    }
    cx.update_global::<HandSteps, _>(|hands, _| hands.0.get_mut(&uid).map(act))
}

/// Watch session `uid` on checkout `root` and work issue `issue` in it in
/// steps, starting with the plan once the agent is up. `extra` is what the
/// person asked of every step.
#[allow(clippy::too_many_arguments)]
pub(super) fn watch(
    uid: u64,
    session: &Entity<ChatSession>,
    shell: WeakEntity<Shell>,
    root: PathBuf,
    tracker: Tracker,
    issue: Issue,
    extra: Option<String>,
    cx: &mut App,
) {
    let watch = cx.subscribe(session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    let release = cx.observe_release(session, move |_, cx| {
        cx.defer(move |cx| forget(uid, cx));
    });
    let hand = Hand {
        root,
        tracker,
        issue,
        progress: Progress::new(Start::Fresh, None, extra),
        from: None,
        missed: 0,
        sent: 0,
        awaiting: false,
        session: session.downgrade(),
        shell,
        _watch: watch,
        _release: release,
    };
    cx.default_global::<HandSteps>().0.insert(uid, hand);
}

/// The agent is up and nobody has typed yet: set the mode and send the plan.
pub(super) fn begin(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let mode = crate::unattended::mode(cx);
    session.update(cx, |session, _| {
        if session.chat.modes.iter().any(|m| m.id == mode) {
            session.chat.set_mode(&mode);
        }
    });
    send_step(uid, session, Step::Plan, cx);
}

/// Where session `uid` stands, if it is working in steps.
pub(crate) fn shown(uid: u64, cx: &App) -> Option<Shown> {
    let hand = cx.try_global::<HandSteps>()?.0.get(&uid)?;
    Some(Shown {
        name: hand.tracker.shown(&hand.issue),
        step: hand.progress.step,
        awaiting: hand.awaiting,
    })
}

/// Every session working an issue in steps, for the Issues panel's rows.
pub(crate) fn by_issue(cx: &App) -> Vec<crate::unattended::IssueRun> {
    let Some(hands) = cx.try_global::<HandSteps>() else {
        return Vec::new();
    };
    let mut runs: Vec<_> = hands
        .0
        .iter()
        .map(|(uid, hand)| {
            let run = crate::unattended::IssueRun {
                repo: hand.root.clone(),
                number: hand.issue.number,
                step: hand.progress.step,
                awaiting: hand.awaiting,
                in_session: true,
            };
            (*uid, run)
        })
        .collect();
    // In the order they started, so the list compares equal while nothing
    // changed.
    runs.sort_by_key(|(uid, _)| *uid);
    runs.into_iter().map(|(_, run)| run).collect()
}

/// The plan is approved: go on to the change, carrying it.
pub(crate) fn approve(uid: u64, cx: &mut App) {
    let session = with(uid, cx, |hand| {
        let waited = std::mem::take(&mut hand.awaiting);
        hand.session.upgrade().filter(|_| waited)
    })
    .flatten();
    if let Some(session) = session {
        send_step(uid, &session, Step::Implement, cx);
    }
}

/// Send the plan back to be written again, with what to change in it.
pub(crate) fn revise(uid: u64, said: String, cx: &mut App) {
    let session = with(uid, cx, |hand| {
        let waited = std::mem::take(&mut hand.awaiting);
        hand.progress.revise = Some(said);
        hand.session.upgrade().filter(|_| waited)
    })
    .flatten();
    if let Some(session) = session {
        send_step(uid, &session, Step::Plan, cx);
    }
}

fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    let Some((sent, awaiting)) = with(uid, cx, |hand| (hand.sent, hand.awaiting)) else {
        return;
    };
    match event {
        ChatEvent::Appended => {
            if prompted_by_someone_else(session, sent, cx) {
                end(
                    uid,
                    session,
                    "Taken over by hand; onehand stopped checking the steps",
                    cx,
                );
            }
        }
        // Only a turn the steps sent is judged: the first prompt may still be
        // on its way, and a plan waiting for approval has no turn running.
        ChatEvent::TurnEnded if sent == 0 || awaiting => {}
        ChatEvent::TurnEnded => after_turn(uid, session, cx),
        // A person is at the window to answer it.
        ChatEvent::AwaitingUser(_) => {}
        ChatEvent::Disconnected => end(uid, session, "The agent stopped; so did the steps", cx),
        ChatEvent::OpenFile(_) => {}
    }
}

/// Stop working in steps, saying `said` in the transcript. Deferred, because
/// this is reached from inside the session's own event, and dropping the
/// steps drops the subscription it is delivered through.
///
/// Taken off the list at once, so the notice's own event finds nothing to
/// end a second time; only dropped once the event is over.
fn end(uid: u64, session: &Entity<ChatSession>, said: &str, cx: &mut App) {
    let hand = cx
        .try_global::<HandSteps>()
        .is_some()
        .then(|| cx.update_global::<HandSteps, _>(|hands, _| hands.0.remove(&uid)))
        .flatten();
    if let Some(hand) = hand {
        note(session, said.to_string(), cx);
        cx.defer(move |cx| {
            drop(hand);
            cx.refresh_windows();
        });
    }
}

/// Let go of session `uid`'s steps.
fn forget(uid: u64, cx: &mut App) {
    if cx.has_global::<HandSteps>()
        && cx
            .update_global::<HandSteps, _>(|hands, _| hands.0.remove(&uid))
            .is_some()
    {
        // The header strip and the Issues panel's row go with it.
        cx.refresh_windows();
    }
}

/// Measure the checkout, then send the prompt that starts `step`: the plan
/// and the change are each judged by what they changed from where they
/// started, so a checkout that was already dirty is measured fairly.
fn send_step(uid: u64, session: &Entity<ChatSession>, step: Step, cx: &mut App) {
    let Some(root) = with(uid, cx, |hand| hand.root.clone()) else {
        return;
    };
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let from = cx
            .background_executor()
            .spawn(async move {
                Ok::<_, String>((
                    worktree::head_blocking(&root)?,
                    worktree::work_digest_blocking(&root)?,
                ))
            })
            .await;
        cx.update(|cx| {
            let Some(session) = weak.upgrade() else {
                return;
            };
            let from = match from {
                Ok(from) => from,
                Err(why) => {
                    let said = format!("Steps stopped: the checkout could not be read: {why}");
                    end(uid, &session, &said, cx);
                    return;
                }
            };
            let Some(sent) = with(uid, cx, |hand| hand.sent) else {
                return;
            };
            if prompted_by_someone_else(&session, sent, cx) {
                end(
                    uid,
                    &session,
                    "Taken over by hand; onehand stopped checking the steps",
                    cx,
                );
                return;
            }
            let Some(text) = with(uid, cx, |hand| {
                hand.from = Some(from);
                hand.progress.step = step;
                hand.sent += 1;
                prompt(hand, step)
            }) else {
                return;
            };
            note_step(uid, &session, step, cx);
            if !session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
                end(
                    uid,
                    &session,
                    "Steps stopped: the agent did not take the prompt",
                    cx,
                );
            }
        });
    })
    .detach();
}

/// The prompt that starts `step`, in the checkout.
fn prompt(hand: &Hand, step: Step) -> String {
    core::step_prompt(
        step,
        &hand.issue,
        "",
        &hand.tracker,
        None,
        &hand.progress,
        None,
        Place::Checkout,
    )
}

/// Say that the session is now in `step`: in its transcript, and as a note on
/// the issue.
fn note_step(uid: u64, session: &Entity<ChatSession>, step: Step, cx: &mut App) {
    let said = format!("Step: {}", step.label());
    note(session, said.clone(), cx);
    if let Some((tracker, number)) = with(uid, cx, |h| (h.tracker.clone(), h.issue.number)) {
        cx.background_executor()
            .spawn(async move {
                if let Err(why) = tracker.note_blocking(number, &said) {
                    eprintln!("onehand: could not note a step on issue #{number}: {why}");
                }
            })
            .detach();
    }
    cx.refresh_windows();
}

/// A turn the steps sent ended: read the checkout, then judge the step.
fn after_turn(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some((root, step, from, missed, shell)) = with(uid, cx, |h| {
        (
            h.root.clone(),
            h.progress.step,
            h.from.clone(),
            h.missed,
            h.shell.clone(),
        )
    }) else {
        return;
    };
    let Some((head, digest)) = from else {
        return;
    };
    let answer = match step {
        Step::Plan => turn_answer(session, cx),
        Step::Implement | Step::Verify | Step::OpenPr => String::new(),
    };
    // Read at each turn, so a switch flipped meanwhile counts from then.
    let project = shell
        .upgrade()
        .map(|shell| shell.read(cx).project_for_runs(&root));
    let turns_left = core::turns_left(crate::unattended::turns(cx), missed);
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let facts = cx
            .background_executor()
            .spawn(async move { Facts::read_checkout_blocking(&root, &head, &digest) })
            .await;
        cx.update(|cx| {
            let Some(session) = weak.upgrade() else {
                return;
            };
            let facts = match facts {
                Ok(facts) => facts,
                Err(why) => {
                    let said = format!("Steps stopped: the checkout could not be read: {why}");
                    end(uid, &session, &said, cx);
                    return;
                }
            };
            let gate = Gate {
                committed: false,
                forge: false,
                approve_plans: project.as_ref().is_some_and(|p| p.approve_plans),
                check: project.as_ref().is_some_and(|p| p.check.is_some()),
                turns_left,
                answer: &answer,
            };
            let next = core::next(step, &facts, &gate);
            dispatch(uid, &session, next, answer, cx);
        });
    })
    .detach();
}

/// Do what the step's gate or the check decided.
fn dispatch(uid: u64, session: &Entity<ChatSession>, next: Next, answer: String, cx: &mut App) {
    let Some(step) = with(uid, cx, |hand| hand.progress.step) else {
        return;
    };
    if step == Step::Plan && plan_passed(&next) {
        keep_plan(uid, answer, cx);
    }
    match next {
        Next::CarryOn(missing) => carry_on(uid, session, missing, cx),
        Next::Advance(step) => send_step(uid, session, step, cx),
        Next::RunCheck => run_check(uid, session, cx),
        Next::AwaitApproval => {
            with(uid, cx, |hand| hand.awaiting = true);
            note(
                session,
                "Plan ready: Continue or Revise… under the header".to_string(),
                cx,
            );
            cx.refresh_windows();
        }
        // There is no pull request to wait on in a checkout.
        Next::Settle | Next::AwaitChecks => end(
            uid,
            session,
            "Steps done; the change is left uncommitted in this checkout",
            cx,
        ),
        Next::Exhausted => {
            let said = format!(
                "Steps stopped: {} turns missed, at the {} step",
                crate::unattended::turns(cx),
                step.label()
            );
            end(uid, session, &said, cx);
        }
    }
}

/// Keep the plan that passed for the change, and put it on the issue as a
/// note: it is what the work is about to be.
fn keep_plan(uid: u64, plan: String, cx: &mut App) {
    let Some((tracker, number)) = with(uid, cx, |hand| {
        hand.progress.plan = Some(plan.clone());
        hand.progress.revise = None;
        (hand.tracker.clone(), hand.issue.number)
    }) else {
        return;
    };
    let said = format!("onehand's session in this checkout wrote this plan:\n\n{plan}");
    cx.background_executor()
        .spawn(async move {
            if let Err(why) = tracker.note_blocking(number, &said) {
                eprintln!("onehand: could not note the plan on issue #{number}: {why}");
            }
        })
        .detach();
}

/// Send the session back to work, saying what the checkout still lacks.
/// Costs one of its turns.
fn carry_on(uid: u64, session: &Entity<ChatSession>, missing: Missing, cx: &mut App) {
    let Some(sent) = with(uid, cx, |hand| hand.sent) else {
        return;
    };
    if prompted_by_someone_else(session, sent, cx) {
        end(
            uid,
            session,
            "Taken over by hand; onehand stopped checking the steps",
            cx,
        );
        return;
    }
    let Some((text, missed)) = with(uid, cx, |hand| {
        hand.sent += 1;
        hand.missed += 1;
        let text = core::carry_on(&missing, hand.progress.step, None, Place::Checkout);
        (text, hand.missed)
    }) else {
        return;
    };
    if session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        note(
            session,
            format!("The step is not done yet; asked to carry on ({missed} missed)"),
            cx,
        );
    } else {
        end(
            uid,
            session,
            "Steps stopped: the agent did not take the prompt",
            cx,
        );
    }
}

/// Run the project's check command in the checkout, then finish or hand what
/// it said back to the session.
fn run_check(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some((root, shell)) = with(uid, cx, |hand| {
        hand.progress.step = Step::Verify;
        (hand.root.clone(), hand.shell.clone())
    }) else {
        return;
    };
    note_step(uid, session, Step::Verify, cx);
    let command = shell
        .upgrade()
        .and_then(|shell| shell.read(cx).project_for_runs(&root).check);
    let Some(command) = command else {
        // Switched off since the gate read it: nothing to run.
        dispatch(
            uid,
            session,
            core::after_check(Ok(()), 0, false),
            String::new(),
            cx,
        );
        return;
    };
    note(session, format!("Running the check: {command}"), cx);
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let ran = cx
            .background_executor()
            .spawn(async move { core::verify_blocking(&root, &command) })
            .await;
        cx.update(|cx| {
            let Some(session) = weak.upgrade() else {
                return;
            };
            let max = crate::unattended::turns(cx);
            let Some(turns_left) = with(uid, cx, |hand| core::turns_left(max, hand.missed)) else {
                return;
            };
            let said = match &ran {
                Ok(()) => "The check passed",
                Err(_) => "The check failed",
            };
            note(&session, said.to_string(), cx);
            let next = core::after_check(ran, turns_left, false);
            dispatch(uid, &session, next, String::new(), cx);
        });
    })
    .detach();
}
