use super::complete::picker_rows;
use super::presentation::{Row, segmented_group};
use super::rows::{candidate_row, choice_row};
use super::{CHIP_H, Composer, Overlay, highlight};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::scroll::{Scrollbar, ScrollbarMode};
use gpui_component::{ActiveTheme, Selectable as _, Sizable as _, StyledExt};
use onehand_core::completion::TriggerKind;

/// The least room a list is ever given, however short the panel is.
///
/// Rems, like every other size here: a panel's zoom overrides the rem base for
/// its whole subtree, so a popup measured in pixels is the one thing on screen
/// that does not grow with the text it is completing.
pub(super) const POPUP_MIN_H: Rems = rems(9.);
/// What a list leaves between itself and the top of the panel.
///
/// A card that grows until it touches the header's rule reads as one that has
/// run out of window rather than as one sized to its contents, and there is
/// nothing above it to say whether anything was cut.
const POPUP_HEADROOM: Rems = rems(1.);
/// How many choices a list shows before the rest are behind a scroll.
///
/// **A reach rather than a fit.** The panel bound below answers a different
/// question — where the window ends — and on a maximized one it answers it
/// thirty rows up, which is a column of candidates taller than the reader's
/// span of attention and anchored at the end furthest from the text being
/// typed. Past about this many the list stops being something taken in at a
/// glance and becomes something searched, and the query in the field is the
/// better tool for that than the eye is.
///
/// Twelve, and the scroll for the rest. Eight was the floor of that reasoning
/// and turned out to be under it in the case the list was built for: the `@`
/// list is three groups deep, and each one spends a row on its own heading, so
/// eight rows of box held five or six actual candidates spread across two
/// headings — a list that scrolled almost as soon as it opened, which is the
/// one thing a short list is supposed to avoid.
pub(super) const POPUP_MAX_ROWS: f32 = 12.;
/// How much of the card underneath a popup is left showing.
///
/// **The one cue that works in both palettes.** A popup drawn over a parked
/// card has the card's exact width and very nearly its surface, so with their
/// edges flush the two read as a single tall panel rather than as one thing
/// resting on another. The usual answer to that is a drop shadow, and here it
/// is not available: over a near-black surface a shadow is invisible, which is
/// the same reason the dark palette needs a real step for a floating control
/// rather than an elevation cue.
///
/// So the popup is lifted instead, and the card's bottom edge and border stay
/// visible under it. Two horizontal edges a few pixels apart is a stack;
/// one is a panel.
///
/// **Only when there is something to show.** With nothing pinned the popup
/// sits where it always did, hard against the composer's own gap — a lift
/// there would be a space with nothing in it, and the width and position of
/// this card must not drift for reasons the reader cannot see.
pub(in crate::chat) const POPUP_STACK_PEEK: Rems = rems(0.375);
/// What the popup spends on itself, outside the box that scrolls.
///
/// **An over-estimate on purpose, and that is the whole design.** This was one
/// hand-measured number subtracted from the surface before flooring it, which
/// is the same mistake it was written to fix: nothing tied it to the elements
/// it claimed to measure, the test asserted arithmetic against the constant
/// itself so drift passed, and it was silently wrong for the three popups that
/// draw no footer at all.
///
/// The fold is no longer this number's problem. The bound that has to come out
/// a whole number of rows is on the **scrolling box**, so the cut lands between
/// two rows however tall the chrome turns out to be. All this decides is how
/// much room the box is offered — and being generous there costs at most a row
/// of the twelve, while being short by a pixel used to cost the thing the
/// flooring was for.
///
/// The parts, each named for the call it mirrors: the pinned header
/// (`CHIP_H` + `pb_1` + `mb_1`), the footer where one is drawn (`CHIP_H` +
/// `pt_2` + `mt_1`), the segment rail where one is (`py_3` around a row), and
/// the surface's own `p_1` top and bottom.
const POPUP_HEADER_H: Rems = rems(2.);
const POPUP_FOOTER_H: Rems = rems(2.25);
const POPUP_RAIL_H: Rems = rems(3.);
const POPUP_INSET_H: Rems = rems(0.5);

