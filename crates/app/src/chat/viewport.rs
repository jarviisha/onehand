//! What the transcript looks like on screen: the run layout the virtual list
//! draws, the list's own scroll and measurement state, and the find bar.
//!
//! These three are one thing wearing three names. The list draws *runs*, not
//! items, so its item count is the run count; the find bar hits an *item*, and
//! scrolling to it means knowing which run holds it. Kept apart, the pairing
//! was implicit — a plan on the pane and a scroll position on the session, made
//! to line up by the order two calls happened in — and the find bar could not
//! scroll at all, because nothing on either side could turn a hit into a row.

use super::transcript;
use gpui::{App, Entity, FollowMode, ListAlignment, ListOffset, ListState, Pixels, Window, px};
use gpui_component::input::InputState;
use onehand_core::chat::{
    ActivityGroup, Chat, ChatItem, TranscriptItemId, TranscriptMatch, compute_matches,
};

#[derive(Clone)]
pub struct ActivityPlan {
    pub group: ActivityGroup,
    pub summary: String,
}

/// What cadence a run asks of the run above it.
///
/// Decided here rather than at draw time because the space between two runs is
/// a property of the **boundary**, not of either side: to give one number to a
/// boundary, the lower run has to be able to see what the upper one was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunKind {
    /// A user prompt — the head of a turn.
    Prompt,
    /// An index entry: a folded activity strip, or a settled step nobody has
    /// opened.
    Compact,
    /// Prose, a card, an opened strip — anything read rather than scanned.
    Block,
}

/// One drawable run: either a single transcript item, or a folded stretch of
/// quiet steps.
pub struct RunPlan {
    pub members: Vec<TranscriptItemId>,
    /// `Some` when this run draws as an activity strip.
    pub strip: Option<ActivityPlan>,
    pub open: bool,
    pub kind: RunKind,
}

impl RunPlan {
    /// What this run *begins* with, which is what decides the space above it.
    ///
    /// **A run has two ends and they are not always the same kind.** An opened
    /// activity group is a block's worth of reading, so the run after it takes
    /// a block's gap — but it still *starts* with the index row it started with
    /// when it was closed, and the boundary above that row has not changed.
    /// Read from the whole run instead, opening a group tripled the gap over
    /// its own header: the row moved down under the pointer that had just
    /// clicked it, and the list shifted for a reason nothing on screen
    /// explained.
    pub fn head_kind(&self) -> RunKind {
        match self.strip {
            Some(_) => RunKind::Compact,
            None => self.kind,
        }
    }

    /// What this run *ends* with, which is what decides the space below it.
    pub fn tail_kind(&self) -> RunKind {
        self.kind
    }
}

/// The two edges a held prompt is measured against: where the transcript's
/// first row lands, and how much of its foot the floating composer covers.
///
/// Given by the pane because both are the pane's numbers — the list's own top
/// padding, and the height the composer overlay measured last frame — and the
/// hold has to end at the exact point the answer stops fitting between them.
#[derive(Clone, Copy)]
pub struct TopRoom {
    pub head: Pixels,
    pub floor: Pixels,
}

/// A prompt being kept at the top of the panel while its answer arrives.
struct Hold {
    prompt: TranscriptItemId,
    /// The list has not been told about this position yet.
    ///
    /// A prompt is spotted while the layout is being rebuilt, one call before
    /// the list has been told how many runs there now are — and a list asked to
    /// scroll to a row it does not know about clamps the request to the last
    /// row it does.
    pending: bool,
    /// The reader has taken the scroll position over.
    ///
    /// **What ends here is the scroll, not the layout.** The room under the turn
    /// stays exactly the size it was, because taking it away under somebody who
    /// has just scrolled is the one thing that moves the conversation while it
    /// is being read — and moving it is what they scrolled to stop.
    reading: bool,
    /// The room the turn is asking for, or a whole panel's worth until the turn
    /// has been measured once.
    ///
    /// Kept rather than worked out afresh each frame: it can only be measured
    /// on a frame the list is not chasing its own tail, and a frame that cannot
    /// measure has to leave the transcript exactly where it is instead of
    /// guessing.
    room: Option<Pixels>,
}

