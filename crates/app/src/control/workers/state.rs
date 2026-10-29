//! `session_state`, carried out: which worker a handle names, whether the
//! caller may read it, and what it is doing.
//!
//! **A worker of this run whose session is gone reads `link: closed`**, the
//! one answer that proves it stopped: closing a session ends its agent. A
//! uid that never was one reads the same, since a uid is never given twice
//! in a run and the caller learns nothing more from it.

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

/// What session `uid` is to `caller`, from its relation and whether its
/// session is `open`.
pub(in crate::control) fn whose(relation: Option<&Relation>, caller: u64, open: bool) -> Whose {
    match (relation, open) {
        (_, false) => Whose::Closed,
        (Some(relation), true) if relation.lead == caller => Whose::Mine(Held {
            turn: relation.turn,
            sent: relation.sent,
            dirs: relation.dirs.clone(),
        }),
        _ => Whose::NotYours,
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
    let relation = sessions().workers.get(&uid).cloned();
    match whose(relation.as_ref(), caller, entity.is_some()) {
        Whose::Closed => Ok("link: closed".to_string()),
        Whose::NotYours => Err(format!("{session} is not a worker you opened")),
        Whose::Mine(held) => {
            let entity = entity.ok_or("link: closed")?;
            Ok(state(&entity.read(cx).chat, &held))
        }
    }
}
