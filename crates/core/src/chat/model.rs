//! The chat model and its event reducer — the whole conversation, with no
//! widget state in it.
//!
//! [`Chat`] holds the transcript, the agent-advertised sources (files, slash
//! commands, modes, config options) and the live request channel; [`Chat::apply`]
//! folds an [`AcpEvent`] into it. Everything a *front end* needs on top of this
//! — a composer buffer, scroll offsets, find state, parsed-markdown or texture
//! caches — belongs to that front end, not here.

use crate::acp::{
    AcpEvent, AcpRequest, Attachment, ConfigOption, ElicitKind, ElicitOutcome, ElicitValue,
    Elicitation, Mode, PermissionRequest, PlanEntry, PlanStatus, ReqTx, SlashCommand, ToolCall,
    ToolCallUpdate, ToolContent, ToolStatus,
};
use crate::attachment::{AttachmentDelivery, AttachmentSnapshot, StagedAttachment};
use crate::chat::store;
use crate::chat::store::{ConfigPick, ConversationSnapshot, MetaWrite, PendingWrite, Prefs};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// Max bytes of terminal output retained per terminal (tail kept).
pub const MAX_TERM_BYTES: usize = 64 * 1024;

/// Stable identity of one transcript item across the read-only resumed history
/// and the live tail. Keeping the source in the type removes the old
/// `usize::MAX` sentinel and prevents fold/search actions from addressing the
/// wrong collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptItemId {
    History(usize),
    Live(usize),
}

impl TranscriptItemId {
    pub const fn index(self) -> usize {
        match self {
            Self::History(index) | Self::Live(index) => index,
        }
    }

    pub const fn is_history(self) -> bool {
        matches!(self, Self::History(_))
    }
}

/// A permission request rendered as a card with option buttons.
pub struct PermItem {
    pub req: PermissionRequest,
    /// The chosen option's name once answered (buttons then disable).
    pub resolved: Option<String>,
}

/// An elicitation — the agent's *question* (`AskUserQuestion`) — rendered as a
/// card of option buttons, plus the answer the user is building up.
pub struct AskItem {
    pub req: Elicitation,
    /// Per field, the picked choice indices. A single-select keeps at most one.
    pub picked: Vec<Vec<usize>>,
    /// Per field, the typed free-text answer (its "Other" box, or the field
    /// itself when it's a text field). Overrides that field's picks.
    pub custom: Vec<String>,
    /// A one-line summary of the answer once settled (controls then disable).
    pub resolved: Option<String>,
    /// Which question the card is showing. A multi-question form renders as a
    /// tab strip (one tab per field) with only the active field's choices
    /// below it — stacking them all made a form taller than the pane, and the
    /// overflow was simply lost off the top of the sticky bar.
    pub tab: usize,
}

impl AskItem {
    pub fn new(req: Elicitation) -> Self {
        let n = req.fields.len();
        Self {
            req,
            picked: vec![Vec::new(); n],
            custom: vec![String::new(); n],
            resolved: None,
            tab: 0,
        }
    }

    /// The showing question, clamped — `tab` is a view cursor, and a form is
    /// never empty (an empty schema is declined before it becomes a card).
    pub fn active_field(&self) -> usize {
        self.tab.min(self.req.fields.len().saturating_sub(1))
    }

    /// Whether `field` carries an answer yet — drives the tab's done tick, so
    /// the user can see what's still open without visiting every tab.
    pub fn field_answered(&self, field: usize) -> bool {
        self.picked.get(field).is_some_and(|p| !p.is_empty())
            || self.custom.get(field).is_some_and(|c| !c.trim().is_empty())
    }

    /// Pick (single-select) or toggle (multi-select) a choice. Picks and typed
    /// text are mutually exclusive per field — the adapter resolves a field
    /// that carries both in favour of the text, so letting both show at once
    /// would render a selection that isn't the answer.
    pub fn toggle(&mut self, field: usize, option: usize) {
        let Some(f) = self.req.fields.get(field) else {
            return;
        };
        if let Some(c) = self.custom.get_mut(field) {
            c.clear();
        }
        let Some(picked) = self.picked.get_mut(field) else {
            return;
        };
        if f.kind.is_multi() {
            match picked.iter().position(|&i| i == option) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(option),
            }
        } else {
            *picked = vec![option];
        }
    }

    /// Type into a field's free-text box; a non-blank answer drops that
    /// field's picks (see [`Self::toggle`] for why they can't coexist).
    pub fn set_custom(&mut self, field: usize, value: String) {
        let Some(slot) = self.custom.get_mut(field) else {
            return;
        };
        *slot = value;
        if !slot.trim().is_empty() {
            if let Some(picked) = self.picked.get_mut(field) {
                picked.clear();
            }
        }
    }

    /// A one-question single-select form: a click *is* the answer, so it
    /// submits straight away instead of arming a Submit button (the shape
    /// `AskUserQuestion` almost always takes).
    pub fn is_quick(&self) -> bool {
        matches!(self.req.fields.as_slice(), [f] if matches!(f.kind, ElicitKind::Select(_)))
    }

    /// Anything picked or typed — gates the Submit button.
    pub fn has_answer(&self) -> bool {
        self.picked.iter().any(|p| !p.is_empty())
            || self.custom.iter().any(|c| !c.trim().is_empty())
    }

    /// Whether `field`'s free-text box is offered (a select's "Other", or the
    /// field itself being free text).
    pub fn has_custom(&self, field: usize) -> bool {
        self.req
            .fields
            .get(field)
            .is_some_and(|f| f.custom_key.is_some() || matches!(f.kind, ElicitKind::Text))
    }

    /// The `(key, value)` pairs for the response `content`: a typed answer wins
    /// over that field's picks (the user wrote their own instead of choosing),
    /// and an unanswered field contributes nothing.
    pub fn answers(&self) -> Vec<(String, ElicitValue)> {
        let mut out = Vec::new();
        for (i, f) in self.req.fields.iter().enumerate() {
            let typed = self.custom.get(i).map(|s| s.trim()).unwrap_or_default();
            if !typed.is_empty() {
                let key = match (&f.kind, &f.custom_key) {
                    (ElicitKind::Text, _) => f.key.clone(),
                    (_, Some(k)) => k.clone(),
                    // A select with no "Other" property has nowhere to put free
                    // text; its box isn't rendered, so this can't be reached.
                    (_, None) => continue,
                };
                out.push((key, ElicitValue::Text(typed.to_string())));
                continue;
            }
            let choices = f.kind.choices();
            let values: Vec<String> = self.picked[i]
                .iter()
                .filter_map(|&c| choices.get(c).map(|c| c.value.clone()))
                .collect();
            if values.is_empty() {
                continue;
            }
            out.push(match f.kind {
                ElicitKind::MultiSelect(_) => (f.key.clone(), ElicitValue::List(values)),
                _ => (f.key.clone(), ElicitValue::Text(values.concat())),
            });
        }
        out
    }

    /// The question as plain text for the clipboard: the prompt, then each
    /// field's own heading and its choices as a bullet list (an option's
    /// description trails its label). What the user picked is deliberately left
    /// out — this copies the *question*, so it stays pasteable while the card
    /// is still open.
    pub fn copy_text(&self) -> String {
        let mut out = self.req.message.clone();
        for f in &self.req.fields {
            for line in [f.title.as_ref(), f.description.as_ref()]
                .into_iter()
                .flatten()
            {
                out.push_str("\n\n");
                out.push_str(line);
            }
            for c in f.kind.choices() {
                out.push_str("\n- ");
                out.push_str(&c.label);
                if let Some(d) = &c.description {
                    out.push_str(": ");
                    out.push_str(d);
                }
            }
        }
        out.push('\n');
        out
    }

    /// The audit-trail line shown once answered — the chosen *labels* (not the
    /// wire values), comma-joined.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (i, f) in self.req.fields.iter().enumerate() {
            let typed = self.custom.get(i).map(|s| s.trim()).unwrap_or_default();
            if !typed.is_empty() && self.has_custom(i) {
                parts.push(typed.to_string());
                continue;
            }
            let choices = f.kind.choices();
            parts.extend(
                self.picked[i]
                    .iter()
                    .filter_map(|&c| choices.get(c).map(|c| c.label.clone())),
            );
        }
        if parts.is_empty() {
            "Skipped".into()
        } else {
            parts.join(", ")
        }
    }
}

/// Identity of one [`Md`] block, unique for the life of the process.
///
/// Front-end parse caches are keyed by this. A monotone counter rather than an
/// index or an address: transcript items move when the `Vec` grows, and an
/// index renumbers when history is prepended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MdId(u64);

impl MdId {
    fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// One block of streamed markdown: the raw source plus the fold state of its
/// code blocks and tables.
///
/// The *parsed* form a renderer needs (a `TextViewState` in the GPUI front end)
/// is deliberately absent — it is a derived cache of `source`, and it is
/// framework-shaped. The front end keeps its own, keyed by [`Md::id`];
/// `source` staying authoritative is also what lets persistence round-trip a
/// block without a parser.
pub struct Md {
    /// Process-unique, assigned at construction: a front end's parsed-markdown
    /// cache needs a key that survives the transcript `Vec` reallocating under
    /// it, so neither an index nor an address will do.
    pub id: MdId,
    pub source: String,
    /// Code blocks the user expanded, keyed by **fence-open order** within this
    /// block : a monotone counter that never renumbers on
    /// re-parse, so a fold set mid-stream survives later blocks arriving.
    pub open_blocks: HashSet<usize>,
    /// Tables whose >`MAX_TABLE_ROWS` tail the user revealed, keyed by table
    /// order within this block.
    pub open_tables: HashSet<usize>,
}

impl Md {
    pub fn parse(s: &str) -> Self {
        Self {
            id: MdId::next(),
            source: s.to_string(),
            open_blocks: HashSet::new(),
            open_tables: HashSet::new(),
        }
    }
    pub fn push(&mut self, s: &str) {
        self.source.push_str(s);
    }
    /// Toggle a code block's fold (by fence-open order).
    pub fn toggle_block(&mut self, n: usize) {
        if !self.open_blocks.remove(&n) {
            self.open_blocks.insert(n);
        }
    }
    /// Toggle a table's row-cap fold (by table order).
    pub fn toggle_table(&mut self, n: usize) {
        if !self.open_tables.remove(&n) {
            self.open_tables.insert(n);
        }
    }
    /// The fence-open index of a still-unterminated ``` fence, if any — an odd
    /// number of fence lines means the last code block is still streaming and
    /// renders force-open, chevron-less.
    pub fn streaming_block(&self) -> Option<usize> {
        let fences = self
            .source
            .lines()
            .filter(|l| l.trim_start().starts_with("```"))
            .count();
        (fences % 2 == 1).then_some(fences / 2)
    }
}

/// The agent's reasoning stream, rendered collapsed as "Thought for Xs".
pub struct Thought {
    pub md: Md,
    /// When the reasoning began — runtime only (`None` once restored from disk).
    pub started: Option<Instant>,
    /// Final duration once the thought is done (persisted); `None` while live.
    pub elapsed_secs: Option<u64>,
    /// Whether the reasoning text is expanded in the transcript.
    pub expanded: bool,
}

impl Thought {
    fn live(s: &str) -> Self {
        Self {
            md: Md::parse(s),
            started: Some(Instant::now()),
            elapsed_secs: None,
            expanded: false,
        }
    }
}

/// Where one agent block sits in its turn: whether it is the last (which
/// carries the footer, so there is one Copy per turn rather than one per
/// fragment), and whether the turn is still streaming (Copy stays hidden until
/// it settles).
///
/// **The prose itself is deliberately not here.** Copy wants every agent block
/// of the turn joined together, and joining them is proportional to the whole
/// answer — paid on every redraw, for a string that is only read if a button is
/// clicked. [`Chat::turn_prose`] is that join, asked for at the click.
pub struct TurnAnswer {
    pub is_last: bool,
    pub is_active: bool,
    /// Wall-clock duration from prompt submission to `TurnEnded`. Legacy and
    /// replayed turns without timestamps leave this absent.
    pub elapsed_secs: Option<u64>,
}

/// Add/remove counts between two line sequences. Small and medium payloads use
/// an exact LCS; very large payloads use a bounded multiset fallback so a UI
/// redraw can never turn into an unbounded quadratic diff.
pub(crate) fn line_change_counts(old: Option<&str>, new: &str) -> (usize, usize) {
    let Some(old) = old else {
        return (new.lines().count(), 0);
    };
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let old = &old[prefix..old.len() - suffix];
    let new = &new[prefix..new.len() - suffix];
    if old.is_empty() || new.is_empty() {
        return (new.len(), old.len());
    }

    const MAX_LCS_CELLS: usize = 250_000;
    let common = if old.len().saturating_mul(new.len()) <= MAX_LCS_CELLS {
        let mut previous = vec![0usize; new.len() + 1];
        let mut current = vec![0usize; new.len() + 1];
        for old_line in old {
            for (j, new_line) in new.iter().enumerate() {
                current[j + 1] = if old_line == new_line {
                    previous[j] + 1
                } else {
                    current[j].max(previous[j + 1])
                };
            }
            std::mem::swap(&mut previous, &mut current);
            current.fill(0);
        }
        previous[new.len()]
    } else {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for line in old {
            *counts.entry(line).or_default() += 1;
        }
        let mut common = 0;
        for line in new {
            if let Some(count) = counts.get_mut(line) {
                if *count > 0 {
                    *count -= 1;
                    common += 1;
                }
            }
        }
        common
    };
    (new.len() - common, old.len() - common)
}

/// A tool call plus its per-item view state (fold state lives **on the item**,
/// not in global string-keyed sets).
pub struct ToolItem {
    pub call: ToolCall,
    /// Cached per-path diff metrics. Computing LCS belongs at event-apply time,
    /// never in the per-frame view path.
    pub diff_summary: Vec<(String, usize, usize)>,
    /// The hunks each `Diff` section draws, keyed by that section's index in
    /// `call.content`.
    ///
    /// Here for the same reason as the metrics above, and it is the half that
    /// was still being paid every frame: a card on screen recomputed the whole
    /// edit script — the quadratic table included — once per redraw, and a
    /// redraw happens for every notch of a scroll wheel. The edit only changes
    /// when the tool reports new content, which is exactly where this is
    /// filled.
    pub diff_rows: HashMap<usize, Vec<crate::diff::Row>>,
    /// The user opened this card.
    pub fold: bool,
    /// Content sections whose OUT well is un-folded past the threshold,
    /// keyed by the section's index in `call.content`.
    pub out_open: HashSet<usize>,
}

impl ToolItem {
    pub fn new(call: ToolCall) -> Self {
        let diff_summary = Self::summarize_diffs(&call);
        let diff_rows = Self::hunks(&call);
        Self {
            call,
            diff_summary,
            diff_rows,
            fold: false,
            out_open: HashSet::new(),
        }
    }

    fn hunks(call: &ToolCall) -> HashMap<usize, Vec<crate::diff::Row>> {
        call.content
            .iter()
            .enumerate()
            .filter_map(|(i, content)| match content {
                ToolContent::Diff { old, new, .. } => Some((
                    i,
                    crate::diff::rows(old.as_deref().unwrap_or_default(), new),
                )),
                _ => None,
            })
            .collect()
    }

    fn summarize_diffs(call: &ToolCall) -> Vec<(String, usize, usize)> {
        let mut summary: Vec<(String, usize, usize)> = Vec::new();
        for content in &call.content {
            if let ToolContent::Diff { path, old, new } = content {
                let (adds, removes) = line_change_counts(old.as_deref(), new);
                match summary.iter_mut().find(|(current, _, _)| current == path) {
                    Some(entry) => {
                        entry.1 += adds;
                        entry.2 += removes;
                    }
                    None => summary.push((path.clone(), adds, removes)),
                }
            }
        }
        summary
    }

