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

use crate::acp::{PlanEntry, PlanStatus, ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::attachment::{AttachmentDelivery, AttachmentKind, AttachmentSnapshot};
use crate::chat::model::{Chat, ChatItem, Md, NoticeLevel, PlanItem, Thought, ToolItem, UserMsg};
use crate::config::config_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
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

/// FNV-1a — tiny, dependency-free, and stable across runs, which is all either
/// caller needs of it.
fn fnv1a(bytes: &[u8]) -> u64 {
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
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<ConfigPick>,
}

/// One config-option pick (`id` → chosen `value`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigPick {
    pub id: String,
    pub value: String,
}

/// One line of `items.jsonl`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Line {
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<StoredAttachment>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sent_at: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        completed_at: Option<u64>,
    },
    Agent {
        text: String,
    },
    Thought {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        secs: Option<u64>,
    },
    Tool {
        id: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        kind: String,
        status: String,
        content: Vec<StoredContent>,
    },
    Plan {
        entries: Vec<StoredPlanEntry>,
    },
    Notice {
        text: String,
        /// Always written. An absent key meaning "quiet" is an implicit case
        /// worth more than the fifteen bytes it saves.
        level: String,
    },
}

#[derive(Serialize, Deserialize)]
struct StoredAttachment {
    path: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u64>,
    kind: String,
    delivery: String,
}

#[derive(Serialize, Deserialize)]
struct StoredPlanEntry {
    text: String,
    status: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum StoredContent {
    Text {
        text: String,
    },
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
    /// An image kept beside the transcript rather than inside it. Base64 in the
    /// line would inflate it by a third and make every read of the conversation
    /// — including the ones that only wanted its title — carry the picture.
    Image {
        blob: String,
        bytes: u64,
    },
    /// An image too large to keep. Recorded, because a card that silently lost
    /// its picture reads as the transcript having been wrong about there being
    /// one.
    ImageOmitted {
        bytes: u64,
    },
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
    pub created: u64,
    pub prefs: Prefs,
    /// Renderable items, already parsed. Not lines: the restart path hands over
    /// live items, and round-tripping those through the archive form would
    /// flatten a running terminal into a snapshot of its output.
    pub items: Vec<ChatItem>,
    /// How many transcript positions are already on disk — the mark the
    /// adopting chat carries on from.
    pub written: usize,
    /// Whether `items` is the whole conversation. False when the read hit its
    /// bound, which is what forbids rewriting the file from it.
    pub complete: bool,
}

/// Everything one save has to put on disk, prepared while the transcript is in
/// hand so the writing itself needs nothing but this.
pub struct PendingWrite {
    pub(crate) dir: PathBuf,
    pub(crate) lines: Vec<String>,
    pub(crate) blobs: Vec<Blob>,
    pub(crate) meta: MetaWrite,
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
    pub(crate) session_id: String,
    pub(crate) root: String,
    pub(crate) agent: String,
    pub(crate) title: Option<String>,
    pub(crate) preview: String,
    pub(crate) prefs: Prefs,
    /// `Some` when this save added messages, and then it is the moment they
    /// were added. `None` leaves whatever is on disk alone.
    pub(crate) updated: Option<u64>,
    pub(crate) items: usize,
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

/// Read one conversation's metadata. Blocking.
pub fn read_meta(dir: &Path) -> Option<ConvMeta> {
    meta_at(dir).map(|meta| describe(dir, meta))
}

/// Conversations under `store` belonging to `project`, and to `agent` when one
/// is named, newest first.
///
/// Reads one small file per conversation and opens no transcript at all.
/// Blocking — call it off the UI loop.
pub fn list_conversations(store: &Path, project: &Path, agent: Option<&str>) -> Vec<ConvMeta> {
    let project = project.display().to_string();
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
        if meta.root != project || agent.is_some_and(|a| meta.agent != a) {
            continue;
        }
        out.push(describe(&dir, meta));
    }
    out.sort_by_key(|conv| std::cmp::Reverse(conv.updated));
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
    let parsed: Vec<Line> = transcript
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    let complete = parsed.len() <= MAX_RESTORED_ITEMS;
    let tail = parsed.len().saturating_sub(MAX_RESTORED_ITEMS);
    let mut items: Vec<ChatItem> = Vec::with_capacity(parsed.len() - tail + 1);
    if !complete {
        items.push(ChatItem::notice(
            "Older messages in this conversation are not shown",
        ));
    }
    items.extend(parsed[tail..].iter().map(|line| restore_line(dir, line)));

