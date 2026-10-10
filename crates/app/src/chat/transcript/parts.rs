use super::fold_key;
use super::metrics::{
    BUTTON_H, CONTROL_ROW, DETAIL_OPEN_H, LEADING, MARK_SIZE, TEXT_SM, TIGHT_GAP,
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

/// Height the body of a blocking card may occupy before it scrolls inside
/// itself.
///
/// Set a step under the transcript's other bounded card because what it holds
/// back is different: that one is a detail somebody chose to unfold, this one is
/// standing between the conversation and everything after it. The number is what
/// leaves the card's header, its body and the buttons that answer it on one
/// screen together at the sizes around them — which is the whole point of
/// bounding it, and is why it is a height rather than a count of lines or rows.
const MAX_BLOCKING_BODY_H: Rems = rems(16.);

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

/// Copy the paragraph `target`'s turn closes on, gathered **when the button is
/// clicked**: finding it walks the whole answer, and a redraw is not a reason
/// to do that.
pub(super) fn copy_turn_button(session: &Entity<ChatSession>, target: TranscriptItemId) -> Button {
    let session = session.clone();
    crate::controls::action("copy-answer")
        .ghost()
        .xsmall()
        .icon(IconName::Copy)
        .on_click(move |_, _, cx: &mut App| {
            let closing = session.read(cx).chat.turn_closing(target);
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(closing));
        })
}

/// Where whatever a row opens into is set in to: under the row's glyph, past
/// the chevron's column.
pub(super) fn detail_well(body: impl IntoElement) -> gpui::Div {
    div().w_full().min_w_0().pl_5().pt_1().child(body)
}

/// The ink of what runs, and of a way into more: the theme's link hue, the
/// one the rail marks a running session with.
pub(in crate::chat) fn accent(cx: &App) -> gpui::Hsla {
    cx.theme().link
}

/// A disclosure's arrow: one shape that turns as its block opens, so opening
/// moves nothing beside it.
pub(super) fn chevron(open: bool) -> Icon {
    Icon::new(IconName::ChevronRight)
        .xsmall()
        .rotate(gpui::radians(match open {
            true => std::f32::consts::FRAC_PI_2,
            false => 0.,
        }))
}

/// The arrow's column, kept whether or not a row has anything to open, so a
/// list of rows keeps one left edge for its glyphs.
pub(super) fn chevron_slot(fold: Option<bool>) -> gpui::Div {
    div().w_3().flex_none().children(fold.map(chevron))
}

/// The line every folding block opens from: its arrow first, then what the
/// block says about itself, in the quiet ink and lit to full ink under the
/// pointer. The caller adds the words and what a press does.
pub(super) fn fold_line(
    id: impl Into<gpui::ElementId>,
    open: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let lit = cx.theme().foreground;
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .cursor_pointer()
        .text_size(TEXT_SM)
        .text_color(cx.theme().muted_foreground)
        .hover(move |line| line.text_color(lit))
        .child(chevron(open))
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

/// A well of machine text: the sunk fill, no edge, mono a step under the
/// reading size. What a tool read, printed or changed sits in one, and so does
/// an answer's fenced code, so the two read as the same kind of thing.
pub(super) fn plain_box(cx: &App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().muted)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(TEXT_SM)
        .line_height(relative(LEADING))
}

/// Lines wider than their well scroll sideways inside it, rather than
/// wrapping a command or a diff line into something nobody typed.
pub(super) fn sideways(
    id: impl Into<gpui::ElementId>,
    body: impl IntoElement,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .overflow_x_scroll()
        .child(body)
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

    /// The glyph at the head of a row: the kind of work it was, or a spinner
    /// in the accent while it runs. How a settled step ended is said in words
    /// at the row's end, so the glyph only ever says what the work was.
    fn draw(self, kind: SharedString, cx: &App) -> gpui::AnyElement {
        match self {
            Self::Running => Spinner::new().xsmall().color(accent(cx)).into_any_element(),
            Self::Waiting | Self::Done | Self::Recovered | Self::Failed => Icon::empty()
                .path(kind)
                .xsmall()
                .text_color(cx.theme().muted_foreground)
                .into_any_element(),
        }
    }
}

/// One row of an activity block, and the one shape every step takes.
///
/// **One anatomy on every row.** Left to right: the arrow's column, the kind
/// of work (a spinner while it runs), what was done, what it was done to, and
/// whatever is worth saying at the end. Adding a kind of activity must not add
/// a column; only what the row opens into differs.
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
    fn dress<E>(row_div: E, row: ActivityRow, cx: &App) -> E
    where
        E: Styled + ParentElement,
    {
        let muted = cx.theme().muted_foreground;
        row_div
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .text_size(TEXT_SM)
            .text_color(muted)
            .child(chevron_slot(row.fold))
            .child(row.mark.draw(row.kind, cx))
            .child(div().flex_none().whitespace_nowrap().child(row.verb))
            .children(row.object.map(|object| {
                div()
                    .min_w_0()
                    .h_flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(match row.struck {
                        true => muted,
                        false => cx.theme().foreground,
                    })
                    .when(row.struck, |o| o.line_through())
                    .children(
                        object
                            .dir
                            .map(|dir| div().flex_none().text_color(muted).child(dir)),
                    )
                    .child(div().min_w_0().truncate().child(object.name))
            }))
            .children(row.meta)
    }

    match interactive {
        // **Ink, and no plate**: a fill behind a row is the row answering as a
        // surface, and these are lines of a list rather than things on it.
        true => {
            let lit = cx.theme().foreground;
            div()
                .id(id)
                .cursor_pointer()
                .hover(move |row| row.text_color(lit))
                .on_click(on_click)
                .map(|row_div| dress(row_div, row, cx))
                .into_any_element()
        }
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
pub(in crate::chat) fn line_counts(added: usize, removed: usize, cx: &App) -> Option<gpui::Div> {
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
pub(in crate::chat) fn elapsed(secs: u64) -> String {
    onehand_core::duration(secs)
}

/// A word at the end of a row, in the quiet ink or in one that means something.
pub(super) fn row_note(text: impl Into<SharedString>, ink: gpui::Hsla) -> gpui::AnyElement {
    div()
        .flex_none()
        .whitespace_nowrap()
        .text_color(ink)
        .child(text.into())
        .into_any_element()
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