/// How tall a list may grow, given the panel it is opening inside.
///
/// **A cap and not a size.** A list shorter than this draws whole and does not
/// scroll, which is the point: a popup that scrolled with four choices in it
/// hid the fourth behind a gesture nobody needed to make. What the cap is for
/// is the other end — a list must not grow past the panel, where its top rows
/// would be drawn over the header or off the window entirely, with nothing on
/// screen saying so.
///
/// **Two bounds, and the smaller wins.** The panel's own height is one, and it
/// is not a constant because a constant is a guess about a panel that is
/// dragged: fifteen rems was most of a short pane and a third of a tall one, so
/// the same list scrolled on a maximized window with room to spare beneath it.
/// [`POPUP_MAX_ROWS`] is the other, and it is a constant precisely because it
/// is *not* about the window — it is about how many rows are worth reading
/// before the query does the narrowing instead. Without it a maximized window
/// draws every one of the fifty rows the list is capped at building.
///
/// The footer rides inside this: the two sentences under the list are held out
/// of the scroll, so a bound measured in rows alone would be over by their
/// height every time one appeared.
///
/// The floor is what is left when the panel is too short for any of that: a
/// squeezed pane scrolls its list, which is honest, but it is never reduced to
/// one row and a scrollbar.
pub fn popup_room(panel: gpui::Pixels, reserved: gpui::Pixels, rem: gpui::Pixels) -> gpui::Pixels {
    let row = POPUP_ROW_H.to_pixels(rem);
    let chrome = popup_chrome(true, true).to_pixels(rem);
    let rows = row * POPUP_MAX_ROWS + chrome;
    (panel - reserved - POPUP_HEADROOM.to_pixels(rem))
        .min(rows)
        .max(POPUP_MIN_H.to_pixels(rem))
}

/// Everything the popup draws above and below its scrolling box.
///
/// Both halves are asked for by name rather than inferred, because only the
/// caller knows which of them it is about to draw: the footer belongs to a
/// completion and the rail to the one config group promoted out of the list.
pub(in crate::chat) fn popup_chrome(footer: bool, rail: bool) -> Rems {
    let mut h = POPUP_HEADER_H.0 + POPUP_INSET_H.0;
    if footer {
        h += POPUP_FOOTER_H.0;
    }
    if rail {
        h += POPUP_RAIL_H.0;
    }
    rems(h)
}

/// The inset every popup surface pads its contents by.
pub(super) const POPUP_INSET: Rems = rems(0.25);

/// A popup's scrolling list, with its scrollbar on the popup's own right edge.
///
/// The frame reaches back through the surface's inset, so the thumb runs down
/// the popup's border rather than over the right end of a row -- where it sat
/// on the highlight fill and read as part of the row under it. The parked
/// question card draws its thumb the same way, and two scrolling cards stacked
/// one over the other must not disagree about where a scrollbar goes.
///
/// **The rows give way only while there is a thumb.** A list that fits draws
/// none, so it keeps the inset alone and its rows end where the header's and
/// footer's text does; one that scrolls is held clear of the thumb's lane.
/// Read off last frame's layout, so a list that has just started to overflow
/// moves its right edge one frame late, which nobody can see.
///
/// `list` keeps its own bound and tracks `scroll` itself; this only adds the
/// frame and the thumb.
pub(super) fn edge_scrolled(
    scroll: &gpui::ScrollHandle,
    list: gpui::Stateful<gpui::Div>,
) -> gpui::Div {
    let scrolls = scroll.max_offset().y > gpui::px(0.);
    div()
        .relative()
        .v_flex()
        .min_h_0()
        .mr(rems(-POPUP_INSET.0))
        .child(list.pr(match scrolls {
            true => THUMB_LANE,
            false => POPUP_INSET,
        }))
        .child(Scrollbar::vertical(scroll).mode(ScrollbarMode::Always))
}

/// How far a scrolling list's rows stand off the popup's edge: the thumb and
/// the scrollbar's own inset from the border, with a hair of air after it.
const THUMB_LANE: Rems = rems(0.75);

/// How tall the scrolling box may stand: a whole number of rows, always.
///
/// **The bound is here and not on the surface**, which is the fix for a fold
/// that kept landing mid-row. Floored on the surface, what the reader sees is
/// the surface *less the chrome*, and that remainder is only a whole number of
/// rows if the chrome happens to be — which nothing arranged and no test
/// checked. Floored here it is exact whatever the chrome comes to, so the
/// chrome estimate stops being load-bearing.
pub(in crate::chat) fn popup_list_h(
    room: gpui::Pixels,
    rem: gpui::Pixels,
    chrome: Rems,
) -> gpui::Pixels {
    let row = POPUP_ROW_H.to_pixels(rem);
    let left = room - chrome.to_pixels(rem);
    (left / row).floor().max(1.) * row
}
/// How tall a row in the popup stands.
///
/// **Split from the height of the composer's own controls, which it used to
/// share.** That sharing was right while the popup was a short list of choices
/// opened from a chip: the rows were the chip's own values and standing at the
/// chip's height said so. It stopped being right once the list became something
/// *scanned* — fifty paths, three groups, a second column of prose — because a
/// row you read is not a row you press, and at the control height a full list
/// is a dense block with no space between one line and the next for the eye to
/// find its place again.
///
/// A third more than the height a composer chip takes. The extra is all
/// breathing room: the text is the same size, so what this buys is the gap
/// above and below it, which is the whole of what makes a long column
/// scannable.
///
/// The headings and the footer sentences stay at the control height — they are
/// labels rather than rows, and a heading as tall as the things under it reads
/// as one of them.
pub(in crate::chat) const POPUP_ROW_H: Rems = rems(2.);
/// How the label over a run of rows is lettered, and how tall its line stands.
///
/// A step under the smallest size anything else in this popup is set at. The
/// two values move together on purpose: shrinking the text and leaving the line
/// at a row's height gives the space back to nobody, and shrinking the line
/// without the text crowds a word that is still row-sized.
const GROUP_LABEL_TEXT: Rems = rems(0.6875);
const GROUP_LABEL_H: Rems = rems(1.125);

