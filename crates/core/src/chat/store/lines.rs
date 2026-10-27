//! The line codec: one transcript item as one line of `items.jsonl`, and back.

use super::{fnv1a, MAX_BLOB_BYTES};
use crate::acp::{PlanEntry, PlanStatus, ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::attachment::{AttachmentDelivery, AttachmentKind, AttachmentSnapshot};
use crate::chat::model::{
    Chat, ChatItem, Md, NoticeLevel, PlanItem, SubagentItem, SubagentState, Thought, ToolItem,
    UserMsg,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A turn's failure as a build before it had a level of its own wrote it: an
/// error reading `Error: `, which is how that build said every error a prompt
/// was answered with. The resume fallback it said the same way failed no turn.
/// shortcut: an error that build wrote answering a Stop reads as failed too,
/// which the next prompt answers; drop this once those archives are gone.
fn legacy_failed(text: &str, level: NoticeLevel) -> NoticeLevel {
    match level {
        NoticeLevel::Error
            if text.starts_with("Error: ") && !text.starts_with("Error: session/load failed") =>
        {
            NoticeLevel::Failed
        }
        NoticeLevel::Info | NoticeLevel::Error | NoticeLevel::Failed => level,
    }
}

/// One line of `items.jsonl`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(super) enum Line {
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
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
        /// How long it took, and what it exited with.
        ///
        /// **Both optional on the way in and omitted on the way out when
        /// absent**, which is what lets every conversation written before they
        /// existed still load: a reopened step from an older file simply has
        /// nothing to say about either, which is the truth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secs: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit: Option<i32>,
        /// When it settled, in epoch seconds; absent the same way.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<u64>,
    },
    Plan {
        entries: Vec<StoredPlanEntry>,
    },
    Notice {
        text: String,
        /// Always written. An absent key meaning "quiet" is an implicit case
        /// worth more than the fifteen bytes it saves.
        level: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A subagent's card, its steps inside it. Written only once it has
    /// ended: the file is never rewritten, so a card still working stays out.
    Subagent {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        state: String,
        items: Vec<Line>,
    },
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoredAttachment {
    path: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u64>,
    kind: String,
    delivery: String,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoredPlanEntry {
    text: String,
    status: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(super) enum StoredContent {
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

pub(super) fn restore_line(dir: &Path, line: &Line) -> ChatItem {
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
        Line::Agent { text, message_id } => {
            let mut md = Md::parse(text);
            md.message = message_id.clone();
            ChatItem::Agent(md)
        }
        Line::Thought { text, secs } => ChatItem::Thought(Thought {
            md: Md::parse(text),
            started: None,
            elapsed_secs: *secs,
            fold: None,
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
        Line::Notice {
            text,
            level,
            detail,
        } => ChatItem::Notice {
            text: text.clone(),
            level: legacy_failed(text, NoticeLevel::parse(level)),
            detail: detail.clone(),
        },
        Line::Subagent {
            id,
            name,
            task,
            state,
            items,
        } => ChatItem::Subagent(SubagentItem {
            id: id.clone(),
            name: name.clone(),
            task: task.clone(),
            // Saved working means the session closed under it: nothing will
            // ever end it now.
            state: match state.as_str() {
                "completed" => SubagentState::Completed,
                "failed" => SubagentState::Failed,
                "cancelled" => SubagentState::Cancelled,
                _ => SubagentState::Interrupted,
            },
            items: items.iter().map(|line| restore_line(dir, line)).collect(),
        }),
        Line::Tool {
            id,
            title,
            description,
            kind,
            status,
            content,
            secs,
            exit,
            at,
        } => {
            // A step saved while it ran (a session closed mid-turn) is not
            // running now: nothing will ever settle it, so it comes back
            // stopped rather than starting a clock of its own.
            let status = match ToolStatus::parse(status) {
                ToolStatus::Pending | ToolStatus::InProgress => ToolStatus::Failed,
                settled => settled,
            };
            let mut item = ToolItem::new(ToolCall {
                id: id.clone(),
                title: title.clone(),
                description: description.clone(),
                kind: ToolKind::parse(kind),
                status,
                content: content.iter().map(|c| restore_content(dir, c)).collect(),
            });
            item.elapsed_secs = *secs;
            item.settled_at = *at;
            item.exit_code = *exit;
            ChatItem::Tool(item)
        }
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

// ── into lines ──────────────────────────────────────────────────────────────

/// An image kept beside the transcript: the file it goes in, and its bytes.
pub(crate) type Blob = (String, Arc<Vec<u8>>);

/// One transcript item as a line, plus any blob it needs written beside it.
///
/// `None` for the two cards that are a question rather than a record: a
/// permission or an elicitation whose adapter is gone would come back offering
/// buttons that answer a request nothing is waiting on.
pub(crate) fn line_of(chat: &Chat, item: &ChatItem) -> Option<(String, Vec<Blob>)> {
    let mut blobs = Vec::new();
    let line = to_line(chat, item, &mut blobs)?;
    // Compact, never pretty: this is a line, and a line broken across lines is
    // no longer one.
    let text = serde_json::to_string(&line).ok()?;
    Some((text, blobs))
}

fn to_line(chat: &Chat, item: &ChatItem, blobs: &mut Vec<Blob>) -> Option<Line> {
    Some(match item {
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
            message_id: md.message.clone(),
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
        ChatItem::Notice {
            text,
            level,
            detail,
        } => Line::Notice {
            text: text.clone(),
            level: level.as_str().to_string(),
            detail: detail.clone(),
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
                .map(|c| content_of(chat, c, blobs))
                .collect(),
            secs: t.elapsed_secs,
            exit: t.exit_code,
            at: t.settled_at,
        },
        ChatItem::Subagent(card) => Line::Subagent {
            id: card.id.clone(),
            name: card.name.clone(),
            task: card.task.clone(),
            state: card.state.as_str().to_string(),
            items: card
                .items
                .iter()
                .filter_map(|item| to_line(chat, item, blobs))
                .collect(),
        },
        ChatItem::Permission(_) | ChatItem::Ask(_) => return None,
    })
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
