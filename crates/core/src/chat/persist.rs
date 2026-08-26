//! On-disk transcript persistence. A serde
//! mirror of [`ChatItem`], kept separate so `acp::types` stays serde-free and so
//! a parsed-markdown cache (which can't serialize) is never part of the model.
//!
//! Each conversation is one JSON file under `<config_dir>/onehand/sessions/`,
//! named by the ACP `sessionId`, carrying metadata (root, agent, updated, items).

use crate::acp::{PlanEntry, PlanStatus, ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::attachment::{AttachmentDelivery, AttachmentKind, AttachmentSnapshot};
use crate::chat::model::{Chat, ChatItem, Md, NoticeLevel, PlanItem, Thought, ToolItem, UserMsg};
use crate::config::config_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: u32 = 1;

/// The persisted shape of one conversation.
#[derive(Serialize, Deserialize)]
pub struct StoredConversation {
    pub version: u32,
    pub session_id: String,
    pub root: String,
    pub agent: String,
    pub updated: u64,
    /// User-chosen display title. `None` means derive it from the first prompt;
    /// older archives naturally take that path too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub items: Vec<StoredItem>,
    /// The session's last mode + config-option picks, replayed on resume so a
    /// reopened conversation comes back on the same effort/agent/mode it had
    /// (back-compat: absent in old files → an empty default, nothing replayed).
    #[serde(default)]
    pub prefs: StoredPrefs,
}

/// The selector state (mode + config options) a conversation was last using.
/// The adapter rebuilds effort/agent from *static* settings on every
/// `session/new`/`session/load`, so without replaying these a resumed session
/// silently drops the session's own picks. (Model is the exception — the SDK
/// re-reads it from the transcript — so it's persisted for completeness but not
/// re-applied; see `Chat::reapply_prefs`.)
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct StoredPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<StoredConfig>,
}

/// One persisted config-option pick (`id` → chosen `value`).
#[derive(Serialize, Deserialize, Clone)]
pub struct StoredConfig {
    pub id: String,
    pub value: String,
}

/// Backward-compatible attachment archive. Version-1 conversations stored a
/// bare path string; new writes retain enough presentation metadata for a
/// useful card even after the source file moves.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum StoredAttachment {
    Path(String),
    Snapshot {
        path: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<u64>,
        kind: String,
        delivery: String,
    },
}

impl From<&str> for StoredAttachment {
    fn from(path: &str) -> Self {
        Self::Path(path.to_string())
    }
}

impl StoredAttachment {
    fn from_snapshot(attachment: &AttachmentSnapshot) -> Self {
        Self::Snapshot {
            path: attachment.path.display().to_string(),
            name: attachment.name.clone(),
            bytes: attachment.bytes,
            kind: match attachment.kind {
                AttachmentKind::Image => "image",
                AttachmentKind::File => "file",
            }
            .to_string(),
            delivery: match attachment.delivery {
                AttachmentDelivery::InlineImage => "inline_image",
                AttachmentDelivery::ResourceLink => "resource_link",
                AttachmentDelivery::Unavailable => "unavailable",
            }
            .to_string(),
        }
    }

    fn restore(&self) -> AttachmentSnapshot {
        match self {
            Self::Path(path) => AttachmentSnapshot::from_path(PathBuf::from(path)),
            Self::Snapshot {
                path,
                name,
                bytes,
                kind,
                delivery,
            } => AttachmentSnapshot {
                path: PathBuf::from(path),
                name: name.clone(),
                bytes: *bytes,
                kind: if kind == "image" {
                    AttachmentKind::Image
                } else {
                    AttachmentKind::File
                },
                delivery: match delivery.as_str() {
                    "inline_image" => AttachmentDelivery::InlineImage,
                    "resource_link" => AttachmentDelivery::ResourceLink,
                    _ => AttachmentDelivery::Unavailable,
                },
            },
        }
    }
}

