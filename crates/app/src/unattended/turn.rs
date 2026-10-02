use super::launch::{Claimed, save, save_record};
use super::{Parked, Run, WIND_DOWN, tick, with};
use crate::chat::session::{ChatEvent, ChatSession};
use crate::state::Shared;
use gpui::{App, Entity, Task, WeakEntity};
use onehand_core::chat::UserAsk;
use onehand_core::chat::{ChatItem, TranscriptItemId};
use onehand_core::unattended::{
    self as core, Ending, Facts, Gate, Missing, Next, Phase, Spent, Start, Step, Tracker, Verdict,
};
use onehand_core::worktree;
use std::time::{Duration, Instant};

/// One event from the run's session.
pub(super) fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    let Some((sent, cancelling, waiting)) = with(cx, |u| {
        u.run_mut(uid)
            .map(|run| (run.sent, run.ending.is_some(), run.waiting.clone()))
    })
    .flatten() else {
        return;
    };
    match event {
        ChatEvent::Appended if sent == 0 => prompt(uid, session, cx),
        ChatEvent::Appended => {
            if prompted_by_someone_else(session, sent, cx) {
                settle(uid, Ending::TakenOver, cx);
            }
        }
        ChatEvent::TurnEnded if cancelling => settle_pending(uid, cx),
        ChatEvent::TurnEnded => after_turn(uid, session, cx),
        // **The card is left up for a person**, wherever they answer it from —
        // this window, the desktop notification, a chat on the remote bridge.
        // The run never answers a card itself, and it no longer throws one
        // away: a run that needed one decision is worth more waiting for it
        // than cancelled over it. Answering is not taking over; the turn simply
        // carries on.
        ChatEvent::AwaitingUser(ask) if !cancelling => {
            let asked = question(*ask, session, cx);
            wait(uid, asked, session, cx);
        }
        ChatEvent::AwaitingUser(_) => {}
        ChatEvent::Disconnected if cancelling => settle_pending(uid, cx),
        // A card goes with its adapter, so a run lost while waiting ends on the
        // question it was waiting on: that is what the issue needs to hear.
        ChatEvent::Disconnected => settle(uid, waiting.map_or(Ending::LinkLost, Ending::Asked), cx),
        ChatEvent::OpenFile(_) => {}
    }
}

/// The run's timeout: cancel it when `left` has run out. `limit` is the whole
/// timeout, which is what the issue is told was hit.
pub(super) fn clock(uid: u64, left: Duration, limit: Duration, cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(left).await;
        cx.update(|cx| cancel_toward(uid, Ending::TimedOut(limit), cx));
    })
}

/// The run has parked a card: stop its clock, give up the slot, and let the
/// search look for the next issue at once rather than at the next tick.
fn wait(uid: u64, asked: String, session: &Entity<ChatSession>, cx: &mut App) {
    let first = with(cx, |u| {
        let run = u.run_mut(uid)?;
        run.budget.pause(Instant::now());
        run._clock = Task::ready(());
        Some(run.waiting.replace(asked.clone()).is_none())
    })
    .flatten();
    if first != Some(true) {
        return;
    }
    note(session, format!("Waiting for an answer: {asked}"), cx);
    // The rail's project row says a run is waiting; nothing it watches moved.
    cx.refresh_windows();
    tick(None, cx);
}

/// No card the run parked is waiting any more: start its clock again from
/// where it stopped.
///
/// "Settled" rather than "answered", because a person pressing Stop settles
/// the cards too; the turn ending right after is what ends the run then.
pub(super) fn resume(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some((left, limit)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        run.waiting = None;
        let now = Instant::now();
        run.budget.resume(now);
        Some((run.budget.left(now), run.budget.limit()))
    })
    .flatten() else {
        return;
    };
    let ticking = clock(uid, left, limit, cx);
    with(cx, |u| {
        if let Some(run) = u.run_mut(uid) {
            run._clock = ticking;
        }
    });
    // Only what is known: after a Stop the turn ends next, so "carries on"
    // would be the transcript's last word on a run that did not.
    note(
        session,
        "Card settled; the clock runs again".to_string(),
        cx,
    );
    cx.refresh_windows();
}

/// What a run ends as if it has to end now without a turn ending: what a
/// cancel was heading for, or the question it was waiting on.
pub(super) fn pending_ending(run: &Run) -> Option<Ending> {
    run.ending
        .clone()
        .or_else(|| run.waiting.clone().map(Ending::Asked))
}

