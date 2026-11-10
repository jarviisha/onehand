//! `session_state`, `send_prompt` and `cancel_turn`, carried out on a lead's
//! worker. Whose worker a handle names and what the lead last sent it is the
//! tools' drive's, whether a turn may go `control::turns`'; this finds the
//! session and does it.
//!
//! **A worker of the caller's whose session is gone reads `link: closed`**,
//! the one answer that proves it stopped: closing a session ends its agent.

use super::super::drive;
use super::session_of;
use crate::chat::session::ChatSession;
use gpui::{App, Entity};
use onehand_core::chat::drive::{Code, Intake, Origin};
use onehand_core::control::drive::{Found, run};
use onehand_core::control::open::parse_handle;
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
    let uid = parse_handle(session, &run())?;
    let entity = session_of(uid, cx);
    match drive().find(caller, uid, session, entity.is_some())? {
        Found::Open { uid } => Ok(entity.map(|entity| (uid, entity))),
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
    Ok(state(&worker.read(cx).chat, &held))
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
    match turns::send(&worker.read(cx).chat, &last, turn, text)? {
        turns::Send::AlreadySent(turn) => Ok(format!("turn {turn} already sent")),
        turns::Send::Go => {
            let (intake, _) = worker.update(cx, |worker, cx| {
                worker.prompt(text, &[], Origin::Tools, Code::Here, cx)
            });
            if intake != Intake::Sent {
                return Err("the prompt was not accepted".to_string());
            }
            let sent = worker.read(cx).chat.prompts_sent;
            drive().sent(uid, turn, text, sent);
            Ok(format!("turn {turn} accepted"))
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
    let turn = turns::cancel(&worker.read(cx).chat, &last)?;
    worker.update(cx, |worker, cx| worker.stop(cx));
    Ok(format!("cancelling turn {turn}"))
}
