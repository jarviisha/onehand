use super::items::{AskItem, ChatItem, PermItem, TranscriptItemId, TurnAnswer, UserMsg};
use super::{Chat, Link};
use crate::acp::{AcpRequest, Attachment, ElicitOutcome, ToolStatus};
use crate::attachment::{AttachmentDelivery, AttachmentSnapshot, StagedAttachment};
use crate::chat::store;

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

impl Chat {
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
    pub(crate) fn resolve_permission(&mut self, idx: usize, option_name: String) {
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

    /// The same question, read-only — what a key press asks before it acts, so
    /// that where the cursor is and what row a number names are answered by the
    /// model rather than recomputed at the keyboard.
    pub fn ask_at(&self, idx: usize) -> Option<&AskItem> {
        match self.items.get(idx) {
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
    /// Close off every step the turn left in flight.
    ///
    /// **A turn ending is the last word on its own steps.** Nothing more will
    /// arrive for a call the adapter never settled -- a cancelled turn is the
    /// ordinary way that happens, and an adapter that simply moved on is the
    /// other -- so a step left `InProgress` stays that way for the rest of the
    /// conversation. Everything downstream reads it as live: the cluster it is
    /// in says it is still running and never reports how long it took, and the
    /// line at the foot of the transcript counts it among the steps in flight
    /// for every later turn.
    ///
    /// **`Failed` and not `Completed`.** What is known is that it did not
    /// report finishing; calling that success is the one reading the transcript
    /// cannot recover from, since a card claiming a write went through is worse
    /// than one saying it is unclear. This is the same answer
    /// `cancel_pending_permissions` gives a dangling card one line above.
    pub(super) fn settle_running_steps(&mut self) {
        for item in &mut self.items {
            if let ChatItem::Tool(tool) = item {
                if !matches!(
                    tool.call.status,
                    ToolStatus::Pending | ToolStatus::InProgress
                ) {
                    continue;
                }
                tool.call.status = ToolStatus::Failed;
                // Timed here for the same reason a settling update is: this is
                // the moment the step stopped, and a step that never settles
                // has no duration at all.
                if tool.elapsed_secs.is_none() {
                    tool.elapsed_secs =
                        Some(tool.started.map(|s| s.elapsed().as_secs()).unwrap_or(0));
                }
                // The same seed a failure arriving as an update leaves, so a
                // reader can see what the step had said before it stopped.
                tool.fold = true;
            }
        }
    }

    pub(crate) fn cancel_pending_permissions(&mut self) {
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

    /// Open or close a permission's command block past the lines it folds at.
    pub fn toggle_permission(&mut self, target: TranscriptItemId) {
        if let Some(ChatItem::Permission(p)) = self.list_mut(target).get_mut(target.index()) {
            p.expanded = !p.expanded;
        }
    }

    /// Open or close the settled record of one question.
    pub fn toggle_ask(&mut self, target: TranscriptItemId) {
        if let Some(ChatItem::Ask(a)) = self.list_mut(target).get_mut(target.index()) {
            a.expanded = !a.expanded;
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
    pub(super) fn flush_queued(&mut self) {
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
        self.prompts_sent += 1;

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

    pub(crate) fn push_user(&mut self, text: String, attachments: Vec<AttachmentSnapshot>) {
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
    pub(super) fn finish_active_turn(&mut self, completed_at: u64) {
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

    /// The paths this session has already touched, newest first.
    ///
    /// **What the `@` list offers above the project's own files**, and the
    /// reason it is worth a group of its own: the file somebody wants to talk
    /// about next is nearly always the file that was just written, and in a
    /// repository of several thousand it is otherwise indistinguishable from
    /// every other row — same shape, same sort, found only by remembering its
    /// name well enough to type it. Here it is at the top of a list of four.
    ///
    /// Two sources, because they are the two ways a path enters a conversation:
    /// a diff the agent produced, and a file the user attached to a prompt.
    /// Both are already in the transcript, so this reads what is there rather
    /// than keeping a second list beside it that could come to disagree.
    ///
    /// **Newest first and deduplicated to the newest mention**, which is the
    /// order the question is asked in — "the one from just now" — and the
    /// reason the walk runs backwards. A file edited five times is one row.
    pub fn artifacts(&self, max: usize) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for item in self.items.iter().rev() {
            let paths: Vec<&str> = match item {
                ChatItem::Tool(tool) => tool
                    .call
                    .content
                    .iter()
                    .filter_map(|section| match section {
                        crate::acp::ToolContent::Diff { path, .. } => Some(path.as_str()),
                        _ => None,
                    })
                    .collect(),
                ChatItem::User(msg) => msg
                    .attachments
                    .iter()
                    .filter_map(|a| a.path.to_str())
                    .collect(),
                _ => continue,
            };
            for path in paths {
                if out.len() >= max {
                    return out;
                }
                if seen.insert(path) {
                    out.push(path.to_string());
                }
            }
        }
        out
    }

    /// The end of what the agent said **in the turn that just ended**, for
    /// anything that has to say what a turn came to somewhere the transcript is
    /// not.
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
    /// `None` when *this* turn produced no prose at all: a turn can end on a
    /// tool call or be cancelled before the agent says anything, and inventing a
    /// sentence for that would be worse than the headline alone.
    ///
    /// **Bounded to the turn, which is the whole of the promise above.** Reading
    /// back through the entire transcript would find the previous turn's closing
    /// paragraph and announce it as this one's result — a wrong answer wearing
    /// the shape of a right one, and worse than saying nothing, because the
    /// reader has no way to tell. A turn begins at the last thing the user said,
    /// which is the same boundary [`Self::turn_answer`] works from.
    pub fn answer_tail(&self, max: usize) -> Option<String> {
        // The live transcript only. `history` is a resumed conversation's
        // archive, so reaching into it would let a session that has just come
        // back announce, as the result of its first turn, the end of a turn from
        // last week.
        let turn = self
            .items
            .iter()
            .rposition(|item| matches!(item, ChatItem::User(_)))
            .map_or(0, |index| index + 1);
        let text = self.items[turn..]
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
}