/// The longest plan read from a turn's answer. A plan is a few paragraphs; an
/// answer the length of a book is cut rather than carried whole into the
/// record and onto the issue.
const PLAN_MAX: usize = 20_000;

/// The whole of the last turn's answer, cut at [`PLAN_MAX`] characters.
fn turn_answer(session: &Entity<ChatSession>, cx: &App) -> String {
    let chat = &session.read(cx).chat;
    let last = chat.items.len().saturating_sub(1);
    let prose = chat.turn_prose(TranscriptItemId::Live(last));
    match prose.char_indices().nth(PLAN_MAX) {
        Some((cut, _)) => format!("{}\n\n(cut at {PLAN_MAX} characters)", &prose[..cut]),
        None => prose,
    }
}

/// A turn ended by itself: read what the branch holds, then judge the step
/// the run is in.
///
/// **The branch is the judge, not the answer.** A turn that only analysed, or
/// that says it opened a pull request it never opened, is sent back to work
/// with what is missing named. The one thing read from the answer is the
/// plan, which is nowhere else.
fn after_turn(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let shell = with(cx, |u| u.run_mut(uid).map(|run| run.shell.clone())).flatten();
    let tail = shell
        .and_then(|s| s.upgrade())
        .and_then(|s| s.read(cx).answer_tail(uid, cx));
    let Some((repo, step)) = with(cx, |u| {
        u.run_mut(uid)
            .map(|run| (run.claimed.repo.clone(), run.progress.step))
    })
    .flatten() else {
        return;
    };
    let answer = match step {
        Step::Plan => turn_answer(session, cx),
        Step::Implement | Step::Verify | Step::OpenPr => String::new(),
    };
    let project = project_of(&repo, cx);
    let Some((dir, since, branch, forge, turns_left, from)) = with(cx, |u| {
        let max = u.turns;
        let run = u.run_mut(uid)?;
        let c = &run.claimed;
        // A plan is measured from where its session found the worktree, so
        // work an earlier attempt left there is not taken for the plan's.
        let from = run.plan_from.clone().filter(|_| step == Step::Plan);
        // A plan whose starting point could not be read is judged on its
        // answer alone: measured from the base instead, an earlier attempt's
        // commits would fail every plan turn until the run was spent.
        let since = match (&from, step) {
            (Some((head, _)), _) => head.clone(),
            (None, Step::Plan) => "HEAD".to_string(),
            (None, _) => run.progress.since.clone().unwrap_or_else(|| c.base.clone()),
        };
        Some((
            c.dir.clone(),
            since,
            c.branch.clone(),
            c.forge,
            core::turns_left(max, run.missed),
            from,
        ))
    })
    .flatten() else {
        return;
    };
    let session = session.downgrade();
    cx.spawn(async move |cx| {
        let facts = cx
            .background_executor()
            .spawn(async move { Facts::read_blocking(&dir, &since, &repo, &branch, forge) })
            .await;
        cx.update(|cx| {
            let Some(session) = still_going(uid, &session, cx) else {
                return;
            };
            let next = match facts {
                Ok(mut facts) => {
                    // ponytail: a worktree already dirty when the plan began
                    // cannot tell the plan's edits from the earlier ones; a
                    // content hash of the worktree would.
                    facts.dirty &= match (&from, step) {
                        (Some((_, dirty)), _) => !dirty,
                        (None, Step::Plan) => false,
                        (None, _) => true,
                    };
                    let gate = Gate {
                        forge: forge.is_some(),
                        approve_plans: project.as_ref().is_some_and(|p| p.approve_plans),
                        check: project.as_ref().is_some_and(|p| p.check.is_some()),
                        turns_left,
                        answer: &answer,
                    };
                    core::next(step, &facts, &gate)
                }
                Err(why) => {
                    eprintln!("onehand: could not read what a run left: {why}");
                    Next::Settle
                }
            };
            dispatch(uid, &session, next, tail, answer, cx);
        });
    })
    .detach();
}

/// The run's session, if the run is still going: taken over, cancelled or
/// gone while something was read has already decided how it ends.
fn still_going(
    uid: u64,
    session: &WeakEntity<ChatSession>,
    cx: &mut App,
) -> Option<Entity<ChatSession>> {
    let going = with(cx, |u| u.run_mut(uid).map(|run| run.ending.is_none())).flatten();
    session.upgrade().filter(|_| going == Some(true))
}

