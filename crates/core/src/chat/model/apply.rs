use super::items::{
    AskItem, ChatItem, Md, NoticeLevel, PermItem, PlanItem, Thought, ToolItem, UserMsg,
    MAX_TERM_BYTES,
};
use super::{Chat, Link, Replay};
use crate::acp::{AcpEvent, PlanEntry, ToolCall, ToolCallUpdate, ToolContent, ToolStatus};
use crate::chat::store;

/// What applying one event did, for the front end that has to react to it.
///
/// Deliberately two facts and not a description of the edit. Naming the exact
/// item that changed would let a view update only that one -- and would put the
/// burden of getting it exactly right on every branch of the reducer and every
/// helper it calls, where being subtly wrong shows up as a row that never
/// redraws. These two are cheap to be sure of.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// The agent's offer changed -- modes or config options -- now in
    /// [`Chat::modes`] / [`Chat::config_options`]: what a start reads before
    /// it brings the agent up again.
    pub offer_changed: bool,
    /// The agent refused a pick, put back to what it last confirmed: why,
    /// to be said once.
    pub pick_refused: Option<String>,
    /// A usage report closed a cycle the agent ran by itself, between turns:
    /// a safe moment to save what it wrote.
    pub cycle_ended: bool,
    /// The adapter would not stop a background item: why, to be said once.
    pub stop_refused: Option<String>,
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