/// One session's view of its transcript.
#[derive(Default)]
pub struct Viewport {
    /// The run layout the list is currently drawing.
    ///
    /// `gpui::list` renders items lazily, *after* `render` has returned, so the
    /// closure it calls cannot borrow anything from this frame -- the plan has
    /// to be owned state it can read back through the entity.
    plan: Vec<RunPlan>,
    /// What the plan was built from, so a frame that changed none of it can
    /// keep the plan it already has.
    planned: Option<PlanKey>,
    /// Scroll + measurement state, and how many runs it was last told about.
    ///
    /// Beside the plan rather than a field away, because every use of one is a
    /// use of the other: the count the list is spliced or reset to is the
    /// plan's length, and a plan belonging to one session with a scroll
    /// position belonging to another draws the right rows in the wrong place.
    list: Option<(ListState, usize)>,
    /// Whether the list has been told to report its scrolling yet.
    ///
    /// The state is built lazily, on the first frame that draws a transcript,
    /// so the handler cannot be installed where the viewport is created -- and
    /// installing it every frame would replace the closure on each render.
    scroll_hooked: bool,
    /// The prompt held at the top of the panel, while one is.
    hold: Option<Hold>,
    /// The newest prompt the layout has drawn, which is how the next one is
    /// spotted.
    ///
    /// The prompt's identity rather than a count: a turn adds items on both
    /// sides of it, so "how many prompts are there" answers the question a beat
    /// late on a transcript that also loses rows -- a blocking card leaves the
    /// list while it waits for an answer and comes back once it has one.
    newest_prompt: Option<TranscriptItemId>,
}

/// Everything the run layout is a function of.
///
/// The revision alone would do, and the two lengths are here anyway. They cost
/// two comparisons and they are the backstop: the revision is bumped by hand in
/// the model, and the failure mode of forgetting one is a row that never
/// appears. Anything that *adds* an item moves a length too, so the two of them
/// together fail safe.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PlanKey {
    revision: u64,
    history: usize,
    items: usize,
    folds: u64,
    /// Whether a turn is in flight. It is also a cheap invalidation boundary:
    /// integrations can settle their final tool status immediately before
    /// clearing `busy`, without adding another transcript item.
    busy: bool,
}

impl Viewport {
    /// Rebuild the run layout for `chat`, unless nothing it depends on has
    /// moved.
    ///
    /// Worth the bookkeeping because `render` runs for reasons that have
    /// nothing to do with the conversation -- a hover, a keystroke in the
    /// composer, a panel resize -- and each of those used to walk the whole
    /// transcript, group it into runs and build a summary string per run.
    ///
    /// The fold state arrives as a question rather than as the session that
    /// holds it: the projection needs one bit per activity run and nothing else
    /// about a session, and taking only what it needs is what lets the run
    /// layout be tested without opening a window.
    pub fn replan(&mut self, chat: &Chat, folds: u64, is_open: impl Fn(TranscriptItemId) -> bool) {
        let key = PlanKey {
            revision: chat.revision(),
            history: chat.history.len(),
            items: chat.items.len(),
            folds,
            busy: chat.busy,
        };
        if self.planned == Some(key) {
            return;
        }
        // Whether this session has ever been laid out. A transcript adopted
        // from an archive arrives whole, and its last question is not a
        // question just asked: opening a conversation belongs at its end.
        let first = self.planned.is_none();
        self.planned = Some(key);
        let addressed: Vec<(TranscriptItemId, &ChatItem)> = addressed(chat).collect();

        self.plan = transcript::runs(&addressed)
            .into_iter()
            .map(|run| match run {
                transcript::Run::Single(target) => RunPlan {
                    members: vec![target],
                    strip: None,
                    open: true,
                    kind: item(chat, target).map_or(RunKind::Block, single_kind),
                },
                transcript::Run::Activity { group, members } => {
                    let anchor = members[0];
                    let bodies: Vec<&ChatItem> = members
                        .iter()
                        .copied()
                        .filter_map(|t| item(chat, t))
                        .collect();
                    let open = is_open(anchor);
                    RunPlan {
                        strip: Some(ActivityPlan {
                            group,
                            summary: transcript::activity_summary(&bodies),
                        }),
                        open,
                        members,
                        // Folded it is one index row; opened it is every step it
                        // holds, which is a block's worth of reading.
                        kind: if open {
                            RunKind::Block
                        } else {
                            RunKind::Compact
                        },
                    }
                }
            })
            .collect();

        let newest = self
            .plan
            .iter()
            .rev()
            .find(|run| run.kind == RunKind::Prompt)
            .map(|run| run.members[0]);
        if newest != self.newest_prompt {
            self.newest_prompt = newest;
            if let Some(prompt) = newest.filter(|_| !first) {
                self.hold = Some(Hold {
                    prompt,
                    pending: true,
                    reading: false,
                    room: None,
                });
            }
        }
    }