/// The project `repo` as a window holding it sees it, for how its runs are
/// gated: read at each turn, so a switch flipped mid-run counts from then.
fn project_of(repo: &std::path::Path, cx: &App) -> Option<super::Project> {
    Shared::global(cx)
        .windows
        .iter()
        .filter_map(|w| w.shell.upgrade())
        .find(|s| s.read(cx).holds_root(repo))
        .map(|s| s.read(cx).project_for_runs(repo))
}

/// Do what a step's gate or the check decided. `answer` is the plan when the
/// step was the plan.
fn dispatch(
    uid: u64,
    session: &Entity<ChatSession>,
    next: Next,
    tail: Option<String>,
    answer: String,
    cx: &mut App,
) {
    let step = with(cx, |u| u.run_mut(uid).map(|run| run.progress.step)).flatten();
    if step == Some(Step::Plan) && plan_passed(&next) {
        keep_plan(uid, answer, next == Next::AwaitApproval, cx);
    }
    match next {
        Next::CarryOn(missing) => carry_on(uid, session, missing, tail, cx),
        Next::Advance(step) => advance(uid, session, step, None, tail, cx),
        Next::RunCheck => run_check(uid, session, tail, cx),
        Next::AwaitApproval => park(
            uid,
            Phase::AwaitingApproval {
                since: onehand_core::chat::store::now_secs(),
            },
            "Plan written; waiting for it to be approved",
            cx,
        ),
        Next::AwaitChecks => park(
            uid,
            Phase::AwaitingChecks {
                since: onehand_core::chat::store::now_secs(),
            },
            "Pushed to its pull request; waiting for its checks",
            cx,
        ),
        Next::Settle => settle(uid, Ending::TurnEnded { tail }, cx),
        Next::Exhausted => {
            let max = with(cx, |u| u.turns).unwrap_or_default();
            let step = step.unwrap_or_default();
            settle(uid, Ending::Exhausted(Spent::Turns(max, step)), cx)
        }
    }
}

/// Whether the plan step's gate let the plan through.
fn plan_passed(next: &Next) -> bool {
    match next {
        Next::Advance(_) | Next::AwaitApproval => true,
        Next::CarryOn(_) | Next::RunCheck | Next::AwaitChecks | Next::Settle | Next::Exhausted => {
            false
        }
    }
}

/// Keep the plan that passed, for a later session of the attempt, and post it
/// on the issue whether or not it waits for approval: it is what the work is
/// about to be.
fn keep_plan(uid: u64, plan: String, awaiting: bool, cx: &mut App) {
    let Some((tracker, repo, number)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        run.progress.plan = Some(plan.clone());
        run.progress.revise = None;
        let c = &run.claimed;
        Some((c.tracker.clone(), c.repo.clone(), c.issue.number))
    })
    .flatten() else {
        return;
    };
    let after = match awaiting {
        true => "\n\nIt waits for approval in onehand's Issues panel before any code changes.",
        false => "",
    };
    let said = format!("onehand's run wrote this plan:\n\n{plan}{after}");
    cx.background_executor()
        .spawn(async move { tell_issue(&tracker, &repo, number, &said) })
        .detach();
}

/// Move the run on to `step` in the same session, saying so in its record, on
/// the issue and in the transcript. `failed` is the check's output, for a
/// session that starts by fixing it.
fn advance(
    uid: u64,
    session: &Entity<ChatSession>,
    step: Step,
    failed: Option<&str>,
    tail: Option<String>,
    cx: &mut App,
) {
    let sent = with(cx, |u| u.run_mut(uid).map(|run| run.sent)).flatten();
    if sent.is_none_or(|sent| prompted_by_someone_else(session, sent, cx)) {
        settle(uid, Ending::TakenOver, cx);
        return;
    }
    let Some((text, file, record)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        run.sent += 1;
        run.progress.step = step;
        // The plan passed, so the head is still where the plan found it.
        if step == Step::Implement
            && counts_own_commits(&run.progress.start)
            && let Some((head, _)) = &run.plan_from
        {
            run.progress.since = Some(head.clone());
        }
        let record = enter_step(run);
        let c = &run.claimed;
        let text = core::step_prompt(
            step,
            &c.issue,
            &c.branch,
            &c.tracker,
            c.forge,
            &run.progress,
            failed,
        );
        Some((text, c.file.clone(), record))
    })
    .flatten() else {
        return;
    };
    save_record(file, record, cx);
    note_step(uid, session, step, cx);
    if !session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        settle(uid, Ending::TurnEnded { tail }, cx);
    }
}

