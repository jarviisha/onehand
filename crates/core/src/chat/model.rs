//! The chat model and its event reducer — the whole conversation, with no
//! widget state in it.
//!
//! [`Chat`] holds the transcript, the agent-advertised sources (files, slash
//! commands, modes, config options) and the live request channel; [`Chat::apply`]
//! folds an [`AcpEvent`](crate::acp::AcpEvent) into it. Everything a *front end* needs on top of this
//! — a composer buffer, scroll offsets, find state, parsed-markdown or texture
//! caches — belongs to that front end, not here.

use crate::acp::{ConfigOption, Mode, ReqTx, SlashCommand};
use std::collections::HashMap;
use std::path::PathBuf;

mod apply;
mod background;
mod items;
mod meta;
mod persist;
mod subagent;
mod turn;
pub use apply::{ApplyOutcome, Away, UserAsk};
pub use background::{BackgroundItem, Level, Meter};
pub(crate) use items::line_change_counts;
pub use items::{
    AskItem, AskRow, ChatItem, Md, MdId, NoticeLevel, PermItem, PlanItem, Thought, ToolItem,
    TranscriptItemId, TurnAnswer, UserMsg, MAX_TERM_BYTES,
};
pub(crate) use meta::summarize_title;
pub use meta::{Selector, SelectorChoice, MODE_SELECTOR};
pub use subagent::{SubagentItem, SubagentState};
pub use turn::{QueuedPrompt, SubmitBlock};

/// Where a conversation's adapter is in its life.
///
/// [`Chat::tx`] cannot answer this. It is `None` both *before* the handshake
/// finishes and *after* the adapter dies, and those are opposite things to say
/// to the user: one means wait, the other means this needs you. A front end
/// that inferred "failed" from a missing channel would light up every session
/// for the second or two it takes to connect.
///
/// This lives on the conversation, not on the workspace tree's `Session`: the
/// reducer is the only thing that ever sees `Connected` / `Disconnected`, so
/// it is the only thing that can keep the answer true. (An earlier
/// `Session::runtime` tried to hold it one level up and was never written
/// once.)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Spawned; the handshake has not landed yet.
    #[default]
    Connecting,
    /// Live.
    Connected,
    /// The adapter went away. A restart brings it back.
    Lost,
}

/// Where a resumed conversation is between adopting its archive and finding out
/// what the agent actually replayed.
///
/// It carries the archive rather than a yes/no because of what the middle state
/// costs. A `session/load` replays the conversation as ordinary content events,
/// so the first of them has to drop the adopted copy or the transcript shows
/// everything twice — but at that moment the replay has *started*, not
/// finished, and there is no event anywhere in the protocol that says it has.
/// An adapter that dies or stalls halfway therefore leaves a transcript holding
/// two messages of fifty, and every save writes what the transcript holds. The
/// two-message version was written over the fifty-message file, which is a
/// conversation destroyed by reopening it.
///
/// So the adopted copy is kept until something settles the question, and until
/// then it — not the live transcript — is what gets archived.
#[derive(Default)]
enum Replay {
    /// Not resuming, or the question is answered: `history ⧺ items` is the
    /// record.
    #[default]
    Settled,
    /// An archive was adopted and nothing has been replayed into it yet.
    Armed,
    /// Replayed content has started arriving. The vec is what `history` held —
    /// the last transcript known to be whole. What is in `items` is a
    /// re-delivery of it that may stop at any point, so it is not yet the
    /// record.
    Partial(Vec<ChatItem>),
}

