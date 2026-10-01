use super::fold_key;
use super::metrics::{
    BUTTON_H, CHEVRON_MARK, CHEVRON_SLOT, CODE_LH, CONTROL_ROW, DETAIL_INSET, DETAIL_OPEN_H,
    FRAME_PAD, GLYPH_DROP, KIND_ICON, MARK_SIZE, MAX_BLOCKING_BODY_H, OBJECT_TEXT, PART_GAP,
    PILL_H, ROW_PAD_X, ROW_PAD_Y, STATUS_DOT, TIGHT_GAP, VERB_TEXT, radius_block, radius_control,
};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, InteractiveElement, IntoElement, Length, ParentElement, Rems,
    RenderOnce, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div,
    relative, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::{ToolKind, ToolStatus};
use onehand_core::chat::TranscriptItemId;

/// The body of a blocking card, bounded and scrolling inside itself.
///
/// **The controls have to outlive the content.** A permission names a command
/// the agent wrote and a question offers choices the agent worded, so both
/// bodies are exactly as long as the agent made them — sixty lines of shell, a
/// dozen options each running to a paragraph — and both cards end in the buttons
/// that are the only way past them. Left to grow, the card is taller than the
/// pane and Allow, Deny and Submit sit below the bottom edge of a transcript
/// that has already scrolled itself as far as it goes: the one block nothing
/// proceeds without becomes the one block that cannot be answered.
///
/// **Scrolled rather than cut**, which is the opposite of what every other well
/// in this file does. A tool's output drops its head and reports how many lines
/// went, because nobody is required to read it. This is the text a grant is
/// given on the strength of and the wording a choice is made from, and a
/// permission whose command is hidden in the middle is a permission answered
/// blind.
///
/// **The thumb runs down the card's own edge**, in the card's right padding,
/// rather than over the rows' borders: a choice's border running under the
/// thumb reads as a row drawn wrong rather than as one that scrolls. So the
/// frame reaches back through `inset` -- which must be the card's own right
/// padding -- and the rows are held off by the same amount, which leaves them
/// ending where the free-text box below the list does. That box is the last
/// row of the options and sits outside the scroll because it is a control,
/// so two edges here would read as one list cut in two.
#[derive(IntoElement)]
pub(super) struct BlockingBody {
    pub(super) target: TranscriptItemId,
    pub(super) children: Vec<gpui::AnyElement>,
    pub(super) inset: Rems,
}

impl RenderOnce for BlockingBody {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = fold_key(self.target);
        let scroll = window
            .use_keyed_state(("blocking-body-scroll-state", key), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();

        div()
            .id(("blocking-body-frame", key))
            .relative()
            .mr(rems(-self.inset.0))
            .max_h(MAX_BLOCKING_BODY_H)
            .child(
                div()
                    .id(("blocking-body-scroll", key))
                    .v_flex()
                    // Enough to read the rows as separate things. These are
                    // bordered boxes, not lines of text, and a quarter-rem
                    // between two borders is a gap the eye resolves as one
                    // thick rule with a seam in it.
                    .gap_2()
                    .w_full()
                    .max_h(MAX_BLOCKING_BODY_H)
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .pr(self.inset)
                    .children(self.children),
            )
            // The mask takes vertical wheel input in the capture phase. A bubble
            // listener runs too late inside `gpui::list`: the transcript has
            // already spent the same delta scrolling itself by then.
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("blocking-body-mask", key)))
            .child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
    }
}

/// Copy `text` to the clipboard, as a quiet icon button.
///
/// One constructor for both the copies the transcript offers — a fenced block
/// and a whole answer — because they are the same gesture and the only reason
/// they ever looked different was that they were written months apart.
pub(super) fn copy_button(id: impl Into<gpui::ElementId>, text: impl Into<SharedString>) -> Button {
    let text = text.into();
    crate::controls::action(id)
        .ghost()
        .xsmall()
        // Square at the shared control height, so a copy offered on a fenced
        // block and one offered at the end of an answer are the same target.
        .size(BUTTON_H)
        .icon(Icon::new(IconName::Copy).size(MARK_SIZE))
        .on_click(move |_, _, cx: &mut App| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
        })
}

