//! A step visit's settings. Before a visit's first prompt, the step's mode
//! and options (each one it does not name taken from the run's start) are
//! put in force one pick at a time, each answered by the agent before the
//! next is planned, since an answer can change what the agent offers. The
//! prompt waits until they all are; a setting not offered or refused fails
//! the run at that step.

use super::{Driven, advance, read, send, with};
use crate::chat::session::{ChatSession, note};
use gpui::{App, Entity};
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
/// starting ones.
fn wanted(d: &Driven) -> Settings {
    let start = d.run.started_with.clone().unwrap_or_default();
    d.run
        .current()
        .map(|step| Settings::for_step(&step.kind, &start))
        .unwrap_or_default()
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

/// The agent refused a pick, `why`: when it is the visit's own pick in
/// flight, now answered, the run fails on it. A refusal while the visit's
/// pick is still out, or while the visit waits on picks it did not send (a
/// person's in the composer, the run's mode, those restored on
/// reconnecting), is not the run's.
pub(super) fn refused(uid: u64, session: &Entity<ChatSession>, why: &str, cx: &mut App) {
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
    advance(uid, cx, move |run| {
        run.failed(said, found_late(About::Settings))
    });
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
    let want = read(uid, cx, wanted)?;
    // Whatever was sent before has been answered.
    picking.sent = None;
    loop {
        let chat = &session.read(cx).chat;
        // A pick put in before this one (the run's mode, or one restored on
        // reconnecting) is answered first, so what is planned against holds.
        if chat.picks_waiting() {
            break;
        }
        match next(&want, &chat.offer(), &chat.in_force(), &picking.picked) {
            Next::Pick(pick) => {
                session.update(cx, |session, cx| {
                    session.chat.pick(&pick);
                    cx.notify();
                });
                picking.picked.push(pick.group().to_string());
                picking.sent = Some(pick);
            }
            Next::Refused(why) => {
                advance(uid, cx, move |run| {
                    run.failed(why, found_late(About::Settings))
                });
                return None;
            }
            Next::Done => {
                let in_force = chat.in_force();
                let said = said(&want, &in_force);
                with(uid, cx, |d| {
                    d.run.ran_with(in_force);
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

/// What the visit runs with, as its note says it: the mode, then the model,
/// then the rest by id, each as the session holds it.
fn said(want: &Settings, in_force: &Settings) -> String {
    let mode = want
        .mode
        .as_ref()
        .and(in_force.mode.as_ref())
        .map(|mode| format!("mode {mode}"));
    let ids = want.options.keys();
    let ordered = ids
        .clone()
        .filter(|id| *id == "model")
        .chain(ids.filter(|id| *id != "model"));
    let options = ordered.filter_map(|id| Some(format!("{id} {}", in_force.options.get(id)?)));
    let parts: Vec<String> = mode.into_iter().chain(options).collect();
    format!("Settings: {}", parts.join(" · "))
}
