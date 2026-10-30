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
use crate::chat::session::ChatSession;
use gpui::{App, Entity};
use onehand_core::control::open::parse_handle;
use onehand_core::control::state::{Held, state};
use std::path::PathBuf;

/// What a handle is to the caller.
pub(in crate::control) enum Whose {
    /// Its session is gone.
    Closed,
    /// A worker the caller opened, open.
    Mine,
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
        Some(_) if open => Whose::Mine,
        Some(_) => Whose::Closed,
        None if closed == Some(caller) && !open => Whose::Closed,
        None => Whose::NotYours,
    }
}

/// A worker of the caller's, found by its handle.
pub(in crate::control) enum Found {
    /// Its session, live, and what the relation knows of it.
    Open {
        uid: u64,
        session: Entity<ChatSession>,
        relation: Relation,
    },
    /// Its session is gone.
    Closed,
}

/// The caller's worker `session` names, or why there is none: a handle of
/// another run, or anything that is not the caller's own worker.
pub(in crate::control) fn find(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &App,
) -> Result<Found, String> {
    let (caller, _) = caller.ok_or("this tool is for a session's agent")?;
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
        relation,
    ) {
        (Whose::Mine, Some(session), Some(relation)) => Ok(Found::Open {
            uid,
            session,
            relation,
        }),
        (Whose::Mine | Whose::Closed, _, _) => Ok(Found::Closed),
        (Whose::NotYours, _, _) => Err(format!("{session} is not a worker you opened")),
    }
}

/// Answer `session_state(session)` for `caller`.
pub(in crate::control) fn answer(
    caller: Option<(u64, PathBuf)>,
    session: &str,
    cx: &mut App,
) -> Result<String, String> {
    match find(caller, session, cx)? {
        Found::Closed => Ok("link: closed".to_string()),
        Found::Open {
            session, relation, ..
        } => {
            let held = Held {
                turn: relation.turn,
                sent: relation.sent,
                dirs: relation.dirs.clone(),
            };
            Ok(state(&session.read(cx).chat, &held))
        }
    }
}