/// Copy the whole turn `target` belongs to, gathered **when the button is
/// clicked**.
///
/// The eager form above is right for a fenced block, whose text the caller
/// already holds. A turn's prose is a join of every agent block in it — the
/// length of the answer, built from scratch — and building that on every redraw
/// is the length of the answer per frame, to have it ready in case a button is
/// pressed.
pub(super) fn copy_turn_button(session: &Entity<ChatSession>, target: TranscriptItemId) -> Button {
    let session = session.clone();
    crate::controls::action("copy-answer")
        .ghost()
        .xsmall()
        .size(BUTTON_H)
        .icon(Icon::new(IconName::Copy).size(MARK_SIZE))
        .on_click(move |_, _, cx: &mut App| {
            let prose = session.read(cx).chat.turn_prose(target);
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(prose));
        })
}

/// Where whatever a row opens into is set in to: the row's own words, with the
/// frame's inset kept on the right and the foot.
pub(super) fn detail_well(body: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pl(DETAIL_INSET)
        .pr(ROW_PAD_X)
        .pb(FRAME_PAD)
        .child(body)
}

/// The fixed box a mark sits in, dropped onto the line its neighbours read on.
///
/// One function because every row has two or three of them and they all need
/// the same correction: written out per call site, the first one somebody added
/// without it is a mark a pixel above the words beside it, which reads as the
/// row having come apart rather than as anything measurable.
pub(super) fn mark_slot(size: Rems) -> gpui::Div {
    div()
        .size(size)
        .flex_none()
        .mt(GLYPH_DROP)
        .h_flex()
        .items_center()
        .justify_center()
}

/// A detail that has outgrown its box, scrolling inside it and nowhere else.
///
/// **The wheel has to be taken in the capture phase.** The transcript is a
/// `gpui::list`, which registers its own wheel listener before its children
/// have any say — so a box that merely sets `overflow_y_scroll` is a box the
/// wheel slides the *conversation* behind, and the reader finds themselves
/// somewhere else in the turn while trying to read one command's output.
/// [`ScrollableMask`] sits as a sibling of the scrolled box, consumes the
/// vertical delta before the list sees it, and hands it back at the edges so a
/// detail scrolled to its end lets the transcript carry on — which is what
/// every platform scroller does and what a reader expects without knowing it.
///
/// An element of its own because the handle has to outlive the frame: it is
/// keyed state, and keyed state needs the window.
#[derive(IntoElement)]
struct ScrollBody {
    key: usize,
    max_h: Rems,
    children: Vec<gpui::AnyElement>,
}

impl RenderOnce for ScrollBody {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        let scroll = window
            .use_keyed_state(("detail-scroll-state", key), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();

        div()
            .id(("detail-frame", key))
            .relative()
            .w_full()
            .min_w_0()
            .max_h(self.max_h)
            .child(
                div()
                    .id(("detail-scroll", key))
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .max_h(self.max_h)
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(self.children),
            )
            .child(contain_wheel(scroll.clone()))
            .child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
    }
}

