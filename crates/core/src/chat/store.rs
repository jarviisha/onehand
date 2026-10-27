//! The conversation store: one directory per conversation, on disk.
//!
//! ```text
//! <store>/<session id>/
//!   meta.json      rewritten whole whenever it changes
//!   items.jsonl    one transcript item per line, only ever appended to
//!   blobs/         image results, by content hash
//! ```
//!
//! **The split is the whole point.** A conversation's metadata is small, changes
//! rarely and has to be readable without opening anything else — the picker
//! wants a title and a date, not a transcript. The transcript is large, only
//! ever grows at its end, and is the expensive thing to write. Keeping them in
//! one document meant every finished turn re-serialized the entire conversation,
//! so the cost of a turn grew with the conversation it was part of; and it meant
//! listing conversations parsed every archive in full, images included, to read
//! six fields off the front of each.
//!
//! Appending also removes a whole class of failure rather than guarding against
//! it. When every save replaces the file, any moment where the transcript in
//! memory is *short* — a resume halfway through re-delivering itself, a session
//! taken apart — is a moment where saving destroys what is on disk. A writer
//! that only ever adds has nothing to destroy with.
//!
//! What it costs, said plainly: a line already written is not revisited. An item
//! that changes after its turn has ended keeps the shape it was written in. The
//! one case that reaches in practice is a prompt typed *during* a turn, whose
//! "how long it took" is filled in one turn later than it is written — so a
//! reopened conversation does not show a duration for it. Holding such lines
//! back until they settled would mean a session taken apart mid-turn archived
//! none of that turn, which is a worse trade: a missing number against missing
//! content.

use crate::chat::model::ChatItem;
use crate::config::config_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The layout this build writes and is willing to read.
///
/// Checked, unlike the version the previous store wrote and never looked at. A
/// directory whose format this build does not know is neither listed nor
/// opened, because half-understanding a wider schema is worse than declining
/// it: the next save would write the misreading back over the original.
///
/// It sits in `meta.json` rather than anywhere global, so a later format can
/// live beside this one directory by directory instead of at the store's root,
/// where it would have to describe every conversation at once.
const FORMAT: u32 = 2;

/// The largest image kept beside a conversation.
///
/// Past this the transcript records that there was an image and how big it was,
/// and does not keep it. One conversation full of screenshots is otherwise a
/// few hundred megabytes of a directory nobody browses.
const MAX_BLOB_BYTES: u64 = 4 * 1024 * 1024;

/// The most transcript items a conversation comes back with.
///
/// Taken from the **end**, because the end is the part being continued.
const MAX_RESTORED_ITEMS: usize = 2000;

mod lines;
mod write;
pub(crate) use lines::{line_of, Blob};
use lines::{restore_line, Line};
pub use write::{commit, delete};

/// `<config_dir>/onehand/conversations/`.
pub fn conversations_dir() -> PathBuf {
    config_dir().join("conversations")
}

/// The directory holding one conversation inside `store`.
pub fn conv_dir(store: &Path, session_id: &str) -> PathBuf {
    store.join(sanitize(session_id))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Filename-safe form of a session id. When sanitizing actually changed the
/// string, a short hash of the *raw* id is appended so two ids differing only
/// in punctuation (`a/b` vs `a_b`) can't collapse onto one directory and
/// overwrite each other. UUID-style ids pass through untouched.
fn sanitize(s: &str) -> String {
    let safe: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe == s {
        return safe;
    }
    format!("{safe}-{:08x}", fnv1a(s.as_bytes()))
}

/// FNV-1a — tiny, dependency-free, and stable across runs and Rust releases,
/// which is all any caller needs of it.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── what is on disk ─────────────────────────────────────────────────────────

/// `meta.json`: everything answerable about a conversation without reading it.
#[derive(Serialize, Deserialize)]
struct Meta {
    format: u32,
    session_id: String,
    root: String,
    agent: String,
    /// When the conversation was first written. `updated` moves; this does not,
    /// so "how old is this" stays answerable after a rename or a reopen.
    created: u64,
    /// The last time a **message** was added.
    ///
    /// Not the last time anything was written. Stamping every save meant
    /// opening a conversation and closing it again promoted it above ones that
    /// had real work in them, so the picker's newest-first order was really
    /// last-opened-first and the list rearranged itself behind the reader.
    updated: u64,
    /// Lines in `items.jsonl`, for the picker's subtitle. Approximate across
    /// processes, and deliberately nothing correctness-bearing reads it.
    items: usize,
    title: Option<String>,
    /// The first user prompt, capped. Stored so listing never has to open a
    /// transcript to find out what a conversation was about.
    preview: String,
    prefs: Prefs,
}

/// The selector state a conversation was last using.
///
/// The adapter rebuilds effort/agent from *static* settings on every
/// `session/new`/`session/load`, so without replaying these a resumed session
/// silently drops the session's own picks. Model is the exception — the SDK
/// re-reads it from the transcript — so it is kept for completeness and not
/// re-applied.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) config: Vec<ConfigPick>,
}