impl Chat {
    /// How to log in, once, when the agent has just answered that it is not
    /// logged in: the entry's own words, else the agent's, else its name.
    fn take_login_hint(&mut self) -> Option<String> {
        let advice = self.login_pending.take()?;
        Some(match (&self.login_hint, advice) {
            (Some(hint), _) => hint.clone(),
            (None, Some(advice)) => format!("{} is not logged in. {advice}", self.agent),
            (None, None) => format!("{} needs you to log in", self.agent),
        })
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
        // Session-metadata events (handshake, mode/command advertisements,
        // usage and background reports) aren't conversation activity, and a
        // subagent's event is only when it lands in a card (see its arm);
        // anything else refreshes the rail's relative-time label — "now"
        // while a session works, aging once quiet.
        let metadata = matches!(
            &event,
            AcpEvent::SessionId(_)
                | AcpEvent::AvailableCommands(_)
                | AcpEvent::Modes { .. }
                | AcpEvent::ModeChanged(_)
                | AcpEvent::ConfigOptions(_)
                | AcpEvent::Connected { .. }
                | AcpEvent::Usage(_)
                | AcpEvent::Background(_)
                | AcpEvent::SubagentSpawned { .. }
                | AcpEvent::SubagentEnded { .. }
                | AcpEvent::InChild { .. }
                | AcpEvent::StopRefused { .. }
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
        let offer_changed = matches!(event, AcpEvent::Modes { .. } | AcpEvent::ConfigOptions(_));
        let mut pick_refused = None;
        let mut stop_refused = None;
        let cycle_ended = matches!(&event, AcpEvent::Usage(usage) if usage.cycle_end);
        // What the agent itself writes, as against reports about it or lines
        // onehand adds: between turns, only this says the agent is at work.
        let wrote = matches!(
            &event,
            AcpEvent::AgentChunk { .. }
                | AcpEvent::ThoughtChunk(_)
                | AcpEvent::ToolCall(_)
                | AcpEvent::ToolUpdate(_)
                | AcpEvent::Plan(_)
        );
        // Asked only of the events that can end such work: it scans the
        // transcript for working subagents.
        let aside_was = matches!(
            &event,
            AcpEvent::Background(_) | AcpEvent::SubagentEnded { .. } | AcpEvent::InChild { .. }
        ) && self.works_aside();
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
            | AcpEvent::Connected { .. }
            | AcpEvent::Usage(_)
            | AcpEvent::Background(_)
            | AcpEvent::SubagentSpawned { .. }
            | AcpEvent::SubagentEnded { .. }
            | AcpEvent::InChild { .. }
            | AcpEvent::StopRefused { .. } => {}
            _ => self.user_chunk_open = false,
        }
        match event {
            AcpEvent::Connected {
                tx,
                resumed,
                forkable,
            } => {
                self.tx = Some(tx);
                self.forkable = forkable;
                self.forking = false;
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
            AcpEvent::AgentChunk { text, message } => {
                self.consume_replay();
                self.finalize_thought();
                self.push_agent(&text, message);
            }
            AcpEvent::ThoughtChunk(s) => {
                self.consume_replay();
                self.push_thought(&s);
            }
            AcpEvent::Interrupted => {
                self.consume_replay();
                self.finalize_thought();
                self.user_chunk_open = false;
                self.items.push(ChatItem::notice("Interrupted"));
            }
            AcpEvent::UserChunk(s) => {
                self.consume_replay();
                self.finalize_thought();
                self.push_user_chunk(&s);
            }
            AcpEvent::ToolCall(tc) => {
                self.consume_replay();
                self.finalize_thought();
                tool_call(&mut self.items, tc);
            }
            AcpEvent::ToolUpdate(tu) => {
                self.consume_replay();
                tool_update(&mut self.items, tu);
            }
            AcpEvent::Plan(entries) => {
                self.consume_replay();
                self.finalize_thought();
                plan(&mut self.items, entries);
            }
            AcpEvent::Usage(usage) => self.apply_usage(usage),
            AcpEvent::Background(report) => self.apply_background(report),
            // Changes the transcript only when it lands in a card, so the
            // stamp and the revision wait for that answer.
            event @ (AcpEvent::SubagentSpawned { .. }
            | AcpEvent::SubagentEnded { .. }
            | AcpEvent::InChild { .. }) => {
                if self.apply_subagent(event) {
                    self.last_activity = Some(store::now_secs());
                    self.touch();
                }
            }
            AcpEvent::StopRefused { why } => stop_refused = Some(why),
            AcpEvent::Permission(mut req) => {
                self.consume_replay();
                self.finalize_thought();
                // One adapter puts the command in the request's own title,
                // another a generic label such as "Run command"; the call the
                // request names carries the command either way.
                // A subagent's request names a call in its own card.
                let card = req.child.as_deref().and_then(|id| self.subagent(id));
                let asker = card.map(|card| card.name.clone());
                let steps = card.map_or(&self.items, |card| &card.items);
                let named = steps.iter().rev().find_map(|item| match item {
                    ChatItem::Tool(t) if Some(&t.call.id) == req.tool_call_id.as_ref() => {
                        Some(t.call.title.trim())
                    }
                    _ => None,
                });
                if let Some(title) = named.filter(|title| !title.is_empty()) {
                    req.title = title.to_string();
                }
                self.items.push(ChatItem::Permission(PermItem {
                    req,
                    resolved: None,
                    asker,
                    expanded: false,
                }))
            }
            AcpEvent::Elicitation(req) => {
                self.consume_replay();
                self.finalize_thought();
                let asker = req.child.as_deref().and_then(|id| self.subagent(id));
                let asker = asker.map(|card| card.name.clone());
                self.items.push(ChatItem::Ask(AskItem {
                    asker,
                    ..AskItem::new(req)
                }))
            }
            AcpEvent::AvailableCommands(c) => self.commands = c,
            AcpEvent::Modes { current, available } => {
                self.confirm_mode(current.clone());
                self.current_mode = current;
                self.modes = available;
            }
            AcpEvent::ModeChanged(id) => {
                self.confirm_mode(Some(id.clone()));
                self.current_mode = Some(id);
            }
            AcpEvent::ConfigOptions(opts) => {
                for opt in &opts {
                    if let Some(current) = &opt.current {
                        self.confirmed.insert(opt.id.clone(), current.clone());
                    }
                }
                self.config_options = opts;
            }
            // What is shown follows the newest pick: an older pick's answer
            // only moves what the agent is known to hold.
            AcpEvent::PickConfirmed { group, value } => {
                self.pick_answered(&group);
                self.confirmed.insert(group, value);
            }
            AcpEvent::PickRefused { group, why } => {
                if self.pick_answered(&group) {
                    self.put_back(&group);
                    pick_refused = Some(why);
                }
            }
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
            AcpEvent::TurnEnded { stop_reason } => {
                // Or'd with the cancel asked for: an adapter that answers a
                // cancel with an error ends the turn as `end_turn`.
                self.cancelled |= stop_reason == "cancelled";
                // Ended by the agent without the work: a failed turn, said so,
                // or whatever drives the session reads it as a normal end.
                if let Some(text) = cut_short(&stop_reason) {
                    self.items.push(ChatItem::failure(
                        text.to_string(),
                        NoticeLevel::Failed,
                        None,
                    ));
                }
                self.finish_active_turn(event_at.unwrap_or_else(store::now_secs));
                self.busy = false;
                self.finalize_thought();
                // A turn that ends with unanswered permission cards (a
                // cancelled turn, or an adapter that moved on) resolves them
                // as cancelled — protocol-correct, and the buttons disable.
                // A background subagent's request outlives the turn.
                self.cancel_pending_permissions(true);
                self.settle_running_steps();
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
            // Said by the failure it comes ahead of, so that failure keeps what
            // it is: a turn that failed can be sent again once logged in, and a
            // start that failed leaves the link lost.
            AcpEvent::LoginRequired { advice } => self.login_pending = Some(advice),
            AcpEvent::Error { text, detail } => {
                // An error mid-turn is the prompt's answer: the turn ends on it.
                // Not after a Stop, though: some adapters answer a cancel with
                // an error, and a turn the person stopped did not fail.
                let level = match self.busy && !self.cancelled {
                    true => NoticeLevel::Failed,
                    false => NoticeLevel::Error,
                };
                let (text, detail) = match self.take_login_hint() {
                    Some(hint) => (hint, None),
                    None => (text, detail),
                };
                self.items.push(ChatItem::failure(text, level, detail));
            }
            AcpEvent::Disconnected(e) => {
                if std::mem::take(&mut self.forking) {
                    self.fork_failed = Some(e.clone());
                }
                // A thought cut off with the adapter is over, not still running.
                self.finalize_thought();
                self.tx = None;
                // Picks sent over the lost connection are answered by nothing.
                self.picks_out.clear();
                self.link = Link::Lost;
                self.busy = false;
                // No live adapter to answer — but the cards must not keep
                // offering buttons whose rpc ids died with the connection.
                self.cancel_pending_permissions(false);
                self.settle_lost_link();
                self.forget_activity();
                // No key in the words: the front end names the one its keymap
                // binds, which the person may have changed.
                let text = self
                    .take_login_hint()
                    .unwrap_or_else(|| format!("Disconnected: {e}"));
                self.items
                    .push(ChatItem::failure(text, NoticeLevel::Error, None));
            }
        }

        self.follow_own_cycle(wrote, aside_was, turn_ended || cycle_ended);

        ApplyOutcome {
            transcript_changed: self.revision != before,
            turn_ended,
            asked_user,
            offer_changed,
            pick_refused,
            cycle_ended,
            stop_refused,
        }
    }

    /// The mode the agent says is in force, as confirmed.
    fn confirm_mode(&mut self, mode: Option<String>) {
        match mode {
            Some(mode) => self
                .confirmed
                .insert(super::MODE_SELECTOR.to_string(), mode),
            None => self.confirmed.remove(super::MODE_SELECTOR),
        };
    }

    /// Show what the agent last confirmed for `group` again, after it
    /// refused a pick there.
    fn put_back(&mut self, group: &str) {
        let confirmed = self.confirmed.get(group).cloned();
        if group == super::MODE_SELECTOR {
            self.current_mode = confirmed;
        } else if let Some(value) = confirmed {
            self.set_config_current(group, &value);
        }
    }

    /// Flatten every exited terminal into the tool card that references it
    /// (replacing the live `Terminal(id)` with the captured output + an exit
    /// footer) and drop it from the live `terminals` map. Called at turn end so
    /// the map stays bounded to terminals that are actually still running.
    /// The adapter is gone: nothing more arrives for a call it left running,
    /// and every terminal it was streaming died with it.
    pub(super) fn settle_lost_link(&mut self) {
        for view in self.terminals.values_mut() {
            view.exited = true;
        }
        self.settle_running_steps();
        self.flatten_exited_terminals();
    }

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
            flatten_terminal(&mut self.items, &id, &flat, view.exit_code);
        }
    }