/// Take the wheel for a box that has somewhere to go, and never hand it back.
///
/// **Two conditions, and the first is the one a first attempt forgets.** A box
/// shorter than its own cap has nothing to scroll, so it takes nothing: consumed
/// there, an opened detail is a dead patch of the transcript, where hovering
/// stops the conversation moving for a box that was not going to move either.
///
/// **Then contained, not chained.** The component library's mask stops at the
/// edge and lets the delta bubble — which is what a browser does by default, and is
/// wrong here: these boxes are a few lines tall inside a transcript that is
/// hundreds, so a reader who reaches the end of one command's output has the
/// whole conversation take off under their finger. What they were doing was
/// reading *this*, and arriving at its last line is not a request to leave it.
///
/// Registered in the **capture** phase for the reason the library's is: the
/// transcript is a `gpui::list`, which registers its own wheel listener after
/// its children paint, so in the bubble phase it runs first and has already
/// spent the delta. `should_handle_scroll` rather than a bare bounds test, so
/// the box stays inert under a popup or a dialog.
///
/// A `canvas` rather than an element of its own: the whole of what this needs
/// is a hitbox and one listener, and both are reachable from the two callbacks
/// a canvas already hands out.
fn contain_wheel(scroll: ScrollHandle) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let view = window.current_view();
            let line_height = window.line_height();
            let id = hitbox.id;
            window.on_mouse_event(
                move |event: &gpui::ScrollWheelEvent, phase, window, cx: &mut App| {
                    if !(phase.capture() && id.should_handle_scroll(window)) {
                        return;
                    }
                    // **A box with nothing to scroll takes nothing.** This is
                    // the half the first version got wrong: it consumed the
                    // wheel wherever it was drawn, so an opened detail shorter
                    // than its own cap became a dead patch of the transcript --
                    // hover it and the conversation stopped moving, for a box
                    // that had nothing to move either.
                    //
                    // Last frame's measurement, like everything else measured
                    // here. The first frame after a detail opens has no travel
                    // recorded yet and lets one event past, which is a frame
                    // nobody can see.
                    let travel = scroll.max_offset().y.max(gpui::px(0.));
                    if travel <= gpui::px(0.) {
                        return;
                    }
                    let delta = event.delta.pixel_delta(line_height).y;
                    // The current offset is clamped too: a bubbled event can
                    // push the shared offset past the edge unclamped, and that
                    // transient overscroll reads as room that is not there.
                    let mut offset = scroll.offset();
                    let current = offset.y.clamp(-travel, gpui::px(0.));
                    let next = (current + delta).clamp(-travel, gpui::px(0.));
                    if next != current {
                        offset.y = next;
                        scroll.set_offset(offset);
                        cx.notify(view);
                    }
                    // **And where there *is* travel, at the edge most of all.**
                    // Reaching the end of a box a few lines tall is not a
                    // request to leave it, and handing the delta on there is
                    // the whole of what this exists to stop.
                    cx.stop_propagation();
                },
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A detail's body, scrolling inside its box only once it has been opened.
pub(super) fn scrolled(open: bool, key: usize, body: gpui::Div) -> gpui::AnyElement {
    match open {
        true => ScrollBody {
            key,
            max_h: DETAIL_OPEN_H,
            children: vec![body.into_any_element()],
        }
        .into_any_element(),
        false => body.into_any_element(),
    }
}

/// The arrow of a disclosure, in a slot it keeps whether or not it is drawn.
///
/// **Reserved, not conditional.** A row that grew one on gaining something to
/// open would shift every word beside it, and a column of them down a block
/// would come out ragged for a reason about the rows rather than the arrows.
pub(super) fn chevron_slot(fold: Option<bool>, cx: &App) -> gpui::Div {
    mark_slot(CHEVRON_SLOT).children(fold.map(|open| {
        Icon::new(match open {
            true => IconName::ChevronDown,
            false => IconName::ChevronRight,
        })
        .size(CHEVRON_MARK)
        .text_color(cx.theme().muted_foreground)
    }))
}

/// The box a detail is drawn in, without the row wrapper around it.
pub(super) fn plain_box(cx: &App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(radius_block(cx))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.4))
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(OBJECT_TEXT)
        .line_height(relative(CODE_LH))
}

/// Let a control take the height its own text needs.
///
/// The component library sizes every button to **one fixed row** — 32px at the
/// default size — which is right for a control the app worded and wrong for
/// every control on the two blocking cards, where the wording is the agent's:
/// a permission's options, a question's choices and their explanations are
/// sentences, in whatever language the conversation is being held in. A
/// sentence that wraps to three lines inside a row that reserved one is laid
/// out at 32px and *painted* at ninety, so every line past the first is drawn
/// over whatever the card put below it and the card reads as a pile of
/// overlapping text rather than as a form. Auto height is what puts the space
/// reserved and the text drawn back in agreement.
///
/// **The floor and the padding come with it.** The library gives a button of
/// this size no vertical padding at all, because the fixed row plus centring
/// already held the label off both edges — so taking the row away and nothing
/// else leaves every control shrink-wrapped around its text, which is a
/// different card to be wrong about in the same place. A single line lands back
/// on exactly the row the library would have drawn: [`CONTROL_ROW`] is that
/// height, and the padding is what a second line grows by rather than what a
/// first line needs.
pub(super) fn grows(button: Button) -> Button {
    button.small().h(Length::Auto).min_h(CONTROL_ROW).py_1p5()
}

// ── permission — blocking; the agent parks until answered ───────────────────

