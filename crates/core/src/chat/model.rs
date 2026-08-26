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
use crate::chat::persist;
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
    /// After a `Connected { resumed: true }`, the next real content event drops
    /// the loaded history (so a stale resume keeps history instead of blanking).
    pub replay_pending: bool,
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
    /// root/agent feed the persisted metadata).
    pub fn new(uid: u64, root: PathBuf, agent: String) -> Self {
        let mut chat = Self::default();
        chat.uid = uid;
        chat.root = root;
        chat.agent = agent;
        chat
    }

    /// The transcript's current revision. See the field.
    pub fn revision(&self) -> u64 {
        self.revision
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
    pub fn load_history(&mut self, items: Vec<ChatItem>, session_id: String) {
        // Loading over a live transcript (restart keeps the chat):
        // archive anything the live items still hold, then reset them — the
        // loaded history *is* this conversation, and a `session/load` replay
        // re-delivers it into `items`. Without the reset the view (history ⧺
        // items) shows everything twice and the next turn-end save doubles
        // the archive on disk.
        persist::save_chat(self);
        self.items.clear();
        self.terminals.clear();
        self.history = items;
        self.session_id = Some(session_id);
        self.replay_pending = true;
        self.touch();
    }

    /// Adopt an archived conversation: its transcript, its title, the selector
    /// state to replay, and when it was last touched.
    ///
    /// Four things have to happen together and none of them is optional, which
    /// is why they are one call rather than four at a call site. Forget
    /// `arm_prefs` and a reopened conversation silently loses its effort/agent
    /// (the adapter rebuilds those from static settings on `session/load`);
    /// forget `last_activity` and the rail dates the session from the moment it
    /// was reopened rather than from its last real turn.
    pub fn resume_from(&mut self, stored: &crate::chat::persist::StoredConversation) {
        self.load_history(
            crate::chat::persist::restore(stored),
            stored.session_id.clone(),
        );
        self.custom_title = stored.title.clone();
        self.arm_prefs(
            stored.prefs.mode.clone(),
            stored
                .prefs
                .config
                .iter()
                .map(|c| (c.id.clone(), c.value.clone()))
                .collect(),
        );
        self.last_activity = Some(stored.updated);
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

    /// Drop the loaded history the first time real replayed content arrives.
    fn consume_replay(&mut self) {
        if self.replay_pending {
            self.history.clear();
            self.replay_pending = false;
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
        let event_at = (!metadata).then(persist::now_secs);
        if let Some(event_at) = event_at {
            self.last_activity = Some(event_at);
        }
        // The same test answers both questions, and that is not a coincidence:
        // an event that is conversation activity is an event that rewrote what
        // is on screen.
        let turn_ended = matches!(event, AcpEvent::TurnEnded { .. });
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
                // History-drop was already armed in `load_history`; re-evaluate it
                // here so a `session/new` fallback (resumed=false) disarms and keeps
                // the old history as context, while a resume that hasn't dropped yet
                // (history still present) stays armed for the replay.
                self.replay_pending = resumed && !self.history.is_empty();
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
                self.finish_active_turn(event_at.unwrap_or_else(persist::now_secs));
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
            transcript_changed: !metadata,
            turn_ended,
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
        self.replay_pending = false;
        let sent_at = persist::now_secs();
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
impl Drop for Chat {
    fn drop(&mut self) {
        persist::save_chat(self);
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
        chat.load_history(vec![ChatItem::User(UserMsg::text("hi"))], "sid-1".into());
        assert!(chat.items.is_empty());
        assert_eq!(chat.history.len(), 1);
        assert!(chat.replay_pending);
        assert_eq!(chat.session_id.as_deref(), Some("sid-1"));
    }

    #[test]
    fn user_prompt_keeps_unreplayed_history() {
        // A resume whose `session/load` replays nothing: the first user prompt
        // must keep the loaded history (it is the only copy of the
        // conversation), while real replayed content still consumes it.
        let mut chat = Chat::default();
        chat.load_history(vec![ChatItem::User(UserMsg::text("old"))], "sid-1".into());
        chat.push_user("new prompt".into(), Vec::new());
        assert_eq!(chat.history.len(), 1, "history must survive a user prompt");
        assert!(!chat.replay_pending);

        let mut chat = Chat::default();
        chat.load_history(vec![ChatItem::User(UserMsg::text("old"))], "sid-1".into());
        chat.apply(AcpEvent::AgentChunk("replayed".into()));
        assert!(chat.history.is_empty(), "a real replay still drops history");
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

    /// Until the handshake lands there is no agent to be doing anything, so
    /// that is what the status says -- ahead of every other answer it could
    /// give. A resumed conversation puts its archive on screen the moment it is
    /// picked, which is several seconds before anything can be sent to it.
    #[test]
    fn connecting_outranks_everything_the_status_could_say() {
        let mut chat = Chat::new(1, PathBuf::from("/tmp/p"), "Claude Code".to_string());
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
        use crate::chat::persist::{StoredConfig, StoredConversation, StoredPrefs};

        let mut chat = Chat::default();
        let stored = StoredConversation {
            version: 1,
            session_id: "sess-9".into(),
            root: "/tmp/x".into(),
            agent: "Claude Code".into(),
            updated: 1_700_000_000,
            title: Some("Ship the parser".into()),
            items: Vec::new(),
            prefs: StoredPrefs {
                mode: Some("plan".into()),
                config: vec![StoredConfig {
                    id: "effort".into(),
                    value: "high".into(),
                }],
            },
        };

        chat.resume_from(&stored);

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
        assert!(chat.replay_pending, "a resume arms the replay window");
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
}