/// Whether a change made in this attempt has to be a commit of its own,
/// counted from where the change step started.
///
/// **Not for a repair**: it is told to leave the code alone when a failure is
/// not its change's, and a gate demanding a commit would send it back to
/// change code anyway. Every other start has to add work, or an earlier
/// attempt's commits pass the gate on a turn that only read the code.
fn counts_own_commits(start: &Start) -> bool {
    match start {
        Start::Repair { .. } => false,
        Start::Fresh | Start::Earlier | Start::Review { .. } => true,
    }
}

/// The run's record as it stands, with the time spent so far counted in.
fn enter_step(run: &mut Run) -> onehand_core::unattended::Record {
    run.progress.spent_secs = run.budget.spent(Instant::now()).as_secs();
    run.claimed.record(Phase::Working, &run.progress)
}

/// Say that the run is now in `step`: in its transcript, and on an issue kept
/// in onehand. A forge's issue is not told, because a comment per step is
/// noise to whoever watches it there.
fn note_step(uid: u64, session: &Entity<ChatSession>, step: Step, cx: &mut App) {
    note(session, format!("Step: {}", step.label()), cx);
    if let Some((tracker, number)) = with(cx, |u| {
        u.run_mut(uid)
            .map(|run| (run.claimed.tracker.clone(), run.claimed.issue.number))
    })
    .flatten()
    {
        let said = format!("Step: {}", step.label());
        cx.background_executor()
            .spawn(async move {
                if let Err(why) = tracker.note_blocking(number, &said) {
                    eprintln!("onehand: could not note a step on issue #{number}: {why}");
                }
            })
            .detach();
    }
    // The rail and the Issues panel show the step.
    cx.refresh_windows();
}

/// Run the project's check command on the run's worktree, then move on or
/// hand what it said back to the session.
fn run_check(uid: u64, session: &Entity<ChatSession>, tail: Option<String>, cx: &mut App) {
    let Some((dir, repo, forge, file, record)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        run.progress.step = Step::Verify;
        let record = enter_step(run);
        let c = &run.claimed;
        Some((
            c.dir.clone(),
            c.repo.clone(),
            c.forge.is_some(),
            c.file.clone(),
            record,
        ))
    })
    .flatten() else {
        return;
    };
    save_record(file, record, cx);
    note_step(uid, session, Step::Verify, cx);
    let Some(command) = project_of(&repo, cx).and_then(|p| p.check) else {
        // Switched off since the gate read it: nothing to run.
        let next = core::after_check(Ok(()), 0, forge);
        dispatch(uid, session, next, tail, String::new(), cx);
        return;
    };
    note(session, format!("Running the check: {command}"), cx);
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let ran = cx
            .background_executor()
            .spawn(async move { core::verify_blocking(&dir, &command) })
            .await;
        cx.update(|cx| {
            let Some(session) = still_going(uid, &weak, cx) else {
                return;
            };
            let Some(turns_left) = with(cx, |u| {
                let max = u.turns;
                u.run_mut(uid).map(|run| core::turns_left(max, run.missed))
            })
            .flatten() else {
                return;
            };
            let said = match &ran {
                Ok(()) => "The check passed".to_string(),
                Err(_) => "The check failed".to_string(),
            };
            note(&session, said, cx);
            let next = core::after_check(ran, turns_left, forge);
            dispatch(uid, &session, next, tail, String::new(), cx);
        });
    })
    .detach();
}

