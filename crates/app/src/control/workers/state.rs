//! `session_state`, carried out: which worker a handle names, whether the
//! caller may read it, and what it is doing.
//!
//! **A worker of the caller's whose session is gone reads `link: closed`**,
//! the one answer that proves it stopped: closing a session ends its agent.
//! Anything else that is not the caller's own worker, open or closed, or a
//! uid that never was one, gets the same refusal, so a lead learns nothing
//! of sessions that are not its own.

use super::requests::Relation;
use super::sessions;
use gpui::App;
use onehand_core::control::open::parse_handle;
use onehand_core::control::state::{Held, state};
use std::path::PathBuf;

/// What a handle is to the caller.
pub(in crate::control) enum Whose {
    /// Its session is gone.
    Closed,
    /// A worker the caller opened, with what the relation knows.
    Mine(Held),
    /// Another lead's worker, or a session no lead opened.
    NotYours,
}

/// What session `uid` is to `caller`: from its relation while it lasts, the
/// lead it had once it closed (`closed`), and whether its session is `open`.
pub(in crate::control) fn whose(
    relation: Option<&Relation>,
    closed: Option<u64>,
    caller: u64,
    open: bool,
) -> Whose {
    match relation.filter(|relation| relation.lead == caller) {
        Some(relation) if open => Whose::Mine(Held {
            turn: relation.turn,
            sent: relation.sent,
            dirs: relation.dirs.clone(),
        }),
        Some(_) => Whose::Closed,
        None if closed == Some(caller) && !open => Whose::Closed,
        None => Whose::NotYours,
    }
}

/// Answer `session_state(session)` for `caller`.
pub(in crate::control) fn answer(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    let (caller, _) = caller.ok_or("`session_state` is for a session's agent")?;
    let uid = parse_handle(session, &super::super::RUN)?;
    let entity = crate::state::Shared::global(cx)
        .windows
        .iter()
        .filter_map(|window| window.shell.upgrade())
        .find_map(|shell| shell.read(cx).session_entity(uid, cx));
    let (relation, closed) = {
        let sessions = sessions();
        (
            sessions.workers.get(&uid).cloned(),
            sessions.closed_workers.get(&uid).copied(),
        )
    };
    match (
        whose(relation.as_ref(), closed, caller, entity.is_some()),
        entity,
    ) {
        (Whose::Mine(held), Some(entity)) => Ok(state(&entity.read(cx).chat, &held)),
        (Whose::Mine(_) | Whose::Closed, _) => Ok("link: closed".to_string()),
        (Whose::NotYours, _) => Err(format!("{session} is not a worker you opened")),
    }
}