    /// Have the list report its scrolling, once.
    ///
    /// **Scrolling is the one change to the transcript that comes from outside
    /// the entity.** A `gpui::list` owns its own scroll offset and changes it
    /// without telling anybody, so nothing derived from where the list is
    /// sitting can be drawn correctly without asking it to say so — the
    /// jump-to-the-latest pill is exactly that, and it only appeared once some
    /// *other* change happened to redraw the pane. A keystroke, an arriving
    /// event, anything at all; scrolling alone was the one thing that did not
    /// bring it back.
    pub fn hook_scroll(
        &mut self,
        handler: impl FnMut(&gpui::ListScrollEvent, &mut Window, &mut App) + 'static,
    ) {
        let Some((state, _)) = &self.list else {
            return;
        };
        if self.scroll_hooked {
            return;
        }
        state.set_scroll_handler(handler);
        self.scroll_hooked = true;
    }

    pub fn run(&self, ix: usize) -> Option<&RunPlan> {
        self.plan.get(ix)
    }

    /// The cadence of the run above `ix`, or `None` at the top of the
    /// transcript.
    pub fn kind_before(&self, ix: usize) -> Option<RunKind> {
        self.plan.get(ix.checked_sub(1)?).map(RunPlan::tail_kind)
    }

    /// Whether a prompt is being held at the top of the panel *and* the view is
    /// resting on it.
    ///
    /// The second half is what the jump-to-the-latest control asks: a reader
    /// sitting on the held question has not scrolled anywhere and needs no way
    /// back, but one who has scrolled off it does — and it takes them to the
    /// held question, which is where the latest activity is arriving.
    pub fn holding(&self) -> bool {
        self.hold.as_ref().is_some_and(|hold| !hold.reading)
    }

    /// The empty space the transcript keeps under its last run.
    ///
    /// Ordinarily the composer's floor, so the final row rests clear of a card
    /// floating over it. While a prompt is held at the top it is a whole
    /// panel's worth, and **that is what makes the hold possible at all**: a
    /// list aligned to its bottom pulls its content back down the moment that
    /// content stops filling the view, so a question with nothing under it yet
    /// can only sit at the top of the panel if something scrollable is standing
    /// under it. Padding draws nothing, so the room costs the reader nothing to
    /// look at, and it is gone the frame the answer is long enough to hold the
    /// position by itself.
    pub fn tail_room(&self, floor: Pixels) -> Pixels {
        let (Some(hold), Some((state, _))) = (&self.hold, &self.list) else {
            return floor;
        };
        // **A list chasing its tail must never be given this room.** Following
        // puts the bottom of the *padding* at the bottom of the panel, so a
        // panel's worth of it there is a panel's worth of nothing: the
        // conversation is pushed off the top edge and the transcript draws
        // empty, with no row on screen to say what happened. The room and the
        // tail are already exclusive by construction; this is the second lock
        // on it, because the failure has no symptom to debug from.
        if state.is_following_tail() {
            return floor;
        }
        hold.room
            .unwrap_or_else(|| state.viewport_bounds().size.height)
            .max(floor)
    }

    /// Let go of a held prompt, putting tail-following back.
    ///
    /// `keep` is a position to hand straight back afterwards. Asking for the
    /// tail is the only way to start following again and it *moves* the list to
    /// the tail, so a reader who is somewhere else has to be put back — which
    /// leaves the list following but paused, exactly what an ordinary scroll up
    /// leaves behind, so it picks up again on its own when they come back down.
    fn release(&mut self, state: &ListState, keep: Option<ListOffset>) {
        self.hold = None;
        state.set_follow_mode(FollowMode::Tail);
        if let Some(top) = keep {
            state.scroll_to(top);
        }
    }