/// Send the session back to work, saying what the branch still lacks. Costs
/// the run one of its turns.
fn carry_on(
    uid: u64,
    session: &Entity<ChatSession>,
    missing: Missing,
    tail: Option<String>,
    cx: &mut App,
) {
    // Somebody typed while the branch was read: the session is theirs.
    let sent = with(cx, |u| u.run_mut(uid).map(|run| run.sent)).flatten();
    if sent.is_none_or(|sent| prompted_by_someone_else(session, sent, cx)) {
        settle(uid, Ending::TakenOver, cx);
        return;
    }
    let Some((text, missed, file, record)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        // Counted before it goes, so the prompt is never mistaken for a
        // person's when its own events arrive.
        run.sent += 1;
        run.missed += 1;
        let record = enter_step(run);
        let text = core::carry_on(&missing, run.progress.step, run.claimed.forge);
        Some((text, run.missed, run.claimed.file.clone(), record))
    })
    .flatten() else {
        return;
    };
    save_record(file, record, cx);
    let lacking = match missing {
        Missing::Uncommitted => "uncommitted changes",
        Missing::NoCommits => "no commit",
        Missing::NoPullRequest => "no pull request",
        Missing::Unpushed => "commits not pushed",
        Missing::NoPlan => "no plan",
        Missing::PlanTouchedCode => "a plan that changed code",
        Missing::CheckFailed(_) => "a failing check",
    };
    if session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        note(
            session,
            format!("Turn left {lacking}; asked to carry on ({missed} missed)"),
            cx,
        );
    } else {
        settle(uid, Ending::TurnEnded { tail }, cx);
    }
}

/// Close the session, give up the slot, and wait in `phase`: on the pull
/// request's checks, which the tick polls, or on a person approving the plan.
///
/// **Deferred**, like [`settle`], because this is reached from inside the
/// session's own event.
fn park(uid: u64, phase: Phase, said: &'static str, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(run) = with(cx, |u| {
            let at = u.runs.iter().position(|run| run.uid == uid)?;
            Some(u.runs.remove(at))
        })
        .flatten() else {
            return;
        };
        let owns_root = run.owns_root;
        let Run {
            claimed,
            mut progress,
            budget,
            session,
            window,
            shell,
            ..
        } = run;
        progress.spent_secs = budget.spent(Instant::now()).as_secs();
        save(&claimed, phase, &progress, cx);
        if let Some(session) = session.upgrade() {
            note(&session, said.to_string(), cx);
        }
        take_down(&claimed, false, owns_root, window, &shell, cx);
        with(cx, |u| {
            u.parked.push(Parked {
                claimed,
                phase,
                progress,
            })
        });
        cx.refresh_windows();
    });
}

/// Send the session's first prompt, once the adapter is up.
///
/// Not before: setting the mode and submitting both need the request channel
/// the handshake installs. The modes arrive ahead of that same event, so by
/// the time the link is up the mode can be checked against what is offered.
///
/// A plan first notes where the worktree stands, so its gate can tell what
/// the plan changed; a session resumed at the check runs the check first, and
/// starts on what it reported or moves straight on.
fn prompt(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    if session.read(cx).chat.link != onehand_core::chat::Link::Connected {
        return;
    }
    // Somebody got there first: a prompt typed between the adapter coming up
    // and this one going out makes the session theirs, and the run's own
    // prompt would either be refused as busy or land on top of their work.
    if session.read(cx).chat.busy || prompted_by_someone_else(session, 0, cx) {
        settle(uid, Ending::TakenOver, cx);
        return;
    }
    let Some((mode, step, dir, repo, own)) = with(cx, |u| {
        let mode = u.mode.clone();
        let run = u.run_mut(uid)?;
        // Taken now, so the events that arrive while the worktree is read are
        // not taken for the adapter coming up again.
        run.sent = 1;
        Some((
            mode,
            run.progress.step,
            run.claimed.dir.clone(),
            run.claimed.repo.clone(),
            counts_own_commits(&run.progress.start),
        ))
    })
    .flatten() else {
        return;
    };
    let offered: Vec<String> = session
        .read(cx)
        .chat
        .modes
        .iter()
        .map(|m| m.id.clone())
        .collect();
    if !offered.contains(&mode) {
        // Every run would fail this way, each on a fresh issue, so the tick
        // stops here rather than working through the backlog to say it.
        let why = format!(
            "the agent offers no mode `{mode}` (it offers: {}). Unattended runs are paused \
             until another mode is chosen in Settings, under Workspace.",
            offered.join(", ")
        );
        with(cx, |u| u.mode_refused = Some(why.clone()));
        eprintln!("onehand: {why}");
        settle(uid, Ending::Failed(why), cx);
        return;
    }
    session.update(cx, |session, _| session.chat.set_mode(&mode));
    // A check command removed since the run stopped leaves nothing to fail.
    let check = match step {
        Step::Verify => Some(project_of(&repo, cx).and_then(|p| p.check)),
        Step::Plan | Step::Implement | Step::OpenPr => None,
    };
    if check.is_some() {
        note(
            session,
            "Resuming at the check; running it first".to_string(),
            cx,
        );
    }
    let weak = session.downgrade();
    cx.spawn(async move |cx| {
        let (from, since, ran) = cx
            .background_executor()
            .spawn(async move {
                let from = (step == Step::Plan)
                    .then(|| {
                        Some((
                            worktree::head_blocking(&dir).ok()?,
                            worktree::dirty_blocking(&dir).ok()?,
                        ))
                    })
                    .flatten();
                let since = (step == Step::Implement && own)
                    .then(|| worktree::head_blocking(&dir).ok())
                    .flatten();
                let ran = check.map(|command| {
                    command.map_or(Ok(()), |command| core::verify_blocking(&dir, &command))
                });
                (from, since, ran)
            })
            .await;
        cx.update(|cx| {
            let Some(session) = still_going(uid, &weak, cx) else {
                return;
            };
            if prompted_by_someone_else(&session, 0, cx) {
                settle(uid, Ending::TakenOver, cx);
                return;
            }
            let counted = with(cx, |u| {
                let run = u.run_mut(uid)?;
                run.plan_from = from;
                let counted = since.map(|head| {
                    run.progress.since = Some(head);
                    (run.claimed.file.clone(), enter_step(run))
                });
                Some((run.claimed.forge.is_some(), counted))
            })
            .flatten();
            let (forge, counted) = counted.unwrap_or_default();
            if let Some((file, record)) = counted {
                save_record(file, record, cx);
            }
            let (step, failed) = match ran {
                None => (step, None),
                Some(Err(tail)) => (Step::Verify, Some(tail)),
                Some(Ok(())) if forge => (Step::OpenPr, None),
                // Nothing left to do: the change is committed and checked,
                // and with no forge the branch is the result.
                Some(Ok(())) => {
                    settle(uid, Ending::TurnEnded { tail: None }, cx);
                    return;
                }
            };
            first_prompt(uid, &session, step, failed.as_deref(), &mode, cx);
        });
    })
    .detach();
}

