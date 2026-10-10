use super::{BLOCK_GAP, COMPACT_GAP, ChatPane, SIDE_MARGIN, SIDE_MARGIN_NARROW};
use crate::chat::session::ChatSession;
use crate::chat::transcript;
use crate::chat::viewport::{self, RunKind};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, IntoElement, ListState, ParentElement, Rems, Styled, Window, div,
    relative, rems,
};
use gpui_component::spinner::Spinner;
use gpui_component::{Sizable as _, StyledExt};
use onehand_core::chat::{Chat, ChatItem, Link, TranscriptItemId};

impl ChatPane {
    /// The widest the elapsed column ever has to be.
    ///
    /// **Reserved**, so nothing after it moves when `9s` becomes `10s` or
    /// `59s` becomes `1m 0s`: the digits grow into room already spoken for,
    /// and what follows the clock begins at the same place whatever it says.
    const CLOCK_W: Rems = rems(3.25);

    /// The line under a live turn: a spinner and the turn's clock in the
    /// accent, then how many steps are running and what the agent says it is
    /// doing.
    fn working_strip(&self, cx: &App) -> gpui::AnyElement {
        let running = self
            .active_conversation()
            .and_then(|conv| conv.session())
            .map(|session| session.read(cx))
            .map(|session| {
                session
                    .chat
                    .items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item,
                            onehand_core::chat::ChatItem::Tool(tool)
                                if matches!(
                                    tool.call.status,
                                    onehand_core::acp::ToolStatus::InProgress
                                        | onehand_core::acp::ToolStatus::Pending
                                )
                        )
                    })
                    .count()
            })
            .unwrap_or(0);
        let status = self
            .active_chat(cx)
            .and_then(|chat| chat.activity_status().or_else(|| working_word(chat)));
        let elapsed = self.turn_began.map_or(0, |began| began.elapsed().as_secs());
        let accent = transcript::accent(cx);

        // **Only what is actually there.** A separator standing between a thing
        // and nothing is punctuation for a clause that was never written.
        let mut parts: Vec<gpui::AnyElement> = Vec::new();
        if running > 0 {
            parts.push(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .child(match running {
                        1 => "1 running task".to_string(),
                        n => format!("{n} running tasks"),
                    })
                    .into_any_element(),
            );
        }
        if let Some(status) = status {
            // The one part that gives way: the agent's own words about what it
            // is doing, and the only thing here whose length nothing bounds.
            parts.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(status)
                    .into_any_element(),
            );
        }

        let mut row = div()
            .h_flex()
            .items_center()
            .gap_2()
            .text_size(transcript::TEXT_SM)
            .text_color(accent)
            .child(Spinner::new().small().color(accent))
            .child(
                div()
                    .flex_none()
                    .w(Self::CLOCK_W)
                    .whitespace_nowrap()
                    .child(onehand_core::duration(elapsed)),
            );
        for (n, part) in parts.into_iter().enumerate() {
            if n > 0 {
                row = row.child(div().flex_none().child("·"));
            }
            row = row.child(part);
        }
        row.into_any_element()
    }

    /// Notice a turn starting and ending, and keep its clock true.
    ///
    /// **Stamped here because the model does not carry it.** A turn's start is
    /// `pub(crate)` in core, and the status line is not a good enough reason to
    /// widen it -- so the pane notices `busy` going up and reads its own clock.
    /// What that costs is an approximation: a session switched away from and
    /// back, or an app restarted mid-turn, starts counting again from zero.
    pub(super) fn track_turn(&mut self, window: &Window, cx: &mut Context<Self>) {
        let live = self
            .active_chat(cx)
            .is_some_and(|chat| chat.busy && chat.link != Link::Connecting);
        if !live {
            // The clock is the turn's, so it goes with it.
            self.turn_began = None;
            self.ticker = None;
            return;
        }
        self.turn_began.get_or_insert_with(std::time::Instant::now);
        self.start_ticker(window, cx);
    }

    /// Wake once a second while a turn is live, and not otherwise.
    ///
    /// **Not a frame timer**, which is what a clock drawn from the render pass
    /// would become: this asks for a redraw at the rate the thing it draws
    /// actually changes. It stands down while the window is not the one in
    /// front of the user -- the seconds keep passing either way, and the count
    /// is read off a start instant rather than accumulated, so coming back to
    /// the window shows the right number rather than the number of ticks that
    /// were drawn.
    fn start_ticker(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.ticker.is_some() || !window.is_window_active() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |pane, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let live = pane.update(cx, |pane: &mut Self, cx| {
                    let live = pane.turn_began.is_some();
                    if live {
                        cx.notify();
                    }
                    live
                });
                if !matches!(live, Ok(true)) {
                    break;
                }
            }
        }));
    }

    pub(super) fn busy(&self, cx: &App) -> bool {
        self.active_chat(cx).is_some_and(|chat| chat.busy)
    }

    /// Rebuild the active session's run layout, and hand back the list state
    /// that draws it.
    ///
    /// One call, because the two have to agree: the list's item count is the
    /// plan's length, and a list told about a different number of runs than the
    /// plan holds draws blanks or drops the tail.
    pub(super) fn reproject(&mut self, room: viewport::TopRoom, cx: &App) -> Option<ListState> {
        let session = self.active_conversation()?.session()?.clone();
        let session = session.read(cx);
        let handle = self.handle.clone();
        let conv = self.active_conversation_mut()?;
        conv.viewport.replan(
            &session.chat,
            session.folds_revision(),
            |anchor, default| session.activity_is_open(anchor, default),
            |anchor, default| session.turn_is_open(anchor, default),
        );
        let state = conv.viewport.list_state(session.chat.busy, room);
        // Asked for once the state exists, and only then: the list is what
        // knows it has been scrolled, and the pane is what draws the control
        // that depends on it. Without this the pill waited for whatever
        // happened to redraw the pane next, which on a finished conversation
        // is nothing at all.
        conv.viewport.hook_scroll(move |_, _, cx| {
            let _ = handle.update(cx, |_: &mut Self, cx| cx.notify());
        });
        Some(state)
    }

    /// Take the reader back to where the latest activity is arriving.
    ///
    /// Through the viewport rather than straight at the list, because the list
    /// cannot answer where that is: while a question is held at the top, the
    /// activity is arriving in the room under it and the place to return to is
    /// the question, which only the layout knows the row of.
    pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        if let Some(conv) = self.active_conversation_mut() {
            conv.viewport.jump_to_latest();
        }
        cx.notify();
    }

    /// Draw run `ix`. Called by the list, after `render` has returned.
    /// `window` is here for one thing the renderer cannot get any other way:
    /// the rem size in force for this subtree, which is what per-panel zoom
    /// overrides. The markdown renderer scales its headings off a base given in
    /// pixels, so without the live rem size an answer's headings are the one
    /// part of it that stays put while everything around them grows.
    pub(super) fn run_element(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        // What a block bounds itself against, measured on the panel rather than
        // the window: with a dock open, half the window is taller than the whole
        // conversation.
        //
        // **Handed in, never read here.** The list holds its own state mutably
        // for as long as it is calling this, so asking it how tall its viewport
        // is from inside is a second borrow and a panic -- which is a crash on
        // the first frame of any conversation carrying a permission, not a rare
        // race. The caller reads it before the list starts, which is the last
        // moment it can be asked.
        room: transcript::Room,
        window: &Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let Some(plan) = self
            .active_conversation()
            .and_then(|conv| conv.viewport.run(ix))
        else {
            return div().into_any_element();
        };
        let Some(chat) = self.active_chat(cx) else {
            return div().into_any_element();
        };
        let body =
            |targets: &[TranscriptItemId], room: &transcript::Room| -> Vec<gpui::AnyElement> {
                targets
                    .iter()
                    .filter_map(|&target| {
                        viewport::item(chat, target).map(|item| {
                            transcript::item(session, item, target, room.clone(), window, cx)
                                .into_any_element()
                        })
                    })
                    .collect()
            };

        // The run layout already classified every run's cadence, so the space
        // above this one is decided by the pair it forms with the run before it
        // rather than by this run's opinion of itself.
        let lead = lead_gap(
            self.active_conversation()
                .and_then(|conv| conv.viewport.kind_before(ix)),
            plan.head_kind(),
        );
        let margin = match room.narrow {
            true => SIDE_MARGIN_NARROW,
            false => SIDE_MARGIN,
        };

        let Some(strip) = plan.strip.clone() else {
            // The run the layout appends while a turn is live carries no
            // transcript item, because what it reports is the turn rather than
            // anything in it.
            //
            // **It takes the widest boundary in the conversation**, the one a
            // prompt gets, and ignores the cadence the run above it asked for.
            // Every other gap here is between two things the agent said; this
            // one is between what it said and the app talking about it, and set
            // at a block's distance the line read as one more entry in the
            // turn -- worst directly under a cluster, where the two closed
            // ranks and the status line looked like another folded step.
            if plan.members.is_empty() {
                // The other memberless row: what a finished turn wrote. It
                // takes the same wide boundary, and for the same reason -- it
                // is the app talking about the turn rather than part of it.
                let body = match &plan.changes {
                    Some(plan_changes) => {
                        let anchor = plan_changes.anchor;
                        let this = self.handle.clone();
                        let folded = session.clone();
                        let default = plan_changes.opens_itself;
                        transcript::turn_summary(
                            session,
                            plan_changes,
                            plan.open,
                            move |_, _, cx: &mut App| {
                                folded.update(cx, |session, cx| {
                                    session.toggle_turn(anchor, default);
                                    cx.notify();
                                });
                                let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                            },
                            cx,
                        )
                    }
                    None => self.working_strip(cx),
                };
                return column(rems(BLOCK_GAP.0 * 2.), margin, vec![body]).into_any_element();
            }
            return column(lead, margin, body(&plan.members, &room)).into_any_element();
        };

        let anchor = plan.members[0];
        let this = self.handle.clone();
        let folded = session.clone();

        // **Nothing under the line is built while it is closed.** A cluster is
        // every step between two paragraphs, which in a long turn is dozens —
        // and a collapsed line that built them all to draw none of them is that
        // cost paid per frame for something nobody asked to see.
        let inside = match plan.open {
            false => Vec::new(),
            true => strip
                .sections
                .iter()
                .map(|section| self.section_element(section, session, &room, window, cx))
                .collect(),
        };

        let open = plan.open;
        column(
            lead,
            margin,
            vec![transcript::cluster(
                &strip,
                open,
                move |_, _, cx: &mut App| {
                    folded.update(cx, |session, cx| {
                        session.set_activity_open(anchor, !open);
                        cx.notify();
                    });
                    // The pane owns the run layout the list reads back, so it
                    // is the half that has to be told to draw again -- the
                    // session's own notify redraws the session, not the plan.
                    let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                },
                ("activity", transcript::fold_key(anchor)).into(),
                inside,
                cx,
            )],
        )
        .into_any_element()
    }

    /// One stretch of one kind of work inside an opened cluster.
    ///
    /// A section of one member is that member's own row; a section of several
    /// is one row standing for them that opens into the rest.
    fn section_element(
        &self,
        section: &viewport::Section,
        session: &Entity<ChatSession>,
        room: &transcript::Room,
        window: &Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let Some(chat) = self.active_chat(cx) else {
            return div().into_any_element();
        };
        let body =
            |targets: &[TranscriptItemId], room: &transcript::Room| -> Vec<gpui::AnyElement> {
                targets
                    .iter()
                    .filter_map(|&target| {
                        viewport::item(chat, target).map(|item| {
                            transcript::item(session, item, target, room.clone(), window, cx)
                                .into_any_element()
                        })
                    })
                    .collect()
            };

        let anchor = section.members[0];
        let open = session.read(cx).section_is_open(anchor);
        let this = self.handle.clone();
        let folded = session.clone();

        // **A section of one is that step's own row, and so is a section of
        // several — the merge is in the row's own words.** A row standing for
        // one step would be the same row twice, one inside the other; and a row
        // standing for three reads says `Read 3 files` and opens into the three
        // paths, which is one level rather than two.
        let single = section.members.len() < 2;
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .map(|block| match single {
                true => block.children(body(&section.members, room)),
                false => block.child(transcript::activity_group(
                    section,
                    open,
                    move |_, _, cx: &mut App| {
                        folded.update(cx, |session, cx| {
                            session.toggle_section(anchor);
                            cx.notify();
                        });
                        let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                    },
                    ("section", transcript::fold_key(anchor)).into(),
                    match open {
                        true => body(&section.members, room),
                        false => Vec::new(),
                    },
                    cx,
                )),
            })
            .into_any_element()
    }
}