/// Per-session chat state (keyed by session uid in `App`).
#[derive(Default)]
pub struct Chat {
    /// The owning session's uid. Scopes this chat's widget ids (the composer
    /// input, the completion list): widget operations run against *every*
    /// window's interface, so a process-global id would focus/scroll another
    /// window's composer too.
    pub uid: u64,
    pub items: Vec<ChatItem>,
    /// Read-only transcript loaded on resume; dropped once a real replay
    /// delivers agent content.
    pub history: Vec<ChatItem>,
    pub session_id: Option<String>,
    /// Whether the agent serving this conversation forks sessions, as its
    /// last handshake said.
    pub(crate) forkable: bool,
    /// The session's root + agent name, for the persisted metadata.
    pub root: PathBuf,
    pub agent: String,
    /// What to say when the agent answers that it is not logged in: the
    /// entry's own words, carrying the command that logs it in, when it has
    /// any.
    pub login_hint: Option<String>,
    /// The agent just answered that it is not logged in, so the failure that
    /// follows is said as how to log in rather than in the protocol's words:
    /// with the agent's own advice, when it gave some.
    login_pending: Option<Option<String>>,
    /// User-chosen conversation title. `None` keeps automatic title generation
    /// from the first prompt; persisted independently so Reset can restore it.
    pub custom_title: Option<String>,
    /// Last activity (epoch secs): bumped on every applied agent event and on
    /// each submitted prompt, seeded from the archive's `updated` on resume.
    /// Drives the rail session row's relative-time label. Runtime-only.
    pub(crate) last_activity: Option<u64>,
    /// Where this conversation is between adopting an archive and knowing what
    /// the agent's replay actually delivered. See [`Replay`].
    replay: Replay,
    /// Whether the trailing `User` item is still an *open* chunk target: user
    /// chunks of one replayed message merge into it, but any other content
    /// event seals it so the next user chunk starts its own bubble (two
    /// prompts replayed back-to-back around agent/tool content must not
    /// concatenate into one).
    user_chunk_open: bool,
    /// The live request channel once `Connected`; `None` while not running.
    pub(crate) tx: Option<ReqTx>,
    /// Whether the adapter is coming up, live, or gone. See [`Link`].
    pub link: Link,
    /// A turn is in flight (Send becomes Stop).
    pub busy: bool,
    /// A prompt written mid-turn, sent the moment the turn ends.
    ///
    /// Here rather than in a front end because *when* it goes is decided here:
    /// the reducer is the only thing that sees a turn end, and a queue flushed
    /// from anywhere else is a queue that flushes late, twice, or never.
    pub queued: Option<QueuedPrompt>,
    /// How many prompts this app has sent the agent.
    ///
    /// Not the user rows in the transcript: an adapter can deliver user chunks
    /// of its own mid-turn, and each lands there as a user row, so counting the
    /// rows counts things nobody here typed. This moves only when a prompt
    /// actually goes out, which is what anything asking "has somebody else
    /// prompted this session" needs.
    pub prompts_sent: usize,
    /// The turn was cancelled — a Stop, here or from the remote bridge —
    /// rather than finished by the agent: asked for during it, or said by
    /// its stop reason. Holds until the next prompt goes out.
    pub cancelled: bool,
    pub(crate) resumed: bool,

    // ── composer sources (Phase 3B) ──
    /// Root-relative file paths for `@`-mention completion.
    pub files: Vec<String>,
    /// The directories [`Self::files`] passes through, with what is under each.
    ///
    /// **Derived once, where the list it is derived from changes once.** It was
    /// rebuilt inside the popup, which is rebuilt on every keystroke and on
    /// every frame an agent streams into — a walk of the whole file list and a
    /// `BTreeMap` of every directory in the project, thrown away and done again
    /// a moment later. Nothing about it changes between those frames.
    pub folders: Vec<(String, usize)>,
    /// Whether the scan that fills [`Self::files`] has finished.
    ///
    /// **An empty list is not the same as a list that has not arrived**, and
    /// the `@` popup has to tell them apart: one means "still looking", the
    /// other means there is nothing here to name. Inferred from the list being
    /// empty, a project with no files to offer said it was still looking for
    /// them forever.
    pub files_scanned: bool,
    /// Agent-advertised slash commands for `/` completion.
    pub commands: Vec<SlashCommand>,
    /// Session modes offered by the agent (composer selector).
    pub modes: Vec<Mode>,
    /// The currently selected mode id.
    pub current_mode: Option<String>,
    /// Agent config options (model/effort/agent) — composer selectors.
    pub config_options: Vec<ConfigOption>,
    /// What the agent last confirmed for each pick, by group (the mode
    /// selector or a config option's id): what a refused pick goes back to.
    confirmed: HashMap<String, String>,
    /// How many picks sent to the agent are still unanswered, by group: a
    /// refusal while a newer pick waits is superseded by it, so only the
    /// newest pick's refusal puts back what is shown.
    picks_out: HashMap<String, u32>,
    /// The mode + config picks a *resumed* conversation was last using, armed
    /// from the archive on load and replayed once the adapter reconnects
    /// (`reapply_prefs`). The adapter rebuilds effort/agent from static settings
    /// on `session/load`, so without this a reopened session loses them.
    pub(crate) pending_mode: Option<String>,
    pub(crate) pending_config: Vec<(String, String)>,
    /// Files staged with 📎 to send with the next prompt.
    /// Live terminals (ACP terminal extension), keyed by terminalId.
    pub terminals: HashMap<String, TermView>,