/// A persisted transcript item (mirror of the renderable [`ChatItem`]).
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum StoredItem {
    User {
        text: String,
        /// Attached file paths (back-compat: absent in old files).
        #[serde(default)]
        attachments: Vec<StoredAttachment>,
        /// Send time in epoch seconds (back-compat: absent in old files).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sent_at: Option<u64>,
        /// Response completion in epoch seconds (back-compat: absent in old files).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_at: Option<u64>,
    },
    Agent {
        text: String,
    },
    Thought {
        text: String,
        /// Reasoning duration in seconds (back-compat: absent in old files).
        #[serde(default)]
        secs: Option<u64>,
    },
    Tool {
        id: String,
        title: String,
        /// Optional natural-language tool summary (absent in older archives).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        kind: String,
        status: String,
        content: Vec<StoredContent>,
    },
    /// The agent's plan/checklist (TodoWrite).
    Plan {
        entries: Vec<StoredPlanEntry>,
    },
    Notice {
        text: String,
        /// `error` for a real failure; absent in archives written before
        /// notices carried one, which read back quiet — the same as they were
        /// drawn when they were written.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        level: String,
    },
}

#[derive(Serialize, Deserialize)]
pub struct StoredPlanEntry {
    pub text: String,
    pub status: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum StoredContent {
    Text {
        text: String,
    },
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
    /// An inline image result, stored base64-encoded.
    Image {
        b64: String,
    },
}

/// Lightweight metadata for the resume picker (no full transcript).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvMeta {
    pub path: PathBuf,
    pub session_id: String,
    /// The agent this conversation ran under (its `AgentSpec.name`).
    pub agent: String,
    pub updated: u64,
    pub title: String,
    pub item_count: usize,
}

/// `<config_dir>/onehand/sessions/`.
pub fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}

/// The archive path for a session id (filename sanitized).
pub fn conv_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", sanitize(session_id)))
}

/// Filename-safe form of a session id. When sanitizing actually changed the
/// string, a short hash of the *raw* id is appended so two ids differing only
/// in punctuation (`a/b` vs `a_b`) can't collapse onto one archive file and
/// clobber each other. UUID-style ids pass through untouched.
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
    // FNV-1a over the raw id — tiny, dependency-free, stable across runs.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{safe}-{h:08x}")
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build the serializable snapshot of the chat (history + live items) without
/// touching disk. Returns `None` when there is no session id or nothing to save,
/// so a fresh session never clobbers a saved one. The (cheap) snapshot
/// is built on the UI thread; the (blocking) write is handed to [`write_stored`].
pub fn build_stored(chat: &Chat) -> Option<StoredConversation> {
    let sid = chat.session_id.as_deref()?;
    let items: Vec<StoredItem> = chat
        .history
        .iter()
        .chain(chat.items.iter())
        .filter_map(|it| store_item(chat, it))
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(StoredConversation {
        version: VERSION,
        session_id: sid.to_string(),
        root: chat.root.display().to_string(),
        agent: chat.agent.clone(),
        updated: now_secs(),
        title: chat.custom_title.clone(),
        items,
        prefs: capture_prefs(chat),
    })
}

/// Snapshot the chat's current mode + config-option picks for the archive.
pub fn capture_prefs(chat: &Chat) -> StoredPrefs {
    StoredPrefs {
        mode: chat.current_mode.clone(),
        config: chat
            .config_options
            .iter()
            .filter_map(|o| {
                o.current.clone().map(|value| StoredConfig {
                    id: o.id.clone(),
                    value,
                })
            })
            .collect(),
    }
}

