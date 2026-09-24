//! The chat: the conversation model, its event reducer, and its on-disk archive.
//!
//! Folding an `AcpEvent` stream into a transcript is not drawing, and it is the
//! half worth testing — so it lives here rather than in the front end.
//!
//! The rule that keeps it honest: **nothing in here may know how the chat is
//! drawn.** A composer buffer, a scroll offset, a parsed-markdown cache or a
//! texture handle is front-end state and lives with the front end.

pub mod activity;
pub mod attention;
mod export;
pub mod model;
pub mod steps;
pub mod store;

pub use activity::{
    first_line_trunc, group, presentation, ActivityGroup, ActivityKind, Presentation,
};
pub use attention::{Attention, Presence, Telling};
pub use export::export_markdown;
pub use model::{
    ApplyOutcome, AskItem, AskRow, Away, Chat, ChatItem, Link, Md, MdId, NoticeLevel, PermItem,
    PlanItem, QueuedPrompt, Selector, SelectorChoice, SubmitBlock, TermView, Thought, ToolItem,
    TranscriptItemId, TurnAnswer, UserAsk, UserMsg, COMMAND_FOLD_LINES, MAX_TERM_BYTES,
    MODE_SELECTOR,
};
pub use steps::{
    cluster_summary, redact, run_outcome, turn_changes, turn_file_diff, ClusterSummary, FileChange,
    FileVerdict, Outcome, RunOutcome, SummaryPart, TurnChanges,
};
pub use store::{
    commit, conv_dir, conversations_dir, delete, list_conversations, load, now_secs, ConfigPick,
    ConvMeta, ConversationSnapshot, PendingWrite, Prefs,
};
