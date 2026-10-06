use super::{
    BLOCK_GAP, COMPOSER_MIN_H, COMPOSER_REST, ChatPane, ChatPaneEvent, JUMP_PILL_H, LIST_HEAD,
    NARROW_PANEL, OverlayRoom, ProjectAction, SMOKE, waits_alone,
};
use crate::chat::session::ChatSession;
use crate::chat::transcript::{self};
use crate::chat::viewport::{self};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, IntoElement, ListState, ParentElement, SharedString, Styled, Window, div,
    list, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{Link, TranscriptItemId};

impl ChatPane {
    /// The blocking cards the agent is parked on, drawn just above the composer.
    ///
    /// Pinned rather than left in the transcript because the transcript scrolls
    /// and this does not: a permission that arrived four screens ago is still
    /// the only reason nothing is happening, and hunting for it is not a thing
    /// to ask of someone who is waiting. Once answered the card leaves here and
    /// takes its place in the transcript, where it reads as a record of what
    /// was decided rather than as a control.
    ///
    /// The transcript is what leaves it out — see the projection — so the card
    /// is never drawn twice.
    pub(super) fn pinned(
        &self,
        session: &Entity<ChatSession>,
        // A pinned card rests on the composer rather than inside the list, but
        // what it must not outgrow is the same panel.
        well: Option<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        // A question's free-text box needs a window to be built and the session
        // never has one, so the card's own way to the screen is where it is
        // made. Before the cards are read, since the box is one of them.
        session.update(cx, |s, cx| {
            s.sync_ask_inputs(window, cx);
            s.sync_perm_focus(window, cx);
        });
        let Some(chat) = self.active_chat(cx) else {
            return Vec::new();
        };
        let mut out: Vec<(usize, gpui::AnyElement)> = chat
            .pending_permissions()
            .into_iter()
            .map(|(idx, p)| {
                (
                    idx,
                    transcript::permission(session, p, TranscriptItemId::Live(idx), well, cx)
                        .into_any_element(),
                )
            })
            .chain(chat.pending_asks().into_iter().map(|(idx, a)| {
                (
                    idx,
                    transcript::ask(session, a, TranscriptItemId::Live(idx), cx).into_any_element(),
                )
            }))
            .collect();
        // Two lists merged back into transcript order: the agent can park on a
        // permission and a question at once, and the order they were asked in
        // is the only order that makes sense of them.
        out.sort_by_key(|(idx, _)| *idx);
        let mut pinned: Vec<gpui::AnyElement> =
            out.into_iter().map(|(_, element)| element).collect();
        // Under the blocking cards and directly over the composer, because that
        // is where the prompt it holds was written and where it will reappear
        // if the queue is cancelled.
        pinned.extend(self.connecting_strip(cx).map(IntoElement::into_any_element));
        pinned.extend(self.queued_strip(cx).map(IntoElement::into_any_element));
        pinned
    }

    /// Shown while the adapter is still coming up.
    ///
    /// **A resumed conversation is on screen before it is live.** The archive
    /// is adopted the moment one is picked, deliberately -- blanking the pane
    /// for the seconds an adapter takes to spawn would be worse. But that
    /// leaves a transcript, a header and a composer that all look ready while
    /// nothing can be sent yet, and the only thing that said so was a Send
    /// button that refused once pressed.
    ///
    /// Over the composer rather than in the transcript, because it is a fact
    /// about the *session* and not a thing the conversation said -- and because
    /// this is the corner the user is looking at when they go to type.
    ///
    /// Only a **re**connect ever sees this. A conversation coming up for the
    /// first time draws nothing but the wait, so there is no composer for a
    /// strip to sit over; what is left here is the case where the transcript
    /// stays -- a restart, or an adapter respawned after it died.
    fn connecting_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let chat = self.active_chat(cx)?;
        if chat.link != Link::Connecting {
            return None;
        }
        let status = SharedString::from(chat.activity_status()?);
        Some(
            transcript::floating_card(cx)
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new().xsmall())
                .child(status),
        )
    }

    /// What is waiting for this turn to end, and the way to take it back.
    ///
    /// A prompt that left the composer and is not in the transcript is a prompt
    /// nothing on screen accounts for -- which is indistinguishable from one
    /// the app dropped.
    fn queued_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let queued = self.active_chat(cx)?.queued.as_ref()?;
        let line = SharedString::from(onehand_core::chat::first_line_trunc(&queued.text, 80));
        let count = queued.attachments.len();
        Some(
            transcript::floating_card(cx)
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .text_sm()
                .child(Icon::new(IconName::Calendar).size_3())
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground)
                        .child("Queued"),
                )
                .child(div().flex_1().min_w_0().truncate().child(line))
                .children((count > 0).then(|| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(match count {
                            1 => "1 attachment".to_string(),
                            n => format!("{n} attachments"),
                        })
                }))
                .child(
                    crate::controls::action("unqueue")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Close))
                        .tooltip("Put it back in the composer")
                        .on_click(cx.listener(|pane: &mut Self, _, window, cx| {
                            pane.unqueue(window, cx);
                        })),
                ),
        )
    }
}