    Some(ConversationSnapshot {
        session_id: meta.session_id,
        title: meta.title,
        updated: meta.updated,
        created: meta.created,
        prefs: meta.prefs,
        // Every position restored counts as already written, the synthetic
        // notice included, so nothing on screen is ever appended a second time.
        written: items.len(),
        items,
        complete,
    })
}

fn restore_line(dir: &Path, line: &Line) -> ChatItem {
    match line {
        Line::User {
            text,
            attachments,
            sent_at,
            completed_at,
        } => ChatItem::User(UserMsg {
            text: text.clone(),
            attachments: attachments.iter().map(restore_attachment).collect(),
            sent_at: *sent_at,
            completed_at: *completed_at,
        }),
        Line::Agent { text } => ChatItem::Agent(Md::parse(text)),
        Line::Thought { text, secs } => ChatItem::Thought(Thought {
            md: Md::parse(text),
            started: None,
            elapsed_secs: *secs,
            expanded: false,
        }),
        Line::Plan { entries } => ChatItem::Plan(PlanItem::new(
            entries
                .iter()
                .map(|e| PlanEntry {
                    content: e.text.clone(),
                    status: PlanStatus::parse(&e.status),
                })
                .collect(),
        )),
        Line::Notice { text, level } => ChatItem::Notice {
            text: text.clone(),
            level: NoticeLevel::parse(level),
        },
        Line::Tool {
            id,
            title,
            description,
            kind,
            status,
            content,
        } => ChatItem::Tool(ToolItem::new(ToolCall {
            id: id.clone(),
            title: title.clone(),
            description: description.clone(),
            kind: ToolKind::parse(kind),
            status: ToolStatus::parse(status),
            content: content.iter().map(|c| restore_content(dir, c)).collect(),
        })),
    }
}

fn restore_attachment(a: &StoredAttachment) -> AttachmentSnapshot {
    AttachmentSnapshot {
        path: PathBuf::from(&a.path),
        name: a.name.clone(),
        bytes: a.bytes,
        kind: if a.kind == "image" {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        },
        delivery: match a.delivery.as_str() {
            "inline_image" => AttachmentDelivery::InlineImage,
            "resource_link" => AttachmentDelivery::ResourceLink,
            _ => AttachmentDelivery::Unavailable,
        },
    }
}

fn restore_content(dir: &Path, c: &StoredContent) -> ToolContent {
    match c {
        StoredContent::Text { text } => ToolContent::Text(text.clone()),
        StoredContent::Diff { path, old, new } => ToolContent::Diff {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        },
        // A blob that is gone degrades to a placeholder rather than a dump, the
        // same as an image that could not be decoded always did.
        StoredContent::Image { blob, .. } => std::fs::read(dir.join("blobs").join(blob))
            .map(|bytes| ToolContent::Image(Arc::new(bytes)))
            .unwrap_or_else(|_| ToolContent::Text("(image)".to_string())),
        StoredContent::ImageOmitted { bytes } => {
            ToolContent::Text(format!("(image, {}, not archived)", size_of_bytes(*bytes)))
        }
    }
}

/// A byte count as something to read, for the one line that has to say why a
/// picture is not here.
fn size_of_bytes(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} kB", bytes.max(1024) / 1024)
    }
}

// ── writing ─────────────────────────────────────────────────────────────────