/// What the running line says when the model has nothing to add.
///
/// **The model goes quiet on purpose, and this is the one place that is
/// wrong.** `Chat::activity_status` says nothing while a thought or a tool is
/// live, because the transcript's own block is already saying it a few lines
/// up -- which was right while the only other reader was a notification. The
/// running line is *below* those blocks and says nothing else, so the two
/// states a reader most wants a word for came out as a mark and a clock.
///
/// Read off the live items rather than asked for, so the layer below keeps
/// reporting exactly what it reported before.
pub(super) fn working_word(chat: &Chat) -> Option<String> {
    match chat.items.last()? {
        ChatItem::Thought(thought) if thought.elapsed_secs.is_none() => {
            Some("Thinking…".to_string())
        }
        ChatItem::Tool(tool)
            if matches!(
                tool.call.status,
                onehand_core::acp::ToolStatus::InProgress | onehand_core::acp::ToolStatus::Pending
            ) =>
        {
            // The step's own verb and object, which is the sentence the
            // cluster line would say about it -- not the raw tool title, which
            // is where a path or a whole command would come from.
            let shown = onehand_core::chat::activity::presentation(tool);
            Some(
                format!("{} {}", shown.action, shown.subject)
                    .trim()
                    .to_string(),
            )
        }
        _ => None,
    }
}