    // ── what runs beside the turns ──
    /// The agent's live background work, oldest first, by its own reports.
    background: Vec<background::BackgroundItem>,
    /// The latest context report: tokens used, and the window's size.
    context: Option<(u64, u64)>,
    /// The latest rate-limit report of each kind, in the order first seen.
    limits: Vec<crate::acp::RateLimit>,
    /// The adapter has said which cycle a report closes, so it says when its
    /// own work between turns settles and no quiet spell has to guess.
    cycles_marked: bool,
    /// How many times the transcript has changed, counting from zero.
    ///
    /// A front end derives expensive things from a transcript -- a run layout,
    /// a search -- and needs to know when to derive them again. Lengths alone
    /// cannot answer that: a streaming answer, a tool changing status and a
    /// terminal printing a line all rewrite an item that is already there.
    ///
    /// A counter rather than a description of what changed. It can only ever
    /// be read as "something did", so the worst a missed distinction costs is
    /// work the caller would have done anyway -- where a wrong *description*
    /// would leave the wrong thing on screen.
    revision: u64,
    /// How many times a fold has been toggled, whoever holds it: an item's
    /// own here, a group's in the front end, which reports its own through
    /// [`Self::fold_changed`].
    ///
    /// Apart from [`Self::revision`] because folding changes no item, yet the
    /// run layout reads the item folds to choose how far a run stands from
    /// its neighbours, so it has to be built again when one moves.
    folds: u64,
    /// Moved each time the transcript is numbered afresh, so whatever is keyed
    /// by an item's position knows its keys now name other items.
    numbering: u64,

    // ── what is on disk ──
    /// Where this conversation is kept. `None` never persists — which is what
    /// makes a chat built in a test provably inert, and what lets the store's
    /// own tests point one at a directory of their own.
    pub(crate) store: Option<PathBuf>,
    /// How many transcript positions (history then items) are already written.
    ///
    /// Positions rather than lines: the two differ only by the cards that are a
    /// question rather than a record, which are never written and never move.
    /// Advanced when a save is *built*, not when it lands, so a second save
    /// before the first has finished has nothing left to add rather than a
    /// duplicate of it.
    persisted: usize,
    /// The next save replaces the transcript file instead of adding to it.
    ///
    /// Set in exactly one place, and see it for why: a replay that arrives
    /// chunked differently from the file cannot be spliced onto it.
    rewrite: bool,
    /// Whether what was read back stopped at the read bound.
    ///
    /// A conversation long enough to hit it comes back as its tail, and a tail
    /// must never be written back over the file it is a tail of. Written this
    /// way round so a chat that was never read from disk — which is every chat
    /// in every test — is not one by default.
    bounded: bool,
}

/// Live state of one referenced terminal (the card renders this).
#[derive(Default)]
pub struct TermView {
    pub output: String,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

impl Chat {
    /// A chat bound to a session's uid + root + agent (uid scopes widget ids;
    /// root/agent feed the persisted metadata), kept in `store`.
    ///
    /// The store is a parameter rather than something reached for inside,
    /// because a chat that persists and a chat that does not are the same type
    /// and the difference has to be made where one is built.
    pub fn new(uid: u64, root: PathBuf, agent: String, store: Option<PathBuf>) -> Self {
        let mut chat = Self::default();
        chat.uid = uid;
        chat.root = root;
        chat.agent = agent;
        chat.store = store;
        chat
    }

    /// The transcript's current revision. See the field.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many folds have been toggled. See the field.
    pub fn folds(&self) -> u64 {
        self.folds
    }

    /// How many times the transcript has been numbered afresh.
    pub fn numbering(&self) -> u64 {
        self.numbering
    }

    /// Record that a fold was toggled.
    pub fn fold_changed(&mut self) {
        self.folds = self.folds.wrapping_add(1);
    }

    /// Record that the transcript changed.
    ///
    /// Called from the few places that mutate what is rendered: [`Self::apply`]
    /// for every event that is not pure session metadata, [`Self::push_user`]
    /// for a locally-staged prompt, [`Self::load_history`] for an adopted
    /// archive, and the two that settle a blocking card. Item *fold* state is
    /// deliberately not one of them -- folding changes what a row draws, and a
    /// row is drawn from the live item either way; a fold moves
    /// [`Self::folds`] instead.
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests;
