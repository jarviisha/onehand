use super::complete::picker_rows;
use super::presentation::Row;
use super::rows::{candidate_row, choice_row};
use super::{Composer, Overlay, highlight};
use crate::chat::session::ChatSession;
use gpui::IntoElement as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, div, rems,
};
use gpui_component::scroll::{Scrollbar, ScrollbarMode};
use gpui_component::{ActiveTheme, StyledExt};
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
/// run out of window rather than as one sized to its contents.
const POPUP_HEADROOM: Rems = rems(1.);
/// How many rows a list shows at once; the rest scroll, and a line under the
/// list says how many there are.
///
/// **A glance and not a search.** Past about this many a list stops being taken
/// in at once and becomes something searched, and the query in the field is the
/// better tool for that than the eye is.
pub(super) const POPUP_MAX_ROWS: f32 = 6.;
/// How much of the card underneath a popup is left showing.
///
/// A popup drawn over a parked card has nearly its width and its surface, so
/// with their edges flush the two read as a single tall panel. Lifted, the
/// card's bottom edge stays visible under it, and two horizontal edges a few
/// pixels apart is a stack. Only when there is something under it: with
/// nothing pinned the popup sits hard against the composer's own gap.
pub(in crate::chat) const POPUP_STACK_PEEK: Rems = rems(0.375);
/// What the popup spends on itself, outside the box that scrolls.
///
/// **An over-estimate on purpose.** The bound that has to come out a whole
/// number of rows is on the scrolling box, so the cut lands between two rows
/// however tall the chrome turns out to be; all this decides is how much room
/// the box is offered, and being generous there costs at most a row on a
/// squeezed panel.
///
/// The parts: the pinned header, the line counting what is out of view, and
/// the surface's own inset top and bottom.
const POPUP_HEADER_H: Rems = rems(1.75);
const POPUP_MORE_H: Rems = rems(1.5);
const POPUP_INSET_H: Rems = rems(0.75);
/// A menu opened from a control is this wide, and opens on that control;
/// the wider one holds a model's or a mode's name beside the agent's words
/// about it. A completion spans the stack instead.
const MENU_W: Rems = rems(17.);
/// The air between a menu and the control it opened from.
const MENU_GAP: Rems = rems(0.25);
const MENU_WIDE_W: Rems = rems(20.);

/// How tall a list may grow, given the panel it is opening inside.
///
/// **A cap and not a size.** A list shorter than this draws whole and does not
/// scroll. Two bounds, and the smaller wins: the panel's own height, so the
/// popup never grows over the header, and [`POPUP_MAX_ROWS`]. The floor is
/// what is left when the panel is too short for either: a squeezed pane
/// scrolls its list, but it is never reduced to one row and a scrollbar.
pub fn popup_room(panel: gpui::Pixels, reserved: gpui::Pixels, rem: gpui::Pixels) -> gpui::Pixels {
    let row = POPUP_ROW_H.to_pixels(rem);
    let chrome = popup_chrome().to_pixels(rem);
    let rows = row * POPUP_MAX_ROWS + chrome;
    (panel - reserved - POPUP_HEADROOM.to_pixels(rem))
        .min(rows)
        .max(POPUP_MIN_H.to_pixels(rem))
}

/// Everything the popup draws above and below its scrolling box.
pub(super) fn popup_chrome() -> Rems {
    rems(POPUP_HEADER_H.0 + POPUP_MORE_H.0 + POPUP_INSET_H.0)
}

/// The inset every popup surface pads its contents by.
pub(super) const POPUP_INSET: Rems = rems(0.375);

/// A popup's scrolling list, with its scrollbar on the popup's own right edge.
///
/// The frame reaches back through the surface's inset, so the thumb runs down
/// the popup's edge rather than over the right end of a row. The rows give way
/// only while there is a thumb, read off last frame's layout.
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

/// How tall the scrolling box may stand: a whole number of rows, always, and
/// never more of them than a list shows at once.
pub(super) fn popup_list_h(room: gpui::Pixels, rem: gpui::Pixels, chrome: Rems) -> gpui::Pixels {
    let row = POPUP_ROW_H.to_pixels(rem);
    let left = room - chrome.to_pixels(rem);
    (left / row).floor().clamp(1., POPUP_MAX_ROWS) * row
}
/// How tall a row in the popup stands: a rail row's height, so the choices
/// behind a chip line up with every other list in the window.
pub(super) const POPUP_ROW_H: Rems = rems(1.875);
/// The line a label over a run of rows stands on: its words sit at its foot,
/// with air above them separating the run from the one before. A height rather
/// than padding, so the filler holding a popup's size can match it exactly.
const GROUP_LABEL_H: Rems = rems(1.75);

