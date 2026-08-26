//! ACP data model only — no transport, no serde.
//! `AcpRequest` flows UI→worker; `AcpEvent` flows worker→UI. Keeping this layer
//! serde-free means the on-disk transcript mirror (Phase 4) can own its own
//! serialization without coupling.

use serde_json::Value;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// The channel the UI uses to push requests into a live ACP worker. Unbounded so
/// `send` is synchronous and callable from the (non-async) `update`.
pub type ReqTx = mpsc::UnboundedSender<AcpRequest>;
pub type ReqRx = mpsc::UnboundedReceiver<AcpRequest>;

/// A staged prompt attachment (a file the user added with 📎).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub path: PathBuf,
}

/// A request from the UI into the ACP worker for one session.
#[derive(Debug, Clone)]
pub enum AcpRequest {
    /// Send a user prompt, starting a turn.
    Prompt {
        text: String,
        attachments: Vec<Attachment>,
    },
    /// Cancel the in-flight turn.
    Cancel,
    /// Answer a parked permission request. `option_id == None` cancels it.
    PermissionResponse {
        rpc_id: Value,
        option_id: Option<String>,
    },
    /// Answer a parked `elicitation/create` (the agent's multiple-choice question).
    ElicitationResponse {
        rpc_id: Value,
        outcome: ElicitOutcome,
    },
    /// Switch the session mode (a composer selector choice).
    SetMode(String),
    /// Set a config option (e.g. model) via `session/set_config_option`.
    SetConfigOption { config_id: String, value: String },
}

/// A slash command advertised by the agent (`available_commands_update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

/// A session mode the agent offers (the composer mode selector).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub id: String,
    pub name: String,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// A config option group the agent offers (model / effort / agent persona),
/// from `configOptions` in `session/new` and `config_option_update`. The session
/// *mode* is handled separately via the standard `modes` field, so the `"mode"`
/// group is dropped at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOption {
    /// The option's stable id (e.g. `"model"`, `"effort"`).
    pub id: String,
    /// Human label (e.g. `"Model"`).
    pub name: String,
    /// The currently selected value (matches a `ConfigChoice::value`).
    pub current: Option<String>,
    pub choices: Vec<ConfigChoice>,
}

/// One selectable value within a [`ConfigOption`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChoice {
    pub value: String,
    pub name: String,
}

impl std::fmt::Display for ConfigChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

// ── Plan (ACP `plan` update — Claude Code's TodoWrite) ──────────────────────

/// One entry of the agent's plan/checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub content: String,
    pub status: PlanStatus,
}

/// Lifecycle of a plan entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => PlanStatus::InProgress,
            "completed" => PlanStatus::Completed,
            _ => PlanStatus::Pending,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Pending => "pending",
            PlanStatus::InProgress => "in_progress",
            PlanStatus::Completed => "completed",
        }
    }
}

// ── Tool calls ──────────────────────────────────

/// The category of a tool call — drives the badge icon + role color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Fetch,
    Execute,
    Think,
    Other,
}

impl ToolKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "read" => ToolKind::Read,
            "edit" => ToolKind::Edit,
            "delete" => ToolKind::Delete,
            "move" => ToolKind::Move,
            "search" => ToolKind::Search,
            "fetch" => ToolKind::Fetch,
            "execute" => ToolKind::Execute,
            "think" => ToolKind::Think,
            _ => ToolKind::Other,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolKind::Read => "read",
            ToolKind::Edit => "edit",
            ToolKind::Delete => "delete",
            ToolKind::Move => "move",
            ToolKind::Search => "search",
            ToolKind::Fetch => "fetch",
            ToolKind::Execute => "execute",
            ToolKind::Think => "think",
            ToolKind::Other => "other",
        }
    }
}

/// Lifecycle of a tool call (maps to the status lozenge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => ToolStatus::InProgress,
            "completed" => ToolStatus::Completed,
            "failed" => ToolStatus::Failed,
            _ => ToolStatus::Pending,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolStatus::Pending => "pending",
            ToolStatus::InProgress => "in_progress",
            ToolStatus::Completed => "completed",
            ToolStatus::Failed => "failed",
        }
    }
}

