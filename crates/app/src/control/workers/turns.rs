//! `session_state`, `send_prompt`, `cancel_turn` and `close_worker`, carried
//! out on a lead's worker. Whose worker a handle names and what the lead
//! last sent it is the tools' drive's, whether a turn may go or the worker
//! close `control::turns`'; this finds the session and does it.
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
use onehand_core::control::drive::Found;
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
/// is named: then answered once the model is picked and the turn went, or
/// refused, through `answer`.
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
    // A turn still waiting on its model goes first, or not at all.
    if let Some(waiting) = drive().turn_waiting(uid, turn, text) {
        return waiting;
    }
    let last = drive().last(uid).ok_or(format!("{session} is closed"))?;
    let send = read_chat(&worker, cx, |chat| turns::send(chat, &last, turn, text))?;
    // A chat still connecting has refused the turn above.
    let worker = worker.ok_or(format!("{session} is still connecting"))?;
    match send {
        turns::Send::AlreadySent(turn) => Ok(format!("turn {turn} already sent")),
        turns::Send::Go => match model {
            None => send_now_to(uid, &worker, turn, text, cx),
            Some(model) => {
                if let Some(answer) = answer.take() {
                    cx.default_global::<Turning>().answers.insert(uid, answer);
                }
                let fx = drive().turn_with_model(uid, turn, text.to_string(), model);
                carry(fx, cx);
                Ok(String::new())
            }
        },
    }
}

/// Where each worker's turn waiting on its model is answered.
#[derive(Default)]
struct Turning {
    answers: HashMap<u64, Answer>,
}

impl Global for Turning {}

/// Send worker `uid` turn `turn`, `text`, its model now in force, and
/// answer the call that asked.
///
/// Checked again first, as the call was: the person may have taken the
/// worker, or a turn begun, while the model was picked.
pub(in crate::control) fn send_now(uid: u64, turn: u64, text: &str, cx: &mut App) {
    let closed = || Err("the session was closed before the turn went".to_string());
    let outcome = match (session_of(uid, cx), drive().last(uid)) {
        (Some(worker), Some(last)) => {
            match turns::send(worker.read(cx).chat(), &last, turn, text) {
                Ok(turns::Send::Go) => send_now_to(uid, &worker, turn, text, cx),
                Ok(turns::Send::AlreadySent(turn)) => Ok(format!("turn {turn} already sent")),
                Err(why) => Err(why),
            }
        }
        (None, _) | (_, None) => closed(),
    };
    answered(uid, outcome, cx);
}

/// Answer the call that gave worker `uid` its turn with `outcome`.
pub(in crate::control) fn answered(uid: u64, outcome: Result<String, String>, cx: &mut App) {
    if let Some(answer) = cx.default_global::<Turning>().answers.remove(&uid) {
        let _ = answer.send(outcome);
    }
}

/// Send `worker` the lead's turn `turn`, `text`, and what the drive says of it.
fn send_now_to(
    uid: u64,
    worker: &Entity<ChatSession>,
    turn: u64,
    text: &str,
    cx: &mut App,
) -> Result<String, String> {
    // A tool's prompt sends no code first: that is a person's.
    let intake = worker
        .update(cx, |worker, cx| {
            worker.prompt(text, &[], Origin::Tools, Code::Here, cx)
        })
        .intake;
    let sent = worker.read(cx).chat().prompts_sent;
    let (said, fx) = drive().sent(uid, turn, text, &intake, sent);
    carry(fx, cx);
    said
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
