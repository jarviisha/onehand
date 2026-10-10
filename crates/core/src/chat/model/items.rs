use crate::acp::{
    ElicitKind, ElicitValue, Elicitation, PermissionRequest, PlanEntry, ToolCall, ToolContent,
    ToolStatus,
};
use crate::attachment::AttachmentSnapshot;
use std::collections::{HashMap, HashSet};
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
}

/// A permission request rendered as a card with option buttons.
pub struct PermItem {
    pub req: PermissionRequest,
    /// The chosen option's name once answered (buttons then disable).
    pub resolved: Option<String>,
    /// Whether the command block is open past the lines it folds at.
    ///
    /// Held on the item and not on the card that draws it, for the reason
    /// every other fold in this conversation is: the card is rebuilt from
    /// scratch on every frame, and what changes while a permission is parked
    /// is the agent still streaming underneath it.
    pub expanded: bool,
}

/// Real lines of a command a permission card draws before it folds.
///
/// **Real lines, never wrapped ones.** A command is the text a grant is given
/// on the strength of, so the count is over the newlines the agent wrote and
/// nothing else -- a bound measured in drawn rows would fold a two-line
/// command on a narrow pane and leave a ten-line one whole on a wide one,
/// which is a fold the user cannot predict.
///
/// Eight is what leaves the header, the buttons and enough of a script to
/// recognise it on one screen together.
///
/// Named outside this crate by the block that draws the fold: the collapsed
/// box is this many rows tall, so a command of one very long line is held to
/// the same height as one of eight short ones rather than filling the card. The
/// rules below apply it to the agent's newlines; the height is the only thing
/// that has to know the number itself.
pub const COMMAND_FOLD_LINES: usize = 8;

impl PermItem {
    /// The exact command, whatever the fold is doing to what is drawn.
    ///
    /// **Never the visible part.** Copy is offered on a collapsed block on
    /// purpose -- reading a long command elsewhere is the reason somebody
    /// reaches for it -- so a copy that stopped where the fold does would hand
    /// back a script that runs to a different end than the one approved.
    ///
    /// It is also the one place that says *which field of the request is the
    /// command*. The protocol calls it a title, which is a word for a heading
    /// and not for a script somebody is about to approve; every rule below
    /// reads it through here rather than reaching past to the field, so the
    /// three of them cannot come to disagree about what they are measuring.
    pub fn command(&self) -> &str {
        &self.req.title
    }

    /// The command's own lines, in the agent's order and wording.
    pub fn command_lines(&self) -> Vec<&str> {
        self.command().lines().collect()
    }

    /// Whether there is more command than the block draws unopened.
    pub fn is_long(&self) -> bool {
        self.command().lines().count() > COMMAND_FOLD_LINES
    }

    /// The lines the block draws now, and how many are held back behind the
    /// fold. A short command is always whole and has nothing to open.
    pub fn shown_lines(&self) -> (Vec<&str>, usize) {
        let lines = self.command_lines();
        if self.expanded || lines.len() <= COMMAND_FOLD_LINES {
            return (lines, 0);
        }
        let hidden = lines.len() - COMMAND_FOLD_LINES;
        (lines[..COMMAND_FOLD_LINES].to_vec(), hidden)
    }
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
    /// Whether the settled record of this exchange is showing its
    /// question-and-answer pairs.
    ///
    /// **Only ever read once the form is answered.** While it is open the card
    /// is on screen whole and there is nothing to unfold; what this folds is
    /// the record left behind afterwards, which is one line by default and the
    /// list of what was asked and what was chosen when opened. Held here for
    /// the reason every other fold in this conversation is: the row is rebuilt
    /// from scratch on every frame, so a flag owned by the view lasts exactly
    /// as long as nothing else on screen changes.
    pub expanded: bool,
    /// Which question the card is showing. A multi-question form renders as a
    /// tab strip (one tab per field) with only the active field's choices
    /// below it — stacking them all made a form taller than the pane, and the
    /// overflow was simply lost off the top of the sticky bar.
    pub tab: usize,
    /// Which row of the showing question the keyboard is on.
    ///
    /// Held here rather than in the card that draws it, for the reason every
    /// other cursor in this app is: the card is rebuilt from scratch on every
    /// frame, so a highlight owned by the view survives exactly as long as
    /// nothing else on screen changes — and what changes while a form is open
    /// is the agent still streaming underneath it.
    pub cursor: usize,
}

/// One row of the question showing on the card: the agent's choices, then the
/// free-text box where the form offers one.
///
/// **One list, because the keyboard walks them as one.** The typed answer is
/// the last option and not a control beside the options — it is what the user
/// picks when none of the agent's wording fits — so it takes the next number
/// after the last choice and ↑/↓ reach it without leaving the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRow {
    Choice(usize),
    Custom,
}

