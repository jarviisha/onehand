//! What a lead keeps of its workers, written beside its conversation, and
//! each session's conversation and prompts said to the tools' drive.
//!
//! What is kept, and when, is the drive's (`Effect::Keep`); this finds the
//! lead's conversation directory and writes there off the UI thread. A
//! lead whose conversation moved to a new id has its file moved with it.
//!
//! **The last write asked for is the one that stays.** Writes run on the
//! background executor in any order, so each is numbered as it is asked
//! for, and one overtaken by a later one for the same lead writes nothing.

use super::{carry, drive};
use crate::chat::session::ChatSession;
use gpui::{App, Entity, Global, Subscription};
use onehand_core::control::drive::Input;
use onehand_core::control::kept::{self, Kept};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

#[derive(Default)]
struct Keeping {
    /// The watch on each session, and the conversation and prompts sent it
    /// last said.
    watches: HashMap<u64, (Subscription, Option<(String, usize)>)>,
    /// Where each lead's workers were last written.
    written: HashMap<u64, PathBuf>,
    /// Where the conversation each lead closed on is kept, read before it
    /// went.
    closed: HashMap<u64, PathBuf>,
}

impl Global for Keeping {}

/// The newest write asked for, by lead, held across the write itself.
static NEWEST: LazyLock<Mutex<HashMap<u64, u64>>> = LazyLock::new(Mutex::default);

fn newest() -> std::sync::MutexGuard<'static, HashMap<u64, u64>> {
    NEWEST.lock().unwrap_or_else(|e| e.into_inner())
}

/// Say session `uid`'s conversation and prompts sent to the drive whenever
/// either changes, from `session` on: each new session of `uid` replaces
/// the watch on the one before.
pub(crate) fn watch(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let watch = cx.observe(session, move |session, cx| said(uid, &session, cx));
    cx.default_global::<Keeping>()
        .watches
        .insert(uid, (watch, None));
}

fn said(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let (session_id, prompts_sent) = {
        let chat = session.read(cx).chat();
        let Some(id) = chat.session_id.clone() else {
            return;
        };
        (id, chat.prompts_sent)
    };
    let now = (session_id.clone(), prompts_sent);
    match cx.default_global::<Keeping>().watches.get_mut(&uid) {
        Some((_, last)) if last.as_ref() != Some(&now) => *last = Some(now),
        Some(_) | None => return,
    }
    let fx = drive().handle(Input::Conversation {
        uid,
        session_id,
        prompts_sent,
    });
    carry(fx, cx);
}

/// Session `uid` closed on the conversation kept in `conversation`: its
/// watch goes, and what its close keeps is written beside it.
pub(super) fn closed(uid: u64, conversation: Option<PathBuf>, cx: &mut App) {
    let keeping = cx.default_global::<Keeping>();
    keeping.watches.remove(&uid);
    if let Some(conversation) = conversation {
        keeping.closed.insert(uid, conversation);
    }
}

/// Write `kept` beside lead `lead`'s conversation; nothing while it has
/// none yet, nor for a chat that keeps nothing on disk.
pub(super) fn keep(lead: u64, kept: Kept, cx: &mut App) {
    let live = crate::windows::session(lead, cx)
        .and_then(|session| session.read(cx).chat().conversation_dir());
    let here = live.is_some();
    let keeping = cx.default_global::<Keeping>();
    let Some(dir) = live.or_else(|| keeping.closed.get(&lead).cloned()) else {
        return;
    };
    let moved_from = keeping
        .written
        .insert(lead, dir.clone())
        .filter(|before| *before != dir);
    // A lead gone writes once more, its last.
    if !here {
        keeping.written.remove(&lead);
        keeping.closed.remove(&lead);
    }
    let asked = {
        let mut newest = newest();
        let asked = newest.get(&lead).map_or(1, |n| n + 1);
        newest.insert(lead, asked);
        asked
    };
    cx.background_executor()
        .spawn(async move {
            let newest = newest();
            if let Some(before) = moved_from {
                let _ = kept::write_blocking(&before, &Kept::default());
            }
            if newest.get(&lead) != Some(&asked) {
                return;
            }
            if let Err(why) = kept::write_blocking(&dir, &kept) {
                eprintln!("onehand: cannot keep workers in {}: {why}", dir.display());
            }
        })
        .detach();
}