    /// Work out how much room the held turn still wants, and let go once it
    /// wants none.
    ///
    /// **The room shrinks by exactly what the turn grows**, so the question
    /// stays where it was put and the answer drifts down into the space under
    /// it rather than the whole column sliding. When the turn finally reaches
    /// the composer the room has reached the ordinary floor, and at that one
    /// height *the question at the top* and *the last line above the composer*
    /// are the same picture — so handing the list back to its tail there cannot
    /// be seen, and from then on the transcript scrolls with the stream.
    ///
    /// **Read before anything is spliced.** The run being written into is
    /// re-spliced on every frame of a turn so its height cannot freeze at the
    /// first chunk's, and a spliced run is an unmeasured one -- so asked after,
    /// the growth this watches for is the one thing missing from the answer.
    fn settle_hold(&mut self, room: TopRoom) {
        let Some(prompt) = self
            .hold
            .as_ref()
            .filter(|hold| !hold.pending)
            .map(|hold| hold.prompt)
        else {
            return;
        };
        let Some((state, _)) = &self.list else {
            return;
        };
        let state = state.clone();
        let count = state.item_count();
        let Some(run) = self.run_of(prompt) else {
            // The question is no longer drawn at all, so there is nothing left
            // to hold and no position worth keeping either.
            self.release(&state, None);
            return;
        };

        let top = state.logical_scroll_top();
        let Some(hold) = self.hold.as_mut() else {
            return;
        };
        // **The held position is also the furthest this transcript can be
        // scrolled**, and both of the list's ways of saying so have to be read
        // as the same answer. The room under the turn is the panel minus the
        // turn, so the whole conversation ends exactly where the question meets
        // the top edge: there is nothing under it to scroll into. A list asked
        // to go past that comes to rest with no position of its own and reports
        // the bottom instead of the row it is resting on -- which, read as a
        // row, looks like the reader having walked off the end of the
        // conversation. It cost the room its whole reason to exist: one notch
        // of the wheel and a short answer dropped back onto the composer.
        //
        // So this is not a latch. A wheel, a drag or a jump to a find hit takes
        // the position over, and coming back to the question takes it back --
        // and either way the room stays exactly as it is, see [`Hold::reading`].
        let at_rest = top.item_ix >= count || (top.item_ix == run && top.offset_in_item <= px(1.));
        hold.reading = !at_rest;
        let reading = hold.reading;

        // How tall the turn is: from the top of the held question to the bottom
        // of the lowest row the list has actually measured.
        //
        // **The lowest measured one, not the last one.** A list measures the
        // rows around its viewport and nothing else, and it forgets every
        // measurement it holds whenever the run count changes -- which a turn
        // does to itself, since finished steps fold into one strip as they
        // settle. An unmeasured row is not a row below the fold, so asking the
        // last row alone answered "off the bottom of the screen" every time a
        // turn tidied up after itself. Walking back stops at the question, so
        // the search is the length of one turn rather than of the conversation,
        // and a turn with nothing measured in it yet is a turn that has not
        // moved: the room it already has stands.
        let well = state.viewport_bounds().size.height;
        let measured = state
            .bounds_for_item(run)
            .zip((run..count).rev().find_map(|ix| state.bounds_for_item(ix)));
        let Some((head_row, last_row)) = measured.filter(|_| well > px(0.)) else {
            return;
        };

        let wanted = well - room.head - (last_row.bottom() - head_row.top());
        if wanted <= room.floor {
            self.release(&state, reading.then_some(top));
            return;
        }
        if let Some(hold) = self.hold.as_mut() {
            hold.room = Some(wanted);
        }
    }

