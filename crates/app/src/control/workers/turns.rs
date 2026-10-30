//! `send_prompt` and `cancel_turn`, carried out on a lead's worker. Whether
//! either may go is core's rule; this finds the worker, does it, and moves
//! the relation on to what the lead last sent.

use super::sessions;
use super::state::{Found, find};
use gpui::App;
use onehand_core::control::turns::{self, Last};
use std::path::PathBuf;

/// Give the worker `session` names turn `turn`, `text`.
pub(in crate::control) fn send(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    text: &str,
    turn: u64,
    cx: &mut App,
) -> Result<String, String> {
    let Found::Open {
        uid,
        session: worker,
        relation,
    } = find(caller, session, cx)?
    else {
        return Err(format!("{session} is closed"));
    };
    let last = Last {
        turn: relation.turn,
        text: relation.text.clone(),
        sent: relation.sent,
    };
    match turns::send(&worker.read(cx).chat, &last, turn, text)? {
        turns::Send::AlreadySent(turn) => Ok(format!("turn {turn} already sent")),
        turns::Send::Go => {
            if !worker.update(cx, |worker, cx| worker.submit(text, &[], cx)) {
                return Err("the prompt was not accepted".to_string());
            }
            let sent = worker.read(cx).chat.prompts_sent;
            if let Some(relation) = sessions().workers.get_mut(&uid) {
                relation.turn = Some(turn);
                relation.text = Some(text.to_string());
                relation.sent = sent;
            }
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
    let Found::Open {
        session: worker,
        relation,
        ..
    } = find(caller, session, cx)?
    else {
        return Err(format!("{session} is closed"));
    };
    let last = Last {
        turn: relation.turn,
        text: relation.text.clone(),
        sent: relation.sent,
    };
    let turn = turns::cancel(&worker.read(cx).chat, &last)?;
    worker.update(cx, |worker, cx| {
        worker.chat.cancel_turn();
        cx.notify();
    });
    Ok(format!("cancelling turn {turn}"))
}