impl Composer {
    /// Whatever is open, as the card that sits above the composer.
    ///
    /// Every overlay comes through here. It is drawn by the pane and not by
    /// [`Self::card`] because it must sit *outside* the box the transcript's
    /// bottom clearance is measured from: measured, that clearance would grow
    /// by the popup's height the moment one opened and shrink again when it
    /// closed, so every `@` typed would shove the conversation up.
    pub fn detached_popup(
        &mut self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        rem: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        self.popup(session, room, rem, cx)
    }

    /// The open completion, settings, or attachment surface.
    fn popup(
        &mut self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        rem: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let overlay = self.overlay.clone()?;
        if overlay == Overlay::Attachments {
            return Some(self.attachments_popup(room, cx));
        }
        // Set while the rows are built, because that is the only place both
        // halves of the count are in hand.
        let mut capped = 0usize;
        let rows: Vec<Row> = match &overlay {
            Overlay::Completion => {
                let (rows, held) = self.matches(session, cx);
                capped = held;
                rows
            }
            // Attachments returned above, so what is left is one of the three
            // settings lists and `picker_rows` has them all.
            picker => picker_rows(picker, session, cx).unwrap_or_default(),
        };
        // A trigger that matches nothing still has to say so. Vanishing reads
        // as completion being broken, which is the opposite of the truth: the
        // popup is the only thing on screen that ever confirms the `@` or `/`
        // was understood at all. A selector with no choices has nothing to
        let segments = match overlay {
            Overlay::Options => segmented_group(session, cx),
            _ => None,
        };
        // confirm, so that one stays away -- unless the rail below the list is
        // the whole of what it has, which is an agent offering effort and
        // nothing else.
        if rows.is_empty() && segments.is_none() && overlay != Overlay::Completion {
            return None;
        }
        let selected = highlight(self.selected, rows.len());
        // Taken on the frame the popup first draws, and held until it closes:
        // the list never shrinks below the height the reader started reading.
        // Capped on the way in, so a query that opened on fifty matches does
        // not hold a floor taller than the popup is allowed to be.
        // **The candidate source may not have arrived yet.** `@` files are
        // scanned off the UI loop when the session opens, so a mention typed in
        // the first moments of a conversation has an empty pool to measure —
        // and a height taken from that is zero, which is the popup growing from
        // nothing a frame later. Waiting on an empty source it stands at its
        // full height instead, and re-measures once the pool arrives.
        // **Each trigger draws from its own pool, and each arrives late.**
        // Files are scanned off the UI loop when the session opens; commands
        // come from the agent over the wire once it connects. This asked about
        // the file pool alone, so a `/` typed while connecting measured the
        // composer's own five rows, recorded that as the height, and then grew
        // by the agent's entire command list when it landed — the exact fault
        // the guard exists for, on the trigger it did not cover.
        let pending = self.trigger.as_ref().is_some_and(|trigger| {
            let chat = &session.read(cx).chat;
            match trigger.kind {
                // The scan says when it is done. Read off the list being empty
                // instead, a project with nothing to offer was "still looking"
                // for the life of the session.
                TriggerKind::File => !chat.files_scanned,
                // No such signal for the agent's commands, and none is needed:
                // the composer's own rows are always in this list, so it is
                // never empty and the waiting line never shows. What the flag
                // still buys is the height, measured once the agent has spoken.
                TriggerKind::Command => chat.commands.is_empty(),
            }
        });
        // Measured once, on the frame the popup opens, and held until it
        // closes. Guarded rather than written through `get_or_insert`, whose
        // argument is evaluated on every call — and the argument here builds
        // the whole unfiltered list.
        if self.opened_rows.is_none() && !pending {
            self.opened_rows = Some(self.shape(session, cx));
        }
        // **Nothing is reserved while the pool is still arriving.** A full
        // twelve rows used to stand in, on the reasoning that a popup should
        // open at a stable height rather than grow into one. What that bought
        // was a worse motion than the one it prevented: the stand-in is a
        // guess at a number that cannot be known yet, so a four file project
        // opened at twelve rows and collapsed to four the moment the scan
        // landed. A collapse is read as something being taken away; growth
        // out of a line that says it is still working is read as it working.
        //
        // The height is measured when there is something to measure, which is
        // what the recording below already waited for.
        let (floor, label_floor) = self.opened_rows.unwrap_or_default();
        let headings = rows.iter().filter(|row| row.group.is_some()).count();
        // Drawn *inside* the scroll, which is what makes this a floor on the
        // list rather than a height on the popup. A `min_h` on the surface
        // would win over the panel's own bound on a squeezed pane and push the
        // two sentences under the list off the bottom -- and those two are the
        // ones that only ever appear when the list is long, so the bound meant
        // to keep the popup honest would be silencing it.
        let filler = rows.len()..floor;
        let label_filler = headings..label_floor;
        let title = popup_title(&overlay, self.trigger.as_ref().map(|t| t.kind), &rows);
        // A list of one group is now named twice an inch apart — once on the
        // pinned row and again on the first row under it. The heading is the
        // one that goes: what it marks is a boundary between runs, and a list
        // with one run has no boundary in it to mark.
        let mut rows = rows;
        if let Some(first) = rows
            .first_mut()
            .filter(|r| r.group.as_ref() == Some(&title))
        {
            first.group = None;
        }

        Some(
            // The surface and the scrolling list are two boxes, and the inset
            // between them belongs to the *surface*.
            //
            // Padding on the scrolling box is inside the box that scrolls, and
            // `scroll_to_item` aligns a row to the container's outer edge --
            // so walking the list with the arrows scrolled the inset away and
            // pinned the highlighted row against the border, which is exactly
            // the state the inset exists to prevent. Held out here, nothing the
            // list does to its own offset can consume it.
            div()
                .v_flex()
                // **The whole popup is what the panel has to hold**, not the
                // scrolling list alone. With the two sentences below held out
                // of that list, a bound on the list is a bound on part of the
                // box — and the part left over is what would have grown past
                // the top of the panel.
                .max_h(room)
                // **The popup's radius is the card's, not the rows'.** These
                // two are one stack — the list sits directly over the box it
                // completes, in the same width and on the same surface — and
                // two boxes that agree about everything except how their
                // corners are cut read as two things that failed to line up,
                // which is what a mismatched radius looks like from a foot
                // away. The rows keep the smaller radius, so the nesting runs
                // the right way: a rounder box with rounder-still corners
                // inside it is a box with something in it.
                .rounded(cx.theme().radius_lg)
                // **Every list takes the reading column**, which is the width of
                // the card it opens over. A file candidate is a path and always
                // needed it; a choice needs it too, now that a row carries the
                // agent's own sentence about what the choice is for. Sized to
                // its own rows instead, that sentence had nowhere to go and the
                // names it belongs to were narrower than the words in them.
                //
                // No cap, because the box this is dropped into is already the
                // column: a flex child shrinks to its parent before it
                // overflows, so the column is the maximum without this naming
                // it.
                .w_full()
                .border_1()
                // **A hairline is not enough here, and this is the one place
                // that is true.** A floating control is told apart from the
                // transcript by the step its surface takes above it, with the
                // hairline only drawing the corner. But these lists open from a
                // button *inside the composer*, and the composer is floating
                // too -- so the popup lands on a surface of exactly its own
                // colour, the step is zero, and the panel reads as having no
                // background at all rather than as a panel. The edge is the
                // only thing left that can say where one ends, so it is drawn a
                // real step up instead of at hairline strength.
                .border_color(cx.theme().accent)
                // A list that opens over the field it completes is floating, so
                // it takes the floating surface and the shadow that says so.
                .bg(cx.theme().popover.alpha(1.))
                // The app's own, not the component ladder's. Every step of that
                // ladder is black at a tenth of an alpha, which is a cue on a
                // white page and three parts in 255 on this one — a shadow that
                // was drawn the whole time and could not be seen.
                .shadow(crate::theme::lift(cx))
                // **The conversation behind must not move because of this.**
                // gpui's own handler for a scrolling box adjusts its offset and
                // stops there — it never claims the event — so the wheel went
                // on to the transcript's list underneath and both moved at
                // once, one of them for no reason the reader gave. Claimed
                // here, on the surface rather than on the box that scrolls, so
                // the header, the footer and the inset swallow it too: a wheel
                // over any part of a card that is covering the conversation is
                // aimed at the card.
                //
                // The inner list still scrolls. Its own handler is registered
                // after this one and bubble order runs the deeper listener
                // first, so it takes what it can use before this ends the
                // event's travel. And gpui gates both on the pointer actually
                // being over the box, so nothing is swallowed at a distance.
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .p(POPUP_INSET)
                .child(popup_header(title, cx))
                .child(edge_scrolled(
                    &self.rows_scroll,
                    div()
                        .id("completion")
                        .v_flex()
                        .w_full()
                        // The bound is on the surface now, because what must
                        // not outgrow the panel is the popup and not the list
                        // inside it. `min_h_0` is what lets this shrink to
                        // whatever the surface has left after the sentences
                        // below: a flex child's floor is otherwise its own
                        // content, so the box would push them off the bottom
                        // instead of scrolling.
                        .min_h_0()
                        // A whole number of rows, computed from what this
                        // popup is actually about to draw around itself rather
                        // than from one constant standing for every popup.
                        .max_h(popup_list_h(
                            room,
                            rem,
                            popup_chrome(overlay == Overlay::Completion, segments.is_some()),
                        ))
                        .overflow_y_scroll()
                        // Held by the composer rather than by the element, so walking
                        // the list with the keys can scroll it: the handle is what
                        // `reveal_selected` reaches the rows through, and an element's
                        // own handle is gone by the time a key arrives.
                        .track_scroll(&self.rows_scroll)
                        .children(rows.into_iter().enumerate().map(|(i, row)| {
                            let session = session.clone();
                            let pick = row.pick.clone();
                            let heading = row.group.clone();
                            // A click is a choice already made, so it takes the
                            // row rather than only pointing at it. The highlight
                            // moves first, so what was clicked is what gets
                            // taken and not whatever the keyboard had left
                            // selected.
                            let take = cx.listener(move |composer: &mut Self, _, window, cx| {
                                composer.select(i, cx);
                                composer.apply_pick(&pick, &session, window, cx);
                            });
                            let highlighted = Some(i) == selected;
                            let body = match overlay == Overlay::Completion {
                                true => candidate_row(i, row, highlighted, cx),
                                false => choice_row(i, row, highlighted, cx),
                            }
                            // **The pointer moves the highlight, exactly as the
                            // arrows do.** One fill means one thing -- the row
                            // about to be taken -- so it cannot be left behind
                            // on the row the keyboard last stood on while the
                            // pointer is somewhere else. It also settles what
                            // `Enter` takes when the mouse has moved since:
                            // what is lit.
                            //
                            // `on_mouse_move` rather than a hover style,
                            // because the library gives a `Button` no hook to
                            // set its own hover fill, and a second fill derived
                            // from a different token an inch below the first is
                            // the drift this list is being flattened to avoid.
                            .on_mouse_move(cx.listener(
                                move |composer: &mut Self, _, _, cx| {
                                    composer.select(i, cx);
                                },
                            ));
                            // The heading rides *inside* the row's own box
                            // rather than beside it in the list, so the index
                            // the arrow keys walk still counts choices and
                            // nothing else -- and so scrolling to a row brings
                            // the heading that introduces it along.
                            div()
                                .v_flex()
                                .w_full()
                                .children(
                                    // Smaller than any row and a weight above
                                    // them. A heading names a run of rows and
                                    // is the one line here that can never be
                                    // taken, so what tells it apart from a
                                    // choice must not be a thing a choice could
                                    // also be: it is the smallest text on the
                                    // card, it is the only text never drawn on
                                    // the selected fill, and it is separated by
                                    // a rule. Size, not quietness, is what does
                                    // the work — drawn dim it shared an ink
                                    // with the rows it introduces.
                                    //
                                    // **The rule over it is the separator
                                    // between one run and the next**, which is
                                    // why it is drawn here rather than under
                                    // the last row of the run above: a border
                                    // under a row would have to know it was the
                                    // last of its kind, and the heading already
                                    // knows it is the first of the next. The
                                    // topmost heading goes without, because the
                                    // pinned header directly above it has a
                                    // rule of its own and two hairlines with
                                    // one label between them read as a box.
                                    heading.map(|heading| {
                                        group_label(&heading, cx).when(i > 0, |label| {
                                            // Room on both sides of the rule,
                                            // and more of it above than below.
                                            // A separator with the label
                                            // crowded against its underside is
                                            // one the eye groups with the label
                                            // instead of reading as the end of
                                            // what came before; the gap is what
                                            // makes it a boundary rather than a
                                            // decoration on the heading. More
                                            // above because that side is
                                            // closing a run of rows and this
                                            // side is only introducing one.
                                            label
                                                .border_t_1()
                                                .border_color(cx.theme().border)
                                                .mt_3()
                                                .pt_2()
                                        })
                                    }),
                                )
                                .child(body.on_click(take))
                        }))
                        // This one stays among the rows, because it stands *in
                        // place of* them: an empty list is what it is reporting,
                        // so there is nothing for it to be scrolled away behind.
                        .when(selected.is_none(), |list| {
                            // **Naming the query is what makes this an answer
                            // rather than a shrug.** A bare "No matches" leaves
                            // the reader checking their own typing against a
                            // popup that is not showing it — and the query is
                            // exactly what they cannot see, because it is in
                            // the field behind the card. Said back, a typo
                            // answers itself.
                            //
                            // **And "nothing matched" is only true once there
                            // is something to match against.** The `@` list is
                            // scanned off the UI loop, so for the first moments
                            // of a session the pool is empty and every query
                            // came back with nothing — which this reported as a
                            // failed search, against a search that had not run.
                            // A wrong answer in the shape of a right one: the
                            // reader retypes a filename that was never going to
                            // be found any faster.
                            let query = self
                                .trigger
                                .as_ref()
                                .map(|trigger| trigger.query.clone())
                                .unwrap_or_default();
                            list.child(notice(cx).text_sm().child(
                                match (pending, query.is_empty()) {
                                    (true, _) => "Still looking…".to_string(),
                                    (false, true) => "Nothing to complete".to_string(),
                                    (false, false) => {
                                        format!("No matches for \u{201c}{query}\u{201d}")
                                    }
                                },
                            ))
                        })
                        .children(filler.map(|_| div().h(POPUP_ROW_H).flex_none()))
                        // The same box a heading occupies, margin included.
                        // Built from `h` alone it was short by the gap under
                        // every heading, so the list it was holding steady
                        // still moved by that much for each group that came or
                        // went.
                        .children(
                            label_filler.map(|_| div().min_h(GROUP_LABEL_H).mb_1p5().flex_none()),
                        ),
                ))
                // **Outside the scrolling box, and that is the whole point of
                // them.** Both are sentences about the list rather than choices
                // in it, and both appear only once the list is long -- so held
                // among the rows they were scrolled out of sight in exactly the
                // case that produced them. The count of what is being held back
                // sat past the fiftieth row, so nothing ever said a query had
                // been narrowed at all; and the line naming the keys that walk
                // the list went away the moment the list was long enough to need
                // walking. Out here the surface holds them against its own edge
                // and nothing the list does to its own offset can move them.
                //
                // Tab is named beside Enter because both are bound to take the
                // highlighted row, and a line that lists the keys is read as the
                // complete set.
                //
                // **A rule over them and not a gap**, for the reason the pinned
                // header has one under it: what is above scrolls and what is
                // below does not, and a boundary drawn in empty space reads as
                // spacing rather than as an edge — which leaves the footer
                // looking like the last row of the list, sitting under whatever
                // half-row the scroll happened to stop on.
                // The same condition the footer itself is drawn on, written
                // once: a rule over a footer that is not there is a line under
                // the list for no reason, and the two conditions drifting apart
                // is exactly how that happens.
                // **One row with the slack in the middle, not two stacked
                // lines.** Stacked, these two spend a second line of the popup
                // on chrome — and they read as a short list of their own under
                // the rule, which is the one thing a footer must not look like
                // directly beneath a list of choices. Side by side they are
                // plainly a status line: the left end says something about
                // *this* query and changes as it is typed, the right end names
                // the keys and never changes at all.
                //
                // The keys go right because they are the fixed half. A reader
                // who has learned them stops seeing that end of the row, which
                // only works while it is always the same end.
                .when(overlay == Overlay::Completion, |popup| {
                    popup.child(
                        popup_footer(cx)
                            .gap_2()
                            .children((capped > 0).then(|| {
                                div()
                                    .flex_none()
                                    .child(format!("{capped} more — keep typing to narrow them"))
                            }))
                            // The slack sits between them and never at either
                            // end, so the keys stay against the right edge
                            // whether or not there is a count to the left of
                            // them — pushed by a missing count they would move
                            // across the popup as a query narrowed.
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex_none()
                                    .min_w_0()
                                    .truncate()
                                    .child("↑↓ Navigate · Tab or Enter Select · Esc Close"),
                            ),
                    )
                })
                .children(segments.map(|segments| self.segment_rail(segments, session, cx))),
        )
    }

    /// A config group drawn as one rail of segments at the foot of the list.
    ///
    /// **A rail and not rows**, because effort is the one setting here whose
    /// values are a ladder — less of a thing, then more of it — and three or
    /// four words on one line say that, where a column of rows says only that
    /// there are four of them.
    ///
    /// **Below the list and outside the scroll**, because it is not one of the
    /// choices being scrolled through: it is a second setting, and a control
    /// that scrolls away while the list above it is walked is one the reader
    /// has to go looking for. The rule above it is what says the two are
    /// different questions.
    ///
    /// **It does not close the popup**, unlike every row here, and that is the
    /// difference between picking from a list and nudging a control. A row that
    /// stayed open after being taken would leave the reader wondering whether
    /// the click landed; a segment lights where it was pressed and says so
    /// itself, and the next thing somebody does with a ladder is often try the
    /// rung beside it.
    ///
    /// **The keyboard does not reach it.** The arrow keys walk the list and
    /// `Enter` takes a row, both counting in an index this rail is not part of;
    /// wiring it in means a second key model for a control holding several
    /// values on one line.
    fn segment_rail(
        &self,
        segments: super::presentation::Segments,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let super::presentation::Segments {
            name,
            config_id,
            choices,
            current,
        } = segments;
        let session = session.clone();
        let values: Vec<String> = choices.iter().map(|(_, value)| value.clone()).collect();
        let labels: Vec<SharedString> = choices.into_iter().map(|(label, _)| label).collect();
        let (fill, ink, radius) = (
            cx.theme().accent,
            cx.theme().accent_foreground,
            cx.theme().radius,
        );

        // **Not a `ButtonGroup`**, which is the component for joining *bordered*
        // buttons into one segmented block -- it hands each child the corners
        // and edges of its place in the row, so the two ends round outward and
        // everything between them stays square. That is exactly right for a
        // joined block and exactly wrong here: flat, the only thing drawn is the
        // fill under the rung in force, and a fill square on two sides reads as
        // a rectangle laid over the words rather than as a rounded chip around
        // one. A plain row of buttons has no such opinion, and it costs less
        // than the component did -- each rung carries its own press instead of
        // the group reporting an index for the row to look up.
        let rail = div().h_flex().items_center().gap_1().flex_none().children(
            labels.into_iter().enumerate().map(|(i, label)| {
                let current = Some(i) == current;
                let value = values.get(i).cloned().unwrap_or_default();
                let config_id = config_id.clone();
                let session = session.clone();
                // Through the app's own wrapper, so a segment answers the
                // pointer like every other control here -- the library's
                // buttons draw the arrow, and a rail of six of them is six
                // places for that to show.
                crate::controls::action(("segment", i))
                    .ghost()
                    // The list's own size, so the rail letters at the step the
                    // rows above it do. A step under that and it read as a
                    // footnote on the list rather than as a setting beside it.
                    .small()
                    .h(CHIP_H)
                    .px_2()
                    .rounded(radius)
                    .label(label)
                    .selected(current)
                    // The popup's one spelling for "in force", which is the
                    // same one the rows above use. The library's own selected
                    // fill for a ghost button is derived from a different
                    // token, so the two would disagree an inch apart about what
                    // being selected looks like.
                    .when(current, |segment| segment.bg(fill).text_color(ink))
                    .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                        let (config_id, value) = (config_id.clone(), value.clone());
                        session.update(cx, |session, cx| {
                            session.chat.set_config_option(&config_id, &value);
                            cx.notify();
                        });
                        // The rail is drawn by the composer, so the session's
                        // own notify does not redraw it: without this the value
                        // goes out and the rail keeps lighting the rung that
                        // was in force before.
                        cx.notify();
                    }))
            }),
        );

        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .px_2()
            // Deeper than a row of the list, on purpose. This block is a second
            // setting sitting under the answer to the first, and at a row's own
            // inset it read as one more entry in the list that happened to have
            // buttons in it -- the rule above says they are different questions
            // and the air is what makes the rule look deliberate.
            .py_3()
            .mt_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_none()
                    // The rows' own size too: the rail's label and the list's
                    // headings are the same kind of word, and one of them a
                    // step smaller reads as a caption on the other.
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(name),
            )
            .child(rail)
    }
}