/// One config-option pick (`id` → chosen `value`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigPick {
    pub id: String,
    pub value: String,
}

// ── what callers see ────────────────────────────────────────────────────────

/// Lightweight metadata for the picker (no transcript).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvMeta {
    pub dir: PathBuf,
    pub session_id: String,
    /// The agent this conversation ran under (its `AgentSpec.name`).
    pub agent: String,
    pub updated: u64,
    /// Already resolved: the name the user gave it, else one derived from the
    /// first prompt, else the agent's name.
    pub title: String,
    pub item_count: usize,
}

/// A conversation in hand — read from disk, or lifted out of a live chat for a
/// restart.
///
/// Deliberately **not** serializable. The shape on disk is no longer one
/// document, and a type that was both the file and the handoff was a type where
/// changing either half changed the other: the restart path built one of these
/// and never wrote it, so a change made for storage reasons landed in a code
/// path that never touches storage.
pub struct ConversationSnapshot {
    pub session_id: String,
    pub title: Option<String>,
    pub updated: u64,
    pub(crate) prefs: Prefs,
    /// Renderable items, already parsed. Not lines: the restart path hands over
    /// live items, and round-tripping those through the archive form would
    /// flatten a running terminal into a snapshot of its output.
    pub items: Vec<ChatItem>,
    /// How many transcript positions are already on disk — the mark the
    /// adopting chat carries on from.
    pub(crate) written: usize,
    /// Whether `items` is the whole conversation. False when the read hit its
    /// bound, which is what forbids rewriting the file from it.
    pub(crate) complete: bool,
}

/// Everything one save has to put on disk, prepared while the transcript is in
/// hand so the writing itself needs nothing but this.
pub struct PendingWrite {
    pub dir: PathBuf,
    pub(crate) lines: Vec<String>,
    pub(crate) blobs: Vec<Blob>,
    pub(crate) meta: MetaWrite,
    /// The mark this save was built from, which [`Chat::unflush`] hands back
    /// when it does not land.
    pub(crate) from: usize,
    /// Replace `items.jsonl` with `lines` instead of adding to it.
    ///
    /// The one whole-file write left, and it happens at most once per resume.
    /// See [`Chat::settle_replay`] for what forces it: a replay that arrives
    /// chunked differently from the file cannot be spliced onto it at an index
    /// that means a different thing on each side.
    pub(crate) rewrite: bool,
}

/// The metadata half of a save, which is written whether or not any line was.
pub(crate) struct MetaWrite {
    pub session_id: String,
    pub root: String,
    pub agent: String,
    pub title: Option<String>,
    pub(crate) preview: String,
    pub(crate) prefs: Prefs,
    /// `Some` when this save added messages, and then it is the moment they
    /// were added. `None` leaves whatever is on disk alone.
    pub updated: Option<u64>,
    pub items: usize,
    /// When this was read off the conversation, among every metadata write
    /// in the process. A write that lands after a later-read one (a rename
    /// queued behind a turn's own save, or the reverse) leaves the later
    /// one's fields alone.
    pub(crate) read_at: u64,
}

/// The next [`MetaWrite::read_at`]: one count for the process, so two chats
/// on one conversation are ordered too.
pub(crate) fn next_meta_read() -> u64 {
    static READS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// ── reading ─────────────────────────────────────────────────────────────────

/// The metadata file itself, if this build can read it.
///
/// `None` covers three different things and only one of them is ordinary, so
/// the other two say so on the way past: a conversation missing from the list
/// with no explanation anywhere reads as the app having lost it.
///
/// **Every reader comes through here, the transcript loader included.** An
/// unknown format is then a conversation that is invisible and cannot be
/// opened, rather than one parsed halfway under the wrong schema — and the next
/// save would write that misreading back over the original.
fn meta_at(dir: &Path) -> Option<Meta> {
    let path = dir.join("meta.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("onehand: cannot read {}: {e}", path.display());
            return None;
        }
    };
    let meta: Meta = match serde_json::from_str(&text) {
        Ok(meta) => meta,
        Err(e) => {
            eprintln!(
                "onehand: bad conversation metadata in {}: {e}",
                dir.display()
            );
            return None;
        }
    };
    if meta.format != FORMAT {
        eprintln!(
            "onehand: {} is in a format this build does not read",
            dir.display()
        );
        return None;
    }
    Some(meta)
}