/// The surface every card and strip that floats over the composer is built on.
///
/// **One function because there are four of them** — a parked permission, a
/// parked question, an adapter still connecting, a prompt waiting its turn —
/// and they arrive in one column, stacked, directly above the composer. Written
/// out four times they came apart exactly where four copies do: two sat on the
/// reading surface with a hairline and a single radius while the other two
/// floated on the raised one with a shadow and a doubled radius, so a
/// permission parked above a queued prompt read as two unrelated things rather
/// than as the same kind of interruption twice.
///
/// It is the **composer's own treatment**, and has to be: these are the boxes
/// that stack on top of that card and are read as one object with it. The
/// radius is the theme's named card step for the same reason the composer takes
/// it — one window drawing its floating surfaces at two corners is a difference
/// nobody chose.
///
/// The shadow stays on a card **drawn back in the transcript once it has been
/// answered**, which is deliberate and not an oversight. The same element is
/// used in both places by design — one card that changed on being answered
/// would read as two different cards — and what it carries into the history is
/// the mark of the one block that stopped everything until somebody replied.
pub(in crate::chat) fn floating_card(cx: &App) -> gpui::Div {
    div()
        .w_full()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        // Opaque, and not the reading surface: the transcript runs underneath
        // these and text showing through a box that is asking a question is the
        // one place in the app that cannot afford it.
        .bg(cx.theme().popover.alpha(1.))
        .shadow_lg()
}

// ── shared bits ─────────────────────────────────────────────────────────────

/// What the fixed slot at the head of an activity row holds.
///
/// **Four drawings, one box.** The whole point of the slot is that a step going
/// from waiting to running to done swaps what is in it and moves nothing else
/// on the row — so a group of ten rows finishing one at a time is a column of
/// marks changing in place rather than ten lines of text nudging sideways.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowMark {
    Waiting,
    Running,
    Done,
    /// Something in the run failed and the run went on to succeed anyway.
    ///
    /// **A tick, in the warning tint rather than the danger one.** A run marked
    /// red because one of seven commands exited non-zero is an alarm for
    /// something already dealt with, and a reader who has learned that the red
    /// mark usually means nothing has learned to ignore the one that means
    /// something. The count of what went wrong is still said, in words, at the
    /// other end of the row.
    Recovered,
    Failed,
    /// A step somebody refused. Not a failure — nothing went wrong, a decision
    /// was taken — so it is the one mark here that is neither the tick nor the
    /// danger cross.
    Refused,
}

impl RowMark {
    /// How a whole run reads: by how it ended, not by its worst moment.
    pub fn of_run(run: onehand_core::chat::RunOutcome) -> Self {
        use onehand_core::chat::Outcome;
        match run.outcome {
            Outcome::Running => Self::Running,
            Outcome::Clean => Self::Done,
            Outcome::Recovered => Self::Recovered,
            Outcome::Failed => Self::Failed,
        }
    }

    pub(super) fn of(status: ToolStatus) -> Self {
        match status {
            ToolStatus::Pending => Self::Waiting,
            ToolStatus::InProgress => Self::Running,
            ToolStatus::Completed => Self::Done,
            ToolStatus::Failed => Self::Failed,
        }
    }

    /// Draw the mark into a slot the caller has already sized.
    ///
    /// **The slot is the caller's and the drawing is this.** A parent's slot
    /// and a child's are two sizes on purpose — the difference is what tells
    /// the levels apart before a word is read — so the box cannot be decided
    /// here, and the glyph inside it takes whatever size the caller is drawing
    /// at. Written as one function returning its own box, a child row got its
    /// parent's mark and the two levels lined up exactly.
    fn draw(self, cx: &App) -> gpui::Div {
        let status = crate::theme::status_ink(cx);
        let slot = mark_slot(KIND_ICON);
        let ink = match self {
            Self::Waiting => cx.theme().muted_foreground.opacity(0.5),
            Self::Running => return slot.child(Spinner::new().xsmall()),
            Self::Done => status.success,
            Self::Recovered => status.warning,
            Self::Failed => status.danger,
            // Refused is not a failure -- nothing went wrong, somebody decided
            // -- so it takes the quiet ink and says the rest in words.
            Self::Refused => cx.theme().muted_foreground,
        };
        slot.child(div().size(STATUS_DOT).rounded_full().bg(ink))
    }
}