/// The space between the run above and the run below it.
///
/// **One number per boundary, and it hangs off the lower run.** Owned by the
/// upper run instead -- which is where it used to live -- a block could only say
/// how much room it wanted *after* itself, so what it got above depended on
/// whatever happened to precede it. Two consequences, both visible: a prompt sat
/// 2.25rem below prose and 1.75rem below a folded strip while always giving
/// 1.5rem to the answer under it, so the two sides of one space differed by a
/// step and the turn read as sitting slightly low; and a folded strip pulled the
/// answer *after* it up to 0.25rem, gluing prose to an index row it has nothing
/// to do with.
pub(super) fn lead_gap(previous: Option<RunKind>, this: RunKind) -> Rems {
    // The first run rests on the list's own top padding.
    let Some(previous) = previous else {
        return rems(0.);
    };
    match (previous, this) {
        // **A turn opens above the prompt and not below it.** The space over a
        // question is what a reader scrolling back finds the last one by, so it
        // is the widest boundary inside the conversation -- twice what two
        // blocks of one answer take. Under it the answer is the *reply*, and a
        // gap as wide as the one above would cut the question off from the
        // thing answering it. They were symmetrical, which said the prompt
        // belonged to neither side.
        (_, RunKind::Prompt) => rems(BLOCK_GAP.0 * 2.),
        (RunKind::Prompt, _) => BLOCK_GAP,
        // Index entries close ranks with each other and with nothing else.
        (RunKind::Compact, RunKind::Compact) => COMPACT_GAP,
        _ => BLOCK_GAP,
    }
}

/// The frame one run of the transcript is drawn in: a centred reading column,
/// its inset inside its cap, that shrinks with its panel. Width lives here
/// rather than around each item because a run is what the virtual list draws;
/// a cluster's line drawn by the pane and its steps must share the same edges.
fn column(lead: Rems, margin: Rems, content: Vec<gpui::AnyElement>) -> gpui::Div {
    div()
        .h_flex()
        // Centred by the row rather than by an auto margin, which a list row
        // ignores: each row is a layout root of its own.
        .justify_center()
        .w_full()
        // The reading size and its leading are set here, on the frame every
        // run is drawn in, so one place decides them for prose, rows and
        // wells alike; the leading as a ratio, so a well a step smaller keeps
        // the same rhythm.
        .text_size(transcript::TEXT)
        .line_height(relative(transcript::LEADING))
        .pt(lead)
        .child(
            div()
                .w_full()
                .min_w_0()
                .max_w(transcript::CONTENT_COLUMN)
                .px(margin)
                .children(content),
        )
}
