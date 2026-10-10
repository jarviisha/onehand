//! What the transcript looks like on screen: the run layout the virtual list
//! draws, and the list's own scroll and measurement state.
//!
//! These two are one thing wearing two names. The list draws *runs*, not items,
//! so its item count is the run count — while everything that wants to be
//! *taken* somewhere names an item, and getting there means knowing which run
//! holds it. Kept apart, the pairing was implicit: a plan on the pane and a
//! scroll position on the session, made to line up by the order two calls
//! happened in, with nothing on either side able to turn an item into a row.

use super::transcript;
use gpui::{App, FollowMode, ListAlignment, ListOffset, ListState, Pixels, Window, px};
use onehand_core::chat::{
    ActivityGroup, Chat, ChatItem, RunOutcome, TranscriptItemId, cluster_summary, run_outcome,
};

/// One cluster of activity: the muted line, and what is under it once opened.
///
/// **Worked out at plan time, where the members are already in hand**, and not
/// at draw time — the sentence, the outcome and every child line are walks over
/// the same list, and a row asking for them per frame is that list walked once
/// per frame for an answer the plan was standing next to. The plan is rebuilt
/// only when the transcript or a fold actually changes.
#[derive(Clone)]
pub struct ActivityPlan {
    /// The one sentence the collapsed line says.
    ///
    /// **And the whole of what that line is drawn from.** The cluster used to
    /// carry a run outcome beside this, for a status mark at the head of the
    /// line -- and with the mark gone there was nothing left reading it: what
    /// went wrong is a count, and the count is in here.
    pub summary: onehand_core::chat::ClusterSummary,
    /// The rows inside the frame, grouped by the kind of work they were.
    pub sections: Vec<Section>,
}

/// A stretch of one cluster's members that were the same kind of work.
///
/// **Only the opened frame reads these.** The cluster's own line names kinds in
/// its sentence and carries the status in its mark, so it needs no grouping;
/// what still does is the list under it, which is read as a list of things the
/// agent did and groups the repetitive ones.
#[derive(Clone)]
pub struct Section {
    pub group: ActivityGroup,
    pub members: Vec<TranscriptItemId>,
    /// Counts, for the row standing for the section where it has more than one
    /// member.
    pub summary: String,
    pub outcome: RunOutcome,
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
    /// `Some` when this run draws as one row standing for several steps.
    pub strip: Option<ActivityPlan>,
    /// `Some` on the row closing a finished turn, which stands for no item at
    /// all: it is what the turn *did*, read back out of the steps above it.
    pub changes: Option<ChangePlan>,
    pub open: bool,
    pub kind: RunKind,
}