/// Where a popup sits over the composer.
enum Anchor {
    /// Across the whole stack: a completion, the attachments.
    Span,
    /// A menu at this width, starting at its control's left edge.
    Left(Rems),
    /// A menu at this width, ending at its control's right edge: the mode
    /// chip, at the strip's right end.
    Right(Rems),
}

fn anchor(overlay: &Overlay) -> Anchor {
    match overlay {
        Overlay::Completion | Overlay::Attachments => Anchor::Span,
        Overlay::Add | Overlay::Branch | Overlay::Fast | Overlay::Effort => Anchor::Left(MENU_W),
        Overlay::Options => Anchor::Left(MENU_WIDE_W),
        Overlay::Mode => Anchor::Right(MENU_WIDE_W),
    }
}

/// The surface every popup is drawn on: the floating fill, a control's edge,
/// the app's lift, and the wheel held so the conversation behind does not move
/// with the list.
///
/// **The wheel is claimed on the surface**, so the header and the inset swallow
/// it too: gpui's handler for a scrolling box never claims the event, and the
/// transcript underneath would scroll with the list. The inner list still
/// scrolls, because the deeper listener runs first.
pub(super) fn popup_surface(cx: &App) -> gpui::Div {
    div()
        .v_flex()
        .w_full()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().input)
        .bg(cx.theme().popover.alpha(1.))
        .shadow(crate::theme::lift(cx))
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .p(POPUP_INSET)
}

impl Composer {
    /// Whatever is open, as the card that sits above the composer, placed where
    /// it opens: across the stack, or under the control that opened it.
    ///
    /// Drawn by the pane and not by [`Self::card`] because it must sit
    /// *outside* the box the transcript's bottom clearance is measured from:
    /// measured, that clearance would grow by the popup's height the moment one
    /// opened, and every `@` typed would shove the conversation up.
    pub fn detached_popup(
        &mut self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        rem: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let overlay = self.overlay.clone()?;
        // A menu goes on its control; only one whose control is not on screen
        // falls back to here, over the stack, rather than opening nowhere.
        if !matches!(anchor(&overlay), Anchor::Span) && self.anchor_of(&overlay).is_some() {
            return None;
        }
        let popup = self.popup(session, room, rem, cx)?;
        Some(div().w_full().child(popup))
    }

    /// A menu opened from a control, drawn on that control: its bottom edge
    /// just over the control's top, starting at its left edge (the mode's,
    /// at the strip's right end, ending at its right). Floated above
    /// everything and kept inside the window.
    pub fn anchored_menu(
        &mut self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        rem: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let overlay = self.overlay.clone()?;
        let on = self.anchor_of(&overlay)?;
        let gap = MENU_GAP.to_pixels(rem);
        // Where the menu hangs from, and which way it runs from there: left
        // edges together, or (at the strip's right end) right edges.
        let (width, corner, at, from_right) = match anchor(&overlay) {
            Anchor::Span => return None,
            Anchor::Left(w) => (
                w,
                gpui::Anchor::BottomLeft,
                gpui::point(on.left(), on.top() - gap),
                false,
            ),
            Anchor::Right(w) => (
                w,
                gpui::Anchor::BottomRight,
                gpui::point(on.right(), on.top() - gap),
                true,
            ),
        };
        let popup = self.popup(session, room, rem, cx)?;
        let menu_bounds = self.menu_bounds.clone();
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(at)
                    .anchor(corner)
                    .snap_to_window_with_margin(gap)
                    .child(
                        div().w(width).occlude().child(popup).child(
                            gpui::canvas(
                                // Only the size is taken from here: the box
                                // reports where it was laid out, before it is
                                // moved onto its control, so where it ends up
                                // is worked out from the point it hangs from.
                                move |bounds, _, _| {
                                    let size = bounds.size;
                                    let left = match from_right {
                                        true => at.x - size.width,
                                        false => at.x,
                                    };
                                    let origin = gpui::point(left, at.y - size.height);
                                    menu_bounds.set(Some(gpui::Bounds { origin, size }))
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        ),
                    ),
            )
            .into_any_element(),
        )
    }