/// The popup's bottom element: what the list is not saying about itself, held
/// under the scroll with a rule over it.
///
/// **The rule is carried by this row and not drawn as a line of its own**, and
/// that is the whole reason this function exists. It *was* a bare `div` with a
/// top border and nothing in it, which is a box of zero height — so what got
/// laid out was a border on an element with no area, and nothing appeared. A
/// separator with no thickness is indistinguishable from a separator that was
/// never asked for, and from outside it reads as the footer having been left
/// inside the list.
///
/// Hung off the row instead, the border is on the top edge of something that
/// certainly has height, and the padding under it is what keeps the words off
/// the line.
///
/// **One builder because two popups draw it.** A rule is exactly the kind of
/// thing that drifts unnoticed: a pixel of margin different on one of them
/// reads as one card being slightly wrong rather than as two cards disagreeing,
/// so nobody goes looking for the second copy.
pub(super) fn popup_footer(cx: &App) -> gpui::Div {
    notice(cx)
        .w_full()
        .flex_none()
        .text_xs()
        .mt_1()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
}

/// The popup's one pinned row: what this list is, held above the scroll.
///
/// **Outside the scrolling box, which is the whole of the point.** The headings
/// inside the list belong to the runs of rows under them and travel with those
/// rows, so walking a long list scrolls every one of them away and leaves a
/// column of paths with nothing on screen saying what opened it or what the
/// rows are. This one never moves.
///
/// **A rule under it and not a gap**, because a gap reads as spacing where a
/// line reads as an edge — and an edge is what this is: the part that stays and
/// the part that moves, which is a boundary the reader has to be able to see
/// before they scroll rather than discover by scrolling. Drawn at hairline
/// strength on the popup's own inset, so it stops a few pixels short of the
/// card's border rather than colliding with it at the corners.
pub(super) fn popup_header(title: SharedString, cx: &App) -> gpui::Div {
    notice(cx)
        .w_full()
        .flex_none()
        .text_xs()
        // The same treatment the run labels take, one size up: the quiet step
        // and a weight, in the case it was written in. Left brighter or
        // uppercase while those are neither, the popup's own title would be the
        // loudest line on a card whose whole content is the rows under it.
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .border_b_1()
        .border_color(cx.theme().border)
        .pb_1()
        .mb_1()
        .child(title)
}