/// The file's metadata as the picker wants it, with the title resolved: the
/// name the user gave it, else one derived from the first prompt, else the
/// agent's name.
fn describe(dir: &Path, meta: Meta) -> ConvMeta {
    let title = meta
        .title
        .or_else(|| crate::chat::model::summarize_title(&meta.preview))
        .unwrap_or_else(|| meta.agent.clone());
    ConvMeta {
        dir: dir.to_path_buf(),
        session_id: meta.session_id,
        agent: meta.agent,
        updated: meta.updated,
        title,
        item_count: meta.items,
    }
}

/// Conversations under `store` belonging to `project`, and to `agent` when one
/// is named, newest first.
///
/// Reads one small file per conversation and opens no transcript at all.
/// Blocking — call it off the UI loop.
pub fn list_conversations(store: &Path, project: &Path, agent: Option<&str>) -> Vec<ConvMeta> {
    let project = project.display().to_string();
    list_where(store, |root, conv_agent| {
        root == project && agent.is_none_or(|a| conv_agent == a)
    })
    .into_iter()
    .map(|(_, conv)| conv)
    .collect()
}

/// Conversations under `store` belonging to any of `projects`, each with the
/// project it belongs to, newest first. One read of the store however many
/// projects are asked about. Blocking — call it off the UI loop.
pub fn list_across(store: &Path, projects: &[PathBuf]) -> Vec<(PathBuf, ConvMeta)> {
    let wanted: std::collections::HashMap<String, &PathBuf> = projects
        .iter()
        .map(|p| (p.display().to_string(), p))
        .collect();
    list_where(store, |root, _| wanted.contains_key(root))
        .into_iter()
        .filter_map(|(root, conv)| Some(((*wanted.get(&root)?).clone(), conv)))
        .collect()
}

/// The conversation the agent named `session_id`, with the project root it ran
/// in, whatever that project is — one whose folder has since left the
/// workspace included. Blocking: it reads one `meta.json`.
pub fn find_conversation(store: &Path, session_id: &str) -> Option<(PathBuf, ConvMeta)> {
    let dir = conv_dir(store, session_id);
    let meta = meta_at(&dir)?;
    Some((PathBuf::from(&meta.root), describe(&dir, meta)))
}

/// Every conversation `keep` accepts by its project root and agent, with that
/// root, newest first.
fn list_where(store: &Path, keep: impl Fn(&str, &str) -> bool) -> Vec<(String, ConvMeta)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(store) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        // Read once and filtered from what was read. `root` is not on
        // `ConvMeta` — nothing draws it — so the filter asks the file's own
        // field rather than widening what the picker carries around.
        let Some(meta) = meta_at(&dir) else {
            continue;
        };
        if !keep(&meta.root, &meta.agent) {
            continue;
        }
        let root = meta.root.clone();
        out.push((root, describe(&dir, meta)));
    }
    out.sort_by_key(|(_, conv)| std::cmp::Reverse(conv.updated));
    out
}

/// Read a conversation, ready to render. Blocking.
pub fn load(dir: &Path) -> Option<ConversationSnapshot> {
    // The transcript is not opened at all until the metadata has parsed and
    // matched, so a format this build does not know is refused whole rather
    // than read under the wrong schema.
    let meta = meta_at(dir)?;

    let transcript = std::fs::read_to_string(dir.join("items.jsonl")).unwrap_or_default();
    // A line that will not parse costs one item, not the conversation. That is
    // what append-only buys at the tail: a crash mid-write truncates the last
    // line instead of leaving a document nothing can read.
    let mut skipped = false;
    let parsed: Vec<Line> = transcript
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let line = serde_json::from_str(line).ok();
            skipped |= line.is_none();
            line
        })
        .collect();

    let bounded = parsed.len() > MAX_RESTORED_ITEMS;
    let tail = parsed.len().saturating_sub(MAX_RESTORED_ITEMS);
    let mut items: Vec<ChatItem> = Vec::with_capacity(parsed.len() - tail + 1);
    if bounded {
        items.push(ChatItem::notice(
            "Older messages in this conversation are not shown",
        ));
    }
    items.extend(parsed[tail..].iter().map(|line| restore_line(dir, line)));

    Some(ConversationSnapshot {
        session_id: meta.session_id,
        title: meta.title,
        updated: meta.updated,
        prefs: meta.prefs,
        // Every position restored counts as already written, the synthetic
        // notice included, so nothing on screen is ever appended a second time.
        written: items.len(),
        items,
        // A skipped line — a newer build's item kind, or a torn one — is still
        // in the file, and a rewrite from what was read would delete it.
        complete: !bounded && !skipped,
    })
}

#[cfg(test)]
mod tests;
