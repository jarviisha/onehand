//! The chat: the conversation model, its event reducer, and its on-disk archive.
//!
//! Folding an `AcpEvent` stream into a transcript is not drawing, and it is the
//! half worth testing — so it lives here rather than in the front end.
//!
//! The rule that keeps it honest: **nothing in here may know how the chat is
//! drawn.** A composer buffer, a scroll offset, a parsed-markdown cache or a
//! texture handle is front-end state and lives with the front end.

pub mod activity;
pub mod find;
pub mod model;
pub mod store;

pub use activity::{
    first_line_trunc, group, presentation, ActivityGroup, ActivityKind, Presentation,
};
pub use find::{compute_matches, export_markdown, item_search_text, TranscriptMatch};
pub use model::{
    ApplyOutcome, AskItem, Chat, ChatItem, Link, Md, MdId, NoticeLevel, PermItem, PlanItem,
    QueuedPrompt, SubmitBlock, TermView, Thought, ToolItem, TranscriptItemId, TurnAnswer, UserMsg,
    MAX_TERM_BYTES,
};
pub use store::{
    commit, conv_dir, conversations_dir, delete, list_conversations, load, now_secs, ConfigPick,
    ConvMeta, ConversationSnapshot, PendingWrite, Prefs,
};
