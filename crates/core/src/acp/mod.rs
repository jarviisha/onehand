//! Agent Client Protocol client. A minimal
//! newline-delimited JSON-RPC-over-stdio client, split by concern:
//!
//! - [`types`] — the serde-free data model (`AcpRequest` in, `AcpEvent` out).
//! - [`parse`] — `session/update` → [`parse::Update`] (pure, tested).
//! - [`client`] — the subprocess + JSON-RPC loop, exposed as a `Stream`.
//!
//! Tool calls, selectors, completion, attachments and the terminal extension
//! arrive in later phases.

pub mod client;
pub mod parse;
mod roles;
pub mod terminal;
pub mod types;

pub use client::{base64_encode, connect, Opening, SessionOpts};
pub use parse::base64_decode;
pub use roles::{Role, Switch};
pub use types::{
    AcpEvent, AcpRequest, Attachment, BackgroundReport, ConfigChoice, ConfigOption, ElicitChoice,
    ElicitField, ElicitKind, ElicitOutcome, ElicitValue, Elicitation, LimitStatus, Mode,
    PermissionOption, PermissionRequest, PermissionWeight, PlanEntry, PlanStatus, RateLimit, ReqTx,
    SlashCommand, SubagentEnd, ToolCall, ToolCallUpdate, ToolContent, ToolKind, ToolStatus, Usage,
};