/// Apply a prepared save: the new lines, their blobs, then the metadata.
///
/// Blocking; call it off the UI loop. Errors are returned rather than swallowed
/// — a transcript is the one thing here the user cannot produce again.
pub fn commit(write: &PendingWrite) -> std::io::Result<()> {
    // One writer per conversation within this process, so two windows or two
    // sessions on one conversation are ordered rather than interleaved. The
    // key is the conversation itself, so unlike a check on which window holds
    // what, it cannot fail to notice a second writer.
    let lock = writer_for(&write.dir);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());

    std::fs::create_dir_all(&write.dir)?;
    if !write.blobs.is_empty() {
        let blobs = write.dir.join("blobs");
        std::fs::create_dir_all(&blobs)?;
        for (name, bytes) in &write.blobs {
            // Content-addressed, so writing one that is already there is the
            // same file again — which is what makes a rewrite safe to repeat.
            let path = blobs.join(name);
            if !path.exists() {
                std::fs::write(&path, bytes.as_slice())?;
            }
        }
    }

    let items = write.dir.join("items.jsonl");
    if write.rewrite && !write.lines.is_empty() {
        // The one write here that can take something away, so it is also the
        // one with a condition on it: replacing a transcript with nothing is
        // never what a caller meant, and this is the only path that could.
        let mut body = write.lines.join("\n");
        body.push('\n');
        crate::config::write_atomic(&items, &body)?;
    } else if !write.rewrite && !write.lines.is_empty() {
        // Opened fresh and closed each time, and written in one call: appending
        // is then a single operation the kernel does not split, so two
        // processes adding to one conversation interleave whole turns rather
        // than halves of a line. Holding the file open would also leave a
        // writer adding into an inode a rewrite had already replaced.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&items)?;
        let mut body = write.lines.join("\n");
        body.push('\n');
        file.write_all(body.as_bytes())?;
    }

    write_meta(&write.dir, &write.meta)
}

/// Write `meta.json`, keeping the fields that are the file's to remember.
///
/// `created` and — when this save added no messages — `updated` are read back
/// off the existing file rather than carried in. They belong to the
/// conversation's history rather than to the moment being saved, and a rename
/// that reordered the picker would be the list rearranging itself for something
/// that is not new work.
fn write_meta(dir: &Path, meta: &MetaWrite) -> std::io::Result<()> {
    let path = dir.join("meta.json");
    let existing: Option<Meta> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let now = now_secs();
    let out = Meta {
        format: FORMAT,
        session_id: meta.session_id.clone(),
        root: meta.root.clone(),
        agent: meta.agent.clone(),
        created: existing.as_ref().map(|m| m.created).unwrap_or(now),
        updated: meta
            .updated
            .or_else(|| existing.as_ref().map(|m| m.updated))
            .unwrap_or(now),
        items: meta.items,
        title: meta.title.clone(),
        preview: meta.preview.clone(),
        prefs: meta.prefs.clone(),
    };
    let text = serde_json::to_string_pretty(&out).map_err(std::io::Error::other)?;
    crate::config::write_atomic(&path, &text)
}