    /// The list state, brought back into step with the plan.
    pub fn list_state(&mut self, busy: bool, room: TopRoom) -> ListState {
        let count = self.plan.len();
        self.settle_hold(room);
        let held = self.hold.as_ref().and_then(|hold| self.run_of(hold.prompt));
        let (state, known) = self.list.get_or_insert_with(|| {
            // `Bottom`: a transcript follows its tail, and the list's own
            // documentation names this the chat-log case.
            let state = ListState::new(count, ListAlignment::Bottom, px(512.));
            // A transcript follows its tail, and saying so is what makes the
            // list track *whether* it still is. Left in the default mode the
            // question could only be answered by measuring the whole
            // conversation, which a lazily-measured list has no reason to have
            // done -- and following also survives being scrolled away from and
            // back, where a bottom alignment alone stops following the first
            // time the reader scrolls at all.
            state.set_follow_mode(FollowMode::Tail);
            (state, count)
        });

        let mut lost_position = false;
        if count > *known {
            // Growth is spliced rather than reset so the measurements already
            // taken -- and the user's scroll position -- survive it.
            state.splice(*known..*known, count - *known);
        } else if count != *known {
            state.reset(count);
            lost_position = true;
        }
        *known = count;

        // The tail is the only run that changes shape in place, and only while
        // a turn streams into it. Re-splice just that one so its cached height
        // does not freeze at the first chunk's.
        if busy && count > 0 {
            state.splice(count - 1..count, 1);
        }
        let state = state.clone();

        // Now that the list knows how many runs there are, and with the room
        // under the last one already asked for this frame, the held prompt can
        // be put at the top. A reset throws the scroll position away, so a hold
        // that had already been placed has to be placed again -- but never over
        // a reader who has scrolled, whose position is theirs to keep.
        let placing = self
            .hold
            .as_ref()
            .is_some_and(|hold| !hold.reading && (hold.pending || lost_position));
        if let Some(run) = held
            && placing
            && state.viewport_bounds().size.height > px(0.)
        {
            // **Stop following outright rather than pausing.** A merely paused
            // tail decides for itself when the view has come back to the bottom
            // and resumes -- and it decides that against the padding the list
            // was drawn with *last* frame, which is the frame before the room
            // under the held run was asked for. Measured against the old
            // padding the answer is always yes, so following resumed inside the
            // very prepaint that placed the hold and the next frame snapped
            // straight back to the tail: the transcript never moved, and
            // nothing about it looked wrong enough to point at why.
            state.set_follow_mode(FollowMode::Normal);
            state.scroll_to(ListOffset {
                item_ix: run,
                offset_in_item: px(0.),
            });
            if let Some(hold) = &mut self.hold {
                hold.pending = false;
                if lost_position {
                    // Every measurement went with the position, and the room
                    // worked out from the old ones can be too small for the
                    // rebuilt layout -- a turn shrinks as its finished steps
                    // fold into one strip. Too small, and the list quietly
                    // pulls the conversation back down to fill the panel,
                    // which is the hold being lost with nothing to see. Ask
                    // for a whole panel again and let the next frame, which
                    // can measure, cut it back down.
                    hold.room = None;
                }
            }
        }
        state
    }

    /// Which run draws `target`.
    ///
    /// The find bar hits an item; the list scrolls to a row, and a row is a
    /// run. This is the translation between the two, and without it Next and
    /// Previous could only change a number on screen.
    pub fn run_of(&self, target: TranscriptItemId) -> Option<usize> {
        self.plan
            .iter()
            .position(|run| run.members.contains(&target))
    }

    /// Scroll `target`'s run into view, and return its anchor if that run is a
    /// folded activity strip.
    ///
    /// The caller unfolds it. A hit counted as "3 of 7" that sits inside a
    /// collapsed strip is a hit the user is told about and cannot see, and
    /// unfolding does not move the run: folding changes what a run draws, never
    /// how many runs there are, so the index scrolled to stays the right one.
    pub fn reveal(&self, target: TranscriptItemId) -> Option<TranscriptItemId> {
        let ix = self.run_of(target)?;
        let run = &self.plan[ix];
        if let Some((state, _)) = &self.list {
            state.scroll_to_reveal_item(ix);
        }
        (run.strip.is_some() && !run.open).then(|| run.members[0])
    }
}

/// The transcript as one addressed sequence: read-only history first, then the
/// live tail — minus whatever is currently pinned above the composer.
///
/// A card the agent is parked on is drawn once, where it can be answered
/// without scrolling to it. Filtering *after* the positions are handed out is
/// what keeps the ids stable: the answered card reappears at the place it
/// always had, rather than at the place it would have if it had never been
/// away.
fn addressed(chat: &Chat) -> impl Iterator<Item = (TranscriptItemId, &ChatItem)> {
    chat.history
        .iter()
        .enumerate()
        .map(|(i, item)| (TranscriptItemId::History(i), item))
        .chain(
            chat.items
                .iter()
                .enumerate()
                .map(|(i, item)| (TranscriptItemId::Live(i), item)),
        )
        .filter(|(_, item)| !is_pinned(item))
}

