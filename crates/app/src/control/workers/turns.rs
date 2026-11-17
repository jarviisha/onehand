//! `session_state`, `send_prompt`, `cancel_turn` and `close_worker`, carried
//! out on a lead's worker. Whose worker a handle names, what the lead last
//! sent it and the whole of a turn's send are the tools' drive's, whether
//! a turn may be cancelled or the worker closed `control::turns`'; this
//! finds the session and does what they answer.
//!
//! **A worker of the caller's whose session is gone reads `link: closed`**,
//! the one answer that proves it stopped: closing a session ends its agent.
//! One that did not come back with its lead says why on the next line.
//!
//! **A worker that came back connects when its lead calls on it.** It was
//! left unconnected so nothing starts that nobody uses; until its agent is
//! up it reads as a session still connecting does, and a turn is refused in
//! the composer's words, so the lead calls again.

use super::super::{Answer, carry, drive};
use super::session_of;
use crate::chat::session::ChatSession;
use gpui::{App, Entity, Global};
use onehand_core::chat::Chat;
use onehand_core::chat::drive::{Code, Origin};
use onehand_core::control::drive::{Found, Input, Sending};
use onehand_core::control::state::state;
use onehand_core::control::turns;
use std::collections::HashMap;
use std::path::PathBuf;

/// A worker found open: its uid, and its session once it has begun
/// connecting.
type Open = (u64, Option<Entity<ChatSession>>);

/// The caller's worker `session` names, while open; one not connected yet
/// is connected now when `connect` says so.
fn find(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    connect: bool,
    cx: &mut App,
) -> Result<Option<Open>, String> {
    let (caller, _) = caller.ok_or("this tool is for a session's agent")?;
    let found = drive().find(caller, session, |uid| super::session_here(uid, cx))?;
    let uid = match found {
        Found::Open { uid } => uid,
        Found::Closed => return Ok(None),
    };
    let worker = session_of(uid, cx);
    if worker.is_none()
        && connect
        && let Some((_, shell)) = crate::windows::holding_session(uid, cx)
    {
        shell.update(cx, |shell, cx| shell.connect_unshown(uid, cx));
    }
    Ok(Some((uid, session_of(uid, cx))))
}

/// `read` of the worker's chat, or of a chat still connecting while its
/// session has none yet.
fn read_chat<R>(
    worker: &Option<Entity<ChatSession>>,
    cx: &App,
    read: impl FnOnce(&Chat) -> R,
) -> R {
    match worker {
        Some(worker) => read(worker.read(cx).chat()),
        None => read(&Chat::default()),
    }
}

/// Answer `session_state(session)` for `caller`.
pub(in crate::control) fn answer(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let Some((uid, worker)) = find(caller, session, true, cx)? else {
        return Ok(match drive().why_closed(session) {
            Some(why) => format!("link: closed\nwhy: {why}"),
            None => "link: closed".to_string(),
        });
    };
    let held = drive().held(uid).ok_or("link: closed")?;
    Ok(read_chat(&worker, cx, |chat| state(chat, &held)))
}

/// Give the worker `session` names turn `turn`, `text`, on `model` when one
/// is named: answered now, or once the turn went or was refused, through
/// `answer`, as the drive says.
pub(in crate::control) fn send(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    text: &str,
    turn: u64,
    model: Option<String>,
    answer: &mut Option<Answer>,
    cx: &mut App,
) -> Result<String, String> {
    let (uid, worker) = find(caller, session, true, cx)?.ok_or(format!("{session} is closed"))?;
    let (sending, fx) = {
        let chat = worker.as_ref().map(|worker| worker.read(cx).chat());
        drive().send_prompt(uid, session, turn, text, model, chat.map(|chat| &**chat))
    };
    match sending {
        Sending::Said(said) => {
            carry(fx, cx);
            said
        }
        Sending::Later => {
            if let Some(answer) = answer.take() {
                cx.default_global::<Turning>().answers.insert(uid, answer);
            }
            carry(fx, cx);
            Ok(String::new())
        }
    }
}

/// Where each worker's turn answered later is answered, by the worker.
#[derive(Default)]
struct Turning {
    answers: HashMap<u64, Answer>,
}

impl Global for Turning {}

/// Send worker `uid` the lead's turn `turn`, `text`, and report what it
/// came to.
pub(in crate::control) fn send_now(uid: u64, turn: u64, text: String, cx: &mut App) {
    let went = session_of(uid, cx).map(|worker| {
        // A tool's prompt sends no code first: that is a person's.
        let intake = worker
            .update(cx, |worker, cx| {
                worker.prompt(&text, &[], Origin::Tools, Code::Here, cx)
            })
            .intake;
        (intake, worker.read(cx).chat().prompts_sent)
    });
    let fx = drive().handle(Input::TurnSent {
        uid,
        turn,
        text,
        went,
    });
    carry(fx, cx);
}

/// Answer the call that gave worker `uid` its turn with `outcome`.
pub(in crate::control) fn answered(uid: u64, outcome: Result<String, String>, cx: &mut App) {
    if let Some(answer) = cx.default_global::<Turning>().answers.remove(&uid) {
        let _ = answer.send(outcome);
    }
}

/// Cancel the turn the worker `session` names is running.
pub(in crate::control) fn cancel(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let (uid, worker) = find(caller, session, true, cx)?.ok_or(format!("{session} is closed"))?;
    let last = drive().last(uid).ok_or(format!("{session} is closed"))?;
    let turn = read_chat(&worker, cx, |chat| turns::cancel(chat, &last))?;
    // A chat still connecting has refused the cancel above.
    let worker = worker.ok_or(format!("{session} is still connecting"))?;
    worker.update(cx, |worker, cx| worker.stop(cx));
    Ok(format!("cancelling turn {turn}"))
}

/// Close the worker `session` names, as the person's *Close session* does:
/// its session and agent end, its conversation and folder stay. A worker
/// already closed answers so again, so a retry is harmless.
pub(in crate::control) fn close(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let closed = || Ok("closed".to_string());
    let Some((uid, worker)) = find(caller, session, false, cx)? else {
        return closed();
    };
    let Some(last) = drive().last(uid) else {
        return closed();
    };
    // One not connected yet runs no turn and has no card: it closes as it
    // is, its agent never started.
    read_chat(&worker, cx, |chat| turns::close(chat, &last))?;
    let (window, _) = crate::windows::holding_session(uid, cx).ok_or("the window closed")?;
    // Not from inside whatever window update the call came through.
    cx.defer(move |cx| {
        crate::windows::with_shell(window, cx, |shell, window, cx| {
            shell.end_session(uid, window, cx)
        });
    });
    closed()
}