/// A piece of a tool call's content (rendered inside its card).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolContent {
    /// Generic text output (a command's stdout, a result blob).
    Text(String),
    /// A file edit, shown as a diff.
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
    /// A reference to a live terminal (the ACP terminal extension); the card
    /// renders the streamed output for this `terminalId`.
    Terminal(String),
    /// An inline image result (decoded bytes). `Arc`d so
    /// clones of the (potentially large) payload are cheap and so the render
    /// layer can key its texture cache by pointer identity.
    Image(std::sync::Arc<Vec<u8>>),
}

/// A tool call surfaced by the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    /// The exact ACP title. For Claude Code Bash calls this remains the raw
    /// command so clients can show it in the expanded detail surface.
    pub title: String,
    /// Optional agent-authored, human-readable summary. Claude Code exposes
    /// Bash's input `description` as `_meta.claudeCode.title`.
    pub description: Option<String>,
    pub kind: ToolKind,
    pub status: ToolStatus,
    pub content: Vec<ToolContent>,
}

/// An in-place update to an existing [`ToolCall`] (only set fields change).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallUpdate {
    pub id: String,
    pub status: Option<ToolStatus>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: Option<Vec<ToolContent>>,
}

// ── Permission (permission buttons) ─────────────────────────────

/// One choice the agent offers for a permission request.
#[derive(Debug, Clone)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once` / `allow_always` / `reject_once` / `reject_always`.
    pub kind: String,
}

/// What an option would actually do, reduced from the protocol's four `kind`
/// strings to the three answers a card has to tell apart.
///
/// Here rather than at the button that draws it: which of these is *the* safe
/// yes is a fact about the protocol, not a styling opinion, and a renderer
/// deciding it from the raw string is a renderer that reads `allow_always` as
/// "allow" and offers the widest grant on the card as its most obvious button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionWeight {
    /// Grant, for this call only.
    AllowOnce,
    /// Grant, and stop asking — the same word with a far longer reach.
    AllowAlways,
    /// Any refusal, **and anything this build does not recognise**: an option
    /// whose meaning is unknown is one that must not be dressed as the answer
    /// to give without reading.
    Deny,
}

impl PermissionOption {
    pub fn weight(&self) -> PermissionWeight {
        match self.kind.as_str() {
            "allow_once" => PermissionWeight::AllowOnce,
            "allow_always" => PermissionWeight::AllowAlways,
            _ => PermissionWeight::Deny,
        }
    }
}

/// A parked permission request awaiting a user choice. `rpc_id` is the raw
/// JSON-RPC id the worker must echo back in its response.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub rpc_id: Value,
    pub tool_call_id: Option<String>,
    pub title: String,
    pub options: Vec<PermissionOption>,
}

// ── Elicitation (the agent *asking a question*, not asking permission) ───────
//
// `elicitation/create` is how Claude Code's `AskUserQuestion` tool reaches the
// client: a form whose fields are enum-typed, i.e. the multiple-choice prompt
// the first-party clients render. The adapter only enables the tool when we
// advertise `clientCapabilities.elicitation.form` (see `init_params`), so this
// whole surface is opt-in. The same channel also carries MCP-server
// elicitations and the model's refusal-fallback consent prompt, which are plain
// forms — hence the generic field model rather than an AskUserQuestion-shaped one.

/// One selectable choice of an [`ElicitField`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitChoice {
    /// The value written back into the response (the schema's `const`).
    pub value: String,
    /// The button label (`title`, else the value itself).
    pub label: String,
    /// Optional secondary line under the label.
    pub description: Option<String>,
}

/// What one [`ElicitField`] asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElicitKind {
    /// Exactly one of the choices.
    Select(Vec<ElicitChoice>),
    /// Any number of the choices (an array-typed field).
    MultiSelect(Vec<ElicitChoice>),
    /// Free text (a schema property with no enum).
    Text,
}

impl ElicitKind {
    pub fn choices(&self) -> &[ElicitChoice] {
        match self {
            ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c,
            ElicitKind::Text => &[],
        }
    }
    pub fn is_multi(&self) -> bool {
        matches!(self, ElicitKind::MultiSelect(_))
    }
}

/// One question of an elicitation form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitField {
    /// The schema property name — the key the answer goes back under.
    pub key: String,
    /// Short heading (the schema's `title`).
    pub title: Option<String>,
    /// The question text (the schema's `description`; absent for a
    /// single-question form, where the prompt is the elicitation `message`).
    pub description: Option<String>,
    pub kind: ElicitKind,
    /// The paired free-text "Other" property (`<key>_custom`), when the form
    /// offers one — a typed answer there overrides the picked choice.
    pub custom_key: Option<String>,
}