/// What the open popup is, as its pinned header says it.
///
/// **Derived from the list rather than looked up per overlay.** A table keyed
/// on the overlay would be a second place naming every settings group, kept in
/// step by hand with the headings the rows already carry — and it would be
/// wrong first for the group this app does not know the name of, since what a
/// config group is called is the agent's to choose and arrives over the wire.
///
/// So: a completion says what is being completed, because the trigger is the
/// only thing that knows and a lone `@` is not self-explanatory. Anything else
/// takes the name of its one group where it has one, and the general word where
/// it holds several — a list of models and efforts under a heading reading
/// `Model` would be naming a third of itself.
pub(super) fn popup_title(
    overlay: &Overlay,
    trigger: Option<TriggerKind>,
    rows: &[Row],
) -> SharedString {
    if overlay == &Overlay::Completion {
        return match trigger {
            Some(TriggerKind::Command) => "Run a command".into(),
            _ => "Mention a file".into(),
        };
    }
    let mut groups = rows.iter().filter_map(|row| row.group.clone());
    match (groups.next(), groups.next()) {
        (Some(only), None) => only,
        _ => "Settings".into(),
    }
}

/// Which run the list is in, so a heading is emitted once per run.
///
/// **One walker for both lists.** Each of them wrote this out — remember the
/// run, compare, set and emit — once per trigger kind, in one function, and
/// they had already come to disagree about the type they remembered it as: one
/// held the kind, the other a nested `Option` whose inner value was the
/// namespace. What both actually need is the heading's own text, which is the
/// only thing either of them does with it.
#[derive(Default)]
pub(super) struct Runs(Option<SharedString>);