/// The branch line, as the control it is.
///
/// **A menu and not a label**, because everything the reader might do about
/// what it says is a thing the shell already does: split this branch into a
/// second checkout, rename it, or go and look again. Printed flat, the strip's
/// one piece of project state was the one piece with no way to act on it, and
/// both of those actions were reachable only from a rail row or a page that is
/// not on screen while a conversation is.
///
/// Built by the pane rather than by the composer, which draws the rest of the
/// strip: git is the project's and this panel is what talks to the shell about
/// the project. The composer has no vocabulary for any of it.
///
/// Drawn to match the two setting chips beside it — same height, same inset,
/// same muted ink, a mark then a word and no caret — so the strip stays one row
/// of one kind of thing. It is the same reason the line was never a sentence in
/// prose: what differs is which side of the row it is on.
pub(super) fn branch_control(
    line: SharedString,
    pane: Entity<ChatPane>,
    cx: &mut Context<ChatPane>,
) -> impl IntoElement + use<> {
    let act = |action: ProjectAction, pane: Entity<ChatPane>| {
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            pane.update(cx, |_: &mut ChatPane, cx| {
                cx.emit(ChatPaneEvent::Project(action));
            });
        }
    };
    let (worktree, rename, refresh) = (pane.clone(), pane.clone(), pane);

    crate::controls::action("branch")
        .ghost()
        .xsmall()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_shrink_1()
        .min_w_0()
        .h(crate::chat::composer::CHIP_H)
        .px_1p5()
        .rounded(cx.theme().radius)
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(crate::icons::Icon::GitBranch).size_3())
        // The branch leads the line, so what the cap takes first is the change
        // count behind it.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(crate::chat::composer::CHIP_TEXT)
                // Full strength, as every chip's value is: the muted ink on the
                // button is what the mark beside this takes. A branch written a
                // shade fainter than the setting at the other end of the strip
                // reads as less certain rather than as a different kind of
                // thing.
                .text_color(cx.theme().foreground)
                .child(line),
        )
        .tooltip("The branch checked out here")
        .dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| {
            menu.item(
                crate::controls::menu_item("Rename branch…")
                    .icon(Icon::new(crate::icons::Icon::SquarePen))
                    .on_click(act(ProjectAction::RenameBranch, rename.clone())),
            )
            .item(
                crate::controls::menu_item("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(act(ProjectAction::Worktree, worktree.clone())),
            )
            .separator()
            .item(
                crate::controls::menu_item("Refresh Git status")
                    .icon(Icon::new(IconName::Redo))
                    .on_click(act(ProjectAction::RefreshGit, refresh.clone())),
            )
        })
}