/// One row of an activity block, and the one shape every step takes.
///
/// **One anatomy, and every column holds its place on every row.** Left to
/// right: how it went, what sort of work it was, what it did, what it did it to,
/// whatever is worth saying at the end, and the arrow. Adding a kind of activity
/// must not add a column; only what the row opens into differs.
pub(super) struct ActivityRow {
    id: gpui::ElementId,
    mark: RowMark,
    /// The block's drawing for the sort of work.
    kind: SharedString,
    /// What was done, in the reading face: `Read`, `Ran`, `Edited`.
    verb: SharedString,
    /// What it was done to, in the machine face. A path arrives split so the
    /// directory can recede behind the name.
    object: Option<Object>,
    /// Whatever the row is worth saying at its end — a line count, a result
    /// count, an exit code.
    meta: Option<gpui::AnyElement>,
    /// A file that is gone, which is the one state that strikes its own name
    /// out rather than only marking the ends of the row.
    pub(super) struck: bool,
    fold: Option<bool>,
}

/// What a row was pointed at.
///
/// **Split at the last separator, because the two halves are read
/// differently.** A reader scanning a column of paths is looking for the name;
/// the directory above it is only there for the times two names are the same,
/// and at one weight it takes the eye first every time it is long. So the
/// directory recedes a step and the name keeps the reading ink.
#[derive(Clone)]
pub(super) struct Object {
    pub(super) dir: Option<SharedString>,
    pub(super) name: SharedString,
}

impl Object {
    /// A path, split; anything else whole.
    pub(super) fn path(value: impl Into<SharedString>) -> Self {
        let value: SharedString = value.into();
        match value.rfind('/') {
            // A command is not a path and must not be cut at its last slash.
            Some(_) if value.contains(char::is_whitespace) => Self {
                dir: None,
                name: value,
            },
            Some(at) => Self {
                dir: Some(value[..=at].to_string().into()),
                name: value[at + 1..].to_string().into(),
            },
            None => Self {
                dir: None,
                name: value,
            },
        }
    }

    pub(super) fn plain(value: impl Into<SharedString>) -> Self {
        Self {
            dir: None,
            name: value.into(),
        }
    }
}

impl ActivityRow {
    pub(super) fn new(
        id: gpui::ElementId,
        mark: RowMark,
        kind: SharedString,
        verb: impl Into<SharedString>,
    ) -> Self {
        Self {
            id,
            mark,
            kind,
            verb: verb.into(),
            object: None,
            meta: None,
            struck: false,
            fold: None,
        }
    }

    pub(super) fn object(mut self, object: Option<Object>) -> Self {
        self.object = object;
        self
    }

    pub(super) fn meta(mut self, meta: Option<gpui::AnyElement>) -> Self {
        self.meta = meta;
        self
    }

    pub(super) fn fold(mut self, fold: Option<bool>) -> Self {
        self.fold = fold;
        self
    }
}

pub(super) fn activity_row(
    row: ActivityRow,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let interactive = row.fold.is_some();
    let id = row.id.clone();
    // **The layout and the ink sit on the row itself, not on a box inside it.**
    // A hover styles the element whose hitbox the pointer is over, and text
    // colour cascades *down* -- so a wrapper that hovers over a child which has
    // already set its own colour changes nothing. Everything that should lift
    // therefore inherits, and the one thing that should not says so.
    // Generic over the element, because the interactive row is a
    // `Stateful<Div>` and the inert one is a plain `Div`: both are `Styled` and
    // `ParentElement`, which is the whole of what dressing a row needs.
    fn dress<E>(row_div: E, row: ActivityRow, cx: &App) -> E
    where
        E: Styled + ParentElement,
    {
        row_div
            .h_flex()
            .items_center()
            .gap(PART_GAP)
            .w_full()
            .min_w_0()
            .py(ROW_PAD_Y)
            .px(ROW_PAD_X)
            .text_color(cx.theme().muted_foreground)
            // **A disc in the ink the state means, and nothing else in the
            // slot.** A tick and a cross are two drawings to read at a size
            // where both are a handful of strokes; a disc is one shape wherever
            // it appears, so what the column carries is a colour -- and a
            // colour is read without being looked at.
            .child(row.mark.draw(cx))
            .child(
                mark_slot(KIND_ICON).child(Icon::new(Icon::empty().path(row.kind)).size(KIND_ICON)),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_size(VERB_TEXT)
                    // The one part held at the reading ink, so it does not lift
                    // with the rest: it is already as bright as this row goes.
                    .text_color(cx.theme().foreground)
                    .child(row.verb),
            )
            .children(row.object.map(|object| {
                div()
                    .flex_1()
                    .min_w_0()
                    .h_flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(OBJECT_TEXT)
                    .when(row.struck, |o| o.line_through())
                    .children(object.dir.map(|dir| {
                        div()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground.opacity(0.7))
                            .child(dir)
                    }))
                    .child(div().min_w_0().truncate().child(object.name))
            }))
            .child(div().flex_1().min_w_0())
            .children(row.meta)
            .child(chevron_slot(row.fold, cx))
    }

    match interactive {
        // **The same hover the cluster's line takes: ink, and no plate.** A
        // fill behind a row is the row answering as a surface, and these rows
        // are a list inside a frame that is already one. The weight is left
        // alone here and only here: the verb is `flex_none`, so a heavier one
        // would move where the object column starts, and a block of rows whose
        // columns shift under the pointer is the one thing the frame is for.
        true => div()
            .id(id)
            .cursor_pointer()
            .hover(|row| row.text_color(crate::theme::meta_ink(cx)))
            .on_click(on_click)
            .map(|row_div| dress(row_div, row, cx))
            .into_any_element(),
        // Nothing to open, so nothing to press: a pointer on a row that does
        // not answer is a promise the row cannot keep.
        false => dress(div(), row, cx).into_any_element(),
    }
}