/// Whether an item is a blocking card still waiting on the user, and so is
/// drawn above the composer rather than in the transcript.
pub fn is_pinned(item: &ChatItem) -> bool {
    match item {
        ChatItem::Permission(p) => p.resolved.is_none(),
        ChatItem::Ask(a) => a.resolved.is_none(),
        _ => false,
    }
}

/// The cadence one un-folded item asks for.
///
/// A settled step nobody has opened is the same index entry a folded strip is —
/// it just happened to have no neighbour to fold with. Everything else is read
/// rather than scanned, including a failed tool: its status is the reason to
/// stop at it.
fn single_kind(item: &ChatItem) -> RunKind {
    match item {
        ChatItem::User(_) => RunKind::Prompt,
        ChatItem::Tool(tool) => {
            let settled = matches!(
                tool.call.status,
                onehand_core::acp::ToolStatus::Completed | onehand_core::acp::ToolStatus::Failed
            );
            if settled && !tool.is_open() {
                RunKind::Compact
            } else {
                RunKind::Block
            }
        }
        ChatItem::Thought(thought) if !thought.expanded => RunKind::Compact,
        _ => RunKind::Block,
    }
}

/// The item an id addresses.
///
/// A direct index into whichever of the two collections the id names. It used
/// to be a linear search through the whole addressed sequence, run once per
/// member of every activity run and once per frame -- quadratic in the length
/// of the conversation, for a lookup the id already answers.
pub fn item(chat: &Chat, target: TranscriptItemId) -> Option<&ChatItem> {
    match target {
        TranscriptItemId::History(i) => chat.history.get(i),
        TranscriptItemId::Live(i) => chat.items.get(i),
    }
}

/// The find bar's state while it is open.
pub struct FindState {
    pub query: Entity<InputState>,
    /// Index of the current hit. Clamped against the live hit list on render,
    /// because the transcript can grow under an open find bar.
    pub current: usize,
    /// The last search and what it was a search of.
    ///
    /// The bar redraws its hit count every frame, and computing it reads every
    /// item's searchable text -- the whole conversation, while the user is
    /// mid-word in the query box.
    ///
    /// Keyed by the transcript's revision, which is the only key that is
    /// honest: a match can appear inside an item that is already there, so a
    /// key made of how many items there are would go on reporting the count
    /// from before a streaming answer said the word being searched for.
    cache: Option<Cached>,
}

struct Cached {
    query: String,
    revision: u64,
    hits: Vec<TranscriptMatch>,
}

impl FindState {
    pub fn new(query: Entity<InputState>) -> Self {
        Self {
            query,
            current: 0,
            cache: None,
        }
    }

