//! A step visit's settings. Before a visit's first prompt, the step's mode
//! and options (each one it does not name taken from the run's start) are
//! put in force one pick at a time, each answered by the agent before the
//! next is planned, since an answer can change what the agent offers. The
//! prompt waits until they all are; a setting the step names not offered,
//! or any pick refused, fails the run at that step (and, for an issue's run,
//! pauses unattended runs, since every later one would fail the same way).
//! One the step only takes from the start and the agent no longer offers is
//! left as the agent has it.

use super::{Driven, advance, read, send, with};
use crate::chat::session::{ChatSession, note};
use gpui::{App, Entity};
use onehand_core::chat::MODE_SELECTOR;
use onehand_core::preflight::{About, found_late};
use onehand_core::workflow::{Next, Pick, Settings, next};

/// A visit's picks under way, and the prompt they hold back.
pub(super) struct Picking {
    text: String,
    /// The groups picked this visit, answered or in flight.
    picked: Vec<String>,
    /// The pick in flight, to name it when the agent refuses it.
    sent: Option<Pick>,
}

/// The id of the run's open visit.
fn open_visit(d: &Driven) -> Option<u32> {
    d.run
        .visits()
        .last()
        .filter(|v| v.ended_at.is_none())
        .map(|v| v.id)
}

/// What the open visit runs with: its step's own settings over the run's
/// starting ones, and the groups the step names itself.
fn wanted(d: &Driven) -> (Settings, Vec<String>) {
    let start = d.run.started_with.clone().unwrap_or_default();
    d.run
        .current()
        .map(|step| {
            (
                Settings::for_step(&step.kind, &start),
                Settings::named_by(&step.kind),
            )
        })
        .unwrap_or_default()
}

/// Fail the run on a setting, `why`. A run of an issue pauses unattended
/// runs too: every later one would fail the same way, each on a fresh issue.
/// A person's run from the launcher does not.
fn fail(uid: u64, why: String, cx: &mut App) {
    let Some((task, workflow)) = read(uid, cx, |d| (d.task.clone(), d.run.template.id.clone()))
    else {
        return;
    };
    if crate::task::task(&task, cx).is_some_and(|task| task.issue().is_some()) {
        let fix = format!("the step settings of workflow `{workflow}` are fixed");
        crate::unattended::pause(&why, &fix, cx);
    }
    advance(uid, cx, move |run| {
        run.failed(why, found_late(About::Settings))
    });
}

/// The prompt `text`, given back when it may go now: the open visit's
/// settings are in force. Otherwise it is held until they are, or the run
/// has failed on one.
pub(super) fn settle(
    uid: u64,
    session: &Entity<ChatSession>,
    text: String,
    cx: &mut App,
) -> Option<String> {
    if read(uid, cx, |d| d.settled == open_visit(d))? {
        return Some(text);
    }
    let picking = Picking {
        text,
        picked: Vec::new(),
        sent: None,
    };
    plan(uid, session, picking, cx)
}

/// The session answered something while picks are under way: once every
/// pick is answered, plan the next, and send the held prompt when there is
/// none. Run after everything the same answer emitted, so a refusal has
/// already failed the run and left nothing to plan.
pub(super) fn answered(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    // The run's own mode answered and not refused: nothing more of it.
    if !session.read(cx).chat.pick_waiting(MODE_SELECTOR) {
        with(uid, cx, |d| d.mode_out = None);
    }
    // The visit's own pick answered while another group's is still out (a
    // person's in the composer): a refusal of that one is not the run's.
    let sent = read(uid, cx, |d| {
        Some(d.picking.as_ref()?.sent.as_ref()?.group().to_string())
    })
    .flatten();
    if let Some(group) = sent
        && !session.read(cx).chat.pick_waiting(&group)
    {
        with(uid, cx, |d| {
            if let Some(picking) = d.picking.as_mut() {
                picking.sent = None;
            }
        });
    }
    if session.read(cx).chat.picks_waiting() {
        return;
    }
    let Some(picking) = with(uid, cx, |d| d.picking.take()).flatten() else {
        return;
    };
    if let Some(text) = plan(uid, session, picking, cx) {
        send(uid, session, text, cx);
    }
}

/// The agent refused a pick, `why`: when it is the run's own mode, the run
/// fails on its mode; when it is the visit's own pick in flight, now
/// answered, on the step's setting. A refusal while the visit's pick is
/// still out, or of a pick the run did not send (a person's in the
/// composer, those restored on reconnecting), is not the run's.
pub(super) fn refused(uid: u64, session: &Entity<ChatSession>, why: &str, cx: &mut App) {
    // The run's own mode, refused: what is to be fixed is the mode the run
    // was given, not the workflow's steps.
    let mode = read(uid, cx, |d| d.mode_out.clone()).flatten();
    if let Some(mode) = mode
        && !session.read(cx).chat.pick_waiting(MODE_SELECTOR)
    {
        with(uid, cx, |d| {
            d.mode_out = None;
            d.picking = None;
        });
        let why = format!("the agent refused mode `{mode}`: {why}");
        crate::unattended::pause(&why, "unattended.mode is fixed", cx);
        advance(uid, cx, move |run| run.failed(why, found_late(About::Mode)));
        return;
    }
    let Some(pick) = read(uid, cx, |d| d.picking.as_ref()?.sent.clone()).flatten() else {
        return;
    };
    if session.read(cx).chat.pick_waiting(pick.group()) {
        return;
    }
    with(uid, cx, |d| d.picking = None);
    let said = match pick {
        Pick::Mode(mode) => format!("the agent refused mode `{mode}`: {why}"),
        Pick::Option { id, value } => format!("the agent refused `{value}` for `{id}`: {why}"),
    };
    fail(uid, said, cx);
}

/// Send the visit's next pick and hold the prompt, or fail the run on a
/// setting not offered, or, with nothing left to pick, record what the
/// visit runs with and give the prompt back.
fn plan(
    uid: u64,
    session: &Entity<ChatSession>,
    mut picking: Picking,
    cx: &mut App,
) -> Option<String> {
    let (want, named) = read(uid, cx, wanted)?;
    // Whatever was sent before has been answered.
    picking.sent = None;
    loop {
        let chat = &session.read(cx).chat;
        // A pick put in before this one (the run's mode, or one restored on
        // reconnecting) is answered first, so what is planned against holds.
        if chat.picks_waiting() {
            break;
        }
        match next(
            &want,
            &named,
            &chat.offer(),
            &chat.in_force(),
            &picking.picked,
        ) {
            Next::Pick(pick) => {
                session.update(cx, |session, cx| {
                    session.chat.pick(&pick);
                    cx.notify();
                });
                picking.picked.push(pick.group().to_string());
                picking.sent = Some(pick);
            }
            Next::Refused(why) => {
                fail(uid, why, cx);
                return None;
            }
            Next::Done => {
                let held = chat.held();
                let said = said(&want, &held);
                with(uid, cx, |d| {
                    d.run.ran_with(held);
                    d.settled = open_visit(d);
                });
                if !picking.picked.is_empty() {
                    note(session, said, cx);
                }
                return Some(picking.text);
            }
        }
    }
    with(uid, cx, |d| d.picking = Some(picking));
    None
}

/// What the visit runs with, as its note says it: what the session holds
/// of each setting the visit wanted.
fn said(want: &Settings, held: &Settings) -> String {
    let ran = Settings {
        mode: want.mode.as_ref().and(held.mode.clone()),
        options: want
            .options
            .keys()
            .filter_map(|id| Some((id.clone(), held.options.get(id)?.clone())))
            .collect(),
    };
    format!("Settings: {}", ran.said())
}
