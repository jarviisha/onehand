use super::launch::{Claimed, save, save_record};
use super::{Parked, Run, WIND_DOWN, tick, with};
use crate::chat::session::{ChatEvent, ChatSession};
use gpui::{App, Entity, Task, WeakEntity};
use onehand_core::chat::ChatItem;
use onehand_core::chat::UserAsk;
use onehand_core::unattended::{
    self as core, Ending, Facts, Missing, Next, Phase, Spent, Start, Tracker, Verdict,
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

/// A turn ended by itself: read what the branch holds, then carry on, wait
/// for checks, or end.
///
/// **The branch is the judge, not the answer.** A turn that only analysed, or
/// that says it opened a pull request it never opened, is sent back to work
/// with what is missing named; only commits pushed to an open pull request
/// move the run on.
fn after_turn(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let shell = with(cx, |u| u.run_mut(uid).map(|run| run.shell.clone())).flatten();
    let tail = shell
        .and_then(|s| s.upgrade())
        .and_then(|s| s.read(cx).answer_tail(uid, cx));
    let Some((dir, since, repo, branch, forge, turns, max)) = with(cx, |u| {
        let max = u.turns;
        let run = u.run_mut(uid)?;
        run.turns += 1;
        let c = &run.claimed;
        Some((
            c.dir.clone(),
            run.progress.since.clone().unwrap_or_else(|| c.base.clone()),
            c.repo.clone(),
            c.branch.clone(),
            c.forge,
            run.turns,
            max,
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
            // Taken over, cancelled or gone while the branch was read: that
            // has already decided how the run ends.
            let going = with(cx, |u| u.run_mut(uid).map(|run| run.ending.is_none())).flatten();
            let Some(session) = session.upgrade().filter(|_| going == Some(true)) else {
                return;
            };
            let next = match facts {
                Ok(facts) => core::after_turn(&facts, forge.is_some(), max.saturating_sub(turns)),
                Err(why) => {
                    eprintln!("onehand: could not read what a run left: {why}");
                    Next::Settle
                }
            };
            match next {
                Next::CarryOn(missing) => carry_on(uid, &session, missing, tail, cx),
                Next::AwaitChecks => park(uid, cx),
                Next::Settle => settle(uid, Ending::TurnEnded { tail }, cx),
                Next::Exhausted => settle(uid, Ending::Exhausted(Spent::Turns(max)), cx),
            }
        });
    })
    .detach();
}

/// Send the session back to work, saying what the branch still lacks.
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
    let Some((text, turns, file, record)) = with(cx, |u| {
        let run = u.run_mut(uid)?;
        // Counted before it goes, so the prompt is never mistaken for a
        // person's when its own events arrive.
        run.sent += 1;
        run.progress.spent_secs = run.budget.spent(Instant::now()).as_secs();
        let record = run.claimed.record(Phase::Working, &run.progress);
        let text = core::carry_on(missing, run.claimed.forge);
        Some((text, run.turns, run.claimed.file.clone(), record))
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
    };
    if session.update(cx, |session, cx| session.submit(&text, &[], cx)) {
        note(
            session,
            format!("Turn {turns} left {lacking}; asked to carry on"),
            cx,
        );
    } else {
        settle(uid, Ending::TurnEnded { tail }, cx);
    }
}

/// Everything is pushed to an open pull request: close the session, give up
/// the slot, and leave the checks to the tick.
///
/// **Deferred**, like [`settle`], because this is reached from inside the
/// session's own event.
fn park(uid: u64, cx: &mut App) {
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
        let phase = Phase::AwaitingChecks {
            since: onehand_core::chat::store::now_secs(),
        };
        save(&claimed, phase, &progress, cx);
        if let Some(session) = session.upgrade() {
            note(
                &session,
                "Pushed to its pull request; waiting for its checks".to_string(),
                cx,
            );
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
    let Some((mode, text)) = with(cx, |u| {
        let mode = u.mode.clone();
        let run = u.run_mut(uid)?;
        run.sent = 1;
        Some((
            mode,
            core::prompt_for(
                &run.claimed.issue,
                &run.claimed.branch,
                &run.claimed.tracker,
                run.claimed.forge,
                &run.progress.start,
            ),
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
    let sent = session.update(cx, |session, cx| {
        session.chat.set_mode(&mode);
        session.submit(&text, &[], cx)
    });
    if sent {
        note(
            session,
            format!("Mode {mode} set; issue sent as the prompt"),
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