/// Submit the session's first prompt, for `step`.
fn first_prompt(
    uid: u64,
    session: &Entity<ChatSession>,
    step: Step,
    failed: Option<&str>,
    mode: &str,
    cx: &mut App,
) {
    let Some((text, moved)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        let moved = (run.progress.step != step).then(|| {
            run.progress.step = step;
            enter_step(run)
        });
        let c = &run.claimed;
        let text = core::step_prompt(
            step,
            &c.issue,
            &c.branch,
            &c.tracker,
            c.forge,
            &run.progress,
            failed,
        );
        Some((text, moved.map(|record| (c.file.clone(), record))))
    })
    .flatten() else {
        return;
    };
    if let Some((file, record)) = moved {
        save_record(file, record, cx);
        note_step(uid, session, step, cx);
    }
    if session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        note(
            session,
            format!(
                "Mode {mode} set; issue sent as the prompt, at its {} step",
                step.label()
            ),
            cx,
        );
        name_session(uid, session, cx);
    } else {
        settle(
            uid,
            Ending::Failed("the agent did not accept the prompt".to_string()),
            cx,
        );
    }
}

/// Say on an issue kept in onehand which conversation the run is working it
/// in, so the issue can open it — the run's own notes name the worktree and
/// the outcome, and neither is a way back to what the agent said. An issue
/// that lives only on the forge has nowhere to keep it.
fn name_session(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let Some(id) = session.read(cx).chat.session_id.clone() else {
        return;
    };
    let Some((tracker, number, dir, branch)) = with(cx, |u| {
        u.run_mut(uid).map(|run| {
            (
                run.claimed.tracker.clone(),
                run.claimed.issue.number,
                run.claimed.dir.clone(),
                run.claimed.branch.clone(),
            )
        })
    })
    .flatten() else {
        return;
    };
    let (Tracker::Local(file) | Tracker::Synced { file, .. }) = tracker else {
        return;
    };
    cx.background_executor()
        .spawn(async move {
            let said = format!(
                "Taken up by an unattended run in {}, on branch {branch}",
                dir.display()
            );
            let done = onehand_core::issues::taken_up_blocking(&file, number, &said, id);
            if let Err(why) = done {
                eprintln!("onehand: could not note the session on issue: {why}");
            }
        })
        .detach();
}