impl ChatPane {
    pub(super) fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Cleared here and set only on the one path that mounts a composer, so
        // every early return below leaves it false without having to say so.
        self.composer_drawn = false;
        match &self.page {
            Some(super::Page::Workspace(_)) => return self.workspace_page(cx),
            Some(super::Page::Tasks(_)) => return self.tasks_page(cx),
            Some(super::Page::Issues(view)) => return self.issues_page(view.clone(), cx),
            None => {}
        }
        // A session choosing which conversation to resume has no transcript and
        // no composer yet: nothing is connected until the choice is made.
        if let Some(uid) = self.active.filter(|uid| self.is_choosing(*uid)) {
            return self.resume_picker(uid, cx).into_any_element();
        }
        // Opening is a wait, not an absence: the scan for past conversations is
        // running, or a restart has just dropped one adapter and is a line away
        // from spawning the next. The hint for "no session here" told the user
        // to start one they had already started.
        if self.active.is_some_and(|uid| self.is_opening(uid)) {
            return waiting_hint("Opening the session…".into(), cx).into_any_element();
        }
        let Some(session) = self.session() else {
            return self.project_home(cx);
        };
        // Copied out rather than borrowed, so recording the connect and drawing
        // the header below are not held up by a borrow of the conversation.
        let Some((link, status)) = self
            .active_chat(cx)
            .map(|chat| (chat.link, chat.activity_status()))
        else {
            return self.project_home(cx);
        };
        // Set the first time an adapter is actually up and never unset: it is
        // what tells a first connect from a reconnect.
        if link == Link::Connected
            && let Some(conv) = self.active_conversation_mut()
        {
            conv.was_live = true;
        }
        // A first connect draws nothing but the wait.
        //
        // A resumed conversation is adopted from its archive the moment it is
        // picked, so otherwise the whole of it is on screen -- transcript,
        // header, composer -- seconds before a word can be sent to it, with
        // nothing but a refused Send to say so. A *re*connect is the opposite
        // case, which is what the flag is for: on a restart the conversation is
        // already being read, and taking it away for the seconds a spawn costs
        // reads as data loss.
        //
        // The header stays. It names the conversation being opened, and a pane
        // that drops its own chrome while it waits reads as one that lost it.
        if waits_alone(
            link,
            self.active_conversation().is_some_and(|conv| conv.was_live),
        ) {
            let waiting = status.unwrap_or_else(|| "Connecting…".to_string());
            return div()
                .size_full()
                .v_flex()
                .child(self.header(cx))
                .child(waiting_hint(waiting.into(), cx))
                .into_any_element();
        }
        let Some(chat) = self.active_chat(cx) else {
            return self.project_home(cx);
        };

        // Asked of the conversation, on the composer's current contents, so
        // Send can refuse out loud instead of swallowing the press. Computed
        // here because this is the last point both are borrowed at once.
        let blocked = {
            let composer = self.composer.read(cx);
            chat.submit_blocker(&composer.text(cx), &composer.attachments)
        };
        // Measured last frame. Read once, and turned into one number, because
        // it is the line three separate things rest on -- the last row of the
        // transcript, the jump-to-the-latest pill, and the point a question
        // held at the top of the panel stops being held -- and two of them read
        // a frame apart is the pill floating off the conversation's floor.
        let measure = self.composer_h.clone();
        let measured = self.composer_h.get();
        let minimum = COMPOSER_MIN_H.to_pixels(window.rem_size());
        let overlay_h = if measured > minimum {
            measured
        } else {
            minimum
        };
        let floor = overlay_h + COMPOSER_REST.to_pixels(window.rem_size());
        // The two edges a newly asked question is held between: the list's own
        // top padding below, which is where it comes to rest, and the composer
        // above whatever is left of the panel.
        let room = viewport::TopRoom {
            head: LIST_HEAD.to_pixels(window.rem_size()),
            floor,
        };