/// The lines a change added and removed, mono and each side in the ink it
/// means.
///
/// **The one pair in the transcript a reader takes in without reading**, which
/// is why it is one function and not three: a row inside a cluster, the line
/// standing for the cluster and the block closing the turn all draw it, and
/// three copies of "mono, tight gap, success then danger" is three places for
/// one of them to drift.
///
/// `None` where nothing changed, and **`−0` is never drawn**, which colour is
/// what forces: a zero set in the ink that means "this went" is the colour of a
/// loss that did not happen. The text size is the caller's, since the three
/// rows it lands on are not all at one size.
pub(super) fn line_counts(added: usize, removed: usize, cx: &App) -> Option<gpui::Div> {
    if added == 0 && removed == 0 {
        return None;
    }
    let status = crate::theme::status_ink(cx);
    Some(
        div()
            .h_flex()
            .items_center()
            .gap(TIGHT_GAP)
            .flex_none()
            .whitespace_nowrap()
            .font_family(cx.theme().mono_font_family.clone())
            .children(
                (added > 0).then(|| div().text_color(status.success).child(format!("+{added}"))),
            )
            .children(
                (removed > 0).then(|| div().text_color(status.danger).child(format!("−{removed}"))),
            ),
    )
}

/// A duration, at the coarseness somebody reads it at.
///
/// **Seconds up to a minute, then minutes.** A step that took four hundred and
/// twelve seconds is a step that took seven minutes, and the extra two digits
/// are two digits the eye has to divide before it means anything.
pub(super) fn elapsed(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        _ => format!("{}m {}s", secs / 60, secs % 60),
    }
}

/// A word at the end of a row, in the quiet ink or in one that means something.
pub(super) fn row_note(
    text: impl Into<SharedString>,
    ink: gpui::Hsla,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .flex_none()
        .whitespace_nowrap()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(OBJECT_TEXT)
        .text_color(ink)
        .child(text.into())
        .into_any_element()
}

/// A word in a ring: the one shape a state takes wherever one is named.
pub(in crate::chat) fn pill(
    label: impl Into<SharedString>,
    ink: gpui::Hsla,
    cx: &App,
) -> gpui::Div {
    div()
        .flex_none()
        .h(PILL_H)
        .px(PART_GAP)
        .h_flex()
        .items_center()
        .whitespace_nowrap()
        .overflow_hidden()
        .rounded(radius_control(cx))
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(ink)
        .child(div().min_w_0().truncate().child(label.into()))
}

/// What sort of work a tool kind is, in a word.
///
/// **Only the blocking cards print this now.** A transcript row says what was
/// done with the verb core classified it as — "Inspected", "Ran tests",
/// "Built" — which is finer than a kind and is the thing a reader is scanning
/// for. These are what is left: the word a permission card uses to say what it
/// is being asked to allow, where there is no step yet to have a verb.
pub(super) fn tool_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Execute => "Run",
        ToolKind::Edit => "Edit",
        ToolKind::Read => "Read",
        ToolKind::Search => "Search",
        ToolKind::Think => "Think",
        ToolKind::Fetch => "Fetch",
        ToolKind::Delete => "Delete",
        ToolKind::Move => "Move",
        ToolKind::Other => "Tool",
    }
}