    /// Re-derive everything about the card that is a function of its content.
    ///
    /// One call rather than two, because the two answers come from the same
    /// sections and forgetting either leaves a card describing an edit it is no
    /// longer showing.
    fn refresh_diffs(&mut self) {
        self.diff_summary = Self::summarize_diffs(&self.call);
        self.diff_rows = Self::hunks(&self.call);
    }
    /// Live work stays open. Every settled state, including failure, follows
    /// the user's fold choice and therefore starts collapsed.
    pub fn is_open(&self) -> bool {
        self.fold || matches!(self.call.status, ToolStatus::InProgress)
    }
}

/// The agent's plan/checklist (Claude Code's TodoWrite) plus its fold state —
/// one card per turn, replaced in full on every `plan` update.
pub struct PlanItem {
    pub entries: Vec<PlanEntry>,
    /// The user opened this card.
    pub fold: bool,
}

impl PlanItem {
    pub fn new(entries: Vec<PlanEntry>) -> Self {
        Self {
            entries,
            fold: false,
        }
    }
    /// Force-open while work is mid-flight (an `in_progress` entry) — computed,
    /// never stored, the same way a running tool card is. A second stored flag
    /// for "open because it is busy" would have to be cleared by whatever ends
    /// the work, and the day that is missed the card stays open forever.
    pub fn is_open(&self) -> bool {
        self.fold
            || self
                .entries
                .iter()
                .any(|e| matches!(e.status, PlanStatus::InProgress))
    }
}

/// A user prompt: its text plus any files attached with 📎 (rendered as
/// thumbnails / placeholder chips in the bubble).
pub struct UserMsg {
    pub text: String,
    pub attachments: Vec<AttachmentSnapshot>,
    /// Epoch seconds captured when the prompt was submitted. Replayed/legacy
    /// protocol messages may not carry one.
    pub sent_at: Option<u64>,
    /// Epoch seconds captured when the corresponding response settles. Kept
    /// on the user item because that item is the stable boundary of a turn.
    pub completed_at: Option<u64>,
}

impl UserMsg {
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            text: s.into(),
            attachments: Vec::new(),
            sent_at: None,
            completed_at: None,
        }
    }
}

/// One rendered entry in the transcript.
pub enum ChatItem {
    /// A user prompt.
    User(UserMsg),
    /// A streamed agent reply (markdown, grown incrementally).
    Agent(Md),
    /// The agent's reasoning stream (discovery), shown as "Thought for Xs".
    Thought(Thought),
    /// A tool call card (badge + status + diff/output) with its fold state.
    Tool(ToolItem),
    /// The agent's plan/checklist (TodoWrite) with its fold state.
    Plan(PlanItem),
    /// A permission prompt with option buttons.
    Permission(PermItem),
    /// A question from the agent with its choice buttons.
    Ask(AskItem),
    /// A line the session says about itself: a remark, or a failure.
    Notice { text: String, level: NoticeLevel },
}

/// How loudly a notice draws.
///
/// The distinction is the model's rather than the renderer's because the
/// renderer cannot make it: by the time a notice is a string, "the adapter
/// died" and "the turn was interrupted" are the same shape, and telling them
/// apart means matching on the prose — which is a rule written in the one
/// place that will not be updated when the prose changes. Drawn the same, the
/// louder of the two is a grey line the size of a caption, saying the agent is
/// gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeLevel {
    /// Something happened and it is worth recording. The default: a line whose
    /// severity nobody stated is not an alarm.
    #[default]
    Info,
    /// A real failure — the turn or the connection did not survive it.
    Error,
}

impl NoticeLevel {
    pub fn parse(s: &str) -> Self {
        match s {
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
    /// The archived form. Info is the empty string so a quiet notice costs no
    /// key on disk, and so an archive from before levels existed round-trips
    /// to exactly the bytes it came in as.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "",
            Self::Error => "error",
        }
    }
}

impl ChatItem {
    /// A quiet line about the session.
    pub fn notice(text: impl Into<String>) -> Self {
        Self::Notice {
            text: text.into(),
            level: NoticeLevel::Info,
        }
    }

    /// A failure the user has to see.
    pub fn error(text: impl Into<String>) -> Self {
        Self::Notice {
            text: text.into(),
            level: NoticeLevel::Error,
        }
    }
}

/// Session titles are compact task labels, not first-message previews.
const TITLE_MAX_CHARS: usize = 48;
const TITLE_MAX_WORDS: usize = 8;

/// Turn the first meaningful line of a prompt into a short, stable task label.
///
/// This deliberately stays local and deterministic: deriving a title must not
/// spend another model turn or delay sending the user's prompt. Request filler
/// is removed in the two languages commonly used in the app, then the label is
/// clipped at a word boundary. `None` is reserved for attachment-only turns so
/// callers can keep the agent-name fallback.
pub fn summarize_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut title = line
        .trim_start_matches(['#', '>', '-', '*', '•'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // A title should describe the task, not the way it was requested. Re-run
    // because prefixes are often stacked ("Could you please help me …").
    const PREFIXES: &[&str] = &[
        "i'd like you to ",
        "i would like you to ",
        "i want you to ",
        "would you please ",
        "could you please ",
        "can you please ",
        "would you ",
        "could you ",
        "can you ",
        "help me ",
        "please ",
        "bạn có thể ",
        "có thể ",
        "vui lòng ",
        "làm ơn ",
        "giúp tôi ",
        "giúp mình ",
        "tôi muốn ",
        "mình muốn ",
        "hãy ",
    ];
    loop {
        let lower = title.to_lowercase();
        let Some(prefix) = PREFIXES.iter().find(|p| lower.starts_with(**p)) else {
            break;
        };
        title = title[prefix.len()..].trim_start().to_string();
    }

    // Keep only the first request sentence. A punctuation mark counts as a
    // boundary only when followed by whitespace, avoiding splits in paths and
    // identifiers such as `src/app.rs`.
    if let Some(end) = title.char_indices().find_map(|(i, c)| {
        matches!(c, '.' | '?' | '!' | ';')
            .then(|| i + c.len_utf8())
            .filter(|end| title[*end..].starts_with(char::is_whitespace))
    }) {
        title.truncate(end);
    }

    const SUFFIXES: &[&str] = &[
        " được không nhỉ",
        " được không",
        " không nhỉ",
        " nhé",
        " nha",
        " nhỉ",
        " please",
    ];
    loop {
        title = title
            .trim_end_matches(|c: char| {
                c.is_whitespace() || matches!(c, '.' | '?' | '!' | ';' | ':')
            })
            .to_string();
        let lower = title.to_lowercase();
        let Some(suffix) = SUFFIXES.iter().find(|s| lower.ends_with(**s)) else {
            break;
        };
        title.truncate(title.len() - suffix.len());
    }

    // Common "make X better" phrasing becomes the task-shaped "Improve X".
    // Besides reading more naturally, this handles the most frequent case where
    // removing request filler alone would still leave a sentence fragment.
    let lower = title.to_lowercase();
    let rewrites = [
        (
            "làm cho ",
            [" đẹp hơn", " tốt hơn", " hợp lý hơn"].as_slice(),
            "Cải thiện ",
        ),
        (
            "make ",
            [" better", " prettier", " nicer"].as_slice(),
            "Improve ",
        ),
    ];
    for (prefix, suffixes, replacement) in rewrites {
        if lower.starts_with(prefix) {
            if let Some(suffix) = suffixes.iter().find(|s| lower.ends_with(**s)) {
                let subject = title[prefix.len()..title.len() - suffix.len()].trim();
                title = format!("{replacement}{subject}");
                break;
            }
        }
    }

    let title = title.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '`' | '\'' | '"' | '.' | '?' | '!' | ';' | ':')
    });
    if title.is_empty() {
        return None;
    }

    let mut clipped = String::new();
    let mut truncated = false;
    for (index, word) in title.split_whitespace().enumerate() {
        let separator = usize::from(!clipped.is_empty());
        if index == TITLE_MAX_WORDS
            || clipped.chars().count() + separator + word.chars().count() > TITLE_MAX_CHARS
        {
            truncated = true;
            break;
        }
        if separator == 1 {
            clipped.push(' ');
        }
        clipped.push_str(word);
    }
    // A single long token (usually a path) still needs a useful label.
    if clipped.is_empty() {
        clipped = title.chars().take(TITLE_MAX_CHARS).collect();
        truncated = title.chars().count() > TITLE_MAX_CHARS;
    }
    if truncated {
        clipped.push('…');
    }

    let mut chars = clipped.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

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
    /// The adapter went away. Restartable with `Ctrl+Shift+R`.
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

/// What applying one event did, for the front end that has to react to it.
///
/// Deliberately two facts and not a description of the edit. Naming the exact
/// item that changed would let a view update only that one -- and would put the
/// burden of getting it exactly right on every branch of the reducer and every
/// helper it calls, where being subtly wrong shows up as a row that never
/// redraws. These two are cheap to be sure of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Something rendered is different now, so anything derived from the
    /// transcript is stale.
    pub transcript_changed: bool,
    /// A turn settled: time to archive, and to badge the session if nobody was
    /// watching.
    pub turn_ended: bool,
    /// The agent parked something only the user can clear, and which of the two
    /// it was. `None` on every other event.
    ///
    /// A third fact rather than a second reading of the transcript: whether a
    /// conversation *is* waiting is [`Chat::awaiting_permission`], which scans
    /// every item and answers the same for as long as the card is up. What a
    /// notification needs is the *moment* it started waiting, and only the event
    /// knows that.
    pub asked_user: Option<UserAsk>,
}

/// What an agent parked in front of the user, and what to call it.
///
/// The two are one type because everything downstream treats them alike -- both
/// stop the turn dead, both are cleared only by an answer, both draw the same
/// mark on the rail -- and differ in exactly one thing, which is the sentence
/// that names them. Splitting that sentence across the two call sites that need
/// it is how one of them ends up saying "approval" about a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAsk {
    /// The agent wants to run something and is waiting to be allowed to.
    Permission,
    /// The agent asked the user a question and is waiting for the answer.
    Question,
}

impl UserAsk {
    /// One line naming the agent and what it is waiting for.
    ///
    /// Written here rather than at the notification, because it is a sentence
    /// about the conversation and not about a platform: the same words belong in
    /// anything else that ever has to say a session is blocked. Present tense
    /// and about the agent, so a row of them from several projects reads as a
    /// list of who is waiting rather than of what happened.
    pub fn headline(self, agent: &str) -> String {
        match self {
            Self::Permission => format!("{agent} is waiting for your approval"),
            Self::Question => format!("{agent} has a question for you"),
        }
    }
}

/// Something about a conversation worth saying somewhere the conversation is
/// not.
///
/// The three moments a session stops being self-explanatory to somebody who is
/// not looking at it: a turn it finished, an answer it is waiting for, and an
/// agent that went away. They are one type because the sentences are the same
/// family of sentence and are needed by every surface that speaks for a session
/// from outside — a desktop notification today, a chat on a phone as well now,
/// and whatever comes after that.
///
/// The words live here rather than at each of those surfaces for the reason
/// [`UserAsk::headline`] gives about its own two: split across call sites, one
/// of them drifts, and the day it does the two surfaces disagree about what the
/// same agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Away {
    /// A turn settled while nobody was watching.
    TurnEnded,
    /// The agent parked something only the user can clear.
    Asked(UserAsk),
    /// The adapter died. The transcript stays as read-only history.
    LinkLost,
}

impl Away {
    /// One line naming the agent and what happened to it.
    ///
    /// Present tense for the two that are still true, past for the one that is
    /// over, so a column of these from several projects reads as a list of what
    /// needs doing rather than of what has happened.
    pub fn headline(self, agent: &str) -> String {
        match self {
            Self::TurnEnded => format!("{agent} finished a turn"),
            Self::Asked(ask) => ask.headline(agent),
            Self::LinkLost => format!("{agent} stopped answering"),
        }
    }
}

/// Why a composed prompt cannot be sent right now.
///
/// Ordered by which answer is the most useful one to give: a turn already
/// running outranks anything about the prompt itself, and a file that failed to
/// read outranks an empty buffer because it is a condition the user has to
/// clear rather than one more thing to type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitBlock {
    /// Nothing typed and nothing staged.
    Empty,
    /// A turn is already in flight.
    Busy,
    /// A staged file could not be read, by name.
    UnreadableAttachment(String),
    /// No live request channel: still connecting, or the adapter is gone.
    NotConnected,
}

impl SubmitBlock {
    /// What to tell the user, on the control that refused.
    pub fn hint(&self) -> String {
        match self {
            Self::Empty => "Write a prompt first".to_string(),
            Self::Busy => "The agent is still working on the last turn".to_string(),
            Self::UnreadableAttachment(name) => format!("{name} could not be read — remove it"),
            Self::NotConnected => "The agent is not connected".to_string(),
        }
    }
}

