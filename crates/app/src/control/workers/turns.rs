//! `session_state`, `send_prompt`, `cancel_turn` and `close_worker`, carried
//! out on a lead's worker. Whose worker a handle names and what the lead
//! last sent it is the tools' drive's, whether a turn may go or the worker
//! close `control::turns`'; this finds the session and does it.
//!
//! **A worker of the caller's whose session is gone reads `link: closed`**,
//! the one answer that proves it stopped: closing a session ends its agent.

use super::super::{carry, drive};
use super::session_of;
use crate::chat::session::ChatSession;
use gpui::{App, Entity};
use onehand_core::chat::drive::{Code, Origin};
use onehand_core::control::drive::Found;
use onehand_core::control::state::state;
use onehand_core::control::turns;
use std::path::PathBuf;

/// The caller's worker `session` names: its uid and session while open.
fn find(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &App,
) -> Result<Option<(u64, Entity<ChatSession>)>, String> {
    let (caller, _) = caller.ok_or("this tool is for a session's agent")?;
    let found = drive().find(caller, session, |uid| session_of(uid, cx).is_some())?;
    match found {
        Found::Open { uid } => Ok(session_of(uid, cx).map(|entity| (uid, entity))),
        Found::Closed => Ok(None),
    }
}

/// Answer `session_state(session)` for `caller`.
pub(in crate::control) fn answer(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let Some((uid, worker)) = find(caller, session, cx)? else {
        return Ok("link: closed".to_string());
    };
    let held = drive().held(uid).ok_or("link: closed")?;
    Ok(state(worker.read(cx).chat(), &held))
}

/// Give the worker `session` names turn `turn`, `text`.
pub(in crate::control) fn send(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    text: &str,
    turn: u64,
    cx: &mut App,
) -> Result<String, String> {
    let (uid, worker) = find(caller, session, cx)?.ok_or(format!("{session} is closed"))?;
    let last = drive().last(uid).ok_or(format!("{session} is closed"))?;
    match turns::send(worker.read(cx).chat(), &last, turn, text)? {
        turns::Send::AlreadySent(turn) => Ok(format!("turn {turn} already sent")),
        turns::Send::Go => {
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
    }
}

/// Cancel the turn the worker `session` names is running.
pub(in crate::control) fn cancel(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let (uid, worker) = find(caller, session, cx)?.ok_or(format!("{session} is closed"))?;
    let last = drive().last(uid).ok_or(format!("{session} is closed"))?;
    let turn = turns::cancel(worker.read(cx).chat(), &last)?;
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
    let Some((uid, worker)) = find(caller, session, cx)? else {
        return closed();
    };
    let Some(last) = drive().last(uid) else {
        return closed();
    };
    turns::close(worker.read(cx).chat(), &last)?;
    let (window, _) = crate::windows::holding_session(uid, cx).ok_or("the window closed")?;
    // Not from inside whatever window update the call came through.
    cx.defer(move |cx| {
        crate::windows::with_shell(window, cx, |shell, window, cx| {
            shell.end_session(uid, window, cx)
        });
    });
    closed()
}
