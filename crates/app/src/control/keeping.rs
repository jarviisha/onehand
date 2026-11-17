//! What a lead keeps of its workers, written beside its conversation, and
//! each session's conversation and prompts said to the tools' drive.
//!
//! What is kept, when, in which folder, and which old file goes when a
//! lead's conversation moved, is the drive's (`Effect::Keep`); this writes
//! there off the UI thread.
//!
//! **The last write asked for is the one that stays.** Writes run on the
//! background executor in any order, so each is numbered as it is asked
//! for, and one overtaken by a later one for the same lead writes nothing.

use super::{carry, drive};
use crate::chat::session::ChatSession;
use gpui::{App, Entity, Global, Subscription};
use onehand_core::control::drive::Input;
use onehand_core::control::kept::{self, Kept};
use onehand_core::control::state::turn_over;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

#[derive(Default)]
struct Keeping {
    /// The watch on each session, and what it last said.
    watches: HashMap<u64, (Subscription, Option<Said>)>,
}

impl Global for Keeping {}

/// What a session last said: its conversation, its prompts sent, whether
/// its turn is over, and whether one runs.
type Said = (String, usize, bool, bool);

/// The newest write asked for, by lead, held across the write itself.
static NEWEST: LazyLock<Mutex<HashMap<u64, u64>>> = LazyLock::new(Mutex::default);

fn newest() -> std::sync::MutexGuard<'static, HashMap<u64, u64>> {
    NEWEST.lock().unwrap_or_else(|e| e.into_inner())
}

/// Say session `uid`'s conversation, prompts sent and whether its turn is
/// over or running to the drive whenever any of them changes, from
/// `session` on: each new session of `uid` replaces the watch on the one
/// before.
pub(crate) fn watch(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let watch = cx.observe(session, move |session, cx| said(uid, &session, cx));
    cx.default_global::<Keeping>()
        .watches
        .insert(uid, (watch, None));
}

fn said(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    let (session_id, prompts_sent, turn_over, busy, dir) = {
        let chat = session.read(cx).chat();
        let Some(id) = chat.session_id.clone() else {
            return;
        };
        (
            id,
            chat.prompts_sent,
            turn_over(chat),
            chat.busy,
            chat.conversation_dir(),
        )
    };
    let now = (session_id.clone(), prompts_sent, turn_over, busy);
    match cx.default_global::<Keeping>().watches.get_mut(&uid) {
        Some((_, last)) if last.as_ref() != Some(&now) => *last = Some(now),
        Some(_) | None => return,
    }
    let fx = drive().handle(Input::Conversation {
        uid,
        session_id,
        prompts_sent,
        turn_over,
        busy,
        dir,
    });
    carry(fx, cx);
}

/// Session `uid` closed: its watch goes.
pub(super) fn closed(uid: u64, cx: &mut App) {
    cx.default_global::<Keeping>().watches.remove(&uid);
}

/// Write `kept` in `dir`, beside lead `lead`'s conversation, emptying
/// `moved_from` first; `last` says nothing is numbered for the lead again.
pub(super) fn keep(
    lead: u64,
    kept: Kept,
    dir: PathBuf,
    moved_from: Option<PathBuf>,
    last: bool,
    cx: &mut App,
) {
    let asked = {
        let mut newest = newest();
        let asked = newest.get(&lead).map_or(1, |n| n + 1);
        newest.insert(lead, asked);
        asked
    };
    cx.background_executor()
        .spawn(async move {
            let mut newest = newest();
            if let Some(before) = moved_from {
                let _ = kept::write_blocking(&before, &Kept::default());
            }
            if newest.get(&lead) != Some(&asked) {
                return;
            }
            if let Err(why) = kept::write_blocking(&dir, &kept) {
                eprintln!("onehand: cannot keep workers in {}: {why}", dir.display());
            }
            if last {
                newest.remove(&lead);
            }
        })
        .detach();
}