/// Serialize + write a prepared snapshot to its session file. Blocking; call from
/// a blocking pool (`spawn_blocking`), not inline in the UI loop.
///
/// Write-then-rename, with a per-call temp name: the turn-end pool write and the
/// synchronous `Drop` save can race on the same conversation, and a plain
/// `fs::write` interleaving (or a crash mid-write) would leave truncated JSON
/// that [`load`] rejects forever — the conversation would silently vanish from
/// the resume picker. The rename is atomic, so the file is always one complete
/// snapshot (last writer wins).
pub fn write_stored(conv: &StoredConversation) {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = conv_path(&conv.session_id);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(conv) {
        let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!("tmp{seq}"));
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// Synchronous build + write, for the `Drop` path (session close / app exit,
/// where there is no async context). The hot per-turn path uses
/// [`build_stored`] + an off-thread [`write_stored`] instead.
pub fn save_chat(chat: &Chat) {
    if let Some(conv) = build_stored(chat) {
        write_stored(&conv);
    }
}

fn store_item(chat: &Chat, it: &ChatItem) -> Option<StoredItem> {
    Some(match it {
        ChatItem::User(u) => StoredItem::User {
            text: u.text.clone(),
            attachments: u
                .attachments
                .iter()
                .map(StoredAttachment::from_snapshot)
                .collect(),
            sent_at: u.sent_at,
            completed_at: u.completed_at,
        },
        ChatItem::Plan(p) => StoredItem::Plan {
            entries: p
                .entries
                .iter()
                .map(|e| StoredPlanEntry {
                    text: e.content.clone(),
                    status: e.status.as_str().to_string(),
                })
                .collect(),
        },
        ChatItem::Agent(md) => StoredItem::Agent {
            text: md.source.clone(),
        },
        ChatItem::Thought(th) => StoredItem::Thought {
            text: th.md.source.clone(),
            secs: th.elapsed_secs,
        },
        ChatItem::Notice { text, level } => StoredItem::Notice {
            text: text.clone(),
            level: level.as_str().to_string(),
        },
        ChatItem::Tool(t) => StoredItem::Tool {
            id: t.call.id.clone(),
            title: t.call.title.clone(),
            description: t.call.description.clone(),
            kind: t.call.kind.as_str().to_string(),
            status: t.call.status.as_str().to_string(),
            content: t
                .call
                .content
                .iter()
                .map(|c| store_content(chat, c))
                .collect(),
        },
        // Transient interactions, not archived.
        ChatItem::Permission(_) | ChatItem::Ask(_) => return None,
    })
}

fn store_content(chat: &Chat, c: &ToolContent) -> StoredContent {
    match c {
        ToolContent::Text(s) => StoredContent::Text { text: s.clone() },
        ToolContent::Diff { path, old, new } => StoredContent::Diff {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        },
        // Flatten a live terminal to its captured output at save time.
        ToolContent::Terminal(id) => StoredContent::Text {
            text: chat
                .terminals
                .get(id)
                .map(|v| v.output.clone())
                .unwrap_or_default(),
        },
        ToolContent::Image(bytes) => StoredContent::Image {
            b64: crate::acp::base64_encode(bytes),
        },
    }
}

/// Load a stored conversation file.
pub fn load(path: &Path) -> Option<StoredConversation> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Rebuild renderable [`ChatItem`]s from a stored conversation.
pub fn restore(stored: &StoredConversation) -> Vec<ChatItem> {
    stored.items.iter().map(restore_item).collect()
}

fn restore_item(it: &StoredItem) -> ChatItem {
    match it {
        StoredItem::User {
            text,
            attachments,
            sent_at,
            completed_at,
        } => ChatItem::User(UserMsg {
            text: text.clone(),
            attachments: attachments.iter().map(StoredAttachment::restore).collect(),
            sent_at: *sent_at,
            completed_at: *completed_at,
        }),
        StoredItem::Plan { entries } => ChatItem::Plan(PlanItem::new(
            entries
                .iter()
                .map(|e| PlanEntry {
                    content: e.text.clone(),
                    status: PlanStatus::parse(&e.status),
                })
                .collect(),
        )),
        StoredItem::Agent { text } => ChatItem::Agent(Md::parse(text)),
        StoredItem::Thought { text, secs } => ChatItem::Thought(Thought {
            md: Md::parse(text),
            started: None,
            elapsed_secs: *secs,
            expanded: false,
        }),
        StoredItem::Notice { text, level } => ChatItem::Notice {
            text: text.clone(),
            level: NoticeLevel::parse(level),
        },
        StoredItem::Tool {
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
            content: content.iter().map(restore_content).collect(),
        })),
    }
}

fn restore_content(c: &StoredContent) -> ToolContent {
    match c {
        StoredContent::Text { text } => ToolContent::Text(text.clone()),
        StoredContent::Diff { path, old, new } => ToolContent::Diff {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        },
        // An unreadable stored image degrades to a placeholder, never a dump.
        StoredContent::Image { b64 } => crate::acp::base64_decode(b64)
            .map(|bytes| ToolContent::Image(Arc::new(bytes)))
            .unwrap_or_else(|| ToolContent::Text("(image)".to_string())),
    }
}

/// List archived conversations for a `(root, agent)`, newest first.
pub fn list_conversations(root: &Path, agent: &str) -> Vec<ConvMeta> {
    list_matching(root, Some(agent))
}

/// List archived conversations for a `root` across *all* agents, newest first
/// (the empty-pane picker when a root has no session yet).
pub fn list_root_conversations(root: &Path) -> Vec<ConvMeta> {
    list_matching(root, None)
}