/// A parked `elicitation/create` awaiting the user's answers. `rpc_id` is the
/// raw JSON-RPC id the worker must echo back, exactly as for a permission.
#[derive(Debug, Clone)]
pub struct Elicitation {
    pub rpc_id: Value,
    pub tool_call_id: Option<String>,
    /// The prompt line above the fields.
    pub message: String,
    pub fields: Vec<ElicitField>,
}

/// One answer value in the response `content` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElicitValue {
    Text(String),
    List(Vec<String>),
}

/// How the user settled an elicitation.
#[derive(Debug, Clone)]
pub enum ElicitOutcome {
    /// Answered — `(key, value)` pairs folded into the response `content`.
    Accept(Vec<(String, ElicitValue)>),
    /// Skipped: the model is told the user declined and the turn continues.
    Decline,
    /// Aborted (the turn was cancelled) — kills the asking tool call.
    Cancel,
}

/// An event from the ACP worker out to the UI. `Message` wraps this with the
/// session uid so the app routes it to the right chat.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Handshake complete and a session exists. Carries the request channel so
    /// the UI can start sending prompts.
    Connected { tx: ReqTx, resumed: bool },
    /// The agent's ACP session id (used to name the persisted transcript).
    SessionId(String),
    /// A streamed chunk of the agent's reply (markdown).
    AgentChunk(String),
    /// A streamed chunk of the agent's reasoning (the `think`/discovery stream).
    ThoughtChunk(String),
    /// A streamed chunk echoed as the user's message (seen on resume replay).
    UserChunk(String),
    /// A new tool call appeared in the turn.
    ToolCall(ToolCall),
    /// An update to an existing tool call (status / content).
    ToolUpdate(ToolCallUpdate),
    /// The agent (re)published its plan/checklist (Claude Code's TodoWrite) —
    /// the full entry list each time.
    Plan(Vec<PlanEntry>),
    /// The agent is asking permission; parked until the user chooses.
    Permission(PermissionRequest),
    /// The agent is asking a *question* (`AskUserQuestion` / an MCP form);
    /// parked until the user answers or skips.
    Elicitation(Elicitation),
    /// The agent advertised its slash commands (`/` completion source).
    AvailableCommands(Vec<SlashCommand>),
    /// The session's modes (offered by `session/new`).
    Modes {
        current: Option<String>,
        available: Vec<Mode>,
    },
    /// The current mode changed.
    ModeChanged(String),
    /// The session's config options (model/effort/agent), from `session/new`
    /// and `config_option_update`.
    ConfigOptions(Vec<ConfigOption>),
    /// A chunk of live output from a terminal (ACP terminal extension).
    TerminalOutput { terminal_id: String, chunk: String },
    /// A terminal's process exited.
    TerminalExit {
        terminal_id: String,
        exit_code: Option<i32>,
    },
    /// The current turn finished.
    TurnEnded { stop_reason: String },
    /// A non-fatal error reported by the agent (e.g. a failed prompt).
    Error(String),
    /// The adapter died / timed out / closed — the session is now `Failed`
    /// (restartable with Ctrl+Shift+R). Terminal for this worker.
    Disconnected(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(kind: &str) -> PermissionOption {
        PermissionOption {
            id: "id".into(),
            name: "name".into(),
            kind: kind.into(),
        }
    }

    /// The two grants are *not* the same answer, and the classification is the
    /// only thing that keeps them apart: they share a prefix, so any test of
    /// the raw string that is cheap enough to write inline is a test that reads
    /// "allow for ever" as "allow".
    #[test]
    fn the_two_grants_are_told_apart() {
        assert_eq!(option("allow_once").weight(), PermissionWeight::AllowOnce);
        assert_eq!(
            option("allow_always").weight(),
            PermissionWeight::AllowAlways
        );
    }

    #[test]
    fn a_refusal_and_an_unknown_kind_both_land_on_deny() {
        assert_eq!(option("reject_once").weight(), PermissionWeight::Deny);
        assert_eq!(option("reject_always").weight(), PermissionWeight::Deny);
        // An adapter this build has never met offers something it cannot
        // classify; the safe reading of an unreadable option is "no".
        assert_eq!(option("allow_on_tuesdays").weight(), PermissionWeight::Deny);
        assert_eq!(option("").weight(), PermissionWeight::Deny);
    }
}