/// A prompt written while the agent was still working on the last one.
///
/// Kept whole rather than as text: an attachment staged beside it is part of
/// the same message, and dropping it on the way into the queue would send a
/// prompt that refers to a file the agent was never given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub text: String,
    pub attachments: Vec<StagedAttachment>,
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
    /// The session's root + agent name, for the persisted metadata.
    pub root: PathBuf,
    pub agent: String,
    /// User-chosen conversation title. `None` keeps automatic title generation
    /// from the first prompt; persisted independently so Reset can restore it.
    pub custom_title: Option<String>,
    /// Last activity (epoch secs): bumped on every applied agent event and on
    /// each submitted prompt, seeded from the archive's `updated` on resume.
    /// Drives the rail session row's relative-time label. Runtime-only.
    pub last_activity: Option<u64>,
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
    pub tx: Option<ReqTx>,
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
    pub resumed: bool,

    // ── composer sources (Phase 3B) ──
    /// Root-relative file paths for `@`-mention completion.
    pub files: Vec<String>,
    /// Agent-advertised slash commands for `/` completion.
    pub commands: Vec<SlashCommand>,
    /// Session modes offered by the agent (composer selector).
    pub modes: Vec<Mode>,
    /// The currently selected mode id.
    pub current_mode: Option<String>,
    /// Agent config options (model/effort/agent) — composer selectors.
    pub config_options: Vec<ConfigOption>,
    /// The mode + config picks a *resumed* conversation was last using, armed
    /// from the archive on load and replayed once the adapter reconnects
    /// (`reapply_prefs`). The adapter rebuilds effort/agent from static settings
    /// on `session/load`, so without this a reopened session loses them.
    pub pending_mode: Option<String>,
    pub pending_config: Vec<(String, String)>,
    /// Files staged with 📎 to send with the next prompt.
    /// Live terminals (ACP terminal extension), keyed by terminalId.
    pub terminals: HashMap<String, TermView>,
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

    // ── what is on disk ──
    /// Where this conversation is kept. `None` never persists — which is what
    /// makes a chat built in a test provably inert, and what lets the store's
    /// own tests point one at a directory of their own.
    pub store: Option<PathBuf>,
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

    /// Whether an adopted archive is still waiting for the agent to replay it.
    pub fn replay_pending(&self) -> bool {
        matches!(self.replay, Replay::Armed)
    }

    /// Everything not yet on disk, and the metadata that goes with it.
    ///
    /// `None` when there is nowhere to write, nothing to identify the
    /// conversation by, or nothing new — which is what keeps a session nobody
    /// used from creating a directory and standing in the list beside
    /// conversations that were had.
    ///
    /// The mark moves here, as the save is *built* rather than when it lands,
    /// so a second save started before the first has finished finds nothing
    /// left to add instead of adding it twice.
    pub fn flush(&mut self) -> Option<PendingWrite> {
        // Nothing is written while a replay is in flight. What `items` holds
        // then is a re-delivery of lines that are already on disk, at indices
        // that mean a different thing on each side of the comparison.
        if matches!(self.replay, Replay::Partial(_)) {
            return None;
        }
        let total = self.history.len() + self.items.len();
        let from = if self.rewrite { 0 } else { self.persisted };
        if from >= total {
            return None;
        }

        let mut lines = Vec::new();
        let mut blobs = Vec::new();
        for item in self.history.iter().chain(self.items.iter()).skip(from) {
            if let Some((line, mut carried)) = store::line_of(self, item) {
                lines.push(line);
                blobs.append(&mut carried);
            }
        }

        let mut write = self.meta_write(total)?;
        // Only a save that added messages moves the conversation's date -- see
        // the field on the file for why the list must not reorder otherwise.
        write.meta.updated = (!lines.is_empty()).then(store::now_secs);
        write.lines = lines;
        write.blobs = blobs;
        write.rewrite = self.rewrite;

        self.persisted = total;
        self.rewrite = false;
        Some(write)
    }

    /// The metadata alone — a rename, a reset, a change of selector.
    ///
    /// Separate from [`Self::flush`] because these can happen *during* a turn,
    /// and a line written mid-turn describes a tool call that has not finished.
    /// It would never be revisited: the turn's own save writes only what came
    /// after it, so the half-finished card is what the conversation would hold
    /// from then on.
    ///
    /// `None` for a conversation with nothing on disk yet. Metadata by itself
    /// would put an empty conversation in the list.
    pub fn flush_meta(&mut self) -> Option<PendingWrite> {
        if self.persisted == 0 {
            return None;
        }
        self.meta_write(self.persisted)
    }

    /// The metadata half of a save, with no lines in it yet.
    fn meta_write(&self, items: usize) -> Option<PendingWrite> {
        let store = self.store.as_ref()?;
        let session_id = self.session_id.clone()?;
        Some(PendingWrite {
            dir: store::conv_dir(store, &session_id),
            lines: Vec::new(),
            blobs: Vec::new(),
            rewrite: false,
            meta: MetaWrite {
                session_id,
                root: self.root.display().to_string(),
                agent: self.agent.clone(),
                title: self.custom_title.clone(),
                preview: self.first_prompt(),
                prefs: self.prefs(),
                updated: None,
                items,
            },
        })
    }

    /// The first thing the user said, capped — what names a conversation in the
    /// picker when nobody has renamed it.
    ///
    /// Kept in the metadata so listing never has to open a transcript to find
    /// it, which is what listing used to do to every conversation in the store.
    fn first_prompt(&self) -> String {
        const PREVIEW_MAX: usize = 200;
        self.history
            .iter()
            .chain(self.items.iter())
            .find_map(|it| match it {
                ChatItem::User(u) => Some(u.text.clone()),
                _ => None,
            })
            .map(|text| match text.char_indices().nth(PREVIEW_MAX) {
                Some((cut, _)) => text[..cut].to_string(),
                None => text,
            })
            .unwrap_or_default()
    }

    /// The mode and config picks this conversation is currently on.
    fn prefs(&self) -> Prefs {
        Prefs {
            mode: self.current_mode.clone(),
            config: self
                .config_options
                .iter()
                .filter_map(|o| {
                    o.current.clone().map(|value| ConfigPick {
                        id: o.id.clone(),
                        value,
                    })
                })
                .collect(),
        }
    }

    /// Build and write, for the paths with no async context to hand the write
    /// off to — a session being taken apart, or the app closing.
    ///
    /// A failure is logged and nothing more, deliberately: this runs while the
    /// window it would have spoken to is being torn down. The per-turn save
    /// covers the same conversation every turn and does report, so a standing
    /// condition has already been said out loud long before this runs.
    pub fn save_blocking(&mut self) {
        if let Some(write) = self.flush() {
            if let Err(e) = store::commit(&write) {
                eprintln!("onehand: conversation not archived: {e}");
            }
        }
    }

    /// Lift this conversation out, leaving the chat empty — the handoff a
    /// restart makes to the session replacing it.
    ///
    /// A move rather than a copy, and that is what makes it safe: the chat it
    /// came from is about to be dropped, and a drop still holding these items
    /// would write them a second time. It carries the mark with it, so the
    /// replacement continues the same file rather than starting one.
    pub fn take_snapshot(&mut self) -> Option<ConversationSnapshot> {
        let session_id = self.session_id.clone()?;
        // Whatever is known to be whole. Mid-replay that is the copy that was
        // adopted, not the re-delivery that has not finished arriving.
        let items = match std::mem::replace(&mut self.replay, Replay::Settled) {
            Replay::Partial(stash) => {
                self.history.clear();
                self.items.clear();
                stash
            }
            Replay::Settled | Replay::Armed => {
                let mut items = std::mem::take(&mut self.history);
                items.append(&mut self.items);
                items
            }
        };
        // The cards that are a question go, since the adapter that asked is the
        // one being replaced and an answer would reach nobody. Dropping one
        // shifts every position after it, so any dropped from before the mark
        // come off the mark too.
        let mut dropped_before = 0;
        let mut kept = Vec::with_capacity(items.len());
        for (at, item) in items.into_iter().enumerate() {
            if matches!(item, ChatItem::Permission(_) | ChatItem::Ask(_)) {
                if at < self.persisted {
                    dropped_before += 1;
                }
                continue;
            }
            kept.push(item);
        }

        Some(ConversationSnapshot {
            session_id,
            title: self.custom_title.clone(),
            updated: self.last_activity.unwrap_or_else(store::now_secs),
            // The file's own, and the file keeps it: a snapshot passing through
            // memory has no business telling a conversation when it began.
            created: 0,
            prefs: self.prefs(),
            items: kept,
            written: self.persisted.saturating_sub(dropped_before),
            complete: !self.bounded,
        })
    }

    /// Close the replay window, putting the adopted copy back if the replay
    /// came up short of it.
    ///
    /// Short is the only case that restores. A replay that delivered at least as
    /// much *is* the better record — it is the agent's own, and it can be longer
    /// than the file when a previous run died between the last save and the end
    /// of a turn. Locally-minted notices are not part of the count and are kept
    /// either way: they say why the resume went as it did, which is exactly what
    /// the reader needs on screen when this puts a conversation back.
    ///
    /// The two outcomes leave the file in different states, and that is why the
    /// rewrite exists. Putting the copy back leaves the file as it was, so the
    /// mark is what it was. Keeping the replay means the file has to *become*
    /// it: a re-delivery is chunked as the agent chose rather than as the file
    /// was, so adding to it at an index that means a different thing on each
    /// side is how a seam drops or doubles a message. Once per resume, and the
    /// same size of read the resume already paid for. Never when the transcript
    /// came back bounded, because then what is on screen is a tail and the file
    /// is longer than it.
    fn settle_replay(&mut self) {
        let Replay::Partial(stash) = std::mem::replace(&mut self.replay, Replay::Settled) else {
            return;
        };
        let replayed = self
            .items
            .iter()
            .filter(|it| !matches!(it, ChatItem::Notice { .. }))
            .count();
        if replayed < stash.len() || self.bounded {
            self.persisted = stash.len();
            self.history = stash;
            self.items
                .retain(|it| matches!(it, ChatItem::Notice { .. }));
            self.touch();
        } else {
            self.rewrite = true;
            self.persisted = 0;
        }
    }

    /// Record that the transcript changed.
    ///
    /// Called from the few places that mutate what is rendered: [`Self::apply`]
    /// for every event that is not pure session metadata, [`Self::push_user`]
    /// for a locally-staged prompt, [`Self::load_history`] for an adopted
    /// archive, and the two that settle a blocking card. Item *fold* state is
    /// deliberately not one of them -- folding changes what a row draws, and a
    /// row is drawn from the live item either way.
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// True if there is nothing rendered yet (history + live both empty).
    pub fn is_empty(&self) -> bool {
        self.history.is_empty() && self.items.is_empty()
    }

    /// A title derived from the conversation's **first user prompt** — its first
    /// non-empty line, trimmed and length-capped — so each session reads
    /// distinctly in the rail row / pane header instead of all showing the
    /// agent name. Looks through resumed `history` then live `items`. `None`
    /// until a prompt exists (callers fall back to the agent name).
    pub fn derived_title(&self) -> Option<String> {
        self.history
            .iter()
            .chain(self.items.iter())
            .find_map(|item| match item {
                ChatItem::User(u) => Some(u.text.as_str()),
                _ => None,
            })
            .and_then(summarize_title)
    }

    /// The title shown throughout the UI: an explicit rename wins, otherwise
    /// use the compact task label derived from the first prompt.
    pub fn conversation_title(&self) -> Option<String> {
        self.custom_title.clone().or_else(|| self.derived_title())
    }

    /// Commit a user-entered title. Whitespace is normalized so a value pasted
    /// from multiple lines still behaves like a single-line header label.
    /// Blank input is ignored; Reset is an explicit separate action.
    pub fn rename(&mut self, title: &str) -> bool {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return false;
        }
        self.custom_title = Some(title);
        true
    }

    /// Return to the automatically derived title.
    pub fn reset_title(&mut self) {
        self.custom_title = None;
    }
    /// Switch the session mode: tell the adapter, then reflect it locally.
    ///
    /// Optimistic on purpose — the adapter confirms a mode change in its reply
    /// rather than as a `session/update`, so waiting for an echo would leave
    /// the picker showing the old value until the next turn.
    pub fn set_mode(&mut self, mode_id: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::SetMode(mode_id.to_string()));
        }
        self.current_mode = Some(mode_id.to_string());
    }

    /// Pick a config option (model / effort / sub-agent), same contract as
    /// [`Self::set_mode`].
    pub fn set_config_option(&mut self, config_id: &str, value: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::SetConfigOption {
                config_id: config_id.to_string(),
                value: value.to_string(),
            });
        }
        self.set_config_current(config_id, value);
    }

    /// Optimistically reflect a config-option choice (the adapter answers the
    /// `set_config_option` in its reply, not as a `session/update`).
    pub fn set_config_current(&mut self, config_id: &str, value: &str) {
        if let Some(opt) = self.config_options.iter_mut().find(|o| o.id == config_id) {
            opt.current = Some(value.to_string());
        }
    }

    /// Load a resumed conversation's transcript as read-only history.
    ///
    /// Arms `replay_pending` immediately: a `session/load` resume replays its
    /// history as `session/update`s, and those can arrive *before* the
    /// `Connected { resumed: true }` event (the load response). Arming here — not
    /// on `Connected` — means the first replayed content drops the placeholder
    /// history regardless of event order; a `session/new` fallback disarms it via
    /// `Connected { resumed: false }` so old history stays as read-only context.
    /// `written` is how many of `items` are already on disk — the mark the
    /// conversation carries on from.
    pub fn load_history(&mut self, items: Vec<ChatItem>, session_id: String, written: usize) {
        // The loaded transcript *is* this conversation, and a `session/load`
        // replay re-delivers it into `items`. Without the reset the view
        // (history ⧺ items) shows everything twice.
        //
        // Nothing is saved on the way past any more. Under a file that is only
        // added to there is nothing to rescue: whatever the live items held is
        // either already written, or is being carried in here with the mark
        // that says so.
        self.items.clear();
        self.terminals.clear();
        self.history = items;
        self.persisted = written;
        self.session_id = Some(session_id);
        self.replay = Replay::Armed;
        self.rewrite = false;
        self.touch();
    }

    /// Adopt a conversation: its transcript, its title, the selector state to
    /// replay, when it was last touched, and how much of it is on disk.
    ///
    /// They happen together and none of them is optional, which is why this is
    /// one call rather than five at a call site. Forget `arm_prefs` and a
    /// reopened conversation silently loses its effort/agent (the adapter
    /// rebuilds those from static settings on `session/load`); forget
    /// `last_activity` and the rail dates the session from the moment it was
    /// reopened rather than from its last real turn; forget the mark and the
    /// conversation is written to its own file a second time.
    pub fn resume_from(&mut self, snapshot: ConversationSnapshot) {
        self.bounded = !snapshot.complete;
        self.custom_title = snapshot.title;
        self.last_activity = Some(snapshot.updated);
        self.arm_prefs(
            snapshot.prefs.mode,
            snapshot
                .prefs
                .config
                .into_iter()
                .map(|c| (c.id, c.value))
                .collect(),
        );
        self.load_history(snapshot.items, snapshot.session_id, snapshot.written);
    }

    /// Arm the mode + config picks to replay once this resumed session
    /// reconnects (`Connected { resumed: true }` → [`Self::reapply_prefs`]).
    /// Set right after [`Self::load_history`] from the archive's `prefs`.
    pub fn arm_prefs(&mut self, mode: Option<String>, config: Vec<(String, String)>) {
        self.pending_mode = mode;
        self.pending_config = config;
    }

    /// Replay the armed selector state onto a freshly-reconnected resumed
    /// session. Re-sends `set_mode` / `set_config_option` only where the
    /// adapter came up on a *different* value than the archive recorded, and
    /// only for options/modes the adapter still offers. **Model is skipped**:
    /// the SDK re-reads it from the transcript on resume, and re-pushing a
    /// picker alias (e.g. `opus` vs the live `opus[1m]`) can switch the context
    /// lane rather than describe it (see the adapter's `getAvailableModels`).
    fn reapply_prefs(&mut self) {
        let Some(tx) = self.tx.clone() else { return };
        if let Some(mode) = self.pending_mode.take() {
            if self.current_mode.as_deref() != Some(mode.as_str())
                && self.modes.iter().any(|m| m.id == mode)
            {
                let _ = tx.send(AcpRequest::SetMode(mode.clone()));
                self.current_mode = Some(mode);
            }
        }
        for (id, value) in std::mem::take(&mut self.pending_config) {
            if id.eq_ignore_ascii_case("model") {
                continue;
            }
            let restorable = self
                .config_options
                .iter()
                .find(|o| o.id == id)
                .is_some_and(|o| {
                    o.current.as_deref() != Some(value.as_str())
                        && o.choices.iter().any(|c| c.value == value)
                });
            if restorable {
                let _ = tx.send(AcpRequest::SetConfigOption {
                    config_id: id.clone(),
                    value: value.clone(),
                });
                self.set_config_current(&id, &value);
            }
        }
    }

    /// Take the loaded history off the screen the first time real replayed
    /// content arrives — and keep hold of it, because "the replay has begun" is
    /// not "the replay has finished". See [`Replay`].
    fn consume_replay(&mut self) {
        if matches!(self.replay, Replay::Armed) {
            self.replay = Replay::Partial(std::mem::take(&mut self.history));
        }
    }

    /// Apply a worker event to the transcript/state.
    ///
    /// The outcome is what a front end has to act on, decided here rather than
    /// re-derived by matching on the event a second time at the call site: the
    /// rules for "did this change the transcript" and "did the turn settle"
    /// belong with the reducer that knows them, and a second copy is a second
    /// thing to keep in step.
    pub fn apply(&mut self, event: AcpEvent) -> ApplyOutcome {
        // Asked of the transcript itself rather than derived from the event's
        // kind. The two agree for every ordinary event, and disagree for the one
        // that matters: a failed resume is announced by a *metadata* event, and
        // settling it can put a whole conversation back on screen. Reported as
        // unchanged, that conversation would be drawn from blocks nobody had
        // parsed — every answer in it rendering as its own markdown source.
        let before = self.revision;
        // Session-metadata events (handshake, mode/command advertisements)
        // aren't conversation activity; anything else refreshes the rail's
        // relative-time label — "now" while a session works, aging once quiet.
        let metadata = matches!(
            &event,
            AcpEvent::SessionId(_)
                | AcpEvent::AvailableCommands(_)
                | AcpEvent::Modes { .. }
                | AcpEvent::ModeChanged(_)
                | AcpEvent::ConfigOptions(_)
                | AcpEvent::Connected { .. }
        );
        let event_at = (!metadata).then(store::now_secs);
        if let Some(event_at) = event_at {
            self.last_activity = Some(event_at);
        }
        // The same test answers both questions, and that is not a coincidence:
        // an event that is conversation activity is an event that rewrote what
        // is on screen.
        let turn_ended = matches!(event, AcpEvent::TurnEnded { .. });
        // Read off the event for the same reason `turn_ended` is: this is the
        // one instant the answer is true, and asking the transcript instead
        // would answer the same on every chunk that arrives while the card is
        // up. An elicitation that cannot be drawn never reaches here -- the
        // client declines those where they arrive rather than parking them.
        let asked_user = match event {
            AcpEvent::Permission(_) => Some(UserAsk::Permission),
            AcpEvent::Elicitation(_) => Some(UserAsk::Question),
            _ => None,
        };
        if !metadata {
            self.touch();
        }
        // Any content event that isn't itself a user chunk seals the trailing
        // user bubble (see `user_chunk_open`); session-metadata events pass
        // through so they can't split one message's chunks.
        match &event {
            AcpEvent::UserChunk(_)
            | AcpEvent::SessionId(_)
            | AcpEvent::AvailableCommands(_)
            | AcpEvent::Modes { .. }
            | AcpEvent::ModeChanged(_)
            | AcpEvent::ConfigOptions(_)
            | AcpEvent::Connected { .. } => {}
            _ => self.user_chunk_open = false,
        }
        match event {
            AcpEvent::Connected { tx, resumed } => {
                self.tx = Some(tx);
                self.link = Link::Connected;
                self.resumed = resumed;
                // The replay window was armed in `load_history`; this is where it
                // can close.
                //
                // A window that has already seen content settles here, whichever
                // way the load went: this event *is* the load's answer, so
                // nothing more is coming. Settling is the same call in both
                // cases — including its rule that the adopted archive goes back
                // when the replay came up shorter, which is not only the failed
                // resume. A successful `session/load` replays the conversation
                // as the *agent* holds it, and the archive holds things the
                // agent has no reason to send back: the tool cards, the plans,
                // the reasoning. Taking a replay's word for it there would trade
                // a full transcript for a summary of it, permanently, at the
                // next turn end.
                //
                // One that has seen no content stays armed on a real resume,
                // since the answer can land before the updates it answers for —
                // and settles on a fallback, which is the adapter saying there
                // was nothing to replay.
                if !resumed || !matches!(self.replay, Replay::Armed) || self.history.is_empty() {
                    self.settle_replay();
                }
                // A real resume replays the session's own selector state; a
                // `session/new` fallback (resumed=false) is a fresh session that
                // should keep the adapter's defaults, so drop the armed prefs
                // unused. `Modes`/`ConfigOptions` land before this event (see
                // the client's load path), so the adapter's current values are
                // already in place to diff against.
                if resumed {
                    self.reapply_prefs();
                } else {
                    self.pending_mode = None;
                    self.pending_config.clear();
                }
            }
            AcpEvent::SessionId(id) => self.session_id = Some(id),
            AcpEvent::AgentChunk(s) => {
                self.consume_replay();
                self.finalize_thought();
                self.push_agent(&s);
            }
            AcpEvent::ThoughtChunk(s) => {
                self.consume_replay();
                self.push_thought(&s);
            }
            AcpEvent::UserChunk(s) => {
                self.consume_replay();
                self.finalize_thought();
                self.push_user_chunk(&s);
            }
            AcpEvent::ToolCall(tc) => {
                self.consume_replay();
                self.finalize_thought();
                self.items.push(ChatItem::Tool(ToolItem::new(tc)));
            }
            AcpEvent::ToolUpdate(tu) => {
                self.consume_replay();
                self.apply_tool_update(tu);
            }
            AcpEvent::Plan(entries) => {
                self.consume_replay();
                self.finalize_thought();
                self.apply_plan(entries);
            }
            AcpEvent::Permission(req) => {
                self.consume_replay();
                self.finalize_thought();
                self.items.push(ChatItem::Permission(PermItem {
                    req,
                    resolved: None,
                }))
            }
            AcpEvent::Elicitation(req) => {
                self.consume_replay();
                self.finalize_thought();
                self.items.push(ChatItem::Ask(AskItem::new(req)))
            }
            AcpEvent::AvailableCommands(c) => self.commands = c,
            AcpEvent::Modes { current, available } => {
                self.current_mode = current;
                self.modes = available;
            }
            AcpEvent::ModeChanged(id) => self.current_mode = Some(id),
            AcpEvent::ConfigOptions(opts) => self.config_options = opts,
            AcpEvent::TerminalOutput { terminal_id, chunk } => {
                let view = self.terminals.entry(terminal_id).or_default();
                view.output.push_str(&chunk);
                // Bound the retained text (keep the tail). The cut point must
                // land on a char boundary — chunks carry multibyte text (`→`,
                // box-drawing, Vietnamese) and a raw byte slice would panic.
                if view.output.len() > MAX_TERM_BYTES {
                    let mut cut = view.output.len() - MAX_TERM_BYTES;
                    while !view.output.is_char_boundary(cut) {
                        cut += 1;
                    }
                    view.output = view.output[cut..].to_string();
                }
            }
            AcpEvent::TerminalExit {
                terminal_id,
                exit_code,
            } => {
                let view = self.terminals.entry(terminal_id).or_default();
                view.exited = true;
                view.exit_code = exit_code;
            }
            AcpEvent::TurnEnded { .. } => {
                self.finish_active_turn(event_at.unwrap_or_else(store::now_secs));
                self.busy = false;
                self.finalize_thought();
                // A turn that ends with unanswered permission cards (a
                // cancelled turn, or an adapter that moved on) resolves them
                // as cancelled — protocol-correct, and the buttons disable.
                self.cancel_pending_permissions();
                // Fold finished terminals into their cards so the `terminals` map
                // only ever holds live ones (the turn is over → no more updates).
                self.flatten_exited_terminals();
                // The archive write is kicked off the UI loop by the app handler
                // (it builds the snapshot here, then writes on a blocking pool).
                //
                // Last, so a prompt written mid-turn opens its own turn against
                // a conversation that has finished closing the previous one.
                self.flush_queued();
            }
            AcpEvent::Error(e) => self.items.push(ChatItem::error(format!("Error: {e}"))),
            AcpEvent::Disconnected(e) => {
                self.tx = None;
                self.link = Link::Lost;
                self.busy = false;
                // No live adapter to answer — but the cards must not keep
                // offering buttons whose rpc ids died with the connection.
                self.cancel_pending_permissions();
                self.items.push(ChatItem::error(format!(
                    "Disconnected: {e} — Ctrl+Shift+R to restart"
                )));
            }
        }

        ApplyOutcome {
            transcript_changed: self.revision != before,
            turn_ended,
            asked_user,
        }
    }

    /// Merge a `tool_call_update` into the matching tool card (by id). An
    /// update with no matching card (it raced ahead of its `tool_call`, or
    /// the adapter sends update-only calls) materializes one instead of being
    /// dropped — a silently-vanished `Failed` would read as "the turn did
    /// nothing".
    fn apply_tool_update(&mut self, tu: ToolCallUpdate) {
        for item in self.items.iter_mut().rev() {
            if let ChatItem::Tool(t) = item {
                if t.call.id == tu.id {
                    if let Some(status) = tu.status {
                        t.call.status = status;
                    }
                    if let Some(title) = tu.title {
                        t.call.title = title;
                    }
                    if let Some(description) = tu.description {
                        t.call.description = Some(description);
                    }
                    if let Some(content) = tu.content {
                        if !content.is_empty() {
                            t.call.content = content;
                            t.refresh_diffs();
                        }
                    }
                    return;
                }
            }
        }
        self.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: tu.id,
            title: tu.title.unwrap_or_default(),
            description: tu.description,
            kind: crate::acp::ToolKind::Other,
            status: tu.status.unwrap_or(ToolStatus::InProgress),
            content: tu.content.unwrap_or_default(),
        })));
    }

    /// Fold a `plan` update into the transcript: the agent republishes the
    /// full checklist on every change, so the current turn's plan card is
    /// replaced in place (its fold survives); a new turn gets its own card.
    fn apply_plan(&mut self, entries: Vec<PlanEntry>) {
        let turn_start = self
            .items
            .iter()
            .rposition(|i| matches!(i, ChatItem::User(_)))
            .map(|p| p + 1)
            .unwrap_or(0);
        for item in self.items[turn_start..].iter_mut() {
            if let ChatItem::Plan(p) = item {
                p.entries = entries;
                return;
            }
        }
        self.items.push(ChatItem::Plan(PlanItem::new(entries)));
    }
    /// Flatten every exited terminal into the tool card that references it
    /// (replacing the live `Terminal(id)` with the captured output + an exit
    /// footer) and drop it from the live `terminals` map. Called at turn end so
    /// the map stays bounded to terminals that are actually still running.
    fn flatten_exited_terminals(&mut self) {
        let exited: Vec<String> = self
            .terminals
            .iter()
            .filter(|(_, v)| v.exited)
            .map(|(id, _)| id.clone())
            .collect();
        for id in exited {
            let Some(view) = self.terminals.remove(&id) else {
                continue;
            };
            let footer = match view.exit_code {
                Some(0) => "\n[exited 0]".to_string(),
                Some(c) => format!("\n[exited {c}]"),
                None => "\n[exited]".to_string(),
            };
            let flat = format!("{}{footer}", view.output);
            for item in self.items.iter_mut() {
                if let ChatItem::Tool(t) = item {
                    for c in t.call.content.iter_mut() {
                        if matches!(c, ToolContent::Terminal(tid) if *tid == id) {
                            *c = ToolContent::Text(flat.clone());
                        }
                    }
                }
            }
        }
    }

    /// The pending (unresolved) permission's rpc id at `idx`, with the option
    /// name for a chosen id — used by the app to answer and mark it resolved.
    pub fn permission_at(&self, idx: usize) -> Option<&PermissionRequest> {
        match self.items.get(idx) {
            Some(ChatItem::Permission(p)) if p.resolved.is_none() => Some(&p.req),
            _ => None,
        }
    }

    /// Every still-pending (unresolved) permission with its live-items index,
    /// in transcript order. Rendered as a slide-up prompt above the composer
    /// (resolved ones stay inline in the transcript as an audit trail).
    pub fn pending_permissions(&self) -> Vec<(usize, &PermItem)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| match it {
                ChatItem::Permission(p) if p.resolved.is_none() => Some((i, p)),
                _ => None,
            })
            .collect()
    }

    /// Mark a permission resolved with the chosen option's display name.
    pub fn resolve_permission(&mut self, idx: usize, option_name: String) {
        if let Some(ChatItem::Permission(p)) = self.items.get_mut(idx) {
            p.resolved = Some(option_name);
            self.touch();
        }
    }

    /// Answer the permission at `idx` with `option_id`: tell the adapter, then
    /// record the choice on the card.
    ///
    /// Lives here for the same reason [`Self::answer_ask`] does. Split across
    /// the view — resolve the option's display name, send the rpc, then call
    /// `resolve_permission` — it is three steps that must all happen, in order,
    /// on the one path where getting it wrong leaves the agent parked
    /// forever. One method is one chance to get it wrong.
    pub fn answer_permission(&mut self, idx: usize, option_id: &str) {
        let Some(ChatItem::Permission(p)) = self.items.get(idx) else {
            return;
        };
        if p.resolved.is_some() {
            return;
        }
        // The card records the option's *name*; the wire carries its id.
        let name = p
            .req
            .options
            .iter()
            .find(|option| option.id == option_id)
            .map(|option| option.name.clone())
            .unwrap_or_else(|| option_id.to_string());
        let rpc_id = p.req.rpc_id.clone();

        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::PermissionResponse {
                rpc_id,
                option_id: Some(option_id.to_string()),
            });
        }
        self.resolve_permission(idx, name);
    }

    /// The still-open question at `idx`, mutable — the option clicks and typed
    /// "Other" text edit it in place before it's submitted.
    pub fn ask_at_mut(&mut self, idx: usize) -> Option<&mut AskItem> {
        match self.items.get_mut(idx) {
            Some(ChatItem::Ask(a)) if a.resolved.is_none() => Some(a),
            _ => None,
        }
    }

    /// Every unanswered question with its live-items index, in transcript order
    /// — pinned above the composer just like [`Self::pending_permissions`].
    pub fn pending_asks(&self) -> Vec<(usize, &AskItem)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| match it {
                ChatItem::Ask(a) if a.resolved.is_none() => Some((i, a)),
                _ => None,
            })
            .collect()
    }

    /// Answer the question at `idx` and settle its card. `Decline` is the Skip
    /// button: the model is told the user passed and the turn carries on.
    pub fn answer_ask(&mut self, idx: usize, skip: bool) {
        let Some(a) = self.ask_at_mut(idx) else {
            return;
        };
        let outcome = if skip {
            ElicitOutcome::Decline
        } else {
            ElicitOutcome::Accept(a.answers())
        };
        let (rpc_id, summary) = (a.req.rpc_id.clone(), a.summary());
        a.resolved = Some(if skip { "Skipped".into() } else { summary });
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::ElicitationResponse { rpc_id, outcome });
        }
        self.touch();
    }

    /// Stop the in-flight turn.
    ///
    /// Cancelling **must** resolve the turn's parked permissions first. ACP
    /// requires it: leave them and the adapter's `session/request_permission`
    /// dangles forever, the card keeps live-looking buttons whose later click
    /// would echo an rpc id a since-restarted adapter never issued, and the
    /// rail dot stays stuck on "waiting for you". Ordering this correctly is
    /// exactly the kind of thing a second front end gets wrong, so it lives
    /// here rather than in either one.
    pub fn cancel_turn(&mut self) {
        self.cancel_pending_permissions();
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::Cancel);
        }
    }

    /// Answer every still-pending permission with the `cancelled` outcome and
    /// mark its card resolved. ACP requires a cancelled turn to resolve its
    /// pending `session/request_permission`s — without this the adapter's
    /// request dangles forever, the card's buttons stay live (a later click
    /// would echo an rpc id a since-restarted adapter never issued), and the
    /// rail dot stays stuck on "waiting for you". With no live `tx`
    /// (disconnected) the cards are still resolved locally.
    pub fn cancel_pending_permissions(&mut self) {
        for it in &mut self.items {
            match it {
                ChatItem::Permission(p) if p.resolved.is_none() => {
                    if let Some(tx) = &self.tx {
                        let _ = tx.send(AcpRequest::PermissionResponse {
                            rpc_id: p.req.rpc_id.clone(),
                            option_id: None,
                        });
                    }
                    p.resolved = Some("Cancelled".into());
                }
                // A parked question dangles the same way — the agent's asking
                // tool call blocks on our answer. `Cancel` (not `Decline`)
                // aborts that tool call, which is what a cancelled turn means.
                ChatItem::Ask(a) if a.resolved.is_none() => {
                    if let Some(tx) = &self.tx {
                        let _ = tx.send(AcpRequest::ElicitationResponse {
                            rpc_id: a.req.rpc_id.clone(),
                            outcome: ElicitOutcome::Cancel,
                        });
                    }
                    a.resolved = Some("Cancelled".into());
                }
                _ => {}
            }
        }
    }

    /// The item list a toggle targets: live items or the read-only history.
    fn list(&self, target: TranscriptItemId) -> &[ChatItem] {
        match target {
            TranscriptItemId::History(_) => &self.history,
            TranscriptItemId::Live(_) => &self.items,
        }
    }

    fn list_mut(&mut self, target: TranscriptItemId) -> &mut Vec<ChatItem> {
        match target {
            TranscriptItemId::History(_) => &mut self.history,
            TranscriptItemId::Live(_) => &mut self.items,
        }
    }

    /// Toggle a whole tool/plan card's fold (typed, per item).
    pub fn toggle_tool(&mut self, target: TranscriptItemId) {
        match self.list_mut(target).get_mut(target.index()) {
            Some(ChatItem::Tool(t)) => t.fold = !t.fold,
            Some(ChatItem::Plan(p)) => p.fold = !p.fold,
            _ => {}
        }
    }

    /// Toggle one OUT section's past-threshold fold on a tool card.
    pub fn toggle_tool_output(&mut self, target: TranscriptItemId, section: usize) {
        if let Some(ChatItem::Tool(t)) = self.list_mut(target).get_mut(target.index()) {
            if !t.out_open.remove(&section) {
                t.out_open.insert(section);
            }
        }
    }

    /// Toggle an answer/thought code block's fold (by fence-open order).
    pub fn toggle_code(&mut self, target: TranscriptItemId, block: usize) {
        match self.list_mut(target).get_mut(target.index()) {
            Some(ChatItem::Agent(md)) => md.toggle_block(block),
            Some(ChatItem::Thought(th)) => th.md.toggle_block(block),
            _ => {}
        }
    }

    /// Toggle an answer/thought table's row-cap fold (by table order).
    pub fn toggle_prose_table(&mut self, target: TranscriptItemId, table: usize) {
        match self.list_mut(target).get_mut(target.index()) {
            Some(ChatItem::Agent(md)) => md.toggle_table(table),
            Some(ChatItem::Thought(th)) => th.md.toggle_table(table),
            _ => {}
        }
    }

    /// How many *messages* the conversation holds — user prompts + agent reply
    /// blocks, across the loaded history and the live transcript. Drives the
    /// chat header's "· N messages" meta; tool cards, thoughts and notices
    /// don't count — they are how an answer got made, not the conversation.
    pub fn message_count(&self) -> usize {
        self.history
            .iter()
            .chain(self.items.iter())
            .filter(|i| matches!(i, ChatItem::User(_) | ChatItem::Agent(_)))
            .count()
    }

    /// Why this prompt cannot be sent, or `None` when it can.
    ///
    /// The rule lives here, not in a front end: what makes a prompt sendable —
    /// non-blank *or* carrying attachments, no attachment that failed to read, a
    /// live request channel, and no turn already in flight — is a property of
    /// the conversation. Restating it per front end is how two of them come to
    /// disagree about when Send is allowed.
    ///
    /// It answers with the *reason* rather than a bool because a Send that
    /// refuses without saying why is indistinguishable, from the outside, from
    /// one that is broken: the front end needs the reason to put on the button.
    pub fn submit_blocker(&self, text: &str, staged: &[StagedAttachment]) -> Option<SubmitBlock> {
        if self.busy {
            return Some(SubmitBlock::Busy);
        }
        self.prompt_blocker(text, staged)
    }

    /// Everything wrong with the prompt itself, ignoring the running turn.
    ///
    /// Split out because the queue asks a different question: a turn in flight
    /// is what queueing is *for*, while an unreadable attachment is no more
    /// sendable in a minute than it is now.
    fn prompt_blocker(&self, text: &str, staged: &[StagedAttachment]) -> Option<SubmitBlock> {
        // Named, because "one of your attachments" sends the user looking
        // through the whole tray for the one with the red edge.
        if let Some(bad) = staged
            .iter()
            .find(|a| matches!(a.delivery, AttachmentDelivery::Unavailable))
        {
            return Some(SubmitBlock::UnreadableAttachment(bad.name.clone()));
        }
        if text.trim().is_empty() && staged.is_empty() {
            return Some(SubmitBlock::Empty);
        }
        if self.tx.is_none() {
            return Some(SubmitBlock::NotConnected);
        }
        None
    }

    /// Hold a prompt until the running turn ends.
    ///
    /// Only when a turn is what stands in the way — the queue is not a place to
    /// park a prompt that could not be sent for any other reason, since nothing
    /// about the end of a turn fixes an unreadable attachment or an adapter that
    /// is gone. Anything else is still a refusal, and still says why.
    ///
    /// Replaces whatever was queued: one prompt is waiting, and the second one
    /// written is the one the user means.
    pub fn queue(&mut self, text: &str, staged: &[StagedAttachment]) -> bool {
        if !self.busy || self.prompt_blocker(text, staged).is_some() {
            return false;
        }
        self.queued = Some(QueuedPrompt {
            text: text.trim().to_string(),
            attachments: staged.to_vec(),
        });
        true
    }

    /// Take the queued prompt back, for a front end putting it back in its
    /// composer.
    pub fn unqueue(&mut self) -> Option<QueuedPrompt> {
        self.queued.take()
    }

    /// Send whatever was waiting for this turn to end.
    ///
    /// A failed send leaves the prompt queued rather than dropping it: the
    /// adapter dying is not the user's cue to retype what they wrote.
    fn flush_queued(&mut self) {
        let Some(pending) = self.queued.take() else {
            return;
        };
        if !self.submit(&pending.text, &pending.attachments) {
            self.queued = Some(pending);
        }
    }

    /// Stage a user prompt locally (called when the user submits), with any
    /// files that were attached (listed inside the prompt's own card).
    /// Submit a prompt, starting a turn. Returns `false` when nothing was sent,
    /// in which case the caller must leave its composer and staging untouched.
    ///
    /// `staged` is borrowed rather than consumed so a refusal costs the caller
    /// nothing to recover from.
    pub fn submit(&mut self, text: &str, staged: &[StagedAttachment]) -> bool {
        if self.submit_blocker(text, staged).is_some() {
            return false;
        }
        let text = text.trim();

        let request = AcpRequest::Prompt {
            text: text.to_string(),
            attachments: staged
                .iter()
                .map(|a| Attachment {
                    path: a.path.clone(),
                })
                .collect(),
        };
        // Send *before* recording the turn locally: a dead channel must not
        // leave a prompt in the transcript that no agent ever received.
        if self.tx.as_ref().is_none_or(|tx| tx.send(request).is_err()) {
            return false;
        }

        self.push_user(
            text.to_string(),
            staged
                .iter()
                .cloned()
                .map(StagedAttachment::snapshot)
                .collect(),
        );
        self.busy = true;
        true
    }

    pub fn push_user(&mut self, text: String, attachments: Vec<AttachmentSnapshot>) {
        // A user prompt ends the replay window, but must NOT drop the loaded
        // history: if `session/load` succeeded while replaying nothing (the
        // protocol allows it), that history is the only copy of the
        // conversation — clearing it here would erase it from the archive on
        // the next turn-end save. Keep it as read-only context instead.
        //
        // The same call covers the harder half: a replay that started, stopped
        // partway, and was then typed over. Settling puts the adopted copy back
        // when what arrived was less than what was adopted, so the prompt is
        // added to the whole conversation rather than to a fragment of it —
        // and it leaves the mark saying which of the two the file now holds,
        // which is what keeps this prompt from being written as message one of
        // a conversation that already has forty.
        self.settle_replay();
        let sent_at = store::now_secs();
        self.last_activity = Some(sent_at);
        self.items.push(ChatItem::User(UserMsg {
            text,
            attachments,
            sent_at: Some(sent_at),
            completed_at: None,
        }));
        // A locally-staged prompt is complete — nothing may append to it.
        self.user_chunk_open = false;
        self.touch();
    }

    /// Close the most recent locally-timed turn. A replayed/legacy prompt has
    /// no `sent_at`, so it is deliberately left without a synthetic duration.
    fn finish_active_turn(&mut self, completed_at: u64) {
        let Some(ChatItem::User(user)) = self
            .items
            .iter_mut()
            .rev()
            .find(|item| matches!(item, ChatItem::User(_)))
        else {
            return;
        };
        if user.sent_at.is_some() && user.completed_at.is_none() {
            user.completed_at = Some(completed_at);
        }
    }

    /// For the agent block at `idx`, describe its turn for the Copy affordance
    /// (see [`TurnAnswer`]). `None` when `idx` is not an agent block. Turns are
    /// delimited by user prompts: the range runs from just after the previous
    /// `User` item up to the next one (or the transcript end).
    pub fn turn_answer(&self, target: TranscriptItemId) -> Option<TurnAnswer> {
        let items = self.list(target);
        let idx = target.index();
        if !matches!(items.get(idx), Some(ChatItem::Agent(_))) {
            return None;
        }
        let user_index = items[..idx]
            .iter()
            .rposition(|it| matches!(it, ChatItem::User(_)));
        let start = user_index.map(|p| p + 1).unwrap_or(0);
        let elapsed_secs = user_index.and_then(|p| match &items[p] {
            ChatItem::User(user) => user
                .sent_at
                .zip(user.completed_at)
                .map(|(start, end)| end.saturating_sub(start)),
            _ => None,
        });
        let end = items[idx + 1..]
            .iter()
            .position(|it| matches!(it, ChatItem::User(_)))
            .map(|p| idx + 1 + p)
            .unwrap_or(items.len());

        let mut last_agent = idx;
        for (i, it) in items[start..end].iter().enumerate() {
            if matches!(it, ChatItem::Agent(_)) {
                last_agent = start + i;
            }
        }
        Some(TurnAnswer {
            is_last: idx == last_agent,
            // The active turn is the trailing region (no user prompt after it)
            // while a turn is in flight — Copy waits for it to settle.
            is_active: matches!(target, TranscriptItemId::Live(_))
                && end == self.items.len()
                && self.busy,
            elapsed_secs,
        })
    }

    /// Every agent block of `target`'s turn, joined — what Copy puts on the
    /// clipboard, so it grabs the whole reply rather than the fragment the
    /// button happens to sit under.
    ///
    /// Asked for at the click and not before: the answer is proportional to the
    /// turn, and a redraw is not a reason to build it.
    pub fn turn_prose(&self, target: TranscriptItemId) -> String {
        let items = self.list(target);
        let idx = target.index();
        let start = items[..idx.min(items.len())]
            .iter()
            .rposition(|it| matches!(it, ChatItem::User(_)))
            .map(|p| p + 1)
            .unwrap_or(0);
        let end = items
            .get(idx + 1..)
            .and_then(|rest| rest.iter().position(|it| matches!(it, ChatItem::User(_))))
            .map(|p| idx + 1 + p)
            .unwrap_or(items.len());

        items[start..end]
            .iter()
            .filter_map(|it| match it {
                ChatItem::Agent(md) => Some(md.source.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Aggregate the file changes made during the turn that contains the agent
    /// block at `idx`: every `Diff` in the turn's tool cards, summed per path
    /// (adds = new-line count, removes = old-line count), in first-seen order.
    /// Drives the summary that closes a turn: one line per file touched, so the
    /// reader sees what changed without opening every tool card in the run.
    pub fn turn_changes(&self, target: TranscriptItemId) -> Vec<(String, usize, usize)> {
        let items = self.list(target);
        let idx = target.index();
        if idx >= items.len() {
            return Vec::new();
        }
        let start = items[..idx]
            .iter()
            .rposition(|it| matches!(it, ChatItem::User(_)))
            .map(|p| p + 1)
            .unwrap_or(0);
        let end = items[idx + 1..]
            .iter()
            .position(|it| matches!(it, ChatItem::User(_)))
            .map(|p| idx + 1 + p)
            .unwrap_or(items.len());
        let mut acc: Vec<(String, usize, usize)> = Vec::new();
        for it in &items[start..end] {
            let ChatItem::Tool(t) = it else { continue };
            for (path, adds, removes) in &t.diff_summary {
                match acc.iter_mut().find(|(p, _, _)| p == path) {
                    Some(entry) => {
                        entry.1 += adds;
                        entry.2 += removes;
                    }
                    None => acc.push((path.clone(), *adds, *removes)),
                }
            }
        }
        acc
    }

    fn push_agent(&mut self, s: &str) {
        match self.items.last_mut() {
            Some(ChatItem::Agent(md)) => md.push(s),
            _ => self.items.push(ChatItem::Agent(Md::parse(s))),
        }
    }

    fn push_thought(&mut self, s: &str) {
        match self.items.last_mut() {
            // Append only to a still-running thought; a finalized one starts anew.
            Some(ChatItem::Thought(th)) if th.elapsed_secs.is_none() => th.md.push(s),
            _ => self.items.push(ChatItem::Thought(Thought::live(s))),
        }
    }

    /// Stamp the trailing thought's duration once non-thought content follows it.
    fn finalize_thought(&mut self) {
        if let Some(ChatItem::Thought(th)) = self.items.last_mut() {
            if th.elapsed_secs.is_none() {
                let secs = th.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                th.elapsed_secs = Some(secs);
            }
        }
    }

    /// Toggle a thought's expanded reasoning — in the live items or, for a
    /// resumed transcript's read-only history, in `history`.
    pub fn toggle_thought(&mut self, target: TranscriptItemId) {
        if let Some(ChatItem::Thought(th)) = self.list_mut(target).get_mut(target.index()) {
            th.expanded = !th.expanded;
        }
    }

    /// A parked permission prompt is waiting for the user's answer (it blocks
    /// the turn until they click an option). Also drives the rail session
    /// row's status dot.
    pub fn awaiting_permission(&self) -> bool {
        self.items.iter().any(|it| match it {
            ChatItem::Permission(p) => p.resolved.is_none(),
            // An unanswered question blocks the turn exactly the same way.
            ChatItem::Ask(a) => a.resolved.is_none(),
            _ => false,
        })
    }
    /// What this conversation is doing right now, for the header's status line —
    /// The end of the agent's last answer, for anything that has to say what a
    /// turn came to somewhere the transcript is not.
    ///
    /// **The end and not the beginning**, which is the whole point. An answer
    /// opens by restating the problem and closes by saying what was done about
    /// it, so a notification carrying the first paragraph is a notification
    /// telling the user something they already knew, and one carrying the last
    /// is the answer.
    ///
    /// The cut is moved forward to the next paragraph, failing that the next
    /// line, failing that the next space — so the excerpt starts on something
    /// rather than halfway through a word. All three are looked for in the same
    /// short window, because a boundary hunted far enough would throw away most
    /// of what was asked for: one unbroken run of characters is a URL or a blob
    /// rather than prose, and beginning in the middle of one costs nothing worth
    /// the rest of the answer. What is left is marked with a leading ellipsis,
    /// because an excerpt that does not say it is one reads as the whole reply.
    ///
    /// `None` when the turn produced no prose at all: a turn can end on a tool
    /// call or be cancelled before the agent says anything, and inventing a
    /// sentence for that would be worse than the headline alone.
    pub fn answer_tail(&self, max: usize) -> Option<String> {
        // The live transcript only. `history` is a resumed conversation's
        // archive, so reaching into it would let a session that has just come
        // back announce, as the result of its first turn, the end of a turn from
        // last week.
        let text = self
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                ChatItem::Agent(md) => Some(md.source.trim()),
                _ => None,
            })
            .filter(|text| !text.is_empty())?;

        let total = text.chars().count();
        if total <= max {
            return Some(text.to_string());
        }
        let cut = text
            .char_indices()
            .nth(total - max)
            .map_or(0, |(index, _)| index);
        let tail = &text[cut..];

        // How far a boundary may be hunted for. A quarter of the excerpt is
        // enough to clear a broken word or a stray list marker and not enough to
        // turn a paragraph's worth of answer into two lines.
        let give_up = tail
            .char_indices()
            .nth(max / 4)
            .map_or(tail.len(), |(index, _)| index);
        let head = &tail[..give_up];
        // A paragraph break beats a line break: inside a list or a fenced block
        // every line ends in one, and stopping at the first would start the
        // excerpt on the second half of an enumeration. A space is the last
        // resort and the common one -- in ordinary prose it is a character or
        // two away, and it is what keeps the excerpt from opening on the tail of
        // a broken word.
        let start = head
            .find("\n\n")
            .map(|index| index + 2)
            .or_else(|| head.find('\n').map(|index| index + 1))
            .or_else(|| {
                head.char_indices()
                    .find(|(_, c)| c.is_whitespace())
                    .map(|(index, c)| index + c.len_utf8())
            })
            .unwrap_or(0);
        Some(format!("…{}", tail[start..].trim_start()))
    }

    /// `None` when there is nothing to say. Derived from the link and, once
    /// that is up, from the live transcript while a turn is in flight.
    pub fn activity_status(&self) -> Option<String> {
        // Before anything else, because it outranks everything else: until the
        // handshake lands there is no agent to be doing any of it. A resumed
        // conversation shows its archive the moment it is picked, so without
        // this the pane looks live several seconds before it is -- and the only
        // thing that said otherwise was a Send that refused when pressed.
        if self.link == Link::Connecting {
            return Some(format!("Connecting to {}…", self.agent));
        }
        if !self.busy {
            return None;
        }
        if self.awaiting_permission() {
            return Some("Waiting for your approval…".to_string());
        }
        match self.items.last() {
            // A live thought already renders "Thinking…"; a running tool already
            // shows "· running" — don't repeat them in the trailing status.
            Some(ChatItem::Thought(th)) if th.elapsed_secs.is_none() => None,
            Some(ChatItem::Tool(t))
                if matches!(t.call.status, ToolStatus::InProgress | ToolStatus::Pending) =>
            {
                None
            }
            Some(ChatItem::Agent(_)) => Some("Responding…".to_string()),
            _ => Some("Working…".to_string()),
        }
    }

    fn push_user_chunk(&mut self, s: &str) {
        match self.items.last_mut() {
            Some(ChatItem::User(u)) if self.user_chunk_open => u.text.push_str(s),
            _ => self.items.push(ChatItem::User(UserMsg::text(s))),
        }
        self.user_chunk_open = true;
    }
}

/// Archive the conversation when the chat is dropped (session closed / app
/// exit). `save_chat` is a no-op while empty, so this never clobbers.
///
/// A failure here is logged and nothing more, which is the whole answer at this
/// one point: a chat is dropped while its window is being torn down, so there
/// is nothing left to raise a message on, and blocking teardown on one would be
/// worse than the loss. The per-turn save covers the same conversation every
/// turn and *does* speak up, so any standing condition — a full disk, a
/// read-only directory — has already been said out loud long before this runs.
impl Drop for Chat {
    fn drop(&mut self) {
        self.save_blocking();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_uses_first_nonblank_line_collapsed() {
        assert_eq!(
            summarize_title("\n\n  fix   the   bug \nsecond line").as_deref(),
            Some("Fix the bug"),
        );
    }

    #[test]
    fn title_removes_request_filler_and_question_suffixes() {
        assert_eq!(
            summarize_title("Could you please fix the flaky login test?").as_deref(),
            Some("Fix the flaky login test"),
        );
        assert_eq!(
            summarize_title("Có thể giúp mình sửa luồng đăng nhập được không nhỉ?").as_deref(),
            Some("Sửa luồng đăng nhập"),
        );
    }

    #[test]
    fn title_rewrites_make_better_as_a_task() {
        assert_eq!(
            summarize_title("có thể làm cho phần tên Conversation đẹp hơn được không nhỉ")
                .as_deref(),
            Some("Cải thiện phần tên Conversation"),
        );
        assert_eq!(
            summarize_title("Can you make the conversation names better?").as_deref(),
            Some("Improve the conversation names"),
        );
    }

    #[test]
    fn title_caps_at_a_word_boundary() {
        let t = summarize_title(
            "Implement OAuth callback handling for the desktop application and document every edge case",
        )
        .unwrap();
        assert_eq!(t, "Implement OAuth callback handling for the…");
        assert!(t.chars().count() <= TITLE_MAX_CHARS + 1);
    }

    #[test]
    fn title_is_none_for_blank_prompt() {
        assert_eq!(summarize_title("   \n\t "), None);
    }

    #[test]
    fn derived_title_prefers_first_user_prompt() {
        let mut chat = Chat::default();
        chat.items.push(ChatItem::Agent(Md::parse("hi")));
        chat.items
            .push(ChatItem::User(UserMsg::text("Add a title per session")));
        chat.items
            .push(ChatItem::User(UserMsg::text("second prompt")));
        assert_eq!(
            chat.derived_title().as_deref(),
            Some("Add a title per session")
        );
    }

    #[test]
    fn derived_title_is_none_before_any_prompt() {
        let chat = Chat::default();
        assert_eq!(chat.derived_title(), None);
    }

    #[test]
    fn custom_title_overrides_and_reset_restores_automatic_title() {
        let mut chat = Chat::default();
        chat.items
            .push(ChatItem::User(UserMsg::text("Fix the login flow")));

        assert!(chat.rename("  Auth   cleanup\n"));
        assert_eq!(chat.conversation_title().as_deref(), Some("Auth cleanup"));

        chat.reset_title();
        assert_eq!(
            chat.conversation_title().as_deref(),
            Some("Fix the login flow")
        );
    }

    #[test]
    fn blank_custom_title_is_ignored() {
        let mut chat = Chat::default();
        assert!(!chat.rename("  \n\t "));
        assert!(chat.custom_title.is_none());
    }

    #[test]
    fn tool_out_sections_fold_independently() {
        // Two text outputs on one tool must fold independently (per-section
        // indices in `out_open`), and the whole-card fold is its own flag.
        let mut chat = Chat::default();
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "t1".into(),
            title: "cmd".into(),
            description: None,
            kind: crate::acp::ToolKind::Other,
            status: ToolStatus::Completed,
            content: vec![],
        })));
        chat.toggle_tool_output(TranscriptItemId::Live(0), 0);
        let ChatItem::Tool(t) = &chat.items[0] else {
            panic!()
        };
        assert!(t.out_open.contains(&0));
        assert!(!t.out_open.contains(&1));
        assert!(!t.is_open(), "completed + unfolded card stays collapsed");
        chat.toggle_tool(TranscriptItemId::Live(0));
        let ChatItem::Tool(t) = &chat.items[0] else {
            panic!()
        };
        assert!(t.is_open());
    }

    #[test]
    fn only_running_tools_start_open() {
        let call = |status| ToolCall {
            id: "t".into(),
            title: String::new(),
            description: None,
            kind: crate::acp::ToolKind::Other,
            status,
            content: vec![],
        };
        assert!(ToolItem::new(call(ToolStatus::InProgress)).is_open());
        assert!(!ToolItem::new(call(ToolStatus::Failed)).is_open());
        assert!(!ToolItem::new(call(ToolStatus::Completed)).is_open());
        assert!(!ToolItem::new(call(ToolStatus::Pending)).is_open());
    }

    #[test]
    fn failed_tool_opens_only_on_user_request() {
        let mut chat = Chat::default();
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "failed".into(),
            title: "cargo test".into(),
            description: None,
            kind: crate::acp::ToolKind::Execute,
            status: ToolStatus::Failed,
            content: vec![ToolContent::Text("Exit code 1".into())],
        })));

        let open = |chat: &Chat| match &chat.items[0] {
            ChatItem::Tool(tool) => tool.is_open(),
            _ => false,
        };
        assert!(!open(&chat), "a new failure starts collapsed");
        chat.toggle_tool(TranscriptItemId::Live(0));
        assert!(open(&chat), "the user can reveal a failed tool");
        chat.toggle_tool(TranscriptItemId::Live(0));
        assert!(!open(&chat), "the failed tool can be collapsed again");
    }

    #[test]
    fn plan_updates_replace_in_place_per_turn() {
        // The agent republishes the full checklist on every change: within one
        // turn the card updates in place (fold survives); a new turn gets its
        // own card.
        let entry = |s: &str, st| PlanEntry {
            content: s.into(),
            status: st,
        };
        let mut chat = Chat::default();
        chat.push_user("go".into(), Vec::new());
        chat.apply(AcpEvent::Plan(vec![entry("a", PlanStatus::Pending)]));
        chat.toggle_tool(TranscriptItemId::Live(1)); // user opens the card
        chat.apply(AcpEvent::Plan(vec![
            entry("a", PlanStatus::Completed),
            entry("b", PlanStatus::InProgress),
        ]));
        let plans: Vec<&PlanItem> = chat
            .items
            .iter()
            .filter_map(|i| match i {
                ChatItem::Plan(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(plans.len(), 1, "same turn replaces in place");
        assert_eq!(plans[0].entries.len(), 2);
        assert!(plans[0].fold, "user fold survives the update");
        assert!(plans[0].is_open(), "in_progress force-opens");

        chat.push_user("next".into(), Vec::new());
        chat.apply(AcpEvent::Plan(vec![entry("c", PlanStatus::Pending)]));
        let count = chat
            .items
            .iter()
            .filter(|i| matches!(i, ChatItem::Plan(_)))
            .count();
        assert_eq!(count, 2, "a new turn gets its own card");
    }

    #[test]
    fn code_block_ids_follow_fence_open_order() {
        // BlockId = fence-open order, stable under streaming re-parse:
        // an unterminated fence is the streaming block; closing it (and adding
        // more prose) never renumbers earlier blocks.
        let mut md = Md::parse("```rust\nlet x = 1;\n");
        assert_eq!(md.streaming_block(), Some(0));
        md.push("```\n\ntext\n\n```sh\nls\n");
        assert_eq!(md.streaming_block(), Some(1));
        md.toggle_block(0);
        md.push("```\n");
        assert_eq!(md.streaming_block(), None);
        assert!(md.open_blocks.contains(&0), "fold survives later blocks");
    }

    #[test]
    fn turn_answer_combines_a_turn_and_marks_the_last_block() {
        // A turn split across a tool: two agent blocks bracketing one tool call.
        let mut chat = Chat::default();
        let mut user = UserMsg::text("hi");
        user.sent_at = Some(100);
        user.completed_at = Some(112);
        chat.items.push(ChatItem::User(user));
        chat.items.push(ChatItem::Agent(Md::parse("first")));
        chat.items.push(ChatItem::notice("a tool ran"));
        chat.items.push(ChatItem::Agent(Md::parse("second")));

        // Copy from either block takes the whole turn, not the fragment…
        let a = chat.turn_answer(TranscriptItemId::Live(1)).unwrap();
        let b = chat.turn_answer(TranscriptItemId::Live(3)).unwrap();
        assert_eq!(
            chat.turn_prose(TranscriptItemId::Live(1)),
            "first\n\nsecond"
        );
        assert_eq!(
            chat.turn_prose(TranscriptItemId::Live(3)),
            "first\n\nsecond"
        );
        // …but only the trailing block is the turn's last (one Copy per turn).
        assert!(!a.is_last);
        assert!(b.is_last);
        // Idle transcript → not the active streaming turn.
        assert!(!b.is_active);
        assert_eq!(b.elapsed_secs, Some(12));
        // A non-agent index has no turn answer.
        assert!(chat.turn_answer(TranscriptItemId::Live(0)).is_none());
        assert!(chat.turn_answer(TranscriptItemId::Live(2)).is_none());
    }

    #[test]
    fn turn_changes_is_safe_for_history_indices() {
        // Even a typed but out-of-range history target must return safely.
        let chat = Chat::default();
        assert!(chat
            .turn_changes(TranscriptItemId::History(usize::MAX))
            .is_empty());
        assert!(chat.turn_changes(TranscriptItemId::Live(0)).is_empty());
    }

    /// The card carries its hunks, and a later report of the same tool call
    /// replaces them. The view draws what is here and computes nothing: an edit
    /// script is quadratic in the worst case, and a redraw happens for every
    /// notch of a scroll wheel.
    #[test]
    fn a_card_holds_the_hunks_its_edit_produced() {
        let diff = |new: &str| ToolContent::Diff {
            path: "a.rs".into(),
            old: Some("x\ny\n".into()),
            new: new.into(),
        };
        let mut chat = Chat::default();
        chat.apply(AcpEvent::ToolCall(ToolCall {
            id: "t1".into(),
            title: "edit".into(),
            description: None,
            kind: crate::acp::ToolKind::Edit,
            status: ToolStatus::InProgress,
            content: vec![diff("x\ny\nz\n")],
        }));

        let hunks = |chat: &Chat| match &chat.items[0] {
            ChatItem::Tool(t) => t.diff_rows.get(&0).cloned().unwrap_or_default(),
            _ => panic!("expected a tool card"),
        };
        assert!(
            hunks(&chat).contains(&crate::diff::Row::Added("z".to_string())),
            "the added line is in the card's hunks"
        );

        chat.apply(AcpEvent::ToolUpdate(crate::acp::ToolCallUpdate {
            id: "t1".into(),
            title: None,
            description: None,
            status: Some(ToolStatus::Completed),
            content: Some(vec![diff("x\ny\nw\n")]),
        }));
        let after = hunks(&chat);
        assert!(after.contains(&crate::diff::Row::Added("w".to_string())));
        assert!(
            !after.contains(&crate::diff::Row::Added("z".to_string())),
            "a re-reported edit replaces the hunks rather than adding to them"
        );
    }

    #[test]
    fn turn_changes_aggregates_diffs_per_path() {
        let mut chat = Chat::default();
        chat.push_user("go".into(), Vec::new());
        chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
            id: "t1".into(),
            title: "edit".into(),
            description: None,
            kind: crate::acp::ToolKind::Edit,
            status: ToolStatus::Completed,
            content: vec![ToolContent::Diff {
                path: "a.rs".into(),
                old: Some("x\ny\n".into()),
                new: "x\ny\nz\n".into(),
            }],
        })));
        chat.items.push(ChatItem::Agent(Md::parse("done")));
        let changes = chat.turn_changes(TranscriptItemId::Live(2));
        assert_eq!(changes, vec![("a.rs".to_string(), 1, 0)]);
    }

    #[test]
    fn line_change_counts_reports_actual_edits() {
        assert_eq!(line_change_counts(Some("a\nb\nc\n"), "a\nx\nc\n"), (1, 1));
        assert_eq!(line_change_counts(Some("a\nb\n"), "a\nb\nc\n"), (1, 0));
        assert_eq!(line_change_counts(Some("a\nb\n"), "a\n"), (0, 1));
        assert_eq!(line_change_counts(Some("same\n"), "same\n"), (0, 0));
        assert_eq!(line_change_counts(None, "new\nfile\n"), (2, 0));
    }

    #[test]
    fn history_turns_keep_label_and_copy_metadata() {
        let mut chat = Chat::default();
        chat.history
            .push(ChatItem::User(UserMsg::text("old prompt")));
        chat.history.push(ChatItem::Agent(Md::parse("first")));
        chat.history.push(ChatItem::notice("tool finished"));
        chat.history.push(ChatItem::Agent(Md::parse("second")));

        let first = chat.turn_answer(TranscriptItemId::History(1)).unwrap();
        let last = chat.turn_answer(TranscriptItemId::History(3)).unwrap();
        assert!(!first.is_last);
        assert!(last.is_last);
        assert!(!last.is_active);
        assert_eq!(
            chat.turn_prose(TranscriptItemId::History(3)),
            "first\n\nsecond",
            "a resumed turn copies out of history, not out of the live tail"
        );
    }

    #[test]
    fn turn_answer_marks_the_streaming_turn_active() {
        let mut chat = Chat::default();
        chat.items.push(ChatItem::User(UserMsg::text("hi")));
        chat.items.push(ChatItem::Agent(Md::parse("streaming")));
        // No user prompt after it + a turn in flight ⇒ active (Copy hidden).
        chat.busy = true;
        assert!(
            chat.turn_answer(TranscriptItemId::Live(1))
                .unwrap()
                .is_active
        );
        // Once the turn settles it is copyable.
        chat.busy = false;
        assert!(
            !chat
                .turn_answer(TranscriptItemId::Live(1))
                .unwrap()
                .is_active
        );
    }

    #[test]
    fn finishing_a_turn_stamps_only_the_latest_timed_prompt() {
        let mut chat = Chat::default();
        chat.items.push(ChatItem::User(UserMsg::text("legacy")));
        let mut current = UserMsg::text("current");
        current.sent_at = Some(40);
        chat.items.push(ChatItem::User(current));

        chat.finish_active_turn(47);

        let ChatItem::User(legacy) = &chat.items[0] else {
            panic!("expected legacy prompt")
        };
        let ChatItem::User(current) = &chat.items[1] else {
            panic!("expected current prompt")
        };
        assert_eq!(legacy.completed_at, None);
        assert_eq!(current.completed_at, Some(47));
    }

    #[test]
    fn terminal_output_trim_lands_on_char_boundary() {
        // Force the retained-tail cut to land mid-'→' (3 bytes each): one ASCII
        // byte shifts every char boundary to 1+3k, and the raw cut offset is
        // not on that grid. The old byte-slice trim panicked here.
        let mut chat = Chat::default();
        let big = "→".repeat((MAX_TERM_BYTES / 3) + 100);
        chat.apply(AcpEvent::TerminalOutput {
            terminal_id: "t1".into(),
            chunk: format!("a{big}"),
        });
        let view = &chat.terminals["t1"];
        assert!(view.output.len() <= MAX_TERM_BYTES);
        assert!(view.output.chars().all(|c| c == '→'));
    }

    #[test]
    fn load_history_resets_the_live_transcript() {
        // Restart keeps the chat; loading the archive over live items must not
        // leave both — the view chains history ⧺ items (double render) and the
        // next save would double the archive.
        let mut chat = Chat::default();
        chat.items.push(ChatItem::User(UserMsg::text("hi")));
        chat.items.push(ChatItem::Agent(Md::parse("reply")));
        chat.load_history(vec![ChatItem::User(UserMsg::text("hi"))], "sid-1".into(), 1);
        assert!(chat.items.is_empty());
        assert_eq!(chat.history.len(), 1);
        assert!(chat.replay_pending());
        assert_eq!(chat.session_id.as_deref(), Some("sid-1"));
    }

    #[test]
    fn user_prompt_keeps_unreplayed_history() {
        // A resume whose `session/load` replays nothing: the first user prompt
        // must keep the loaded history (it is the only copy of the
        // conversation), while real replayed content still consumes it.
        let mut chat = resumed_chat(1);
        chat.push_user("new prompt".into(), Vec::new());
        assert_eq!(chat.history.len(), 1, "history must survive a user prompt");
        assert!(!chat.replay_pending());
        // And the prompt is added to the conversation, not written as the whole
        // of it: one line, after the one already there.
        let write = chat.flush().expect("the prompt is new");
        assert_eq!(write.lines.len(), 1);
        assert!(!write.rewrite);

        let mut chat = resumed_chat(1);
        chat.apply(AcpEvent::AgentChunk("replayed".into()));
        assert!(chat.history.is_empty(), "a real replay still drops history");
    }

    /// A conversation of `n` messages, resumed from a store it will never be
    /// written to — every assertion here is about what a save *would* carry.
    fn resumed_chat(n: usize) -> Chat {
        let mut chat = Chat::new(
            1,
            PathBuf::from("/r"),
            "Claude".into(),
            Some(PathBuf::from("/store")),
        );
        chat.resume_from(ConversationSnapshot {
            session_id: "sid-1".into(),
            title: None,
            updated: 1,
            created: 1,
            prefs: Prefs::default(),
            items: (0..n)
                .map(|i| ChatItem::User(UserMsg::text(format!("message {i}"))))
                .collect(),
            written: n,
            complete: true,
        });
        chat
    }

    /// The headline rule: a replay that stops halfway is not written down.
    ///
    /// A `session/load` re-delivers the conversation as ordinary content, so the
    /// first piece of it takes the adopted copy off the screen — and at that
    /// moment an adapter that dies leaves two messages standing where fifty
    /// were. What `items` holds then is a re-delivery of lines already on disk,
    /// so nothing about it can be added to the file without saying those lines
    /// twice or writing a fragment as though it were the whole.
    #[test]
    fn an_interrupted_replay_is_never_written() {
        let mut chat = resumed_chat(3);
        chat.apply(AcpEvent::AgentChunk("first replayed answer".into()));
        // On screen the adopted copy is gone, exactly as before.
        assert!(chat.history.is_empty());
        assert!(chat.flush().is_none(), "and nothing is written from it");

        // The adapter then dies. Closing the session still writes nothing, so
        // the three messages on disk stay three messages.
        chat.apply(AcpEvent::Disconnected("adapter exited".into()));
        assert!(chat.flush().is_none());
    }

    /// A resume that succeeds can still deliver less than the file holds.
    ///
    /// The replay is the conversation as the *agent* holds it, and the file
    /// holds things the agent has no reason to send back — the tool cards, the
    /// plans, the reasoning. Trading the full transcript for the agent's summary
    /// of it would be permanent, so the shorter side never wins, however the
    /// load went.
    #[test]
    fn a_short_replay_loses_to_the_file_even_when_the_load_succeeded() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chat = resumed_chat(4);
        chat.apply(AcpEvent::AgentChunk("a summary of all that".into()));
        chat.apply(AcpEvent::Connected { tx, resumed: true });

        assert_eq!(chat.history.len(), 4, "the conversation is back on screen");
        assert!(chat.flush().is_none(), "and the file is left as it was");
    }

    /// …and once the load has answered with everything, the replay *is* the
    /// record — which means the file has to become it rather than gain it.
    #[test]
    fn a_completed_replay_rewrites_the_file_with_what_was_replayed() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chat = resumed_chat(2);
        chat.apply(AcpEvent::UserChunk("message 0".into()));
        chat.apply(AcpEvent::AgentChunk("answer".into()));
        chat.apply(AcpEvent::Connected { tx, resumed: true });

        assert!(chat.history.is_empty());
        assert!(!chat.replay_pending());
        let write = chat.flush().expect("the replay has to be written down");
        assert!(write.rewrite, "spliced onto the old lines, it would seam");
        assert_eq!(write.lines.len(), 2, "the replayed transcript, not the old");
        // And it is written once: the mark now says the file holds it.
        assert!(chat.flush().is_none());
    }

    /// A resume the adapter could not honour puts the conversation back.
    ///
    /// The client falls back to a fresh session and says so; the file it failed
    /// to load is the only copy there is, and what the adapter managed to
    /// stream before failing is a fragment of it.
    #[test]
    fn a_failed_resume_puts_the_conversation_back() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chat = resumed_chat(3);
        chat.apply(AcpEvent::UserChunk("message 0".into()));
        chat.apply(AcpEvent::Error("session/load failed".into()));
        chat.apply(AcpEvent::Connected { tx, resumed: false });

        assert_eq!(chat.history.len(), 3, "the conversation is back on screen");
        assert_eq!(
            chat.items.len(),
            1,
            "and the fragment is gone, but not the reason"
        );
        assert!(matches!(chat.items[0], ChatItem::Notice { .. }));
        // The three are already on disk; only the reason is new.
        let write = chat.flush().expect("the notice is worth keeping");
        assert!(!write.rewrite);
        assert_eq!(write.lines.len(), 1);
    }

    /// A replay longer than the file is the better record, and survives
    /// settling. That happens when a previous run died between the last save
    /// and the end of a turn: the agent has the turn, the file does not.
    #[test]
    fn a_replay_that_delivered_more_is_kept() {
        let mut chat = resumed_chat(2);
        for i in 0..3 {
            chat.apply(AcpEvent::UserChunk(format!("message {i}")));
            chat.apply(AcpEvent::AgentChunk("answer".into()));
        }
        chat.push_user("carry on".into(), Vec::new());

        assert!(chat.history.is_empty(), "the shorter copy is not put back");
        let write = chat.flush().unwrap();
        assert!(write.rewrite);
        assert_eq!(write.lines.len(), 7);
    }

    /// A transcript that came back as its tail must never be written back over
    /// the file it is a tail of.
    #[test]
    fn a_bounded_transcript_never_rewrites_the_file() {
        let mut chat = resumed_chat(2);
        chat.bounded = true;
        for i in 0..3 {
            chat.apply(AcpEvent::UserChunk(format!("message {i}")));
            chat.apply(AcpEvent::AgentChunk("answer".into()));
        }
        chat.push_user("carry on".into(), Vec::new());

        let write = chat.flush().unwrap();
        assert!(!write.rewrite, "the file is longer than what is on screen");
        assert_eq!(write.lines.len(), 1, "only the new prompt is added");
    }

    /// A restart carries the mark, so the replacement session continues the
    /// file rather than writing the conversation into it again.
    #[test]
    fn a_restart_carries_the_mark() {
        let mut chat = resumed_chat(2);
        chat.push_user("and one more".into(), Vec::new());

        let snapshot = chat.take_snapshot().expect("there is a conversation");
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.written, 2, "two of the three are already on disk");
        // The chat it came from has nothing left to write, which is what keeps
        // its own drop from saying all of it a second time.
        assert!(chat.flush().is_none());

        let mut next = Chat::new(
            2,
            PathBuf::from("/r"),
            "Claude".into(),
            Some(PathBuf::from("/store")),
        );
        next.resume_from(snapshot);
        let write = next.flush().expect("the unwritten prompt still is");
        assert_eq!(write.lines.len(), 1);
    }

    #[test]
    fn replayed_user_messages_split_across_intervening_content() {
        let mut chat = Chat::default();
        // One message replayed in two chunks merges into one bubble…
        chat.apply(AcpEvent::UserChunk("hello ".into()));
        chat.apply(AcpEvent::UserChunk("world".into()));
        assert_eq!(
            chat.items
                .iter()
                .filter(|i| matches!(i, ChatItem::User(_)))
                .count(),
            1
        );
        // …but a second prompt after agent content is its own bubble.
        chat.apply(AcpEvent::AgentChunk("reply".into()));
        chat.apply(AcpEvent::UserChunk("second prompt".into()));
        assert_eq!(
            chat.items
                .iter()
                .filter(|i| matches!(i, ChatItem::User(_)))
                .count(),
            2
        );
    }

    /// A two-question elicitation: one single-select with an "Other" box, one
    /// multi-select without.
    fn ask_item() -> AskItem {
        let choice = |v: &str| crate::acp::ElicitChoice {
            value: v.into(),
            label: v.to_uppercase(),
            description: None,
        };
        AskItem::new(Elicitation {
            rpc_id: serde_json::json!(1),
            tool_call_id: None,
            message: "pick".into(),
            fields: vec![
                crate::acp::ElicitField {
                    key: "question_0".into(),
                    title: None,
                    description: None,
                    kind: ElicitKind::Select(vec![choice("a"), choice("b")]),
                    custom_key: Some("question_0_custom".into()),
                },
                crate::acp::ElicitField {
                    key: "question_1".into(),
                    title: None,
                    description: None,
                    kind: ElicitKind::MultiSelect(vec![choice("x"), choice("y")]),
                    custom_key: None,
                },
            ],
        })
    }

    #[test]
    fn tab_clamps_and_answered_tracks_picks_and_text() {
        let mut a = ask_item();
        assert_eq!(a.active_field(), 0);
        a.tab = 1;
        assert_eq!(a.active_field(), 1);
        a.tab = 9; // a stale cursor never points off the end of the form
        assert_eq!(a.active_field(), 1);

        assert!(!a.field_answered(0) && !a.field_answered(1));
        a.toggle(1, 0);
        assert!(a.field_answered(1));
        a.set_custom(0, "  ".into()); // blank text isn't an answer
        assert!(!a.field_answered(0));
        a.set_custom(0, "mine".into());
        assert!(a.field_answered(0));
    }

    #[test]
    fn single_select_replaces_multi_select_toggles() {
        let mut a = ask_item();
        a.toggle(0, 0);
        a.toggle(0, 1);
        assert_eq!(a.picked[0], vec![1]); // replaced, not accumulated
        a.toggle(1, 0);
        a.toggle(1, 1);
        assert_eq!(a.picked[1], vec![0, 1]);
        a.toggle(1, 0); // toggling an on choice turns it off
        assert_eq!(a.picked[1], vec![1]);
    }

    #[test]
    fn answers_use_wire_values_and_skip_empty_fields() {
        let mut a = ask_item();
        a.toggle(0, 1);
        assert_eq!(
            a.answers(),
            vec![("question_0".to_string(), ElicitValue::Text("b".into()))],
        );
        a.toggle(1, 0);
        assert_eq!(
            a.answers()[1],
            (
                "question_1".to_string(),
                ElicitValue::List(vec!["x".into()])
            ),
        );
        // The summary is the *labels*, not the wire values.
        assert_eq!(a.summary(), "B, X");
    }

    #[test]
    fn typed_text_answers_under_the_custom_key_and_clears_the_pick() {
        let mut a = ask_item();
        a.toggle(0, 0);
        a.set_custom(0, "  something else  ".into());
        assert!(a.picked[0].is_empty());
        assert_eq!(
            a.answers(),
            vec![(
                "question_0_custom".to_string(),
                ElicitValue::Text("something else".into())
            )],
        );
        assert_eq!(a.summary(), "something else");
        // …and picking again drops the text, so the two never both apply.
        a.toggle(0, 0);
        assert_eq!(a.custom[0], "");
        assert_eq!(
            a.answers(),
            vec![("question_0".into(), ElicitValue::Text("a".into()))]
        );
    }

    #[test]
    fn copy_text_carries_the_prompt_and_every_choice() {
        let a = ask_item();
        assert_eq!(a.copy_text(), "pick\n- A\n- B\n- X\n- Y\n");
    }

    #[test]
    fn quick_form_is_one_single_select_only() {
        let a = ask_item();
        assert!(!a.is_quick()); // two questions
        assert!(!a.has_answer());
        let mut one = AskItem::new(Elicitation {
            fields: vec![a.req.fields[0].clone()],
            ..a.req.clone()
        });
        assert!(one.is_quick());
        one.toggle(0, 0);
        assert!(one.has_answer());
    }

    #[test]
    fn a_parked_question_reads_as_awaiting_the_user() {
        let mut chat = Chat::default();
        chat.apply(AcpEvent::Elicitation(ask_item().req));
        assert!(chat.awaiting_permission());
        assert_eq!(chat.pending_asks().len(), 1);
        // Answering settles the card and empties the sticky bar.
        chat.answer_ask(0, false);
        assert!(!chat.awaiting_permission());
        assert!(chat.pending_asks().is_empty());
    }

    /// A chat wired to a request channel, with the drain end returned so a test
    /// can read back what `reapply_prefs` sent to the adapter.
    /// `submit` is the one rule both front ends route through, so the refusals
    /// matter as much as the send: each one is a way a prompt could be silently
    /// lost or double-counted if a front end reimplemented the guard.
    #[test]
    fn submit_sends_a_prompt_and_opens_a_turn() {
        let (mut chat, mut rx) = chat_with_tx();

        assert!(chat.submit("  build it  ", &[]));
        assert!(chat.busy, "a sent prompt opens a turn");
        assert!(matches!(
            rx.try_recv(),
            Ok(AcpRequest::Prompt { text, .. }) if text == "build it"
        ));
        assert!(matches!(chat.items.last(), Some(ChatItem::User(u)) if u.text == "build it"));
    }

    #[test]
    fn submit_refuses_and_records_nothing() {
        let (mut chat, mut rx) = chat_with_tx();

        // Blank with no attachments.
        assert!(!chat.submit("   ", &[]));
        // A turn already in flight.
        chat.busy = true;
        assert!(!chat.submit("second", &[]));
        chat.busy = false;
        // No live channel.
        chat.tx = None;
        assert!(!chat.submit("third", &[]));

        assert!(
            chat.items.is_empty(),
            "a refused prompt leaves no transcript"
        );
        assert!(!chat.busy);
        assert!(rx.try_recv().is_err());
    }

    /// The blocker is what a front end draws on Send, so it has to name the
    /// *same* refusals `submit` acts on. Answering "Empty" while `submit`
    /// refused for a dead channel is a Send that looks fixable by typing.
    #[test]
    fn the_blocker_names_every_refusal_submit_makes() {
        let (mut chat, _rx) = chat_with_tx();

        assert_eq!(chat.submit_blocker("   ", &[]), Some(SubmitBlock::Empty));
        assert_eq!(chat.submit_blocker("go", &[]), None);

        // A path that is not there reads back as unreadable, which is the
        // state the tray marks in the danger tint.
        let unreadable = StagedAttachment::inspect(
            PathBuf::from("/tmp/onehand-no-such-file.png"),
            crate::attachment::AttachmentSource::Picker,
        );
        assert_eq!(
            chat.submit_blocker("go", std::slice::from_ref(&unreadable)),
            Some(SubmitBlock::UnreadableAttachment(
                "onehand-no-such-file.png".to_string()
            )),
            "the file is named, so the tray does not have to be searched"
        );

        // A turn in flight outranks the prompt's own problems: Stop is the
        // only thing the user can do about it.
        chat.busy = true;
        assert_eq!(chat.submit_blocker("   ", &[]), Some(SubmitBlock::Busy));
        chat.busy = false;

        chat.tx = None;
        assert_eq!(
            chat.submit_blocker("go", &[]),
            Some(SubmitBlock::NotConnected)
        );
    }

    /// A prompt written mid-turn goes when the turn does -- and only then, and
    /// only once. Flushed anywhere but at the end of a turn it either races the
    /// turn it was queued behind or never leaves at all.
    #[test]
    fn a_queued_prompt_goes_out_when_the_turn_ends() {
        let (mut chat, mut rx) = chat_with_tx();
        assert!(chat.submit("first", &[]));
        assert!(chat.busy);
        assert!(matches!(rx.try_recv(), Ok(AcpRequest::Prompt { .. })));

        assert!(chat.queue("second", &[]), "a turn in flight is what queues");
        assert!(rx.try_recv().is_err(), "queued is not sent");
        assert_eq!(chat.items.len(), 1, "and not in the transcript either");

        chat.apply(AcpEvent::TurnEnded {
            stop_reason: String::new(),
        });
        assert!(matches!(
            rx.try_recv(),
            Ok(AcpRequest::Prompt { text, .. }) if text == "second"
        ));
        assert!(chat.busy, "the queued prompt opened its own turn");
        assert!(chat.queued.is_none(), "and left the queue empty");
    }

    /// The queue is for the one blocker that time fixes. Everything else is
    /// still a refusal, or a prompt would sit waiting for a turn to end and
    /// then be refused all over again, silently.
    #[test]
    fn only_a_running_turn_queues() {
        let (mut chat, _rx) = chat_with_tx();
        assert!(!chat.queue("idle", &[]), "nothing is in the way");

        chat.busy = true;
        assert!(!chat.queue("   ", &[]), "an empty prompt is still empty");
        let unreadable = StagedAttachment::inspect(
            PathBuf::from("/tmp/onehand-no-such-file.png"),
            crate::attachment::AttachmentSource::Picker,
        );
        assert!(!chat.queue("go", std::slice::from_ref(&unreadable)));
        assert!(chat.queued.is_none());

        assert!(chat.queue("first", &[]));
        assert!(
            chat.queue("second", &[]),
            "the later prompt is the one meant"
        );
        assert_eq!(chat.unqueue().map(|q| q.text).as_deref(), Some("second"));
    }

    fn chat_with(items: Vec<ChatItem>) -> Chat {
        let mut chat = Chat::new(1, PathBuf::from("/tmp/p"), "Claude Code".to_string(), None);
        chat.items = items;
        chat
    }

    fn agent(source: &str) -> ChatItem {
        ChatItem::Agent(Md::parse(source))
    }

    /// A short answer goes whole and unmarked: an ellipsis in front of a
    /// complete reply is the notification claiming something was cut off.
    #[test]
    fn a_short_answer_is_carried_whole() {
        let chat = chat_with(vec![agent("  Done — the test passes now.  ")]);
        assert_eq!(
            chat.answer_tail(200).as_deref(),
            Some("Done — the test passes now.")
        );
    }

    /// The point of the whole thing: an answer opens by restating the problem
    /// and closes by saying what was done, so the end is the half worth sending.
    #[test]
    fn a_long_answer_is_carried_from_its_end() {
        // The closing paragraph is most of what the excerpt can hold, so its
        // break is inside the search window and the excerpt starts on it.
        let last = "So the fix is one line in the parser, and a test now covers it.";
        let source = format!("{}\n\n{last}", "x".repeat(400));
        let tail = chat_with(vec![agent(&source)]).answer_tail(80).unwrap();
        assert_eq!(tail, format!("…{last}"));
    }

    /// A boundary hunted far enough would throw away most of what was asked
    /// for, so past a quarter of the excerpt the cut simply stands. One
    /// unbroken run that long is a URL or a blob, and starting inside one costs
    /// nothing worth the rest of the answer.
    #[test]
    fn a_distant_boundary_is_not_worth_the_text_it_would_cost() {
        let source = format!("{}\n\nend", "y".repeat(300));
        let tail = chat_with(vec![agent(&source)]).answer_tail(100).unwrap();
        assert!(tail.starts_with("…y"), "got {tail:?}");
        assert!(tail.ends_with("end"));
        // Still about the length that was asked for, rather than three letters.
        assert!(
            tail.chars().count() > 90,
            "got {} chars",
            tail.chars().count()
        );
    }

    /// In prose the cut almost always lands inside a word, and the space after
    /// it is a character or two away — so the common case is an excerpt that
    /// opens on a whole word for almost no cost.
    #[test]
    fn an_excerpt_does_not_open_in_the_middle_of_a_word() {
        let source = "the quick brown fox jumps over the lazy dog and keeps on going";
        // 20 characters back from the end lands on the "d" of "dog"; the space
        // one character later is what the excerpt actually starts after.
        let tail = chat_with(vec![agent(source)]).answer_tail(20).unwrap();
        assert_eq!(tail, "…and keeps on going");
        assert!(source.ends_with(tail.trim_start_matches('…')));
    }

    /// A turn can end on a tool call or be cancelled before the agent says
    /// anything. Inventing a sentence for that is worse than the headline alone.
    #[test]
    fn a_turn_with_no_prose_has_no_tail() {
        assert_eq!(chat_with(vec![]).answer_tail(100), None);
        assert_eq!(chat_with(vec![agent("   \n  ")]).answer_tail(100), None);
        assert_eq!(
            chat_with(vec![ChatItem::User(UserMsg::text("hi"))]).answer_tail(100),
            None
        );
    }

    /// The *last* answer, not the first: a turn that spoke, ran a tool and
    /// spoke again ends on the second one, and that is the summary.
    #[test]
    fn the_last_answer_is_the_one_that_is_carried() {
        let chat = chat_with(vec![
            agent("Let me look at the parser."),
            ChatItem::User(UserMsg::text("ok")),
            agent("Fixed it."),
        ]);
        assert_eq!(chat.answer_tail(200).as_deref(), Some("Fixed it."));
    }

    /// Cutting by bytes would split a multi-byte character and produce an
    /// excerpt that is not text at all. Every boundary here is a character.
    #[test]
    fn a_tail_is_cut_by_characters_and_not_by_bytes() {
        let source = "Đã sửa xong phần phân tích cú pháp của trình biên dịch nhé";
        let tail = chat_with(vec![agent(source)]).answer_tail(10).unwrap();
        assert!(tail.ends_with("nhé"), "got {tail:?}");
        assert!(source.ends_with(tail.trim_start_matches('…')));
    }

    /// Until the handshake lands there is no agent to be doing anything, so
    /// that is what the status says -- ahead of every other answer it could
    /// give. A resumed conversation puts its archive on screen the moment it is
    /// picked, which is several seconds before anything can be sent to it.
    #[test]
    fn connecting_outranks_everything_the_status_could_say() {
        let mut chat = Chat::new(1, PathBuf::from("/tmp/p"), "Claude Code".to_string(), None);
        assert_eq!(
            chat.link,
            Link::Connecting,
            "a fresh chat has no adapter yet"
        );
        assert_eq!(
            chat.activity_status().as_deref(),
            Some("Connecting to Claude Code…")
        );

        // Even mid-turn: a turn cannot be in flight down a channel that is not
        // up yet, and saying "Working…" there is the pane claiming otherwise.
        chat.busy = true;
        assert_eq!(
            chat.activity_status().as_deref(),
            Some("Connecting to Claude Code…")
        );

        chat.link = Link::Connected;
        assert_eq!(chat.activity_status().as_deref(), Some("Working…"));
        chat.busy = false;
        assert_eq!(
            chat.activity_status(),
            None,
            "connected and idle says nothing"
        );
    }

    use crate::acp::PermissionOption;

    fn permission(title: &str) -> PermissionRequest {
        PermissionRequest {
            rpc_id: serde_json::Value::from(7),
            tool_call_id: None,
            title: title.into(),
            options: vec![
                PermissionOption {
                    id: "allow-1".into(),
                    name: "Allow once".into(),
                    kind: "allow_once".into(),
                },
                PermissionOption {
                    id: "reject-1".into(),
                    name: "Deny".into(),
                    kind: "reject_once".into(),
                },
            ],
        }
    }

    /// All four halves of a resume have to land. Each one that does not is a
    /// silent loss: the transcript, the user's own title, the selector state
    /// the adapter will *not* rebuild, and the date the rail shows.
    #[test]
    fn resuming_adopts_transcript_title_prefs_and_date() {
        let mut chat = Chat::default();
        chat.resume_from(ConversationSnapshot {
            session_id: "sess-9".into(),
            title: Some("Ship the parser".into()),
            updated: 1_700_000_000,
            created: 1_600_000_000,
            prefs: Prefs {
                mode: Some("plan".into()),
                config: vec![ConfigPick {
                    id: "effort".into(),
                    value: "high".into(),
                }],
            },
            items: Vec::new(),
            written: 0,
            complete: true,
        });

        assert_eq!(chat.session_id.as_deref(), Some("sess-9"));
        assert_eq!(chat.custom_title.as_deref(), Some("Ship the parser"));
        assert_eq!(chat.pending_mode.as_deref(), Some("plan"));
        assert_eq!(
            chat.pending_config,
            vec![("effort".to_string(), "high".to_string())]
        );
        // The rail dates a resumed session from its last real turn, not from
        // the moment it was reopened.
        assert_eq!(chat.last_activity, Some(1_700_000_000));
        assert!(chat.replay_pending(), "a resume arms the replay window");
    }

    /// Cancelling must resolve parked permissions *before* it cancels: ACP
    /// leaves a `session/request_permission` dangling otherwise, and the card
    /// keeps buttons whose click would answer a request nobody is waiting on.
    #[test]
    fn cancelling_resolves_parked_permissions_first() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.busy = true;
        chat.items.push(ChatItem::Permission(PermItem {
            req: permission("rm -rf build"),
            resolved: None,
        }));

        chat.cancel_turn();

        assert!(
            matches!(
                rx.try_recv(),
                Ok(AcpRequest::PermissionResponse {
                    option_id: None,
                    ..
                })
            ),
            "the parked permission is answered before the cancel"
        );
        assert!(matches!(rx.try_recv(), Ok(AcpRequest::Cancel)));
        assert!(!chat.awaiting_permission());
    }

    /// Answering has to reach the adapter *and* mark the card. Losing either
    /// half is how a turn ends up parked forever with live-looking buttons.
    #[test]
    fn answering_a_permission_replies_and_records_the_choice() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.items.push(ChatItem::Permission(PermItem {
            req: permission("rm -rf build"),
            resolved: None,
        }));

        chat.answer_permission(0, "allow-1");

        assert!(matches!(
            rx.try_recv(),
            Ok(AcpRequest::PermissionResponse { option_id: Some(id), .. }) if id == "allow-1"
        ));
        // The card records the *name*, the wire carries the id.
        assert!(matches!(
            chat.items.first(),
            Some(ChatItem::Permission(p)) if p.resolved.as_deref() == Some("Allow once")
        ));
        assert!(!chat.awaiting_permission());
    }

    /// A second click on an answered card would echo an rpc id the adapter has
    /// already resolved -- and after a restart, one it never issued at all.
    #[test]
    fn answering_twice_sends_only_once() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.items.push(ChatItem::Permission(PermItem {
            req: permission("rm -rf build"),
            resolved: None,
        }));

        chat.answer_permission(0, "allow-1");
        chat.answer_permission(0, "reject-1");

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "the second click must not reply");
        assert!(matches!(
            chat.items.first(),
            Some(ChatItem::Permission(p)) if p.resolved.as_deref() == Some("Allow once")
        ));
    }

    fn chat_with_tx() -> (Chat, tokio::sync::mpsc::UnboundedReceiver<AcpRequest>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chat = Chat::default();
        chat.tx = Some(tx);
        chat.modes = vec![
            Mode {
                id: "default".into(),
                name: "Default".into(),
            },
            Mode {
                id: "plan".into(),
                name: "Plan".into(),
            },
        ];
        chat.current_mode = Some("default".into());
        chat.config_options = vec![
            ConfigOption {
                id: "effort".into(),
                name: "Effort".into(),
                current: Some("default".into()),
                choices: vec![
                    crate::acp::ConfigChoice {
                        value: "default".into(),
                        name: "Default".into(),
                    },
                    crate::acp::ConfigChoice {
                        value: "high".into(),
                        name: "High".into(),
                    },
                ],
            },
            ConfigOption {
                id: "model".into(),
                name: "Model".into(),
                current: Some("opus[1m]".into()),
                choices: vec![crate::acp::ConfigChoice {
                    value: "opus".into(),
                    name: "Opus".into(),
                }],
            },
        ];
        (chat, rx)
    }

    #[test]
    fn resume_replays_mode_and_effort_but_never_model() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.arm_prefs(
            Some("plan".into()),
            vec![
                ("effort".into(), "high".into()),
                // The archive recorded the picker alias; on resume the option's
                // current is the verbatim live id — a naive diff would re-push
                // it, which is exactly what we must NOT do.
                ("model".into(), "opus".into()),
            ],
        );
        chat.reapply_prefs();

        let mut sent = Vec::new();
        while let Ok(req) = rx.try_recv() {
            sent.push(req);
        }
        // Mode + effort replayed; model deliberately absent.
        assert!(sent
            .iter()
            .any(|r| matches!(r, AcpRequest::SetMode(m) if m == "plan")));
        assert!(sent.iter().any(|r| matches!(
            r,
            AcpRequest::SetConfigOption { config_id, value } if config_id == "effort" && value == "high"
        )));
        assert!(!sent.iter().any(
            |r| matches!(r, AcpRequest::SetConfigOption { config_id, .. } if config_id == "model")
        ));
        // Optimistic UI: the effort selector reflects the restored pick at once.
        assert_eq!(chat.config_options[0].current.as_deref(), Some("high"));
        assert_eq!(chat.current_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn resume_skips_values_already_current_or_no_longer_offered() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.arm_prefs(
            Some("default".into()), // already the current mode → no resend
            vec![
                ("effort".into(), "default".into()), // already current → skip
                ("phantom".into(), "x".into()),      // option gone → skip
                ("effort".into(), "bogus".into()),   // value not offered → skip
            ],
        );
        // Only the last write per id survives the arm list; use a fresh arm for
        // the "not offered" case to keep the assertion unambiguous.
        chat.pending_config = vec![("effort".into(), "bogus".into())];
        chat.reapply_prefs();
        assert!(rx.try_recv().is_err()); // nothing sent
    }

    #[test]
    fn fresh_session_fallback_drops_armed_prefs_unused() {
        let (mut chat, mut rx) = chat_with_tx();
        chat.arm_prefs(Some("plan".into()), vec![("effort".into(), "high".into())]);
        // A `session/new` fallback: resumed=false must not replay anything.
        chat.apply(AcpEvent::Connected {
            tx: chat.tx.clone().unwrap(),
            resumed: false,
        });
        assert!(rx.try_recv().is_err());
        assert!(chat.pending_mode.is_none());
        assert!(chat.pending_config.is_empty());
    }

    /// The three states have to be distinguishable, because the rail draws a
    /// different thing for each: nothing while connecting, nothing once live,
    /// and a danger dot once lost. Reading `tx.is_none()` instead would make
    /// "coming up" and "died" the same answer.
    #[test]
    fn link_separates_coming_up_from_gone() {
        let (mut chat, _rx) = chat_with_tx();
        // `chat_with_tx` hands the channel over directly, so the reducer has
        // not seen a handshake yet.
        assert_eq!(chat.link, Link::Connecting);
        assert!(chat.tx.is_some(), "a channel alone must not mean connected");

        chat.apply(AcpEvent::Connected {
            tx: chat.tx.clone().unwrap(),
            resumed: false,
        });
        assert_eq!(chat.link, Link::Connected);

        chat.apply(AcpEvent::Disconnected("adapter exited".into()));
        assert_eq!(chat.link, Link::Lost);
        assert!(chat.tx.is_none());
        assert!(!chat.busy, "a lost adapter cannot still be mid-turn");
    }

    /// Advertising modes or commands is bookkeeping, not conversation. A view
    /// that rebuilt its run layout for these would rebuild it several times
    /// during a handshake, before there is anything to lay out.
    #[test]
    fn session_metadata_does_not_touch_the_transcript() {
        let (mut chat, _rx) = chat_with_tx();
        let before = chat.revision();

        let outcome = chat.apply(AcpEvent::ModeChanged("plan".into()));

        assert!(!outcome.transcript_changed);
        assert_eq!(chat.revision(), before, "nothing rendered moved");
    }

    /// The case a count of items cannot see. Two chunks of one answer leave the
    /// transcript exactly one item long both times, while what that item *says*
    /// is different -- so anything cached against a length would go on showing
    /// the first chunk.
    #[test]
    fn streaming_into_one_block_still_counts_as_a_change() {
        let (mut chat, _rx) = chat_with_tx();
        chat.apply(AcpEvent::AgentChunk("half ".into()));
        let (after_first, len) = (chat.revision(), chat.items.len());

        let outcome = chat.apply(AcpEvent::AgentChunk("an answer".into()));

        assert!(outcome.transcript_changed);
        assert_eq!(chat.items.len(), len, "still one answer block");
        assert_ne!(chat.revision(), after_first, "and it is not the same block");
    }

    /// The reducer says the turn settled, rather than every caller matching on
    /// the event a second time to find out.
    #[test]
    fn the_turn_ending_is_reported_once() {
        let (mut chat, _rx) = chat_with_tx();
        assert!(
            !chat
                .apply(AcpEvent::AgentChunk("working".into()))
                .turn_ended
        );

        let outcome = chat.apply(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });

        assert!(outcome.turn_ended);
        assert!(outcome.transcript_changed, "the turn's card settles too");
        assert!(!chat.busy);
    }

    /// The reducer reports the *moment* a session starts waiting on the user,
    /// which is not a question the transcript can answer.
    ///
    /// `awaiting_permission` stays true for as long as the card is unanswered,
    /// so a caller reading it would say "waiting" again on every chunk that
    /// arrived afterwards -- and something that says a session is blocked, out
    /// loud and outside the window, must say it once per ask.
    #[test]
    fn an_ask_is_reported_once_and_says_which_kind_it_was() {
        let (mut chat, _rx) = chat_with_tx();
        assert!(chat
            .apply(AcpEvent::AgentChunk("working".into()))
            .asked_user
            .is_none());

        let parked = chat.apply(AcpEvent::Permission(permission("Run cargo test")));
        assert_eq!(parked.asked_user, Some(UserAsk::Permission));

        // The card is still up, so the transcript still reads as waiting --
        // and the next event must not announce it a second time.
        let after = chat.apply(AcpEvent::AgentChunk("still here".into()));
        assert!(chat.awaiting_permission());
        assert!(after.asked_user.is_none());

        let asked = chat.apply(AcpEvent::Elicitation(ask_item().req));
        assert_eq!(asked.asked_user, Some(UserAsk::Question));
    }

    /// The two asks are named apart. Both stop the turn dead and draw the same
    /// mark, so the sentence is the only thing that says which one is waiting --
    /// and telling someone an agent wants "approval" when it asked them a
    /// question sends them looking for a button that is not there.
    #[test]
    fn each_ask_names_itself_and_the_agent() {
        assert_eq!(
            UserAsk::Permission.headline("Claude Code"),
            "Claude Code is waiting for your approval"
        );
        assert_eq!(
            UserAsk::Question.headline("Claude Code"),
            "Claude Code has a question for you"
        );
    }
}