/// What the run's transcript opens with: which issue, how it was chosen, and
/// where the work is happening. The transcript is the record of the run, and
/// without these lines it would start at a prompt nobody on screen typed.
///
/// **Short lines, one fact each.** A remark in the transcript is one line down
/// the middle of the column and is cut where the column ends, so a sentence
/// carrying the issue, the folder and the branch lost all but the first.
pub(super) fn start_notes(claimed: &Claimed, start: &Start) -> Vec<String> {
    let folder = claimed
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| claimed.dir.display().to_string());
    let what = match start {
        Start::Fresh => None,
        Start::Earlier => Some("Carrying on from an earlier attempt".to_string()),
        Start::Review { pr, .. } => Some(format!("Answering the review on {pr}")),
        Start::Repair { pr, .. } => Some(format!("Repairing {pr}")),
    };
    let mut notes = vec![
        format!(
            "Unattended run on issue #{}, {}",
            claimed.issue.number,
            if claimed.picked_by_hand() {
                "picked by hand"
            } else {
                "found by its label"
            }
        ),
        format!("Branch {} from {}", claimed.branch, claimed.base),
        format!("Working in {folder}"),
    ];
    notes.extend(what);
    notes
}

/// Add a line about the run to its session's transcript.
pub(super) fn note(session: &Entity<ChatSession>, text: String, cx: &mut App) {
    session.update(cx, |session, cx| {
        session.chat.items.push(ChatItem::notice(text));
        cx.emit(ChatEvent::Appended);
        cx.notify();
    });
}

/// Whether somebody other than the run has put a prompt into the session,
/// given how many the run has sent itself.
///
/// Any other prompt sent, or one waiting behind the turn, came from the
/// composer or the remote bridge — and either way a person is driving. Counted
/// from what was *sent*, not from the user rows in the transcript: an adapter
/// delivers user chunks of its own mid-turn, and reading those as prompts took
/// runs over that nobody had touched.
fn prompted_by_someone_else(session: &Entity<ChatSession>, sent: usize, cx: &App) -> bool {
    let chat = &session.read(cx).chat;
    chat.queued.is_some() || chat.prompts_sent > sent
}

/// What the parked card asks, in its own words.
fn question(ask: UserAsk, session: &Entity<ChatSession>, cx: &App) -> String {
    let chat = &session.read(cx).chat;
    let asked = match ask {
        UserAsk::Permission => chat
            .pending_permissions()
            .last()
            .map(|(_, p)| p.req.title.clone()),
        UserAsk::Question => chat
            .pending_asks()
            .last()
            .map(|(_, a)| a.req.message.clone()),
    };
    asked.unwrap_or_else(|| ask.headline("The agent"))
}

/// Cancel the run's turn and end as `ending` once it has wound down.
///
/// A session that is already gone — its window closed under it — has nothing
/// to wind down, and is settled on the spot.
fn cancel_toward(uid: u64, ending: Ending, cx: &mut App) {
    let Some(session) = with(cx, |u| {
        let run = u.run_mut(uid).filter(|run| run.ending.is_none())?;
        run.ending = Some(ending.clone());
        Some(run.session.clone())
    })
    .flatten() else {
        return;
    };
    let Some(session) = session.upgrade() else {
        settle(uid, ending, cx);
        return;
    };
    session.update(cx, |session, cx| {
        session.chat.cancel_turn();
        cx.notify();
    });
    let why = match &ending {
        Ending::TimedOut(_) => "the run timed out",
        _ => "the run is ending",
    };
    note(&session, format!("Cancelling the turn: {why}"), cx);
    let wind_down = cx.spawn(async move |cx| {
        cx.background_executor().timer(WIND_DOWN).await;
        cx.update(|cx| settle_pending(uid, cx));
    });
    with(cx, |u| {
        if let Some(run) = u.run_mut(uid) {
            run._clock = wind_down;
        }
    });
}

/// Settle as whatever the cancel was heading toward.
///
/// Only a cancel's ending, unlike [`pending_ending`]: this is reached when a
/// cancelled turn has wound down, and a run can be cancelled only while it is
/// working, never while it waits on a card.
fn settle_pending(uid: u64, cx: &mut App) {
    let pending = with(cx, |u| u.run_mut(uid).and_then(|run| run.ending.clone())).flatten();
    if let Some(ending) = pending {
        settle(uid, ending, cx);
    }
}