/// Shared listing: conversations whose `root` matches and (if `agent` is
/// `Some`) whose agent matches, newest first.
fn list_matching(root: &Path, agent: Option<&str>) -> Vec<ConvMeta> {
    let root = root.display().to_string();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(sessions_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(conv) = load(&path) else { continue };
        if conv.root != root || agent.is_some_and(|a| conv.agent != a) {
            continue;
        }
        let preview = conv
            .items
            .iter()
            .find_map(|it| match it {
                StoredItem::User { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "(no messages)".to_string());
        let title = conv
            .title
            .or_else(|| crate::chat::model::summarize_title(&preview))
            .unwrap_or_else(|| conv.agent.clone());
        out.push(ConvMeta {
            path,
            session_id: conv.session_id,
            agent: conv.agent,
            updated: conv.updated,
            title,
            item_count: conv.items.len(),
        });
    }
    // Newest first: the picker's top row is the conversation most likely wanted.
    out.sort_by_key(|conv| std::cmp::Reverse(conv.updated));
    out
}

/// Delete a conversation archive.
pub fn delete(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_uuid_and_replaces_unsafe() {
        assert_eq!(sanitize("3ff1f752-4189-8601"), "3ff1f752-4189-8601");
        // A changed id gains a hash suffix of the raw string…
        assert!(sanitize("a/b c").starts_with("a_b_c-"));
        // …so punctuation-differing ids can no longer share one file.
        assert_ne!(sanitize("a/b"), sanitize("a.b"));
        assert_ne!(sanitize("a/b"), "a_b");
    }

    #[test]
    fn snapshot_persists_only_the_custom_title_override() {
        let mut chat = Chat::new(1, PathBuf::from("/r"), "Claude".into());
        chat.session_id = Some("s1".into());
        chat.items
            .push(ChatItem::User(UserMsg::text("Fix the login flow")));

        assert!(build_stored(&chat).unwrap().title.is_none());
        assert!(chat.rename("Authentication cleanup"));
        assert_eq!(
            build_stored(&chat).unwrap().title.as_deref(),
            Some("Authentication cleanup")
        );
        chat.reset_title();
        assert!(build_stored(&chat).unwrap().title.is_none());
    }

    #[test]
    fn stored_conversation_roundtrips() {
        let conv = StoredConversation {
            version: VERSION,
            session_id: "s1".into(),
            root: "/r".into(),
            agent: "Claude".into(),
            updated: 42,
            title: Some("Fix login".into()),
            items: vec![
                StoredItem::User {
                    text: "hi".into(),
                    attachments: Vec::new(),
                    sent_at: Some(1_700_000_000),
                    completed_at: Some(1_700_000_012),
                },
                StoredItem::Tool {
                    id: "t1".into(),
                    title: "Edit".into(),
                    description: Some("Update the login handler".into()),
                    kind: "edit".into(),
                    status: "completed".into(),
                    content: vec![StoredContent::Diff {
                        path: "a.rs".into(),
                        old: Some("x".into()),
                        new: "y".into(),
                    }],
                },
            ],
            prefs: StoredPrefs {
                mode: Some("plan".into()),
                config: vec![StoredConfig {
                    id: "effort".into(),
                    value: "high".into(),
                }],
            },
        };
        let json = serde_json::to_string(&conv).unwrap();
        let back: StoredConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title.as_deref(), Some("Fix login"));
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.prefs.mode.as_deref(), Some("plan"));
        let StoredItem::User {
            sent_at,
            completed_at,
            ..
        } = &back.items[0]
        else {
            panic!("expected stored user message")
        };
        assert_eq!(*sent_at, Some(1_700_000_000));
        assert_eq!(*completed_at, Some(1_700_000_012));
        let StoredItem::Tool { description, .. } = &back.items[1] else {
            panic!("expected stored tool")
        };
        assert_eq!(description.as_deref(), Some("Update the login handler"));
        assert_eq!(back.prefs.config[0].id, "effort");
        let items = restore(&back);
        assert!(matches!(items[0], ChatItem::User(_)));
        assert!(matches!(items[1], ChatItem::Tool(_)));
    }

    #[test]
    fn plan_attachments_and_images_roundtrip() {
        let conv = StoredConversation {
            version: VERSION,
            session_id: "s2".into(),
            root: "/r".into(),
            agent: "Claude".into(),
            updated: 42,
            title: None,
            items: vec![
                StoredItem::User {
                    text: "see this".into(),
                    attachments: vec!["/tmp/shot.png".into()],
                    sent_at: Some(1_700_000_000),
                    completed_at: Some(1_700_000_045),
                },
                StoredItem::Plan {
                    entries: vec![
                        StoredPlanEntry {
                            text: "a".into(),
                            status: "completed".into(),
                        },
                        StoredPlanEntry {
                            text: "b".into(),
                            status: "in_progress".into(),
                        },
                    ],
                },
                StoredItem::Tool {
                    id: "t1".into(),
                    title: "Read shot.png".into(),
                    description: None,
                    kind: "read".into(),
                    status: "completed".into(),
                    content: vec![StoredContent::Image {
                        b64: "Zm9vYmFy".into(),
                    }],
                },
            ],
            prefs: StoredPrefs::default(),
        };
        let json = serde_json::to_string(&conv).unwrap();
        let back: StoredConversation = serde_json::from_str(&json).unwrap();
        let items = restore(&back);
        let ChatItem::User(u) = &items[0] else {
            panic!()
        };
        assert_eq!(u.attachments.len(), 1);
        assert_eq!(u.attachments[0].path, PathBuf::from("/tmp/shot.png"));
        assert_eq!(u.sent_at, Some(1_700_000_000));
        assert_eq!(u.completed_at, Some(1_700_000_045));
        let ChatItem::Plan(p) = &items[1] else {
            panic!()
        };
        assert_eq!(p.entries[0].status, PlanStatus::Completed);
        assert_eq!(p.entries[1].status, PlanStatus::InProgress);
        let ChatItem::Tool(t) = &items[2] else {
            panic!()
        };
        let ToolContent::Image(bytes) = &t.call.content[0] else {
            panic!("expected image")
        };
        assert_eq!(bytes.as_slice(), b"foobar");

        // Legacy files without `attachments` still load (serde default).
        let legacy = r#"{"t":"user","text":"old"}"#;
        let it: StoredItem = serde_json::from_str(legacy).unwrap();
        assert!(matches!(
            it,
            StoredItem::User {
                ref attachments,
                sent_at: None,
                completed_at: None,
                ..
            } if attachments.is_empty()
        ));
    }

    /// A failure survives being archived, and an archive that predates levels
    /// comes back quiet.
    ///
    /// The round trip is the whole point of the field: a conversation reopened
    /// tomorrow has to still say the adapter died, and it says so in the shape
    /// of the notice rather than in the words, which are the agent's.
    #[test]
    fn a_notices_level_survives_the_archive() {
        let items = [
            ChatItem::notice("the turn was interrupted"),
            ChatItem::error("Disconnected: adapter exited"),
        ];
        let stored: Vec<StoredItem> = items
            .iter()
            .map(|it| store_item(&Chat::default(), it).unwrap())
            .collect();
        let json = serde_json::to_string(&stored).unwrap();
        // The quiet one costs no key on disk; only a failure is spelled out.
        assert!(!json.contains(r#""level":"""#));
        assert!(json.contains(r#""level":"error""#));

        let back: Vec<StoredItem> = serde_json::from_str(&json).unwrap();
        let levels: Vec<NoticeLevel> = back
            .iter()
            .map(|it| match restore_item(it) {
                ChatItem::Notice { level, .. } => level,
                _ => panic!("expected a notice"),
            })
            .collect();
        assert_eq!(levels, vec![NoticeLevel::Info, NoticeLevel::Error]);

        // Written before notices carried a level: quiet, which is how it was
        // drawn when it was written.
        let legacy: StoredItem = serde_json::from_str(r#"{"t":"notice","text":"old"}"#).unwrap();
        assert!(matches!(
            restore_item(&legacy),
            ChatItem::Notice {
                level: NoticeLevel::Info,
                ..
            }
        ));
    }

    #[test]
    fn legacy_conversation_without_prefs_loads_with_empty_prefs() {
        // A file written before `prefs` existed: the `#[serde(default)]` field
        // fills in an empty default, so nothing is replayed on resume.
        let legacy = r#"{"version":1,"session_id":"s","root":"/r","agent":"Claude",
            "updated":1,"items":[{"t":"agent","text":"hi"}]}"#;
        let back: StoredConversation = serde_json::from_str(legacy).unwrap();
        assert!(back.title.is_none());
        assert!(back.prefs.mode.is_none());
        assert!(back.prefs.config.is_empty());
    }
}