impl Runs {
    /// The heading, where `name` opens a run this has not seen.
    pub(super) fn opening(&mut self, name: &str) -> Option<SharedString> {
        (self.0.as_deref() != Some(name)).then(|| {
            let name = SharedString::from(name.to_string());
            self.0 = Some(name.clone());
            name
        })
    }
}

/// The label opening one run of rows.
///
/// **Smaller than anything it introduces, and that is the point.** It stood at
/// a row's own text size and a row's own height, which in a list of three runs
/// put a line the same weight as a choice between every few choices — so the
/// column a reader is scanning was interrupted three times by something that
/// looked like part of it. A label is read once, on the way past; the rows
/// under it are read one against another.
///
/// The height comes down with the size rather than being left at the row's, or
/// the label would be a small word floating in a row-sized gap, which spends
/// the space a shorter label was meant to give back.
fn group_label(text: &str, cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_center()
        .w_full()
        .flex_none()
        .px_2()
        // **A floor rather than a height, because the rule needs room under
        // it.** Set outright, the height leaves the label vertically centred in
        // a box the rule is drawn on the edge of — so the words sit against the
        // hairline with nothing between them, and the line stops reading as a
        // separator and starts reading as an underline belonging to the label.
        // A floor lets the padding below add to the box instead of being
        // absorbed by it.
        .min_h(GROUP_LABEL_H)
        // Air under the label, so the run it names reads as belonging to it
        // rather than as starting at it. Without this the first row sat as
        // close to the heading as the second row sits to the first, which
        // makes the heading one more line of the list rather than the thing
        // introducing it.
        .mb_1p5()
        .text_size(GROUP_LABEL_TEXT)
        // **The quietest step, and a weight to carry it.** A label has to be
        // found when it is looked for and ignored the rest of the time, and
        // those are not opposites: what makes it findable is that it is bold,
        // set apart by a rule and smaller than everything around it, none of
        // which a row can be. What would make it *loud* is ink, and that is the
        // one axis it does not get — the rows are what the eye is running
        // down, so the label has to sit under them.
        //
        // It can afford the bottom step where the rows could not because it is
        // three other things at once. Sharing that step with the detail column
        // costs nothing for the same reason: different column, different size,
        // and only one of the two is bold.
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_string())
}

/// A line of the popup that is a sentence about the list rather than a choice
/// in it: what the list is, that it matched nothing, that it is holding some
/// back, which keys walk it.
///
/// **Deliberately shorter than a row.** It used to stand at exactly a row's
/// height, on the reasoning that a sentence taller than its neighbours reads as
/// a row that can be taken — which was the right worry and is now answered from
/// the other side. Rows grew to be read rather than pressed, and everything
/// here stayed where it was, so the sentences are now the shorter thing on the
/// card and none of them can be mistaken for something that answers.
///
/// The label over a run of rows is no longer one of these. It is smaller again
/// and carries the rule that separates one run from the next, so it has a
/// builder of its own: see [`group_label`].
fn notice(cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_center()
        .px_2()
        // A floor rather than a height. Set outright it absorbs any padding a
        // caller adds — the content stays centred in a box of exactly this
        // size and the padding has nowhere to go — so the one caller that
        // needs room under a rule would silently get none.
        .min_h(CHIP_H)
        .text_color(cx.theme().muted_foreground)
}
