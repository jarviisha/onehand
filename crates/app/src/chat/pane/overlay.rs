//! The composer's overlay: the card that floats over the transcript, with
//! whatever is pinned above it and the popups that open from it.

use super::body::branch_control;
use super::{COMPOSER_COLUMN, ChatPane, OverlayRoom};
use crate::chat::session::ChatSession;
use crate::chat::transcript::{self};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, Styled, Window, div,
};
use gpui_component::StyledExt;

impl ChatPane {
    /// The composer and everything stacked on it, floating over the transcript.
    ///
    /// **A real overlay.** It takes no height out of the conversation, so the
    /// transcript never jumps when the field grows. Only the interactive cards
    /// are opaque; the full-width wrapper stays transparent around the shared
    /// reading column. The measured list padding still lets the final row rest
    /// above the card instead of becoming unreachable behind it.
    ///
    /// The popups belong here for the same reason, but *outside* the box that
    /// is measured: they are transient chrome that may cover the conversation
    /// and must not move it.
    pub(super) fn overlay(
        &mut self,
        session: &Entity<ChatSession>,
        measure: std::rc::Rc<std::cell::Cell<gpui::Pixels>>,
        room: OverlayRoom,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let pinned = self.pinned(session, room.well, window, cx);
        let git = self
            .git
            .clone()
            .map(|line| branch_control(line, cx.entity(), cx).into_any_element());
        let pane = cx.entity();
        // The field draws no ring of its own once the card is its border, so
        // the card has to answer "does typing go here" -- with an app keymap
        // that reaches over the terminal and a rail that can take focus, an
        // input with no focused state is one the user has to test by typing.
        //
        // Asked here rather than handed in: it is a question about a window,
        // and this is the innermost place holding one.
        let typing_here = self
            .composer
            .read(cx)
            .state
            .focus_handle(cx)
            .contains_focused(window, cx);

        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .v_flex()
            // A step over the gap the cards keep between themselves: a pinned
            // card and the composer are two objects and the cards in a stack
            // are one, so the seam that has to read as a seam is this one.
            .gap_2p5()
            .w_full()
            // Reading the conversation is a way of saying the popup is done
            // with. It hangs off this whole block rather than off the list, so
            // a click on a chip, a row or the field -- every one of which is a
            // click *outside* the list -- still reaches the control it was
            // aimed at.
            .on_mouse_down_out(cx.listener(|pane: &mut Self, _, _, cx| {
                if pane.composer.read(cx).overlay_open() {
                    pane.composer
                        .update(cx, |composer, cx| composer.close_overlay(cx));
                }
            }))
            // The popup sits *above* the input, so a long candidate list grows
            // away from the text being typed rather than over it -- and it sits
            // outside the measured box below, which is the whole point.
            //
            // Measured, the transcript's bottom padding would grow by the
            // popup's height the moment one opened and shrink again when it
            // closed, so every `@` typed shoved the conversation up and every
            // completion dropped it back. The popup is transient chrome; it may
            // cover the transcript, but it must not move it.
            // **The popup and the pinned cards share one slot, and the popup
            // is drawn over them.** Stacked in the column instead, the two were
            // additive: a list at its full height plus a permission card
            // carrying a long command plus the composer could outgrow the
            // panel, and since the column is anchored at the bottom and grows
            // up, what ran off the top was the popup — header, first rows and
            // all, unreachable because the list scrolls inside itself. Sharing
            // a slot makes the cost `max` rather than `sum`, so the list keeps
            // its full height and nothing is pushed anywhere.
            //
            // The cards are what is in flow, so the slot is as tall as they
            // are and the transcript's clearance is unchanged. The popup is
            // absolute and anchored to the slot's bottom edge: with no card it
            // sits exactly where it always did, directly above the composer,
            // and with one it covers it and carries on upward.
            //
            // **The popup is the later child on purpose.** Paint order is what
            // puts it over the card rather than under, and it is also what
            // gives it the click: a list of choices opened over a card is the
            // thing being aimed at.
            //
            // What this costs is that a card already on screen is hidden while
            // a popup is open over it. That is the user's own doing — they
            // opened the picker and can see they did. A card that *arrives*
            // while one is open is the case that would be silent, and that is
            // answered at the event instead: parking an ask closes the popup.
            .children({
                let popup = self
                    .composer
                    .update(cx, |composer, cx| {
                        composer.detached_popup(session, room.popup, window.rem_size(), cx)
                    })
                    // **Every overlay is the same card, in the same place.** The
                    // option lists used to hang off the chip that opened them,
                    // on the reasoning that keeping a compact surface against
                    // its trigger says which control it belongs to. What it
                    // cost is the thing a list of choices is for: sized to its
                    // own rows and pinned to one end of the card, a model list
                    // had no room for the sentence the agent sends about each
                    // choice, and the rows it did fit were narrower than the
                    // words in them. The card above the composer is the width
                    // of the reading column, which is what every choice here
                    // needs -- and the chip stays lit underneath for as long as
                    // its list is open, which is what actually says where the
                    // list came from.
                    .map(|popup| {
                        div()
                            .w_full()
                            .px_4()
                            // **Lifted off whatever is under it, and only when
                            // something is.** Flush, the popup and a parked
                            // card have the same width, nearly the same
                            // surface and a shared edge, so the two read as one
                            // tall panel. The usual cue for "this is above
                            // that" is a drop shadow, and it is not available
                            // here: over the dark palette's near-black it is
                            // invisible, which is why that palette needs a real
                            // step for a floating control in the first place.
                            // The gap leaves the card's bottom edge and border
                            // showing, and two horizontal edges a few pixels
                            // apart is a stack where one is a panel.
                            //
                            // Left as a rem. Resolved against
                            // `window.rem_size()` it would be the one length in
                            // this stack measured from the *window's* base — and
                            // a panel's zoom overrides the rem base for its own
                            // subtree, so the peek would be the only part of it
                            // that did not grow with the text beside it.
                            .when(!pinned.is_empty(), |popup| {
                                popup.mb(crate::chat::composer::POPUP_STACK_PEEK)
                            })
                            .child(div().w_full().max_w(COMPOSER_COLUMN).mx_auto().child(popup))
                    });
                // **Outside the measured box, for the reason the popup is.** A
                // parked card is a surface over the conversation, not a floor
                // under it: measured, every card that arrives grows the
                // transcript's bottom padding, and a list anchored at its tail
                // answers that by shifting everything the user was reading
                // upward -- at the exact moment their attention is being asked
                // for, by a card that appeared because the agent chose to park
                // and not because anybody pressed anything. What it costs is
                // the last row or two of the conversation sitting behind the
                // card while it is up, which the reader can scroll to and which
                // comes back the moment the card is answered. The composer
                // stays measured: it is there the whole time, and a transcript
                // that ended behind the box being typed in would hide its own
                // last line permanently.
                let cards = (!pinned.is_empty()).then(|| {
                    div()
                        .w_full()
                        .px_4()
                        // **A card covering the conversation must not move it.**
                        // The same leak the popup above has: gpui's handler for
                        // a scrolling box adjusts its own offset and never
                        // claims the event, so a wheel over a parked card went
                        // on to the transcript underneath and scrolled the very
                        // rows the card is sitting on top of. Claimed on the
                        // wrapper, so the card's own wells still take what they
                        // can use first — bubble order runs the deeper listener
                        // before this one.
                        //
                        // The composer is deliberately not given this. It is
                        // the one surface down here the transcript *clears*
                        // rather than hides behind, so there is nothing under
                        // it being moved out of sight.
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .w_full()
                                .max_w(COMPOSER_COLUMN)
                                .mx_auto()
                                // The composer's column, because while a card is
                                // pinned it is part of that stack: these boxes sit
                                // directly on the card, share its surface and its
                                // radius, and are read as one object with it. A card
                                // an inch wider than the box it rests on reads as
                                // two panels that failed to line up.
                                //
                                // **Width follows where a card is, not what it is.**
                                // Answered, it is drawn in the transcript and takes
                                // the transcript's column like every block around
                                // it. That was already half true -- a transcript row
                                // is inset inside the reading column while a pinned
                                // card was not -- so the rule that said the two must
                                // match was describing something the layout had
                                // never quite done.
                                //
                                // The text size is still the transcript's, which is
                                // the part that does have to hold: a question
                                // re-read in the history has to be the same words at
                                // the same weight as the question that stopped
                                // everything.
                                .text_size(transcript::TEXT)
                                .children(pinned),
                        )
                });
                // Nothing at all rather than an empty box, so the column's own
                // gap is not spent on a slot with no height and the composer
                // does not drift down whenever neither is showing.
                // **The popup is the one in flow and the card is the one taken
                // out of it**, which is the opposite of the obvious way round
                // and the only way round that works.
                //
                // Absolute, the popup contributed no height, so this block's
                // own bounds ended below it — and the handler that closes a
                // popup when the mouse goes down *outside* this block compares
                // against exactly those bounds, in the capture phase. Every
                // click on a row was therefore a click outside, and the list
                // closed before the press could reach it: the keyboard picked
                // rows and the mouse could not.
                //
                // In flow the popup sets the height and the card hangs off the
                // bottom of it, behind it, which is where it was drawn anyway.
                // The card is the later thing to lose its height, and it can
                // afford to: nothing measures a parked card by design.
                match (popup, cards) {
                    (Some(popup), Some(cards)) => Some(
                        div()
                            .relative()
                            .w_full()
                            .child(div().absolute().bottom_0().left_0().right_0().child(cards))
                            .child(popup),
                    ),
                    (Some(popup), None) => Some(div().w_full().child(popup)),
                    (None, Some(cards)) => Some(div().w_full().child(cards)),
                    (None, None) => None,
                }
            })
            .child(
                // What the transcript has to clear: the pinned cards and the
                // composer, and the transparent space under them.
                div()
                    .relative()
                    .w_full()
                    // Measures this whole box, padding included, which is why
                    // the padding sits on the child rather than here: an
                    // absolutely positioned `size_full` resolves against the
                    // padding box, so a padded parent would report itself short
                    // by exactly the margin the transcript most needs to clear.
                    .child(
                        gpui::canvas(
                            move |bounds, _, cx| {
                                let height = bounds.size.height;
                                if measure.replace(height) != height {
                                    // Prepaint has finished rendering the
                                    // entity, so defer the notification instead
                                    // of updating it re-entrantly from its own
                                    // element tree.
                                    cx.defer(move |cx| {
                                        pane.update(cx, |_: &mut Self, cx| cx.notify());
                                    });
                                }
                            },
                            |_, _: (), _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .px_4()
                            // Transparent spacing around the cards is what makes
                            // this read as an overlay rather than a footer. The
                            // transcript keeps painting through it; only the
                            // surfaces below cover what sits directly behind
                            // them.
                            .pb_4()
                            .child(
                                div()
                                    .v_flex()
                                    .w_full()
                                    .max_w(COMPOSER_COLUMN)
                                    .mx_auto()
                                    .child(self.composer.update(cx, |composer, cx| {
                                        composer.card(session, blocked, typing_here, cx)
                                    }))
                                    // Under the card and inside the measured
                                    // box, so the transcript ends above the
                                    // strip rather than behind it: the height
                                    // the conversation clears is whatever this
                                    // whole overlay comes to, and the strip
                                    // appears and disappears with the project.
                                    .children(self.composer.update(cx, |composer, cx| {
                                        composer.status_row(session, git, cx)
                                    })),
                            ),
                    ),
            )
    }
}