impl AskItem {
    pub fn new(req: Elicitation) -> Self {
        let n = req.fields.len();
        Self {
            req,
            picked: vec![Vec::new(); n],
            custom: vec![String::new(); n],
            resolved: None,
            expanded: false,
            tab: 0,
            cursor: 0,
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

    /// Whether `field` is the last question of the form — which is what turns
    /// the card's forward button from *Next* into *Submit*, and what makes
    /// skipping it the end of the form rather than a step through it.
    pub fn is_last(&self, field: usize) -> bool {
        field + 1 >= self.req.fields.len()
    }

    /// Show `field`, from its first row.
    ///
    /// The cursor goes back to the top rather than being carried over: it is a
    /// place in *this* question's list, and a question with two choices
    /// followed by one with five would otherwise open on whichever row the last
    /// one was left on.
    pub fn go_to(&mut self, field: usize) {
        self.tab = field;
        self.cursor = 0;
    }

    /// How many rows `field` offers the keyboard: the agent's choices, plus the
    /// free-text box where the form has one.
    pub fn row_count(&self, field: usize) -> usize {
        let choices = self
            .req
            .fields
            .get(field)
            .map_or(0, |f| f.kind.choices().len());
        choices + usize::from(self.has_custom(field))
    }

    /// The `n`th row of `field`, or `None` past the end — which is how a number
    /// key nobody offered is refused rather than rounded to the nearest row.
    pub fn row(&self, field: usize, n: usize) -> Option<AskRow> {
        let choices = self.req.fields.get(field)?.kind.choices().len();
        if n < choices {
            Some(AskRow::Choice(n))
        } else if n < self.row_count(field) {
            Some(AskRow::Custom)
        } else {
            None
        }
    }

    /// The row the keyboard is on, clamped — a cursor left past the end of a
    /// shorter question lands on its last row rather than on nothing.
    pub fn cursor_row(&self, field: usize) -> Option<AskRow> {
        self.row(
            field,
            self.cursor.min(self.row_count(field).saturating_sub(1)),
        )
    }

    /// Walk the cursor, wrapping at both ends: a list this short is one the eye
    /// holds whole, so stopping at the bottom only costs presses.
    pub fn move_cursor(&mut self, field: usize, delta: isize) {
        let rows = self.row_count(field);
        if rows == 0 {
            return;
        }
        let at = self.cursor.min(rows - 1) as isize;
        self.cursor = (at + delta).rem_euclid(rows as isize) as usize;
    }

    /// Pass on `field`: whatever was picked or typed there is dropped and the
    /// card moves to the next question. Answers `true` when there is no next
    /// one, which is the caller's cue to settle the whole form.
    ///
    /// **Dropped and not merely stepped over.** Skipping is the user saying
    /// this question gets no answer, and a half-typed line left behind would be
    /// sent as one — the response is built from what each field holds, not from
    /// which tab was last on screen.
    pub fn skip_field(&mut self, field: usize) -> bool {
        if let Some(picked) = self.picked.get_mut(field) {
            picked.clear();
        }
        if let Some(custom) = self.custom.get_mut(field) {
            custom.clear();
        }
        if self.is_last(field) {
            return true;
        }
        self.go_to(field + 1);
        false
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
    pub(crate) fn answers(&self) -> Vec<(String, ElicitValue)> {
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

    /// The audit-trail line shown once answered — the chosen *labels* (not the
    /// wire values), comma-joined.
    pub(crate) fn summary(&self) -> String {
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

/// One block of streamed markdown: its raw source, and an id to cache the
/// parse against.
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
}

impl Md {
    pub fn parse(s: &str) -> Self {
        Self {
            id: MdId::next(),
            source: s.to_string(),
        }
    }
    pub(crate) fn push(&mut self, s: &str) {
        self.source.push_str(s);
    }
}

/// The agent's reasoning stream, rendered collapsed as "Thought for Xs".
pub struct Thought {
    pub md: Md,
    /// When the reasoning began — runtime only (`None` once restored from disk).
    pub started: Option<Instant>,
    /// Final duration once the thought is done (persisted); `None` while live.
    pub elapsed_secs: Option<u64>,
    /// What the user chose by opening or closing it; `None` until they do.
    pub fold: Option<bool>,
}

impl Thought {
    pub(super) fn live(s: &str) -> Self {
        Self {
            md: Md::parse(s),
            started: Some(Instant::now()),
            elapsed_secs: None,
            fold: None,
        }
    }

    /// Still being written. A restored thought carries no start, so one saved
    /// without a duration does not read as live for the rest of its life.
    pub fn is_running(&self) -> bool {
        self.elapsed_secs.is_none() && self.started.is_some()
    }

    /// Open while it runs and closed once done, unless the user said otherwise.
    pub fn is_open(&self) -> bool {
        self.fold.unwrap_or_else(|| self.is_running())
    }
}

/// Where one agent block sits in its turn: whether it is the last (which
/// carries the footer, so there is one Copy per turn rather than one per
/// fragment), and whether the turn is still streaming (Copy stays hidden until
/// it settles).
///
/// **The prose itself is deliberately not here.** Copy wants the paragraph the
/// turn closes on, and finding it walks the whole answer — paid on every
/// redraw, for a string that is only read if a button is clicked.
/// [`Chat::turn_closing`](super::Chat::turn_closing) is that walk, asked for at the click.
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
    /// What the user chose by opening or closing this card; `None` until
    /// they do, or until a failure opens it for them.
    pub(crate) fold: Option<bool>,
    /// Content sections whose OUT well is un-folded past the threshold,
    /// keyed by the section's index in `call.content`.
    pub out_open: HashSet<usize>,
    /// When this step began, for the duration below.
    ///
    /// **Not persisted, and it must not be.** An `Instant` is a reading of a
    /// clock this process started; carried into a file and back it would be a
    /// point in another process's timeline, which is not a time at all. What
    /// survives a reopen is the duration it produced.
    pub(crate) started: Option<Instant>,
    /// How long the step took, stamped once when it settled.
    ///
    /// **Measured here and nowhere else.** A duration is the difference between
    /// two moments, and the only place that sees both is the reducer the events
    /// arrive at: a renderer asking "how long has this been going" is asking
    /// per frame and getting a different answer each time, which is a number
    /// that never settles even after the step has.
    pub elapsed_secs: Option<u64>,
    /// What the command exited with, where it ran through a real terminal.
    ///
    /// `None` for every step that did not: the protocol carries no exit status
    /// outside its terminal extension, so an adapter reporting a failure as a
    /// plain `tool_call` has no code to report and the row says `failed`
    /// instead. Guessing one from the output would be inventing a fact the
    /// reader would then act on.
    pub exit_code: Option<i32>,
}

impl ToolItem {
    pub fn new(call: ToolCall) -> Self {
        let diff_summary = Self::summarize_diffs(&call);
        let diff_rows = Self::hunks(&call);
        let running = matches!(call.status, ToolStatus::Pending | ToolStatus::InProgress);
        Self {
            // Only work that has not finished gets a start: a step that arrives
            // already settled was timed by whoever ran it, and stamping it here
            // would measure the moment it reached this process.
            started: running.then(Instant::now),
            elapsed_secs: None,
            exit_code: None,
            call,
            diff_summary,
            diff_rows,
            fold: None,
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
    pub(super) fn refresh_diffs(&mut self) {
        self.diff_summary = Self::summarize_diffs(&self.call);
        self.diff_rows = Self::hunks(&self.call);
    }
    /// Live work starts open and settled work closed; the user's choice,
    /// once made, wins either way, while it runs too.
    pub fn is_open(&self) -> bool {
        self.fold
            .unwrap_or(matches!(self.call.status, ToolStatus::InProgress))
    }
}

/// The agent's plan/checklist (Claude Code's TodoWrite) plus its fold state —
/// one card per turn, replaced in full on every `plan` update.
pub struct PlanItem {
    pub entries: Vec<PlanEntry>,
    /// The user closed this card.
    pub(crate) fold: bool,
}

impl PlanItem {
    pub fn new(entries: Vec<PlanEntry>) -> Self {
        Self {
            entries,
            fold: false,
        }
    }
    /// Open until the user closes it: a plan is what the turn is working
    /// through, so it stays in sight whether or not a step is running.
    pub fn is_open(&self) -> bool {
        !self.fold
    }
}

/// A user prompt: its text plus any files attached with 📎 (rendered as
/// thumbnails / placeholder chips in the bubble).
pub struct UserMsg {
    pub text: String,
    pub attachments: Vec<AttachmentSnapshot>,
    /// Epoch seconds captured when the prompt was submitted. Replayed/legacy
    /// protocol messages may not carry one.
    pub(crate) sent_at: Option<u64>,
    /// Epoch seconds captured when the corresponding response settles. Kept
    /// on the user item because that item is the stable boundary of a turn.
    pub(crate) completed_at: Option<u64>,
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
    pub(crate) fn as_str(self) -> &'static str {
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
    pub(crate) fn error(text: impl Into<String>) -> Self {
        Self::Notice {
            text: text.into(),
            level: NoticeLevel::Error,
        }
    }
}