        // History and live items are two collections, and a fold or a
        // permission answer has to reach the right one -- hence the typed id
        // rather than a render position.
        let Some(list_state) = self.reproject(room, cx) else {
            return self.project_home(cx);
        };
        // Asked after the layout, because holding a question at the top is what
        // decides both: the room under the last run, and whether being parked
        // above the tail is news worth a control to undo. It is not -- the
        // reader did not scroll anywhere, the transcript came to them, and the
        // answer they are waiting for is arriving in the space below.
        let holding = self
            .active_conversation()
            .is_some_and(|conv| conv.viewport.holding());
        // **Where the transcript stops being drawn: the composer's own middle.**
        // The overlay is transparent around its surfaces, so a row scrolling
        // under it stayed visible in the strip above the card, at both sides of
        // it and under it -- a line of the conversation cut in two by a box
        // resting on top of it, which reads as the card having been dropped on
        // the text rather than as the text ending. Clipped here it ends behind
        // the card's opaque top half, so nothing is ever seen sliced: the cut
        // itself is under a surface. Half the overlay rather than half the card
        // measured separately, which lands inside that half for every composer
        // taller than its own status row plus its inset -- and the resting
        // composer is four times that.
        let cut = overlay_h / 2.;
        let tail_room = self
            .active_conversation()
            .map_or((floor - cut).max(px(0.)), |conv| {
                conv.viewport.tail_room(floor, cut)
            });
        let this = cx.entity();
        let for_render = session.clone();
        let scrolled_up = away_from_tail(&list_state) && !holding;
        // The clip is already taken off inside `tail_room`, which is the only
        // place that knows whether the number it returned was measured from the
        // clipped viewport or is a constant that never heard of it.
        let tail_pad = tail_room;
        // How much room a composer popup has to open into: the well, less what
        // the composer and its rest already stand in.
        //
        // **The list's own viewport is the measurement**, because the list is
        // inside that well -- so the number is there for the asking and a second
        // canvas measuring the same box would be a second answer to keep in
        // step. It is last frame's, which is the frame the popup was opened
        // from. The clip is added back because the popup opens into the *well*
        // and the well is what the list stops short of.
        let popup_room = crate::chat::composer::popup_room(
            list_state.viewport_bounds().size.height + cut,
            floor,
            window.rem_size(),
        );
        // What a block inside the conversation bounds itself against, read here
        // and handed down rather than asked for where it is used.
        //
        // **This is the last moment it can be asked.** The list borrows its own
        // state mutably for as long as it is building rows, so a row reaching
        // back to ask how tall the viewport is panics -- and it is the rows of
        // *this* list that want the answer. Read before the list starts, one
        // value serves every row and the pinned cards above them alike.
        let well = (list_state.viewport_bounds().size.height > px(0.))
            .then(|| list_state.viewport_bounds().size.height);
        // Read off the same frame and for the same reason: how wide the panel
        // is decides what a block may spend on margins and on columns that are
        // not the one thing it has to say. A width of zero is the frame before
        // the list has measured itself, which is not a narrow panel.
        let list_w = list_state.viewport_bounds().size.width;
        let narrow = list_w > px(0.) && list_w < NARROW_PANEL.to_pixels(window.rem_size());
        let room = transcript::Room::new(well, narrow);
        self.composer_drawn = true;

        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .children(self.workflow_strip(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        // The list runs to the well's top edge, so it clips
                        // exactly at the header's rule rather than at an inset
                        // below it -- a band of blank surface above a line of
                        // text sliced in half reads as a rendering fault, not
                        // as a margin. At the bottom it stops at the cut, and
                        // that edge is under the composer where no slice shows.
                        // Its breathing room is *inside* the scroll: padding on
                        // the list is part of what scrolls, which is how the
                        // transcript comes to rest above the composer rather
                        // than merely disappearing behind it.
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(cut)
                            .overflow_hidden()
                            .child(
                                list(list_state, move |ix, window: &mut Window, cx: &mut App| {
                                    this.read(cx).run_element(
                                        ix,
                                        &for_render,
                                        room.clone(),
                                        window,
                                        cx,
                                    )
                                })
                                .size_full()
                                .pt(LIST_HEAD)
                                .pb(tail_pad),
                            ),
                    )
                    // The transcript dissolving into the surface it is drawn
                    // on, right down to the clip. Between the list and every
                    // control, so what fades is the conversation alone: the
                    // jump pill, the pinned cards and the composer are all
                    // drawn after this and each carries its own opaque
                    // surface. It ends *at the clip* rather than at the top of
                    // the composer, because the composer's card is narrower
                    // than the panel -- a fade stopping at the card's own edge
                    // would leave the strips either side of it showing full
                    // strength text for the height of the card.
                    .child(div().absolute().bottom(cut).left_0().right_0().h(SMOKE).bg(
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(cx.theme().background.alpha(0.), 0.),
                            // Solid a little before the end, so the last of
                            // the text is gone by the time the clip takes
                            // it rather than exactly as it does.
                            gpui::linear_color_stop(cx.theme().background, 0.9),
                        ),
                    ))
                    // Over the transcript rather than in a row of its own: a
                    // control that appears and disappears cannot own layout, or
                    // the whole conversation shifts by its height every time the
                    // reader scrolls up and back down.
                    .when(scrolled_up, |well| {
                        well.child(
                            div()
                                .absolute()
                                // **Measured from the composer's own top edge,
                                // not from where the transcript comes to rest.**
                                // It was placed against that resting line, which
                                // is deliberately the widest space in the
                                // conversation -- so the control floated most of
                                // an inch clear of the thing it belongs beside.
                                // Held off by a block's gap and no more: two
                                // floating surfaces touching read as one surface
                                // with a notch taken out of it.
                                .bottom(overlay_h + BLOCK_GAP.to_pixels(window.rem_size()))
                                .left_0()
                                .right_0()
                                .h_flex()
                                .justify_center()
                                .child(
                                    // The outline button's own fill is partly
                                    // transparent. Give the floating control an
                                    // opaque raised surface so transcript text
                                    // scrolling beneath cannot show through it.
                                    div()
                                        .rounded_full()
                                        .bg(cx.theme().popover.alpha(1.))
                                        .shadow_lg()
                                        .child(
                                            crate::controls::action("to-bottom")
                                                .outline()
                                                .rounded_full()
                                                // **The arrow alone.** The
                                                // words named what was down
                                                // there, which the transcript
                                                // itself says the moment the
                                                // control is used -- and a
                                                // label on a thing floating
                                                // over the conversation is a
                                                // sentence competing with the
                                                // one being read. What it
                                                // means is in the tooltip,
                                                // where a control that needs
                                                // explaining keeps it.
                                                .size(JUMP_PILL_H)
                                                .icon(Icon::new(IconName::ChevronDown))
                                                .tooltip("Jump to the latest activity")
                                                .on_click(cx.listener(
                                                    |pane: &mut Self, _, _, cx| {
                                                        pane.jump_to_latest(cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    })
                    .child(self.overlay(
                        &session,
                        measure,
                        OverlayRoom {
                            popup: popup_room,
                            well,
                        },
                        blocked,
                        window,
                        cx,
                    )),
            )
            .into_any_element()
    }
}

/// Shown while no session is on screen.
///
/// Named by the project it would start in. "Pick a session in the rail" was a
/// wrong instruction in the state it appeared in most: a project with no
/// sessions has none to pick, which is what every freshly added project looks
/// like, so the window's centre was telling the user to do something the rail
/// gave them no way to do.
fn waiting_hint(what: SharedString, cx: &App) -> impl IntoElement + use<> {
    div()
        .size_full()
        .flex_1()
        .min_h_0()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(cx.theme().muted_foreground)
        .child(Spinner::new().small())
        .child(what)
}

/// Whether the reader is parked above the conversation's tail.
///
/// **Not `is_scrolled_to_end`**, which is the obvious answer and the wrong one:
/// it needs the height of every run, and a list that measures rows lazily has
/// no reason to know most of them. On any transcript long enough for the
/// question to matter it returned "don't know", which read as "not scrolled
/// away" — so the way back to the latest appeared on short conversations and
/// went missing on exactly the long ones it exists for.
///
/// Follow-tail state answers it without measuring anything: the list stops
/// following the moment the reader scrolls up and starts again when they come
/// back down. The second half of the test is for the transcript that fits on
/// screen — a wheel event there stops following without moving anything, and a
/// conversation with no bottom to be away from must not offer a way back to it.
pub(super) fn away_from_tail(list: &ListState) -> bool {
    !list.is_following_tail() && list.logical_scroll_top().item_ix < list.item_count()
}