    fn push_agent(&mut self, s: &str, message: Option<String>) {
        // A block already on disk is never added to: the file is only
        // appended to, so text that lands after it was written (the agent
        // writing between turns) goes in a block of its own.
        let written = !self.rewrite
            && !matches!(self.replay, Replay::Partial(_))
            && self.persisted >= self.history.len() + self.items.len();
        if written || !matches!(self.items.last(), Some(ChatItem::Agent(_))) {
            self.items.push(ChatItem::Agent(Md::parse("")));
        }
        let Some(ChatItem::Agent(md)) = self.items.last_mut() else {
            unreachable!("the transcript ends on an answer block");
        };
        md.push(s);
        if message.is_some() {
            md.message = message;
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
        seal_thought(&mut self.items);
    }

    fn push_user_chunk(&mut self, s: &str) {
        match self.items.last_mut() {
            Some(ChatItem::User(u)) if self.user_chunk_open => u.text.push_str(s),
            _ => self.items.push(ChatItem::User(UserMsg::text(s))),
        }
        self.user_chunk_open = true;
    }
}

/// Stamp the trailing thought of `items` with its duration.
pub(super) fn seal_thought(items: &mut [ChatItem]) {
    if let Some(ChatItem::Thought(th)) = items.last_mut() {
        if th.elapsed_secs.is_none() {
            let secs = th.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
            th.elapsed_secs = Some(secs);
        }
    }
}

/// Draw a tool call, or update its card when the id is already on screen.
pub(super) fn tool_call(items: &mut Vec<ChatItem>, tc: ToolCall) {
    // A call announced again under an id already on screen is the same step,
    // said over: it updates its card rather than drawing a second one.
    let known = items.iter_mut().rev().find_map(|item| match item {
        ChatItem::Tool(t) if t.call.id == tc.id => Some(t),
        _ => None,
    });
    if let Some(card) = known {
        card.call.kind = tc.kind;
        tool_update(
            items,
            ToolCallUpdate {
                id: tc.id,
                status: Some(tc.status),
                title: Some(tc.title),
                description: tc.description,
                content: Some(tc.content),
            },
        );
    } else {
        let mut item = ToolItem::new(tc);
        // The same rule for a step that arrives already failed, which is how
        // an adapter reports one it never started.
        item.fold = (item.call.status == ToolStatus::Failed).then_some(true);
        items.push(ChatItem::Tool(item));
    }
}

/// Merge a `tool_call_update` into the matching tool card (by id). An
/// update with no matching card (it raced ahead of its `tool_call`, or
/// the adapter sends update-only calls) materializes one instead of being
/// dropped — a silently-vanished `Failed` would read as "the turn did
/// nothing". Only when it carries a title or content a person can read:
/// Claude's adapter drops the spawning `Agent` call from the parent yet
/// still sends bare updates for its id, which would otherwise draw an
/// empty "Used tool" row that later turns failed.
pub(super) fn tool_update(items: &mut Vec<ChatItem>, tu: ToolCallUpdate) {
    for item in items.iter_mut().rev() {
        if let ChatItem::Tool(t) = item {
            if t.call.id == tu.id {
                if let Some(status) = tu.status {
                    // Settling is the moment the duration exists, and it is
                    // stamped once: a later update to the same card -- of
                    // which there are several, since content arrives after
                    // the status does -- must not restart the clock.
                    if !matches!(status, ToolStatus::Pending | ToolStatus::InProgress) {
                        t.settle_clock();
                    }
                    // **A failure opens itself, and can still be shut.**
                    // The one thing a reader needs from a settled step is
                    // whether it worked, and for the one that did not the
                    // next question is always the same -- what did it say?
                    // -- so making them ask is a click charged for the case
                    // that already went badly.
                    //
                    // Seeded into the fold the user owns rather than
                    // OR-ed into `is_open`, which is where this was first
                    // written and is a trap: a terminal status that forces
                    // the row open forces it open *for ever*, so the
                    // control that shuts it does nothing and the one state
                    // that most wants a way out is the one with none.
                    // Running gets away with the OR because it stops being
                    // true on its own.
                    if status == ToolStatus::Failed && t.call.status != ToolStatus::Failed {
                        t.fold.get_or_insert(true);
                    }
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
    let readable = tu.title.as_deref().is_some_and(|t| !t.is_empty())
        || tu.content.as_ref().is_some_and(|c| !c.is_empty());
    if !readable {
        return;
    }
    items.push(ChatItem::Tool(ToolItem::new(ToolCall {
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
pub(super) fn plan(items: &mut Vec<ChatItem>, entries: Vec<PlanEntry>) {
    let turn_start = items
        .iter()
        .rposition(|i| matches!(i, ChatItem::User(_)))
        .map(|p| p + 1)
        .unwrap_or(0);
    for item in items[turn_start..].iter_mut() {
        if let ChatItem::Plan(p) = item {
            p.entries = entries;
            return;
        }
    }
    items.push(ChatItem::Plan(PlanItem::new(entries)));
}

/// Fail every step still running in `items`: nothing will settle it now.
pub(super) fn settle_steps(items: &mut [ChatItem]) {
    for item in items {
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
            tool.settle_clock();
            // The same seed a failure arriving as an update leaves, so a
            // reader can see what the step had said before it stopped.
            tool.fold.get_or_insert(true);
        }
    }
}

/// Put a finished terminal's text in place of every live reference to it in
/// `items`, steps inside subagent cards included.
fn flatten_terminal(items: &mut [ChatItem], id: &str, flat: &str, exit_code: Option<i32>) {
    for item in items {
        match item {
            ChatItem::Tool(t) => {
                let mut ours = false;
                for c in t.call.content.iter_mut() {
                    if matches!(c, ToolContent::Terminal(tid) if tid == id) {
                        *c = ToolContent::Text(flat.to_string());
                        ours = true;
                    }
                }
                // **Kept off the text on the way past.** This is the one
                // moment the exit status and the step it belongs to are both
                // in hand: after it the terminal is gone and the code is a
                // line inside a string, which the row would have to parse
                // back out -- and a number recovered by parsing is a number
                // that is wrong the first time the wording changes.
                if ours {
                    t.exit_code = exit_code;
                }
            }
            ChatItem::Subagent(card) => flatten_terminal(&mut card.items, id, flat, exit_code),
            ChatItem::User(_)
            | ChatItem::Agent(_)
            | ChatItem::Thought(_)
            | ChatItem::Plan(_)
            | ChatItem::Permission(_)
            | ChatItem::Ask(_)
            | ChatItem::Notice { .. } => {}
        }
    }
}

/// What a turn the agent ended for `stop_reason` failed on, when it ended
/// without finishing the work.
fn cut_short(stop_reason: &str) -> Option<&'static str> {
    match stop_reason {
        "refusal" => Some("The agent refused to go on with this turn."),
        "max_tokens" => Some("The turn stopped: the agent reached its output limit."),
        "max_turn_requests" => {
            Some("The turn stopped: the agent reached its limit of requests for one turn.")
        }
        _ => None,
    }
}