    fn anchor_of(&self, overlay: &Overlay) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.anchors
            .borrow()
            .iter()
            .find(|(o, _)| o == overlay)
            .map(|(_, bounds)| *bounds)
    }

    /// `element`, measured where it is drawn as the control `overlay`'s menu
    /// opens from.
    pub(super) fn opens_menu(
        &self,
        overlay: Overlay,
        element: impl gpui::IntoElement,
    ) -> gpui::AnyElement {
        let anchors = self.anchors.clone();
        div()
            .relative()
            .h_flex()
            .min_w_0()
            .child(element)
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        let mut anchors = anchors.borrow_mut();
                        anchors.retain(|(o, _)| *o != overlay);
                        anchors.push((overlay, bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }

    /// The open completion, settings, menu or attachment surface.
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
        let completion = overlay == Overlay::Completion;
        // Set while the rows are built, because that is the only place both
        // halves of the count are in hand.
        let mut capped = 0usize;
        let rows: Vec<Row> = match &overlay {
            Overlay::Completion => {
                let (rows, held) = self.matches(session, cx);
                capped = held;
                rows
            }
            picker => picker_rows(picker, session, cx).unwrap_or_default(),
        };
        // A trigger that matches nothing still has to say so: the popup is the
        // only thing on screen confirming the `@` or `/` was understood. A
        // selector with no choices has nothing to confirm, so it stays away.
        if rows.is_empty() && !completion {
            return None;
        }
        let selected = highlight(self.selected, rows.len());
        // **Each trigger draws from its own pool, and each arrives late.** Files
        // are scanned off the UI loop when the session opens; commands come from
        // the agent once it connects. Until the pool is in, the popup is not
        // measured, so it grows out of a line saying it is still looking rather
        // than collapsing from a guess.
        let pending = self.trigger.as_ref().is_some_and(|trigger| {
            let chat = &session.read(cx).chat;
            match trigger.kind {
                TriggerKind::File => !chat.files_scanned,
                TriggerKind::Command => chat.commands.is_empty(),
            }
        });
        // Measured once, on the frame the popup opens, against an empty query,
        // and held until it closes: the list never moves under the hand aiming
        // at it because typing narrowed it.
        if self.opened_rows.is_none() && !pending {
            self.opened_rows = Some(self.shape(session, cx));
        }
        let (opened, label_floor) = self.opened_rows.unwrap_or_default();
        let floor = opened.min(POPUP_MAX_ROWS as usize);
        let headings = rows.iter().filter(|row| row.group.is_some()).count();
        // Drawn inside the scroll, so this is a floor on the list rather than a
        // height on the popup, and a squeezed pane still bounds it.
        let filler = rows.len()..floor;
        let label_filler = headings..label_floor;
        // How many rows are out of view at once, said under the list. Its line
        // is held while the popup is open if it opened with one, so a query
        // narrowing below the cap does not take a line out from under the list.
        let more = (rows.len() + capped).saturating_sub(POPUP_MAX_ROWS as usize);
        let more_line = more > 0 || opened > POPUP_MAX_ROWS as usize;
        let title = popup_title(&overlay, self.trigger.as_ref().map(|t| t.kind), &rows);
        // A list of one group would be named twice an inch apart, on the pinned
        // row and on the first row under it; the heading is the one that goes.
        let mut rows = rows;
        if let Some(first) = rows
            .first_mut()
            .filter(|r| r.group.as_ref() == Some(&title))
        {
            first.group = None;
        }

        Some(
            // The surface and the scrolling list are two boxes, and the inset
            // between them belongs to the surface: padding inside the box that
            // scrolls would be scrolled away by `scroll_to_item`.
            popup_surface(cx)
                // The whole popup is what the panel has to hold, not the list
                // alone.
                .max_h(room)
                .child(popup_header(title, cx))
                .child(edge_scrolled(
                    &self.rows_scroll,
                    div()
                        .id("completion")
                        .v_flex()
                        .w_full()
                        // What lets this shrink to whatever the surface has
                        // left after the lines below, rather than pushing them
                        // off the bottom.
                        .min_h_0()
                        .max_h(popup_list_h(room, rem, popup_chrome()))
                        .overflow_y_scroll()
                        // Held by the composer, so walking the list with the
                        // keys can scroll it.
                        .track_scroll(&self.rows_scroll)
                        .children(rows.into_iter().enumerate().map(|(i, row)| {
                            let session = session.clone();
                            let pick = row.pick.clone();
                            let heading = row.group.clone();
                            // A click takes the row; the highlight moves first,
                            // so what was clicked is what gets taken.
                            let take = cx.listener(move |composer: &mut Self, _, window, cx| {
                                composer.select(i, cx);
                                composer.apply_pick(&pick, &session, window, cx);
                            });
                            let highlighted = Some(i) == selected;
                            let body = match completion {
                                true => candidate_row(i, row, highlighted, cx),
                                false => choice_row(i, row, highlighted, cx),
                            }
                            // The pointer moves the highlight exactly as the
                            // arrows do: one fill, meaning the row Enter takes.
                            .on_mouse_move(cx.listener(
                                move |composer: &mut Self, _, _, cx| {
                                    composer.select(i, cx);
                                },
                            ));
                            // The heading rides inside the row's own box, so the
                            // index the arrows walk counts choices only, and
                            // scrolling to a row brings its heading along.
                            div()
                                .v_flex()
                                .w_full()
                                .children(heading.map(|heading| group_label(&heading, cx)))
                                .child(body.on_click(take))
                        }))
                        // In place of the rows, because an empty list is what
                        // it reports: naming the query lets a typo answer
                        // itself, and a pool still arriving is not a failed
                        // search.
                        .when(selected.is_none() && completion, |list| {
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
                        .children(label_filler.map(|_| div().h(GROUP_LABEL_H).flex_none())),
                ))
                // Outside the scroll, so the count is never scrolled out of
                // sight in exactly the case that produced it. A completion can
                // be narrowed by typing and says so; a menu cannot.
                .when(more_line, |popup| {
                    popup.child(more_text(cx).children((more > 0).then(|| match completion {
                        true => format!("{more} more — keep typing to narrow"),
                        false => format!("{more} more"),
                    })))
                }),
        )
    }
}

/// The line under a list counting what is out of view.
pub(super) fn more_text(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .h(POPUP_MORE_H)
        .h_flex()
        .items_center()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
}

/// The popup's one pinned row: what this list is, held above the scroll with a
/// rule under it, because the part that stays and the part that moves is a
/// boundary the reader has to see before they scroll.
pub(super) fn popup_header(title: SharedString, cx: &App) -> gpui::Div {
    div()
        .w_full()
        .flex_none()
        .px_2()
        .pb_1()
        .mb_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

/// What the open popup is, as its pinned header says it.
///
/// A completion says what is being completed, because the trigger is the only
/// thing that knows and a lone `@` is not self-explanatory. A menu says what it
/// is about. A list of settings takes the name of its one group where it has
/// one, and the general word where it holds several.
pub(super) fn popup_title(
    overlay: &Overlay,
    trigger: Option<TriggerKind>,
    rows: &[Row],
) -> SharedString {
    match overlay {
        Overlay::Completion => match trigger {
            Some(TriggerKind::Command) => "Run a command".into(),
            _ => "Mention a file".into(),
        },
        Overlay::Add => "Add to the prompt".into(),
        Overlay::Branch => "Branch".into(),
        Overlay::Attachments => "Attachments".into(),
        Overlay::Mode | Overlay::Options | Overlay::Fast | Overlay::Effort => {
            let mut groups = rows.iter().filter_map(|row| row.group.clone());
            match (groups.next(), groups.next()) {
                (Some(only), None) => only,
                _ => "Settings".into(),
            }
        }
    }
}

/// Which run the list is in, so a heading is emitted once per run.
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

/// The label opening one run of rows: the quiet size and ink, with no rule, so
/// the air above it is what ends the run before.
fn group_label(text: &str, cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_end()
        .w_full()
        .flex_none()
        .h(GROUP_LABEL_H)
        .px_2()
        .pb_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_string())
}

/// A line of the popup that is a sentence about the list rather than a choice
/// in it, standing in for the rows when there are none.
fn notice(cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_center()
        .px_2()
        .min_h(POPUP_ROW_H)
        .text_color(cx.theme().muted_foreground)
}