    /// Every item matching the current query.
    pub fn matches(&mut self, chat: &Chat, cx: &App) -> Vec<TranscriptMatch> {
        let query = self.query.read(cx).value().to_string();
        let revision = chat.revision();

        if let Some(cached) = &self.cache
            && cached.query == query
            && cached.revision == revision
        {
            return cached.hits.clone();
        }

        let hits = compute_matches(chat, &query);
        self.cache = Some(Cached {
            query,
            revision,
            hits: hits.clone(),
        });
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::Viewport;
    use onehand_core::acp::{PermissionRequest, ToolCall, ToolKind, ToolStatus};
    use onehand_core::chat::{
        ActivityGroup, Chat, ChatItem, Md, PermItem, ToolItem, TranscriptItemId, UserMsg,
    };
    use std::path::PathBuf;

    fn read(title: &str) -> ChatItem {
        read_with_status(title, ToolStatus::Completed)
    }

    fn read_with_status(title: &str, status: ToolStatus) -> ChatItem {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind: ToolKind::Read,
            status,
            content: Vec::new(),
        }))
    }

    /// An answer, then two reads that fold into one activity strip.
    fn chat() -> Chat {
        let mut chat = Chat::new(1, PathBuf::from("/tmp/project"), "claude".to_string(), None);
        chat.items = vec![
            ChatItem::Agent(Md::parse("here is what I found")),
            read("Read src/a.rs"),
            read("Read src/b.rs"),
        ];
        chat
    }

    #[test]
    fn quiet_steps_collapse_into_one_row() {
        let mut viewport = Viewport::default();
        viewport.replan(&chat(), 0, |_| false);

        assert_eq!(viewport.run(0).map(|r| r.members.len()), Some(1));
        assert_eq!(
            viewport.run(1).map(|r| r.members.len()),
            Some(2),
            "two reads are one run"
        );
        let strip = viewport.run(1).and_then(|run| run.strip.as_ref()).unwrap();
        assert_eq!(strip.group, ActivityGroup::Explored);
        assert_eq!(strip.summary, "Inspected 2 files");
        assert!(viewport.run(2).is_none(), "three items, two rows");
    }

    /// The find bar counts *items* and the list scrolls to *rows*. An item
    /// folded inside a strip still has a row to be taken to -- the strip's.
    #[test]
    fn an_item_inside_a_strip_still_names_a_row() {
        let mut viewport = Viewport::default();
        viewport.replan(&chat(), 0, |_| false);

        assert_eq!(viewport.run_of(TranscriptItemId::Live(0)), Some(0));
        assert_eq!(viewport.run_of(TranscriptItemId::Live(1)), Some(1));
        assert_eq!(viewport.run_of(TranscriptItemId::Live(2)), Some(1));
        assert_eq!(
            viewport.run_of(TranscriptItemId::Live(9)),
            None,
            "an item that is not there names no row"
        );
    }

    fn permission(resolved: Option<&str>) -> ChatItem {
        ChatItem::Permission(PermItem {
            req: PermissionRequest {
                rpc_id: Default::default(),
                tool_call_id: None,
                title: "Run `rm -rf build`?".to_string(),
                options: Vec::new(),
            },
            resolved: resolved.map(str::to_string),
        })
    }

    /// A card the agent is parked on is drawn above the composer, so the
    /// transcript leaves it out -- otherwise the one thing the user has to
    /// answer is on screen twice, and answering one copy leaves the other
    /// looking live.
    #[test]
    fn an_unanswered_blocking_card_is_not_in_the_transcript() {
        let mut chat = chat();
        chat.items.push(permission(None));
        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);

        assert_eq!(
            viewport.run_of(TranscriptItemId::Live(3)),
            None,
            "the pending card has no row of its own"
        );
    }

    /// Answered, it takes the place it always had -- the record of what was
    /// decided, in the order it was decided.
    #[test]
    fn an_answered_one_returns_to_where_it_was() {
        let mut chat = chat();
        chat.items.push(permission(Some("Allow once")));
        chat.items.push(ChatItem::Agent(Md::parse("done")));
        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);

        let card = viewport.run_of(TranscriptItemId::Live(3));
        let after = viewport.run_of(TranscriptItemId::Live(4));
        assert!(card.is_some(), "a resolved card is part of the transcript");
        assert!(card < after, "and it sits before what came after it");
    }

    /// A frame that changed nothing about the conversation keeps the layout it
    /// has. `render` runs for a hover, a keystroke in the composer, a panel
    /// drag -- none of which is a reason to walk the transcript again.
    #[test]
    fn an_unchanged_transcript_is_not_laid_out_twice() {
        let mut chat = chat();
        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);
        assert_eq!(viewport.run(1).map(|r| r.open), Some(false));

        // Someone opened the strip, but the viewport was not told: same
        // revision, same folds count, so the answer it already has stands.
        viewport.replan(&chat, 0, |_| true);
        assert_eq!(viewport.run(1).map(|r| r.open), Some(false));

        // Toggling a fold is a change, and it is counted.
        viewport.replan(&chat, 1, |_| true);
        assert_eq!(viewport.run(1).map(|r| r.open), Some(true));

        // So is the transcript growing.
        chat.items.push(read("Read src/c.rs"));
        viewport.replan(&chat, 1, |_| true);
        assert_eq!(viewport.run(1).map(|r| r.members.len()), Some(3));
    }

    /// Live tools are attention cards, not members of a history summary. Once
    /// they settle, adjacent work may collapse into a user-controlled strip.
    #[test]
    fn live_tools_become_activity_only_after_settling() {
        let mut chat = chat();
        chat.items.push(ChatItem::User(UserMsg::text("and now?")));
        chat.items.push(read("Read src/c.rs"));
        chat.items.push(read("Read src/d.rs"));
        chat.items
            .push(ChatItem::Agent(Md::parse("first checkpoint")));
        chat.items
            .push(read_with_status("Read src/e.rs", ToolStatus::InProgress));
        chat.items
            .push(read_with_status("Read src/f.rs", ToolStatus::InProgress));
        chat.busy = true;

        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);

        // 0: old answer · 1: old reads · 2: prompt · 3: completed reads in the
        // live turn · 4: checkpoint · 5/6: reads changing right now. Each live
        // tool owns a row/card and cannot disappear behind a summary.
        assert_eq!(viewport.run(1).map(|r| r.open), Some(false));
        assert_eq!(viewport.run(3).map(|r| r.open), Some(false));
        assert!(viewport.run(5).and_then(|r| r.strip.as_ref()).is_none());
        assert!(viewport.run(6).and_then(|r| r.strip.as_ref()).is_none());
        assert_eq!(
            viewport.run(5).map(|r| r.members.as_slice()),
            Some([TranscriptItemId::Live(7)].as_slice())
        );
        assert_eq!(
            viewport.run(6).map(|r| r.members.as_slice()),
            Some([TranscriptItemId::Live(8)].as_slice())
        );

        // The tools settle and become one folded history row.
        for item in &mut chat.items[7..=8] {
            let ChatItem::Tool(tool) = item else {
                unreachable!();
            };
            tool.call.status = ToolStatus::Completed;
        }
        chat.busy = false;
        viewport.replan(&chat, 0, |_| false);
        assert!(viewport.run(5).and_then(|r| r.strip.as_ref()).is_some());
        assert_eq!(viewport.run(5).map(|r| r.members.len()), Some(2));
        assert_eq!(viewport.run(5).map(|r| r.open), Some(false));
        assert!(viewport.run(6).is_none());
    }

    /// A question just asked is taken to the top of the panel, so the answer
    /// arrives in the space under it instead of pushing it off the screen.
    #[test]
    fn a_new_prompt_asks_to_be_held_at_the_top() {
        let mut chat = chat();
        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);
        assert!(!viewport.holding(), "nothing was asked");

        chat.items.push(ChatItem::User(UserMsg::text("and now?")));
        viewport.replan(&chat, 0, |_| false);
        assert!(viewport.holding());

        // The answer to it is not another question, so the hold stands: it is
        // ended by the answer growing tall enough to need the room, which is a
        // measurement rather than a layout.
        chat.items.push(ChatItem::Agent(Md::parse("this")));
        viewport.replan(&chat, 0, |_| false);
        assert!(viewport.holding());
    }

    /// A transcript adopted from an archive arrives whole, and its last
    /// question is not a question just asked -- reopening a conversation
    /// belongs at its end, where it was left.
    #[test]
    fn a_transcript_that_arrives_whole_is_not_held() {
        let mut chat = chat();
        chat.items.push(ChatItem::User(UserMsg::text("and now?")));
        chat.items.push(ChatItem::Agent(Md::parse("this")));

        let mut viewport = Viewport::default();
        viewport.replan(&chat, 0, |_| false);
        assert!(!viewport.holding());
    }

    /// The assumption the scroll target rests on: opening a strip changes what
    /// a row *draws*, never how many rows there are. If it moved them, the
    /// unfold that follows a find would scroll the user somewhere else.
    #[test]
    fn unfolding_moves_no_row() {
        let (chat, mut folded, mut open) = (chat(), Viewport::default(), Viewport::default());
        folded.replan(&chat, 0, |_| false);
        open.replan(&chat, 0, |_| true);

        for item in 0..3 {
            let target = TranscriptItemId::Live(item);
            assert_eq!(folded.run_of(target), open.run_of(target));
        }
        assert_eq!(folded.run(1).map(|r| r.open), Some(false));
        assert_eq!(open.run(1).map(|r| r.open), Some(true));
    }
}