/// End the run: close or keep its session, then tell the issue.
///
/// **Deferred**, because this is reached from inside the session's own event
/// and closing the session drops the subscription that event is being
/// delivered through.
pub(super) fn settle(uid: u64, ending: Ending, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(run) = with(cx, |u| {
            let at = u.runs.iter().position(|run| run.uid == uid)?;
            Some(u.runs.remove(at))
        })
        .flatten() else {
            return;
        };
        cx.refresh_windows();
        let owns_root = run.owns_root;
        let Run {
            claimed,
            window,
            shell,
            session,
            ..
        } = run;
        // A run picked by hand stays where the person watching it can read how
        // it ended, and is kept for good like a taken-over one — somebody who
        // watched it end may well carry on in it, and a project that vanished
        // at the next launch would take their place in it too. Only a found
        // run is taken down.
        take_down(
            &claimed,
            ending == Ending::TakenOver,
            owns_root,
            window,
            &shell,
            cx,
        );
        conclude(claimed, ending, Some(session), cx);
    });
}

/// Keep the run's project for good, or drop it. A project the run did not
/// add itself — a worktree somebody kept from an earlier attempt — is
/// somebody's, and is never dropped.
fn take_down(
    claimed: &Claimed,
    keep: bool,
    owns_root: bool,
    window: gpui::AnyWindowHandle,
    shell: &WeakEntity<crate::shell::Shell>,
    cx: &mut App,
) {
    let keep = keep || claimed.picked_by_hand();
    let Some(shell) = shell.upgrade() else {
        return;
    };
    let _ = window.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| match (keep, owns_root) {
            (true, _) => shell.adopt_unattended(&claimed.dir, window, cx),
            (false, true) => shell.end_unattended(&claimed.dir, window, cx),
            (false, false) => {}
        })
    });
}

/// Tell the issue how the run ended, let go of its file, and put the outcome
/// as the last line of `session`'s transcript, if there is one.
pub(super) fn conclude(
    claimed: Claimed,
    ending: Ending,
    session: Option<WeakEntity<ChatSession>>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let line = cx
            .background_executor()
            .spawn(async move {
                let found = verdict_blocking(&claimed, &ending);
                let said = core::report(&ending, &found, &claimed.branch);
                tell_issue(&claimed.tracker, &claimed.repo, claimed.issue.number, &said);
                // ponytail: removed after the forge calls above, which leaves a
                // save of the same file queued just before them seconds to
                // land first. A save landing after this would bring the run
                // back at the next launch; a per-run write queue is the fix if
                // that is ever seen.
                if let Err(why) = std::fs::remove_file(&claimed.file)
                    && why.kind() != std::io::ErrorKind::NotFound
                {
                    eprintln!("onehand: could not remove a finished run's file: {why}");
                }
                core::outcome_line(&ending, &found)
            })
            .await;
        // How it ended, in the one line a remark gets, as the transcript's
        // last — where the session is still there to carry it. The whole
        // account is the comment on the issue.
        cx.update(|cx| {
            if let Some(session) = session.and_then(|s| s.upgrade()) {
                note(&session, line, cx);
            }
        });
    })
    .detach();
}

/// What the run left: the pull request on its branch where the project has a
/// forge, its commits past where it started where it has none. Blocking.
fn verdict_blocking(claimed: &Claimed, ending: &Ending) -> Result<Verdict, String> {
    if !ending.may_have_work() {
        return Ok(match claimed.forge {
            Some(_) => Verdict::NoPullRequest,
            None => Verdict::NoCommits,
        });
    }
    match claimed.forge {
        Some(forge) => forge
            .pull_request_for_blocking(&claimed.repo, &claimed.branch)
            .map(|pr| pr.map_or(Verdict::NoPullRequest, |pr| Verdict::PullRequest(pr.url))),
        None => worktree::commits_since_blocking(&claimed.dir, &claimed.base).map(|n| match n {
            0 => Verdict::NoCommits,
            n => Verdict::Commits(n),
        }),
    }
}

/// Leave `body` on issue `number`, saying on stderr if that failed — there is
/// nowhere else left to say it.
pub(super) fn tell_issue(tracker: &Tracker, repo: &std::path::Path, number: u64, body: &str) {
    if let Err(why) = tracker.comment_blocking(repo, number, body) {
        eprintln!("onehand: could not comment on issue #{number}: {why}");
    }
}