/// What a finished turn wrote, and the prompt it hangs off.
///
/// **Keyed by the prompt that began the turn**, and folded through the same
/// set an activity cluster is. A prompt is a run of its own and never a
/// cluster's anchor, so the two uses of that set cannot collide -- and a
/// fourth fold set for one row would be a fourth place to reconcile whenever
/// a conversation is rebuilt.
pub struct ChangePlan {
    pub anchor: TranscriptItemId,
    pub changes: onehand_core::chat::TurnChanges,
    /// What this block does if nobody has said otherwise -- true only for the
    /// last finished turn. Carried so the toggle knows which way "untouched"
    /// currently points, and so the renderer does not have to work it out
    /// again from a plan it cannot see the rest of.
    pub opens_itself: bool,
    /// The turn's own items, so a file row opened later can be diffed without
    /// the whole conversation being diffed now.
    pub body: Vec<TranscriptItemId>,
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

/// How much room the list leaves under its last row.
///
/// **The clip comes off once, and only off the answer that has not already had
/// it taken off.** `measured` is read from the list's own viewport, which *is*
/// the clipped box — so subtracting `cut` from it again left the list that much
/// short of the padding a scroll to the tail needs. It never came to rest, so
/// the prompt stayed held and the jump-to-latest pill appeared on every turn.
/// `floor` is a constant that has never heard of the clip, so there the
/// subtraction is right.
///
/// Split out from [`Viewport::tail_room`] because that reads a `ListState`,
/// which cannot be built without a window — and the arithmetic is the whole of
/// what went wrong.
fn room_for(floor: Pixels, cut: Pixels, measured: Option<Pixels>) -> Pixels {
    let bare = (floor - cut).max(Pixels::ZERO);
    measured.map_or(bare, |measured| measured.max(bare))
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
    /// The first run whose drawing changed in the last replan.
    ///
    /// A turn folds its settled steps into one strip as it goes, so the run
    /// count *shrinks* mid-turn — and a shrink used to reset the list whole.
    /// That throws away every measured row height and the scroll position on
    /// every tool that finishes: the transcript jumps, the frame hitches while
    /// the rows above are measured again, and the way-back pill flickers as the
    /// anchor lands at the tail for one frame. The fold happens at the tail, so
    /// naming where the plan actually diverged is what lets everything above it
    /// be left alone.
    changed_from: usize,
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
    pub fn replan(
        &mut self,
        chat: &Chat,
        folds: u64,
        is_open: impl Fn(TranscriptItemId) -> bool,
        // Asked with the default in hand, since what "untouched" means for a
        // turn summary changes as the conversation grows. See `close_turns`.
        turn_open: impl Fn(TranscriptItemId, bool) -> bool,
    ) {
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

        let mut plan: Vec<RunPlan> = transcript::runs(&addressed)
            .into_iter()
            .map(|run| match run {
                transcript::Run::Single(target) => RunPlan {
                    members: vec![target],
                    strip: None,
                    changes: None,
                    open: true,
                    kind: item(chat, target).map_or(RunKind::Block, single_kind),
                },
                transcript::Run::Activity { members } => {
                    let anchor = members[0];
                    let bodies: Vec<&ChatItem> = members
                        .iter()
                        .copied()
                        .filter_map(|t| item(chat, t))
                        .collect();

                    let summary = cluster_summary(&bodies);
                    // **Open while anything in it runs, closed once it is
                    // done**, and a fold the reader made flips whichever of
                    // the two it is: the set holds where they disagreed with
                    // the default, which is all a toggle can say.
                    let open = is_open(anchor) != summary.running.is_some();
                    RunPlan {
                        strip: Some(ActivityPlan {
                            summary,
                            sections: sections(chat, &members),
                        }),
                        changes: None,
                        open,
                        members,
                        // Folded it is one muted line; opened it is a frame with
                        // every step it holds in it.
                        kind: if open {
                            RunKind::Block
                        } else {
                            RunKind::Compact
                        },
                    }
                }
            })
            .collect();

        // **Accumulated, never overwritten.** A replan can find a divergence the
        // list is not told about in the same breath -- a lone step settling
        // turns its run from a card into an index row without changing how many
        // runs there are -- and that row stays stale until something else moves
        // the count. Written fresh each time, the later replan's answer painted
        // over the earlier one and the splice began past the row that had
        // actually changed, leaving the list holding a tall card's height for a
        // row now one line high.
        // **A turn that is still going gets a row of its own, at the end.**
        // Empty members and no strip is what marks it: the renderer draws its
        // own line there rather than any transcript item, because there is no
        // item -- the thing being reported is the turn itself. It is a row
        // rather than something floating over the composer so that it sits
        // where the next block will appear, in the same reading column; the
        // cost is that scrolling up takes it off screen, which is the trade
        // taken deliberately. `busy` is already part of the key above, so the
        // row arrives and leaves with the turn and nothing else invalidates
        // for it.
        if chat.busy {
            plan.push(RunPlan {
                members: Vec::new(),
                strip: None,
                changes: None,
                open: true,
                kind: RunKind::Compact,
            });
        }

        let plan = close_turns(chat, plan, &turn_open);

        let diverged = self
            .plan
            .iter()
            .zip(plan.iter())
            .position(|(was, now)| !draws_the_same(was, now))
            .unwrap_or_else(|| self.plan.len().min(plan.len()));
        self.changed_from = self.changed_from.min(diverged);
        self.plan = plan;

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
    /// `cut` is what the transcript is already clipped by at its foot, and it
    /// is taken off here rather than by the caller because **only this knows
    /// which of the two answers below it gave**. The held answer is measured
    /// from the list's own viewport, which is the clipped box — so the clip is
    /// inside it already, and subtracting it again outside left the list that
    /// much short of the padding `scroll_to` needs to reach the tail. It never
    /// came to rest, so the prompt stayed "held" and the jump-to-latest pill
    /// appeared on every turn. The bare answer is a constant and knows nothing
    /// about the clip, so there it is the caller's subtraction that was right.
    pub fn tail_room(&self, floor: Pixels, cut: Pixels) -> Pixels {
        let (Some(hold), Some((state, _))) = (&self.hold, &self.list) else {
            return room_for(floor, cut, None);
        };
        // **A list chasing its tail must never be given this room.** Following
        // puts the bottom of the *padding* at the bottom of the panel, so a
        // panel's worth of it there is a panel's worth of nothing: the
        // conversation is pushed off the top edge and the transcript draws
        // empty, with no row on screen to say what happened. The room and the
        // tail are already exclusive by construction; this is the second lock
        // on it, because the failure has no symptom to debug from.
        if state.is_following_tail() {
            return room_for(floor, cut, None);
        }
        let measured = hold
            .room
            .unwrap_or_else(|| state.viewport_bounds().size.height);
        room_for(floor, cut, Some(measured))
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
        // So this is not a latch. A wheel or a drag takes the position over,
        // and coming back to the question takes it back --
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
        let from = self.changed_from.min(count).min(*known);
        if count != *known {
            // **Spliced from where the plan actually diverged, never reset.** A
            // turn shrinks its own layout as it goes -- each step that settles
            // folds into the strip beside it -- so a shrink is the ordinary
            // mid-turn case rather than the exceptional one. Reset whole, every
            // finished tool cost the list every row height it had measured and
            // the reader their place: the transcript jumped, the frame hitched
            // re-measuring the conversation above, and the way back to the
            // latest blinked on as the anchor landed past the tail for a frame.
            // The fold is at the tail and everything above it is the same row
            // it was, so naming the first run that changed is what leaves it
            // alone. Growth is a special case of this: a pure append diverges
            // at the old end, which is the empty splice it already did.
            let anchor = state.logical_scroll_top().item_ix;
            state.splice(from..*known, count - from);
            // Only a reader sitting *inside* the replaced range has lost
            // anything; one above it is still on the row they were on.
            lost_position = anchor >= from;
        } else if from < count {
            // The same rows, one of them a different shape: a settled step that
            // became an index row, a group opened. **Measured again rather than
            // spliced** -- a splice says these are different rows and costs the
            // reader whatever offset they held inside them, which is the same
            // reason the streaming tail below is re-measured and not replaced.
            state.remeasure_items(from..count);
        }
        // Whatever diverged has now been told to the list, so the next replan
        // starts its accumulation from nothing.
        self.changed_from = count;
        *known = count;

        // The tail is the only run that changes shape in place, and only while
        // a turn streams into it. Ask for just that one to be measured again so
        // its cached height does not freeze at the first chunk's.
        //
        // **Re-measured, not spliced.** A splice says the rows in the range are
        // different rows, so the list gives up whatever position it held inside
        // them: a reader partway down an answer taller than the panel is put
        // back at that answer's first line, once per arriving chunk, which is
        // every long answer being unreadable while it is written. Asking for a
        // measurement says the row is the same row and is only a different
        // height, which is exactly what a streamed chunk did to it -- and the
        // list puts the reader back where they were once it knows the new
        // height.
        //
        // **Two rows while a turn is live, not one.** The last row is then the
        // status line the plan appends, and the run growing under the arriving
        // chunks is the one before it -- asking for the last alone left the
        // streaming answer frozen at its first chunk's height.
        if busy && count > 0 {
            state.remeasure_items(count.saturating_sub(2)..count);
        }
        let state = state.clone();

        // Now that the list knows how many runs there are, and with the room
        // under the last one already asked for this frame, the held prompt can
        // be put at the top. A reset throws the scroll position away, so a hold
        // that had already been placed has to be placed again -- but never over
        // a reader who has scrolled, whose position is theirs to keep.
        //
        // The third case is a list resting past its last row, which is where a
        // scroll that came to rest at the very bottom leaves one. That is an
        // ordinary place to be while a question is held -- the room under the
        // turn ends the transcript exactly where the question meets the top
        // edge, so the bottom and the question are one place -- but it is a
        // place with no row beneath it to measure from, and a room that has
        // never been measured asks for a whole panel of padding. The two
        // together draw an empty panel that cannot measure its way back out.
        // Naming the question's own row is the same picture wherever those two
        // places agree, and the only one that can be measured where they do
        // not.
        let adrift = state.logical_scroll_top().item_ix >= count;
        let placing = self
            .hold
            .as_ref()
            .is_some_and(|hold| !hold.reading && (hold.pending || lost_position || adrift));
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

    /// Take the transcript back to where the latest activity is arriving.
    ///
    /// **Which is the held question, while there is one**, and not the end of
    /// the transcript. With a room under the turn those are ordinarily the same
    /// place, so asking the list for its end looks right — but only once the
    /// room has been measured against a turn. Until then it is a whole panel of
    /// padding, the end of the transcript is the end of *that*, and landing
    /// there draws nothing but the padding: no row on screen, and none measured
    /// either, so the room has nothing left to shrink against and nothing takes
    /// the reader off the empty panel again. The question's own row is a place
    /// that always exists.
    ///
    /// With no question held, the latest activity really is the tail, and the
    /// list is handed back to following it.
    pub fn jump_to_latest(&mut self) {
        let Some((state, _)) = &self.list else {
            return;
        };
        let state = state.clone();
        let held = self.hold.as_ref().map(|hold| hold.prompt);
        if let Some(run) = held.and_then(|prompt| self.run_of(prompt)) {
            state.scroll_to(ListOffset {
                item_ix: run,
                offset_in_item: px(0.),
            });
            // The reader asked to come back, so the hold is resting on its
            // question again -- which is also what takes this control off the
            // screen, in the same frame as the press rather than the next one.
            if let Some(hold) = &mut self.hold {
                hold.reading = false;
            }
            return;
        }
        self.release(&state, None);
    }

    /// Which run draws `target`.
    ///
    /// The list scrolls to a row, and a row is a run -- while the thing being
    /// scrolled to is named as an item. This is the translation between the
    /// two, and the held prompt is what asks for it: keeping a question at the
    /// top of the frame means knowing which row that question is drawn in.
    fn run_of(&self, target: TranscriptItemId) -> Option<usize> {
        self.plan
            .iter()
            .position(|run| run.members.contains(&target))
    }
}

/// Put a row closing every finished turn into `plan`.
///
/// **Derived here and never stored.** The diffs it adds up are already in the
/// transcript and in the archive, so a summary written down beside them is a
/// second copy that can disagree with the first -- and `items.jsonl` is
/// appended to and never revisited, so a copy written at the end of a turn
/// could not be corrected if it ever did. Recomputing it is a walk of the
/// turn's own steps, which is work the cluster lines above it are already
/// doing.
///
/// **A cancelled turn still gets one.** What was written before the stop is on
/// disk exactly as if the turn had run to the end, and that is the moment a
/// reader most needs to be told which files those were.
fn close_turns(
    chat: &Chat,
    plan: Vec<RunPlan>,
    is_open: &impl Fn(TranscriptItemId, bool) -> bool,
) -> Vec<RunPlan> {
    let mut out: Vec<RunPlan> = Vec::with_capacity(plan.len());
    // The turn being walked: where it began, and what it has done so far.
    let mut turn: Option<(TranscriptItemId, Vec<TranscriptItemId>)> = None;

    // **The last finished turn opens itself; every older one is a line.** The
    // block answers "what did that do to my tree", which is a question about
    // the turn that just ended -- and a conversation that kept every one of
    // them open would be a column of tables with the reading between them.
    // Sending the next prompt is what puts the one above away, which is the
    // moment the reader stopped asking.
    //
    // **Which is why the fold is asked with its default in hand.** A reader who
    // decided about this block keeps that decision; one who never touched it
    // follows the rule above. Recorded the other way round -- a set of
    // exceptions to whatever the default happens to be -- the recorded fact
    // changes meaning when the default moves under it, and a block closed while
    // it was newest sprang open the moment the next prompt went out.
    let closer = |anchor: TranscriptItemId, body: &[TranscriptItemId], newest: bool| {
        let items: Vec<&ChatItem> = body.iter().filter_map(|&t| item(chat, t)).collect();
        onehand_core::chat::turn_changes(&items).map(|changes| RunPlan {
            members: Vec::new(),
            strip: None,
            changes: Some(ChangePlan {
                anchor,
                changes,
                body: body.to_vec(),
                opens_itself: newest,
            }),
            open: is_open(anchor, newest),
            kind: RunKind::Compact,
        })
    };

    // Which prompt begins the last turn, since that is the one that opens.
    let newest = plan
        .iter()
        .rev()
        .find(|run| run.kind == RunKind::Prompt)
        .and_then(|run| run.members.first().copied());

    for run in plan {
        if run.kind == RunKind::Prompt {
            if let Some((anchor, body)) = turn.take()
                && let Some(row) = closer(anchor, &body, Some(anchor) == newest)
            {
                out.push(row);
            }
            turn = run.members.first().map(|&head| (head, run.members.clone()));
        } else if let Some((_, body)) = turn.as_mut() {
            body.extend(run.members.iter().copied());
        }
        out.push(run);
    }
    // The last turn closes only once it has stopped: a running one is still
    // writing, and a total that grows under the eye is not a summary.
    if !chat.busy
        && let Some((anchor, body)) = turn
        && let Some(row) = closer(anchor, &body, Some(anchor) == newest)
    {
        out.push(row);
    }
    out
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

/// Whether two runs at the same position draw the same row.
///
/// Everything that decides what the row *is*, which is what the list's cached
/// height is a measurement of. The group is left out because it is a function
/// of the members, and the summary is a function of their contents -- so a step
/// that settles inside a strip changes the summary and is caught there.
fn draws_the_same(was: &RunPlan, now: &RunPlan) -> bool {
    was.members == now.members
        && was.open == now.open
        && was.kind == now.kind
        && was.strip.as_ref().map(|s| &s.summary) == now.strip.as_ref().map(|s| &s.summary)
        // **Compared in full, unlike a cluster's.** The two rows that carry no
        // members at all -- the line saying a turn is running and the line
        // saying what it wrote -- are otherwise indistinguishable from each
        // other and from every other empty run, so without this the list keeps
        // a one-line height for a row that has become a list of files.
        && was.changes.as_ref().map(|c| (c.anchor, &c.changes))
            == now.changes.as_ref().map(|c| (c.anchor, &c.changes))
}

/// Split a cluster into the stretches of one kind of work the frame lists.
fn sections(chat: &Chat, members: &[TranscriptItemId]) -> Vec<Section> {
    let mut out: Vec<Vec<TranscriptItemId>> = Vec::new();
    let mut last: Option<ActivityGroup> = None;
    for &target in members {
        let group = item(chat, target).and_then(transcript::section_group);
        // `None` never extends: a step that stands on its own does so however
        // many of its kind are beside it, so two of them are two sections.
        match (group.is_some() && group == last, out.last_mut()) {
            (true, Some(run)) => run.push(target),
            _ => out.push(vec![target]),
        }
        last = group;
    }
    out.into_iter()
        .map(|members| {
            let bodies: Vec<&ChatItem> = members
                .iter()
                .copied()
                .filter_map(|t| item(chat, t))
                .collect();
            Section {
                group: bodies
                    .first()
                    .and_then(|item| transcript::section_group(item))
                    .unwrap_or(ActivityGroup::Other),
                summary: transcript::activity_summary(&bodies),
                outcome: run_outcome(&bodies),
                members,
            }
        })
        .collect()
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
/// it just happened to have no neighbour to fold with, and so is a closed
/// thought or the one line a settled question or grant leaves. Everything else
/// is read rather than scanned, including a failed tool: its status is the
/// reason to stop at it.
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
        ChatItem::Thought(thought) if !thought.is_open() => RunKind::Compact,
        ChatItem::Permission(p) if p.resolved.is_some() && !p.expanded => RunKind::Compact,
        ChatItem::Ask(a) if a.resolved.is_some() && !a.expanded => RunKind::Compact,
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

#[cfg(test)]
mod tests;