/// The lock for one conversation directory, made on first use.
fn writer_for(dir: &Path) -> Arc<Mutex<()>> {
    static WRITERS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = WRITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// An image kept beside the transcript: the file it goes in, and its bytes.
pub(crate) type Blob = (String, Arc<Vec<u8>>);

/// One transcript item as a line, plus any blob it needs written beside it.
///
/// `None` for the two cards that are a question rather than a record: a
/// permission or an elicitation whose adapter is gone would come back offering
/// buttons that answer a request nothing is waiting on.
pub(crate) fn line_of(chat: &Chat, item: &ChatItem) -> Option<(String, Vec<Blob>)> {
    let mut blobs = Vec::new();
    let line = match item {
        ChatItem::User(u) => Line::User {
            text: u.text.clone(),
            attachments: u
                .attachments
                .iter()
                .map(|a| StoredAttachment {
                    path: a.path.display().to_string(),
                    name: a.name.clone(),
                    bytes: a.bytes,
                    kind: match a.kind {
                        AttachmentKind::Image => "image",
                        AttachmentKind::File => "file",
                    }
                    .to_string(),
                    delivery: match a.delivery {
                        AttachmentDelivery::InlineImage => "inline_image",
                        AttachmentDelivery::ResourceLink => "resource_link",
                        AttachmentDelivery::Unavailable => "unavailable",
                    }
                    .to_string(),
                })
                .collect(),
            sent_at: u.sent_at,
            completed_at: u.completed_at,
        },
        ChatItem::Agent(md) => Line::Agent {
            text: md.source.clone(),
        },
        ChatItem::Thought(th) => Line::Thought {
            text: th.md.source.clone(),
            secs: th.elapsed_secs,
        },
        ChatItem::Plan(p) => Line::Plan {
            entries: p
                .entries
                .iter()
                .map(|e| StoredPlanEntry {
                    text: e.content.clone(),
                    status: e.status.as_str().to_string(),
                })
                .collect(),
        },
        ChatItem::Notice { text, level } => Line::Notice {
            text: text.clone(),
            level: level.as_str().to_string(),
        },
        ChatItem::Tool(t) => Line::Tool {
            id: t.call.id.clone(),
            title: t.call.title.clone(),
            description: t.call.description.clone(),
            kind: t.call.kind.as_str().to_string(),
            status: t.call.status.as_str().to_string(),
            content: t
                .call
                .content
                .iter()
                .map(|c| content_of(chat, c, &mut blobs))
                .collect(),
        },
        ChatItem::Permission(_) | ChatItem::Ask(_) => return None,
    };
    // Compact, never pretty: this is a line, and a line broken across lines is
    // no longer one.
    let text = serde_json::to_string(&line).ok()?;
    Some((text, blobs))
}

fn content_of(chat: &Chat, content: &ToolContent, blobs: &mut Vec<Blob>) -> StoredContent {
    match content {
        ToolContent::Text(s) => StoredContent::Text { text: s.clone() },
        ToolContent::Diff { path, old, new } => StoredContent::Diff {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        },
        // A live terminal is flattened to what it has printed: the process it
        // belongs to does not outlive the session, so a handle to it would come
        // back pointing at nothing.
        ToolContent::Terminal(id) => StoredContent::Text {
            text: chat
                .terminals
                .get(id)
                .map(|v| v.output.clone())
                .unwrap_or_default(),
        },
        ToolContent::Image(bytes) => {
            let len = bytes.len() as u64;
            if len > MAX_BLOB_BYTES {
                return StoredContent::ImageOmitted { bytes: len };
            }
            let name = format!("{:016x}.bin", fnv1a(bytes));
            blobs.push((name.clone(), bytes.clone()));
            StoredContent::Image {
                blob: name,
                bytes: len,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp directory of this test's own, cleaned up on the way out.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onehand-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn chat_in(store: &Path, sid: &str) -> Chat {
        let mut chat = Chat::new(
            1,
            PathBuf::from("/r"),
            "Claude".into(),
            Some(store.to_path_buf()),
        );
        chat.session_id = Some(sid.to_string());
        chat
    }

    fn save(chat: &mut Chat) {
        if let Some(write) = chat.flush() {
            commit(&write).unwrap();
        }
    }

    #[test]
    fn sanitize_keeps_uuid_and_replaces_unsafe() {
        assert_eq!(sanitize("3ff1f752-4189-8601"), "3ff1f752-4189-8601");
        assert!(sanitize("a/b c").starts_with("a_b_c-"));
        assert_ne!(sanitize("a/b"), sanitize("a.b"));
        assert_ne!(sanitize("a/b"), "a_b");
    }

    /// The reason the format changed: a turn writes its own turn, not the
    /// conversation it is part of.
    #[test]
    fn a_turn_appends_only_what_it_added() {
        let store = scratch("append");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("first".into(), Vec::new());
        save(&mut chat);

        let items = conv_dir(&store, "s1").join("items.jsonl");
        let after_one = std::fs::read(&items).unwrap();

        chat.push_user("second".into(), Vec::new());
        save(&mut chat);
        let after_two = std::fs::read(&items).unwrap();

        assert!(
            after_two.starts_with(&after_one),
            "the first turn's bytes were rewritten rather than kept"
        );
        assert_eq!(after_two.iter().filter(|b| **b == b'\n').count(), 2);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_second_save_with_nothing_new_writes_no_line() {
        let store = scratch("nothing-new");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("only".into(), Vec::new());
        save(&mut chat);
        let items = conv_dir(&store, "s1").join("items.jsonl");
        let once = std::fs::read(&items).unwrap();

        save(&mut chat);
        assert_eq!(std::fs::read(&items).unwrap(), once);
        let _ = std::fs::remove_dir_all(&store);
    }

    /// The property that keeps a session nobody used from overwriting one
    /// somebody did.
    #[test]
    fn an_empty_chat_creates_no_directory() {
        let store = scratch("empty");
        let mut chat = chat_in(&store, "s1");
        assert!(chat.flush().is_none());

        // …and neither does one that has never been given a session id.
        let mut nameless = Chat::new(1, PathBuf::from("/r"), "Claude".into(), Some(store.clone()));
        nameless.push_user("typed but never sent anywhere".into(), Vec::new());
        assert!(nameless.flush().is_none());

        assert!(!conv_dir(&store, "s1").exists());
        let _ = std::fs::remove_dir_all(&store);
    }

    /// Listing is the operation that used to parse every transcript in full,
    /// images included, to read a title off the front.
    #[test]
    fn listing_never_opens_the_transcript() {
        let store = scratch("listing");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("Fix the login flow".into(), Vec::new());
        save(&mut chat);

        std::fs::write(conv_dir(&store, "s1").join("items.jsonl"), "not json").unwrap();
        let found = list_conversations(&store, Path::new("/r"), Some("Claude"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Fix the login flow");
        assert_eq!(found[0].session_id, "s1");
        // Another project's conversations are not this project's.
        assert!(list_conversations(&store, Path::new("/other"), None).is_empty());
        // Nor another agent's.
        assert!(list_conversations(&store, Path::new("/r"), Some("Other")).is_empty());
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn an_unknown_format_is_neither_listed_nor_loaded() {
        let store = scratch("format");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("hello".into(), Vec::new());
        save(&mut chat);

        let dir = conv_dir(&store, "s1");
        let text = std::fs::read_to_string(dir.join("meta.json")).unwrap();
        let bumped = text.replace(
            &format!("\"format\": {FORMAT}"),
            &format!("\"format\": {}", FORMAT + 1),
        );
        assert_ne!(bumped, text, "the format field must be what was replaced");
        std::fs::write(dir.join("meta.json"), bumped).unwrap();

        assert!(read_meta(&dir).is_none());
        assert!(load(&dir).is_none());
        assert!(list_conversations(&store, Path::new("/r"), None).is_empty());
        assert!(dir.exists(), "and it is refused, not removed");
        let _ = std::fs::remove_dir_all(&store);
    }

    /// What append-only buys at the tail.
    #[test]
    fn a_torn_last_line_costs_one_item_not_the_conversation() {
        let store = scratch("torn");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("one".into(), Vec::new());
        chat.push_user("two".into(), Vec::new());
        save(&mut chat);

        let items = conv_dir(&store, "s1").join("items.jsonl");
        let mut text = std::fs::read_to_string(&items).unwrap();
        text.push_str("{\"t\":\"user\",\"tex");
        std::fs::write(&items, text).unwrap();

        let back = load(&conv_dir(&store, "s1")).unwrap();
        assert_eq!(back.items.len(), 2);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn an_image_is_a_blob_and_the_transcript_stays_small() {
        let store = scratch("blob");
        let mut chat = chat_in(&store, "s1");
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "t1".into(),
            title: "Read shot.png".into(),
            description: None,
            kind: ToolKind::parse("read"),
            status: ToolStatus::parse("completed"),
            content: vec![ToolContent::Image(Arc::new(b"pretend-png".to_vec()))],
        })));
        save(&mut chat);

        let dir = conv_dir(&store, "s1");
        let text = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
        assert!(
            !text.contains("pretend-png"),
            "the bytes are not in the line"
        );
        let blobs: Vec<_> = std::fs::read_dir(dir.join("blobs"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(blobs.len(), 1);
        assert_eq!(std::fs::read(blobs[0].path()).unwrap(), b"pretend-png");

        let back = load(&dir).unwrap();
        let ChatItem::Tool(t) = &back.items[0] else {
            panic!("expected a tool card")
        };
        let ToolContent::Image(bytes) = &t.call.content[0] else {
            panic!("expected an image")
        };
        assert_eq!(bytes.as_slice(), b"pretend-png");

        // And a blob that has gone missing degrades rather than disappearing.
        std::fs::remove_file(blobs[0].path()).unwrap();
        let back = load(&dir).unwrap();
        let ChatItem::Tool(t) = &back.items[0] else {
            panic!()
        };
        assert!(matches!(&t.call.content[0], ToolContent::Text(s) if s == "(image)"));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn an_oversized_image_is_recorded_but_not_stored() {
        let store = scratch("big-blob");
        let mut chat = chat_in(&store, "s1");
        let huge = Arc::new(vec![0u8; (MAX_BLOB_BYTES + 1) as usize]);
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "t1".into(),
            title: "Read huge.png".into(),
            description: None,
            kind: ToolKind::parse("read"),
            status: ToolStatus::parse("completed"),
            content: vec![ToolContent::Image(huge)],
        })));
        save(&mut chat);

        let dir = conv_dir(&store, "s1");
        assert!(!dir.join("blobs").exists(), "nothing that large is kept");
        let back = load(&dir).unwrap();
        let ChatItem::Tool(t) = &back.items[0] else {
            panic!()
        };
        // It says what was there, rather than leaving a card that looks wrong
        // about having held a picture.
        assert!(matches!(&t.call.content[0], ToolContent::Text(s) if s.contains("not archived")));
        let _ = std::fs::remove_dir_all(&store);
    }

    /// A rename is not new work, and the list must not reorder for it.
    #[test]
    fn a_rename_rewrites_only_the_metadata() {
        let store = scratch("rename");
        let mut chat = chat_in(&store, "s1");
        chat.push_user("Fix the login flow".into(), Vec::new());
        save(&mut chat);

        let dir = conv_dir(&store, "s1");
        let items = std::fs::read(dir.join("items.jsonl")).unwrap();
        let was = read_meta(&dir).unwrap().updated;

        assert!(chat.rename("Authentication cleanup"));
        commit(&chat.flush_meta().unwrap()).unwrap();

        assert_eq!(std::fs::read(dir.join("items.jsonl")).unwrap(), items);
        let meta = read_meta(&dir).unwrap();
        assert_eq!(meta.title, "Authentication cleanup");
        assert_eq!(meta.updated, was, "a rename is not a message");
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_bounded_load_says_so_and_keeps_the_tail() {
        let store = scratch("bounded");
        let mut chat = chat_in(&store, "s1");
        for i in 0..MAX_RESTORED_ITEMS + 10 {
            chat.push_user(format!("message {i}"), Vec::new());
        }
        save(&mut chat);

        let back = load(&conv_dir(&store, "s1")).unwrap();
        assert!(!back.complete);
        assert_eq!(
            back.items.len(),
            MAX_RESTORED_ITEMS + 1,
            "the tail plus a line saying so"
        );
        assert!(matches!(back.items[0], ChatItem::Notice { .. }));
        let ChatItem::User(last) = back.items.last().unwrap() else {
            panic!()
        };
        assert_eq!(last.text, format!("message {}", MAX_RESTORED_ITEMS + 9));
        let _ = std::fs::remove_dir_all(&store);
    }

    /// Two writers on one conversation add whole turns, never halves of a line.
    #[test]
    fn two_writers_in_one_process_do_not_interleave() {
        let store = scratch("writers");
        let dir = conv_dir(&store, "s1");
        std::thread::scope(|scope| {
            for who in 0..2 {
                let store = store.clone();
                scope.spawn(move || {
                    let mut chat = chat_in(&store, "s1");
                    for i in 0..20 {
                        chat.push_user(format!("writer {who} message {i}"), Vec::new());
                        save(&mut chat);
                    }
                });
            }
        });

        let text = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 40);
        for line in lines {
            serde_json::from_str::<Line>(line).expect("every line parses whole");
        }
        let _ = std::fs::remove_dir_all(&store);
    }
}
