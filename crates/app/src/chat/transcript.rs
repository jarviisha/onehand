//! The transcript: one element per [`ChatItem`].
//!
//! Follows the design language's **structure** — the block types, the user's
//! right-hand bubble against the agent's full-width lane —
//! while every colour, radius and size comes from
//! `cx.theme()`. That split is deliberate: the component library's theme is
//! this app's look, so no token table is carried anywhere and nothing here can
//! drift away from the rest of the window.
//!
//! Rendering stays **bounded**. Diffs and command output
//! draw one element per line, so an unbounded result would freeze the frame.
//! The caps below are correctness, not tuning.

use super::session::ChatSession;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, HighlightStyle, InteractiveElement, IntoElement, Length,
    ParentElement, Rems, RenderOnce, ScrollHandle, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div, relative, rems,
};
use gpui_component::Disableable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::spinner::Spinner;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::{
    ElicitKind, PermissionWeight, PlanStatus, ToolContent, ToolKind, ToolStatus,
};
use onehand_core::chat::activity;
use onehand_core::chat::{
    AskItem, COMMAND_FOLD_LINES, ChatItem, Md, NoticeLevel, PermItem, PlanItem, Thought, ToolItem,
    TranscriptItemId, TurnAnswer, UserMsg,
};
use onehand_core::diff::Row as DiffRow;
use std::path::Path;

/// Diff lines drawn per tool card, **shared across all its hunks** — a
/// MultiEdit touching twenty files must not cost twenty times the budget.
const MAX_DIFF_LINES: usize = 200;
/// Lines of a mono output well before the tail is dropped.
const MAX_MONO_LINES: usize = 60;
/// Plan entries drawn before the list is truncated.
const MAX_TODO_ITEMS: usize = 50;
/// Attachment rows drawn under a prompt before the rest are counted.
const MAX_ATTACHMENT_ROWS: usize = 8;
/// Height a fenced code block in prose is allowed before it scrolls inside
/// itself instead of pushing the rest of the answer off screen.
const MAX_CODE_BLOCK_H: Rems = rems(22.5);
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
/// The size every well of machine text is set at — a tool's output, a diff, a
/// live terminal, and the fenced blocks inside an answer.
///
/// **One size, because they are one thing.** All four are "a machine produced
/// this", and the two that arrived by different routes had drifted apart: the
/// wells this file draws were a step below the meta labels naming them, while
/// the markdown renderer set its blocks from a fixed pixel size of its own. The
/// same command run by the agent therefore read at one size in a tool card and
/// another when quoted back in prose.
const CODE_TEXT: Rems = rems(0.8125);
/// The transcript's own reading size, one step under the app's base.
///
/// A conversation is read the way a page is, not the way a form is filled in:
/// it is long, it is mostly prose, and the eye travels down it rather than
/// stopping at each field. Set at the app's base — a size chosen for labels and
/// controls that have to be hit — a long answer is a wall. One step down fits
/// more of a thought in one glance and leaves the *controls* around it, which
/// are still at base size, reading as the larger things they are.
///
/// Applied where a run is framed, so every block gets it at once, and fed to
/// the markdown renderer's heading base as well: headings scaled off the app's
/// base while the body sat a step under it would print a third-level heading
/// larger than the prose it names for no reason the reader can see.
pub(super) const TEXT: Rems = rems(0.875);
/// The line that stands for a cluster of work: the transcript's own reading
/// size, and never more.
///
/// **It has been all three, and this is the one that holds.** A step under the
/// reading size — where it started — it read as a footnote to the paragraph
/// above rather than as the heading of what came next. A step over it, which is
/// what replaced that, made it the only thing in the transcript set larger than
/// the agent's own words, and size is loud in a way ink is not: the line then
/// out-measured every answer it sat between, which is the one thing nothing
/// here may do. At the reading size it is neither — it takes its place in the
/// column and lets the light weight and the muted ink say how much of the
/// reader it wants, which is what those two channels are for.
///
/// Written as the reading size rather than as the same number again, because
/// that is the relationship: if the transcript's own size moves, this moves
/// with it.
const CLUSTER_TEXT: Rems = TEXT;
/// The transcript's second voice: the record of how an answer got made.
///
/// A tool card, a plan and a blocking card's secondary line are *about* the
/// work rather than part of it, and size is what tells the two voices apart.
/// That distinction is the one thing the reading size above cost: it used to be
/// a step under the app's base, which the reading size then dropped to as well,
/// so an answer and the tool card beside it came out identical. This is the
/// step put back, under the new reading size rather than under the old one.
///
/// It lands on the same number as the wells of machine text and stays a
/// separate decision from them: a card's header is chrome around output, not
/// output, and the two are free to move apart. Nothing else marks them the
/// same — a well is mono, tinted and padded, a card header is none of those.
const WORK_TEXT: Rems = rems(0.8125);
/// Leading for those wells, as a multiple of the size above.
///
/// Prose is set at the golden ratio, which is right for a paragraph and wrong
/// for a diff: two hundred lines each carrying two thirds of a blank line is a
/// column of half-empty rows the eye cannot track down. Tight enough to read as
/// a block, loose enough to separate the lines.
const CODE_LEADING: f32 = 1.45;
/// Space between the paragraphs of one answer.
///
/// **Bounded by the space between whole blocks**, which is the rule this had
/// been breaking: at the renderer's own default two paragraphs of one answer
/// stood further apart than the answer stood from the tool card beneath it, so
/// the inside of a turn was louder than the transcript's own rhythm. It is now
/// a step under that gap rather than equal to it, which is the same rule stated
/// properly: two paragraphs are parts of one block.
const PARAGRAPH_GAP: Rems = PART_GAP;
// ── the scale ───────────────────────────────────────────────────────────────
//
// **Every gap, pad, inset, height and corner below is one of these steps, and
// nothing in the transcript is sized off them.** Written out per call site they
// had already come apart: two paragraphs of one answer stood further apart than
// the answer stood from the card beneath it, a tool's detail was inset to one
// number while the group holding it used another, and four different values
// were in use for the one job of "a line and the caption under it". Named for
// the job rather than for the number, so the next place needing one asks for
// the role and lands on whatever the role currently is.
//
// The gaps are a ladder rather than a range: **what is inside a thing is always
// closer than what surrounds it**, which is the whole of how the transcript
// says where one block ends and the next begins. A value chosen between two
// steps is a boundary the eye cannot resolve.

/// Between one speaker's turn and the next — the unit a reader scrolls looking
/// for, and the only gap wide enough to read as a break rather than as air.
pub(super) const TURN_GAP: Rems = rems(2.);
/// Between two blocks of one turn.
pub(super) const BLOCK_GAP: Rems = rems(0.75);
/// Between the parts of one block.
const PART_GAP: Rems = rems(0.5);
/// Between two rows that are read as one list, and between a line and the
/// caption belonging to it.
pub(super) const TIGHT_GAP: Rems = rems(0.25);
/// A caption sitting directly over the thing it captions, where anything wider
/// would read as two lines rather than one two-line thing.
const HAIR_GAP: Rems = rems(0.125);
/// The air between two small stacked things — an attachment and its neighbour,
/// a bubble and the row of controls under it.
const STACK_GAP: Rems = rems(0.375);
/// Left and right inside a frame that lays its contents out in columns.
const FRAME_PAD: Rems = rems(0.75);
/// The wider frame inset, for a box whose text runs its full width rather than
/// sitting in columns: a bubble, a well, a callout.
const TEXT_PAD_X: Rems = rems(0.875);
/// The vertical half of that pair, a touch under the horizontal for the reason
/// every line of type is — a line box already carries its own leading.
const TEXT_PAD_Y: Rems = rems(0.625);

/// A status pill: as tall as the words in it and no taller.
const PILL_H: Rems = rems(1.125);
/// A control the pointer has to hit.
const BUTTON_H: Rems = rems(1.5);
/// One line of machine text.
const LINE_H: Rems = rems(1.25);
/// The fixed box every mark in a row is drawn inside. Drawings are smaller than
/// it; what the box buys is that a spinner, a tick and a cross take each other's
/// place without the words beside them moving.
const MARK_SLOT: Rems = rems(1.);
/// The drawing inside that box.
const MARK_SIZE: Rems = rems(0.75);
/// An activity row's own insets and columns.
///
/// **Its padding is not the frame's.** A row is a line of a list rather than a
/// box with something in it, so it stands off the frame's edge by more than the
/// frame's own hairline inset and sits tighter top to bottom than a card would.
const ROW_PAD_Y: Rems = rems(0.5);
const ROW_PAD_X: Rems = rems(0.875);
/// The disc that carries a row's state, and the only thing in its slot.
const STATUS_DOT: Rems = rems(0.375);
/// The drawing for a kind of work, and the box it sits in — one and the same,
/// because unlike a state there is only ever one shape here.
const KIND_ICON: Rems = rems(0.875);
/// The verb, in the reading face; what it was done to, in the machine one.
const VERB_TEXT: Rems = rems(0.8125);
const OBJECT_TEXT: Rems = rems(0.75);
/// The arrow, and the slot holding its column while it turns.
const CHEVRON_MARK: Rems = rems(0.6875);
const CHEVRON_SLOT: Rems = rems(0.75);

/// Where a row's verb starts, and so where whatever it opens is set in to.
///
/// **A sum, written once and read twice.** It is the row's padding, the state
/// disc, the kind icon and the two gaps between them — and what unfolds beneath
/// a row lines up with the words that opened it rather than with the marks
/// beside them. Written out as a number in the second place, the two drifted
/// the first time either column moved.
const DETAIL_INSET: Rems = rems(ROW_PAD_X.0 + STATUS_DOT.0 + PART_GAP.0 + KIND_ICON.0 + PART_GAP.0);

/// The detail box: machine text, at the leading a list of it wants.
const CODE_LH: f32 = 1.7;
/// A diff's three columns.
const DIFF_NUM_W: Rems = rems(2.75);
const DIFF_NUM_PAD: Rems = rems(0.5);
const DIFF_TEXT_PAD: Rems = rems(0.625);

/// What a collapsed detail shows before it says there is more.
///
/// **A diff from the top and an output from the bottom**, because that is where
/// each keeps its point: a diff opens on the change, and a command's failure is
/// the last thing it printed.
const PREVIEW_DIFF: usize = 12;
const PREVIEW_OUT: usize = 5;
/// How far the cut fades into the box, at whichever end the cut is.
const SMOKE_DIFF: Rems = rems(4.);
const SMOKE_OUT: Rems = rems(2.75);
/// How tall an opened detail stands before it scrolls inside itself.
const DETAIL_OPEN_H: Rems = rems(20.);
/// Changed lines past which a diff is offered rather than drawn.
///
/// **A thousand-line diff is not read, it is searched** — and drawing it costs
/// one element per line in a list that is already virtualising rows. Past this
/// the row says how big it is and waits to be asked.
const LARGE_DIFF: usize = 400;

/// The little pill that opens and closes a detail.
const PILL_PAD_Y: Rems = rems(0.125);
const PILL_PAD_X: Rems = rems(0.75);

/// The reading column, capped by what the widest thing in it has to hold.
///
/// **Derived, not chosen.** A column set by prose alone is narrower than this —
/// and what that costs is everything in the transcript that is not prose, which
/// is what somebody is reading it *for* when something has gone wrong. A diff
/// is the sharpest case: at the narrower cap this replaced, a line of this
/// project's own code had 74 columns to sit in, against the 100 `rustfmt`
/// writes it at, so nearly every line of nearly every diff wrapped.
///
/// So the number is the width at which 100 mono columns clear the chrome around
/// them — the child inset, the detail's gutters, the well's padding, the diff's
/// sign column and the frame's own border — with a little over, because the
/// mono advance the arithmetic uses is an estimate and some faces are wider.
/// `a_hundred_columns_of_this_projects_code_fits` holds it, so the cap moves
/// when any of those insets does instead of quietly getting tighter.
///
/// `w_full` lets it shrink with a narrow panel; this is only the maximum.
pub(super) const CONTENT_COLUMN: Rems = rems(59.);
/// What a diff line has to clear before it starts wrapping, in mono columns.
///
/// This project's own formatter width: the transcript's job when something
/// breaks is reading diffs of this repository, and a cap under it means the
/// common case is the wrapped one.
///
/// Read only by the test that holds the cap above it, which is the whole point
/// of it having a name: the number is an argument, and an argument written into
/// a comment is one nothing checks.
#[cfg_attr(not(test), allow(dead_code))]
const DIFF_COLUMNS: f32 = 100.;
/// The `+` / `-` column of a diff: one mono character and the air either side.
///
/// A column of its own rather than a character glued to the front of the line,
/// which is what it had been. Glued, the sign shifted every line of the file
/// one place right of where the file has it and left a removed line and the
/// added line replacing it starting at two different columns — so the one thing
/// a diff is read for, what changed between two nearly identical lines, was
/// being compared across an offset the diff itself introduced.
const DIFF_SIGN_W: Rems = rems(0.875);
/// The corner ladder: a mark, a control, a block, a bubble.
///
/// **Small and nearly square, all the way up.** What the transcript is meant to
/// read as is a technical document — a record of what was asked and what was
/// done about it — and a generous corner is what turns a record into a feed of
/// cards. The widest corner here is the user's bubble, and it is only there
/// because the bubble is the one block shaped like a thing somebody said.
///
/// **Anchored on the theme's two named steps rather than written out**, so a
/// theme that squares its corners squares every one of these with it. The
/// ladder's rule is that an inner corner is never wider than the one enclosing
/// it: a box drawn at its container's own radius leaves the two arcs crossing,
/// which reads as a box that failed to fit rather than as one thing inside
/// another.
fn corner(base: gpui::Pixels, step: f32) -> gpui::Pixels {
    match base <= gpui::px(0.) {
        // A squared theme is squared all the way down. Stepping up from zero
        // would round exactly the corners such a theme asked to be left alone.
        true => base,
        false => (base + gpui::px(step)).max(gpui::px(0.)),
    }
}

/// A kind mark, a checkbox: the tightest corner drawn anywhere here.
fn radius_tag(cx: &App) -> gpui::Pixels {
    corner(cx.theme().radius, -4.)
}
/// A button, a status pill, an inline mark, a callout's open side.
///
/// **A pill is a corner and not a capsule**, which is the one place this ladder
/// overrules the shape a pill usually has. Fully round, it was the only
/// silhouette in the transcript that curved — a lozenge at the end of a row of
/// square boxes, reading as a badge stuck onto the row rather than as one of
/// its columns.
fn radius_control(cx: &App) -> gpui::Pixels {
    corner(cx.theme().radius, -2.)
}
/// Everything that is a box with content in it: an activity block, a well of
/// machine text, a diff, a banner, a plan, a thumbnail.
///
/// **One step for all of them**, because they are one thing — a bounded region
/// of the column — and a ladder that gave a well and the block around it two
/// different corners would be spending a rung on a distinction nobody reading
/// it is making.
pub(super) fn radius_block(cx: &App) -> gpui::Pixels {
    cx.theme().radius
}
/// A user's bubble, and nothing else.
fn radius_bubble(cx: &App) -> gpui::Pixels {
    cx.theme().radius_lg
}
/// Width a question's tab label is elided at. Only a *tab* is ever elided.
const ASK_TAB_W: Rems = rems(8.75);
/// The numbered circle at the head of a question's tab.
///
/// Sized to the digit rather than to the row: a strip of four tabs carries four
/// of these, and a circle as tall as the label beside it is a bullet the eye
/// reads before the word it belongs to.
const ASK_TAB_MARK: Rems = rems(1.125);
/// The radio or checkbox at the head of a choice row, and the mark inside it
/// once the choice is taken.
///
/// Two numbers because the inner one is not a fraction of the outer: the ring
/// has a border of its own, and a dot derived from the outside measurement
/// would grow into it at one size and float inside it at another.
const ASK_CHOICE_MARK: Rems = rems(1.0);
const ASK_CHOICE_DOT: Rems = rems(0.5);
/// The ring around that mark, in pixels and not rems: it is a hairline's job
/// done a touch heavier, and a hairline is the one measurement in the window
/// that must not scale with a panel's zoom — doubled, it stops being a line.
const ASK_MARK_RING: gpui::Pixels = gpui::px(1.5);
/// The key-hint chip at the end of a choice row: a floor rather than a size, so
/// a two-digit hint grows sideways instead of overrunning its border.
const ASK_HINT_SIZE: Rems = rems(1.25);
const ASK_HINT_PAD: Rems = rems(0.3125);
/// The height a choice row stands at whatever it holds.
///
/// A floor, not a height. The label and the description under it are the
/// agent's sentences, so the row grows with them; what this answers is the
/// other end — a row whose label is one word, which shrink-wrapped is a target
/// half the size of the one below it.
const ASK_ROW_MIN: Rems = rems(3.25);
/// The same floor for the free-text row, a step under it: that row is one line
/// by construction, and standing it at a two-line height would leave a band of
/// empty surface under an input that is nowhere near filling it.
const ASK_CUSTOM_ROW_MIN: Rems = rems(2.75);
/// The height of one tab, which is also the strip's.
const ASK_TAB_H: Rems = rems(2.25);
/// Leading for the question itself — looser than a control's and tighter than
/// prose, because it is one sentence that has to be read once and is as long as
/// the agent made it.
const ASK_PROMPT_LEADING: f32 = 1.4;
/// The box a plan entry's state is drawn in, and the mark inside a pending one.
///
/// A square rather than an icon, because what the column is asking is whether
/// a line is done — and a checkbox is the one shape a reader does not have to
/// be taught. The three states change what is *in* the box and never the box,
/// so a plan does not reflow as it is worked through.
const PLAN_BOX: Rems = rems(0.875);
const PLAN_DOT: Rems = rems(0.25);
/// The bar under a plan's heading. A rule's weight rather than a control's:
/// it is read in one glance beside a count that already says the same thing.
const PLAN_BAR_H: Rems = rems(0.1875);
/// An attached file's plate beside a prompt.
///
/// **A fixed plate, not a bounded picture.** A preview is here to be
/// recognised — it answers "which screenshot did I send" and nothing more, and
/// the file itself is one click away in whatever opens it. Bounded only in
/// height, a row of them came out a different width each and the row read as
/// ragged rather than as a set; at one size they are a strip of cards, and a
/// file with no picture takes the same plate with its name in it.
const THUMB_W: Rems = rems(5.75);
const THUMB_H: Rems = rems(3.875);
/// How much of the row a user prompt may take before it wraps.
///
/// The prompt is the one block that shrinks to what was typed: a one-line
/// question stretched edge to edge is indistinguishable from an answer, and
/// telling the two apart at a glance is the whole reason it sits on the other
/// side. Past this it wraps rather than crowding the answer beneath it.
///
/// The wider figure is for a narrow panel, where the column is already short
/// enough that holding a fifth of it clear costs more than the edge is worth.
const USER_BUBBLE_MAX: f32 = 0.85;
const USER_BUBBLE_MAX_NARROW: f32 = 0.9;
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
#[derive(IntoElement)]
struct BlockingBody {
    target: TranscriptItemId,
    children: Vec<gpui::AnyElement>,
    /// Whether the rows are held off the right edge to leave the scrollbar
    /// thumb a column of its own.
    gutter: bool,
}

impl BlockingBody {
    fn new(target: TranscriptItemId, children: Vec<gpui::AnyElement>) -> Self {
        Self {
            target,
            children,
            gutter: true,
        }
    }

    /// Give the gutter up, so these rows run to the same right edge as whatever
    /// is drawn beside the box.
    ///
    /// **For the one caller whose list continues outside it.** A question's
    /// free-text answer is the last row of its options and is drawn below this
    /// box rather than inside it, because it is a control and must not scroll
    /// away from the card that offers it -- so the gutter made four rows that
    /// read as one list end at two different edges, which is a mistake rather
    /// than a margin. What it costs is the case that gutter is for: a question
    /// with enough options to scroll draws its thumb over the right-hand
    /// border of a row, which is a hairline crossed rather than a row cut off.
    fn flush(mut self) -> Self {
        self.gutter = false;
        self
    }
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
            .w_full()
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
                    // The thumb overlays the viewport instead of taking a column
                    // of its own, so the body is held off the right edge: a
                    // choice's border running underneath the thumb reads as a
                    // row drawn wrong rather than as one that scrolls.
                    .when(self.gutter, |body| body.pr_2())
                    .children(self.children),
            )
            // The mask takes vertical wheel input in the capture phase. A bubble
            // listener runs too late inside `gpui::list`: the transcript has
            // already spent the same delta scrolling itself by then.
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("blocking-body-mask", key)))
            .child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
    }
}

/// Render one transcript item.
///
/// `target` addresses fold toggles back into the model, and carries whether the
/// item came from the read-only resumed history or the live tail: history and
/// live items are two collections, so a fold addressed by render position lands
/// in the wrong one.
/// How much room the block is being drawn in, and whether it is one of a
/// group's own rows.
///
/// One value rather than three arguments threaded through a dozen signatures,
/// and the two facts travel together because they are read off the same frame:
/// a caller that passed a fresh width beside a stale panel height would be
/// sizing one half of a block against a layout the other half never saw.
#[derive(Clone)]
pub struct Room {
    /// The panel's own height, for the blocks that bound themselves against the
    /// room they have rather than against the window.
    pub well: Option<gpui::Pixels>,
    /// A column too short to hold a block off its edges and still leave a line
    /// worth reading.
    pub narrow: bool,
}

impl Room {
    pub fn new(well: Option<gpui::Pixels>, narrow: bool) -> Self {
        Self { well, narrow }
    }
}

pub fn item(
    session: &Entity<ChatSession>,
    it: &ChatItem,
    target: TranscriptItemId,
    find_emphasis: Option<bool>,
    room: Room,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let chat = &session.read(cx).chat;
    // Only an answer has a footer, and only its turn's last one draws it.
    let turn = matches!(it, ChatItem::Agent(_))
        .then(|| chat.turn_answer(target))
        .flatten();
    let body = match it {
        ChatItem::User(u) => user(u, room, cx).into_any_element(),
        ChatItem::Agent(md) => agent(session, md, target, turn, window, cx).into_any_element(),
        ChatItem::Thought(th) => thought(session, th, target, window, cx).into_any_element(),
        ChatItem::Tool(t) => tool(session, t, target, cx).into_any_element(),
        ChatItem::Plan(p) => plan(session, p, target, cx).into_any_element(),
        ChatItem::Permission(p) => permission(session, p, target, room.well, cx).into_any_element(),
        ChatItem::Ask(a) => ask(session, a, target, cx).into_any_element(),
        ChatItem::Notice { text, level } => notice(text, *level, cx).into_any_element(),
    };

    // Width is owned by the pane's run so an activity summary drawn by the
    // pane and the steps rendered here always share the same two edges.
    div()
        .w_full()
        .min_w_0()
        // Search used to move to a matching item without marking what in the
        // viewport had changed. Keep the marker on the row's existing box so
        // opening or closing Find never changes transcript geometry. Every hit
        // gets the quiet list fill; the current hit gets the stronger selected
        // fill used by the find controls themselves.
        .when_some(find_emphasis, |row, current| {
            row.rounded(radius_control(cx)).bg(if current {
                cx.theme().accent.opacity(0.35)
            } else {
                cx.theme().list_hover
            })
        })
        .child(body)
}

// ── user prompt — filled, shrink-to-fit, against the right edge ─────────────

/// What the user asked, on its own side of the column.
///
/// **The side is the label.** Every other block in the transcript is the
/// agent's, so the one thing a reader scanning back through a long
/// conversation is looking for — where they last asked something — is also the
/// only thing that has to be findable without reading. A fill alone said that
/// when the transcript was two blocks long; twenty blocks down it is one more
/// box among boxes, while an edge is still an edge.
///
/// **What was handed over is not what was typed, so it sits outside the
/// bubble.** A picture inside the fill reads as part of the sentence and is
/// bounded by the sentence's box; a prompt that was nothing but a screenshot
/// drew an empty filled card above it, which says the user sent a blank
/// message. Both keep the right edge, because the edge is what says whose they
/// are, and the files come first — they were handed over before the question
/// was asked about them.
fn user(u: &UserMsg, room: Room, cx: &App) -> impl IntoElement + use<> {
    let over = u.attachments.len().saturating_sub(MAX_ATTACHMENT_ROWS);
    let share = match room.narrow {
        true => USER_BUBBLE_MAX_NARROW,
        false => USER_BUBBLE_MAX,
    };

    div()
        .v_flex()
        .items_end()
        // Tighter than the gap between blocks, because these are the parts of
        // one thing said: what was handed over, the sentence asking about it,
        // and whatever is offered underneath.
        .gap(STACK_GAP)
        .w_full()
        .children((!u.attachments.is_empty()).then(|| {
            div()
                // **A strip, not a column.** Handed over together, they were
                // handed over at once, and stacked they pushed the question
                // itself further off the screen with every file added.
                .h_flex()
                .flex_wrap()
                .justify_end()
                .items_end()
                .gap(STACK_GAP)
                .max_w(relative(share))
                .children(
                    u.attachments
                        .iter()
                        .take(MAX_ATTACHMENT_ROWS)
                        .map(|a| attachment(a, cx)),
                )
                .when(over > 0, |list| {
                    list.child(
                        div()
                            .h(THUMB_H)
                            .h_flex()
                            .items_center()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("+{over} more attachment(s)")),
                    )
                })
        }))
        .children((!u.text.trim().is_empty()).then(|| {
            div()
                .max_w(relative(share))
                .py(TEXT_PAD_Y)
                .px(TEXT_PAD_X)
                .rounded(radius_bubble(cx))
                // **The corner nearest the speaker is the tight one.** A bubble
                // rounded evenly is a lozenge that could belong to either side;
                // one corner cut back points at the edge the prompt came from,
                // which is the whole of what the right-hand lane is saying.
                .rounded_br(radius_tag(cx))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .text_color(cx.theme().secondary_foreground)
                .child(u.text.clone())
        }))
}

/// One attached file: what it is, and — for a picture — what it looks like.
///
/// **The prompt is what the user wrote plus what they handed over, and "3
/// attachment(s)" is neither.** A count cannot be checked against what was
/// meant to be sent, so the mistake it hides — the wrong screenshot — reads as
/// correct right up until the answer is about the wrong picture. The name can
/// be wrong *visibly*; the thumbnail can be wrong at a glance.
///
/// The image is addressed **by path**, which is what makes it affordable here:
/// gpui loads and caches a path-sourced image off the UI thread, so a row
/// redrawn on every streamed chunk costs a cache lookup rather than a decode.
/// The archive keeps paths and not bytes, so this is also the only form the
/// picture still exists in once the conversation is reopened — and a file that
/// has since moved simply leaves the row as its name.
fn attachment(a: &onehand_core::attachment::AttachmentSnapshot, cx: &App) -> impl IntoElement {
    use onehand_core::attachment::{AttachmentDelivery, AttachmentKind};
    let unavailable = a.delivery == AttachmentDelivery::Unavailable;
    // Nothing to show for a file that was never sent: the thumbnail would say
    // the agent saw this picture.
    let thumbnail = (a.kind == AttachmentKind::Image && !unavailable).then(|| a.path.clone());

    div()
        // **One plate whatever is on it.** A picture fills it and a file
        // carries its mark in the middle of it, so a prompt that handed over
        // both is a strip of one kind of thing rather than a picture beside a
        // chip of some other height. The name sits at the foot of the plate: a
        // preview is read as the thing itself, and the name is its caption
        // rather than its label.
        .relative()
        .flex_none()
        .w(THUMB_W)
        .h(THUMB_H)
        .overflow_hidden()
        .rounded(radius_block(cx))
        .border_1()
        .border_color(cx.theme().border)
        .map(|plate| match thumbnail {
            Some(path) => plate.child(gpui::img(path).size_full()),
            // The registry has no picture glyph, so the plain file mark stands
            // in for both kinds; where there is a preview it says what the file
            // is better than any icon could, so the mark is drawn only here.
            None => plate.child(
                div()
                    .size_full()
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::File).size_4()),
            ),
        })
        .child(
            div()
                // Over the foot of the plate rather than under it: the plate is
                // a fixed size, and a caption taking a row of its own would
                // make every attachment two different heights again depending
                // on whether its name wrapped.
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h_flex()
                .items_center()
                .gap(TIGHT_GAP)
                .px(TIGHT_GAP)
                .py(HAIR_GAP)
                .bg(cx.theme().secondary)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(a.name.clone()))
                // An attachment the agent never received is the one thing about
                // this plate that changes the answer, so it is spelled out.
                .when(unavailable, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(crate::theme::status_ink(cx).danger)
                            .child("not sent"),
                    )
                }),
        )
}

// ── agent answer ────────────────────────────────────────────────────────────

/// One block of the agent's answer, wearing the turn's chrome at its two ends.
///
/// **A turn is one thing said by one speaker, and it is what the footer marks
/// — not this block.** An answer interrupted by three tool calls arrives as
/// four `Agent` items, so a Copy on each would copy a quarter of what it looks
/// like it copies. The model knows which block closes a turn
/// (`Chat::turn_answer`), so the footer goes on the last and Copy takes the
/// whole turn's prose rather than this fragment of it.
fn agent(
    session: &Entity<ChatSession>,
    md: &Md,
    target: TranscriptItemId,
    turn: Option<TurnAnswer>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    // Everything the footer needs, resolved before the borrow ends.
    let footer = turn.as_ref().and_then(|t| {
        // Not while the turn is still arriving: a Copy offered mid-stream
        // copies whatever had landed by the click, silently.
        (t.is_last && !t.is_active).then_some(t.elapsed_secs)
    });

    div()
        .v_flex()
        .gap(TIGHT_GAP)
        .w_full()
        .group("turn-footer")
        .child(md_view(session, md, window, cx))
        .children(footer.map(|elapsed| {
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap(PART_GAP)
                .w_full()
                // **The row holds its height whether or not anything is in
                // it.** Appearing under the pointer it would push the whole
                // conversation down by its own height every time the reader
                // crossed the last paragraph of an answer, which is a page
                // that moves while it is being read.
                .h(rems(1.75))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .invisible()
                .group_hover("turn-footer", |row| row.visible())
                // What can be done with the answer, at the end the reading
                // stopped at; what the answer *was*, opposite it. A control and
                // a record are two different offers and the row is read from
                // the left.
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(HAIR_GAP)
                        .flex_none()
                        .child(copy_turn_button(session, target).tooltip("Copy this answer")),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(TEXT_PAD_Y)
                        .min_w_0()
                        .truncate()
                        .children(elapsed.map(|secs| div().child(format!("Processed in {secs}s")))),
                )
        }))
}

/// The parsed markdown for `md`.
///
/// Falls back to the raw source rather than rendering nothing: a block the
/// cache has not seen is a bug, but a *silent* one would be a transcript with
/// a hole in it.
fn md_view(
    session: &Entity<ChatSession>,
    md: &Md,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    match session.read(cx).md_view(md) {
        Some(state) => TextView::new(state)
            .selectable(true)
            .style(prose_style(window, cx))
            .code_block_actions(|block, _, _| copy_button("copy-code", block.code()))
            .into_any_element(),
        None => div().child(md.source.clone()).into_any_element(),
    }
}

/// Prose styling for a transcript answer.
///
/// The height cap on code blocks is the bounded-rendering concern arriving by
/// a different route. The model can bound a long block by *folding* it, keyed
/// by fence-open order — but `TextViewStyle::code_block` is one style for
/// every block, so per-block fold state has nowhere to live — reaching it would mean
/// replacing gpui-component's code-block renderer through a custom block
/// parser, trading away its syntax highlighting to get a chevron. Capping the
/// height keeps the answer readable, which is what the fold was for; Copy on
/// the block is how the clipped tail stays reachable. The model carries no
/// fold state for it to key against.
///
/// **The rest of this is the renderer's defaults being wrong for a chat.**
/// `TextView` is a document renderer: its headings are scaled off a base of its
/// own choosing, and its code blocks are set from `Theme::mono_font_size`, an
/// absolute pixel value. Left alone that gives an answer whose `####` prints
/// *smaller* than the paragraph it names, whose `#` prints as a document title
/// inside a chat message, and whose code blocks are the one thing on screen
/// that per-panel zoom cannot reach — because zoom works by overriding the rem
/// base, and a pixel size is exactly what ignores it.
///
/// Both are reachable from here. The heading base is taken from the *current*
/// rem size, which is the zoomed one inside a zoomed panel, so headings scale
/// with the prose they belong to; the code block's size is written in rems, and
/// lands because the refinement is applied after the renderer's own text size.
fn prose_style(window: &Window, cx: &App) -> TextViewStyle {
    // Two steps up and one, then nothing: an answer's headings are section
    // marks inside one message, not the top of a document. Below the third
    // level, weight alone carries the hierarchy -- which is also what stops a
    // deep heading printing smaller than its own body text.
    let mut style = TextViewStyle::default()
        .paragraph_gap(PARAGRAPH_GAP)
        .heading_font_size(|level, base| match level {
            1 => base * 1.5,
            2 => base * 1.25,
            _ => base,
        })
        // Keep inline code distinct without putting a full-line-height square
        // behind it. `Some(transparent)` is intentional: `None` makes TextView
        // restore its accent-background fallback.
        //
        // **Colour alone, not colour and weight.** Mono is what would normally
        // mark this and the renderer cannot reach it -- inline code is styled
        // through a highlight that carries colour, weight, slant and background
        // and no font family -- so one substitute channel is chosen rather than
        // stacking two. Weight was the one dropped: a sentence naming five
        // symbols came out patched with semibold runs that read as the
        // markdown's own bold, which is a distinction prose actually uses.
        .inline_code(HighlightStyle {
            color: Some(crate::theme::hue_ink(cx.theme().blue, cx)),
            background_color: Some(cx.theme().transparent),
            ..HighlightStyle::default()
        })
        // The fenced block drawn as the wells around it are: the same corner,
        // the same edge, the same two insets. The renderer's own is a filled
        // box with no border at the control radius, which put a quoted command
        // in an answer and the identical command in the tool card below it in
        // two different boxes.
        .code_block(
            StyleRefinement::default()
                .max_h(MAX_CODE_BLOCK_H)
                .overflow_hidden()
                .py(TEXT_PAD_Y)
                .px(FRAME_PAD)
                .rounded(radius_block(cx))
                .border_1()
                .border_color(cx.theme().border)
                .text_size(CODE_TEXT)
                .line_height(relative(CODE_LEADING)),
        )
        // A table is read across, so its cells are wider than they are tall:
        // padding a cell evenly leaves the columns crowded and the rows loose,
        // which is the one way round a table must not be.
        .table_cell(StyleRefinement::default().py(STACK_GAP).px(TEXT_PAD_Y));
    style.heading_base_font_size = window.rem_size() * TEXT.0;
    style
}

/// Copy `text` to the clipboard, as a quiet icon button.
///
/// One constructor for both the copies the transcript offers — a fenced block
/// and a whole answer — because they are the same gesture and the only reason
/// they ever looked different was that they were written months apart.
fn copy_button(id: &'static str, text: impl Into<SharedString>) -> Button {
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
fn copy_turn_button(session: &Entity<ChatSession>, target: TranscriptItemId) -> Button {
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

// ── thought — collapsed reasoning, never contains tool calls ────────────────

fn thought(
    session: &Entity<ChatSession>,
    th: &Thought,
    target: TranscriptItemId,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    // **One row, whether it is still thinking or has finished.** The verb and
    // the mark change; nothing moves. It used to be a bare line at one height
    // when it stood alone and a row at another inside a group, so the same
    // block was two shapes depending on what happened to be next to it.
    let (verb, summary, mark) = match th.elapsed_secs {
        Some(secs) => ("Reasoned", format!("{secs}s"), RowMark::Done),
        None => ("Reasoning", String::new(), RowMark::Running),
    };
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_thought(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("thought", fold_key(target)).into(),
        mark,
        group_icon(activity::ActivityGroup::Reasoned),
        verb,
    )
    .object((!summary.is_empty()).then(|| Object::plain(summary)))
    .fold(Some(th.expanded));

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(th.expanded, |block| {
            block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(DETAIL_INSET)
                    .pr(ROW_PAD_X)
                    .pb(FRAME_PAD)
                    .text_color(cx.theme().muted_foreground)
                    .child(md_view(session, &th.md, window, cx)),
            )
        })
}

// ── tool call ───────────────────────────────────────────────────────────────

/// A tool step: one activity row, and what it opens into.
///
/// **One geometry for every kind and every status.** A status is what the disc
/// at the head of the row says and what the column at its end says; it is never
/// what the row *is*. What differs between a read and a command is only the
/// shape of the thing underneath, once somebody has asked for it.
fn tool(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let root = session.read(cx).chat.root.clone();
    let presented = activity::presentation(t);
    let open = t.is_open();
    let detail = tool_detail(t, &presented, &root);
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_tool(target);
                cx.notify();
            });
        }
    };

    let (added, removed) = t
        .diff_summary
        .iter()
        .fold((0, 0), |(a, r), (_, plus, minus)| (a + plus, r + minus));
    let deleted = t.call.kind == ToolKind::Delete;
    let object = match t.call.kind {
        // A command is not a path and must not be split at its last slash.
        ToolKind::Execute => Some(Object::plain(activity::first_line_trunc(
            &onehand_core::chat::redact(&presented.subject),
            160,
        ))),
        _ if presented.subject.trim().is_empty() => None,
        _ => Some(Object::path(path_for_display(&root, &presented.subject))),
    };
    let meta = match (t.call.status, deleted) {
        (ToolStatus::Failed, _) => {
            Some(row_note("failed", crate::theme::status_ink(cx).danger, cx))
        }
        (ToolStatus::InProgress, _) | (ToolStatus::Pending, _) => None,
        (_, true) => Some(row_note("deleted", cx.theme().muted_foreground, cx)),
        _ => line_counts(added, removed, cx),
    };

    let mut row = ActivityRow::new(
        ("tool", fold_key(target)).into(),
        RowMark::of(t.call.status),
        group_icon(presented.kind.into()),
        presented.action,
    )
    .object(object)
    .meta(meta)
    .fold(detail.is_some().then_some(open));
    row.struck = deleted;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .children(
            open.then_some(())
                .and(detail)
                .map(|detail| detail_frame(session, t, target, detail, cx)),
        )
}

/// What a step opens into, chosen by what the step is.
enum Detail {
    /// A command and whatever it printed.
    Command { command: String, output: String },
    /// One or more file edits.
    Diffs,
    /// A list of what was looked at.
    Lines(Vec<SharedString>),
    /// A picture the step produced.
    Image(std::sync::Arc<Vec<u8>>),
}

fn tool_detail(t: &ToolItem, presented: &activity::Presentation, root: &Path) -> Option<Detail> {
    if t.call.kind == ToolKind::Execute {
        let command = onehand_core::chat::redact(t.call.title.trim());
        let output = t
            .call
            .content
            .iter()
            .filter_map(|c| match c {
                ToolContent::Text(text) => Some(onehand_core::chat::redact(text)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        return (!command.is_empty()).then_some(Detail::Command { command, output });
    }
    if t.call
        .content
        .iter()
        .any(|c| matches!(c, ToolContent::Diff { .. }))
    {
        return Some(Detail::Diffs);
    }
    if let Some(ToolContent::Image(bytes)) = t
        .call
        .content
        .iter()
        .find(|c| matches!(c, ToolContent::Image(_)))
    {
        return Some(Detail::Image(bytes.clone()));
    }
    // A read or a search says what it looked at, one line each — the paths a
    // merged row stands for, or whatever the tool printed back.
    let lines: Vec<SharedString> = t
        .call
        .content
        .iter()
        .filter_map(|c| match c {
            ToolContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .flat_map(|text| text.lines())
        .map(|line| SharedString::from(onehand_core::chat::redact(line)))
        .take(MAX_MONO_LINES)
        .collect();
    match lines.is_empty() {
        true => {
            let subject = path_for_display(root, &presented.subject);
            (!subject.trim().is_empty()).then(|| Detail::Lines(vec![subject.into()]))
        }
        false => Some(Detail::Lines(lines)),
    }
}

/// The box a row opens into: set in to the row's own words, one step off the
/// frame it sits in, and carrying nothing but the text.
fn detail_frame(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    detail: Detail,
    cx: &App,
) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pl(DETAIL_INSET)
        .pr(ROW_PAD_X)
        .pb(FRAME_PAD)
        .child(
            div()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .rounded(radius_block(cx))
                .border_1()
                .border_color(cx.theme().border)
                // **A step off the frame and no more.** It has to read as a
                // layer rather than as a card somebody dropped in, and the
                // frame it sits in is already on the reading surface — so the
                // whole of the difference is one step of the ramp.
                .bg(cx.theme().muted.opacity(0.4))
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(OBJECT_TEXT)
                .line_height(relative(CODE_LH))
                .map(|box_| match detail {
                    Detail::Command { command, output } => {
                        command_detail(session, t, target, &command, &output, box_, cx)
                    }
                    Detail::Diffs => diff_detail(session, t, target, box_, cx),
                    // An image has an edge of its own already -- the box it is
                    // in -- so it fills it rather than sitting inside a second
                    // one.
                    Detail::Image(bytes) => box_.map(|box_| match session.read(cx).image(&bytes) {
                        Some(handle) => box_.child(gpui::img(handle).max_w_full()),
                        None => box_.child(
                            div()
                                .px(DIFF_TEXT_PAD)
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("[unrecognized image, {} bytes]", bytes.len())),
                        ),
                    }),
                    Detail::Lines(lines) => box_.children(lines.into_iter().map(|line| {
                        div()
                            .w_full()
                            .min_w_0()
                            .px(DIFF_TEXT_PAD)
                            .text_color(cx.theme().muted_foreground)
                            .child(line)
                    })),
                }),
        )
}

/// Where whatever a row opens into is set in to: the row's own words, with the
/// frame's inset kept on the right and the foot.
fn detail_well(body: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .pl(DETAIL_INSET)
        .pr(ROW_PAD_X)
        .pb(FRAME_PAD)
        .child(body)
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
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("detail-mask", key)))
            .child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
    }
}

/// A detail's body, scrolling inside its box only once it has been opened.
fn scrolled(open: bool, key: usize, body: gpui::Div) -> gpui::AnyElement {
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
fn chevron_slot(fold: Option<bool>, cx: &App) -> gpui::Div {
    div()
        .size(CHEVRON_SLOT)
        .flex_none()
        .h_flex()
        .items_center()
        .justify_center()
        .children(fold.map(|open| {
            Icon::new(match open {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            })
            .size(CHEVRON_MARK)
            .text_color(cx.theme().muted_foreground)
        }))
}

/// The box a detail is drawn in, without the row wrapper around it.
fn plain_box(cx: &App) -> gpui::Div {
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

/// A command, then what it printed.
///
/// **No `IN` and no `OUT`.** The `$` says which is which, and the rule under it
/// says where one ends — two labels in a fixed column were four characters of
/// chrome per section and a column taken off the widest text in the transcript.
fn command_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    command: &str,
    output: &str,
    box_: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let lines: Vec<&str> = output.lines().collect();
    let hidden = match open {
        true => 0,
        false => lines.len().saturating_sub(PREVIEW_OUT),
    };
    let shown: Vec<SharedString> = lines
        .iter()
        .skip(hidden)
        .take(MAX_MONO_LINES)
        .map(|line| SharedString::from(line.to_string()))
        .collect();
    let danger = crate::theme::status_ink(cx).danger;

    box_.v_flex()
        .child(
            div()
                .w_full()
                .min_w_0()
                .h_flex()
                .items_start()
                .gap(TIGHT_GAP)
                .py(STACK_GAP)
                .px(FRAME_PAD)
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .child("$"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(cx.theme().foreground)
                        .child(command.to_string()),
                ),
        )
        .children((!shown.is_empty()).then(|| {
            div()
                .w_full()
                .min_w_0()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        // **Collapsed it does not scroll at all.** A preview
                        // short enough to read whole has nothing to scroll, and
                        // a box that scrolled anyway would be a second scroller
                        // under the reader's finger for no reason.
                        .child(scrolled(
                            open,
                            fold_key(target),
                            div().v_flex().w_full().min_w_0().py(STACK_GAP).children(
                                shown.into_iter().map(|line| {
                                    // A failure names itself in what it printed,
                                    // so the ink follows the words rather than
                                    // the row: an error in the middle of a
                                    // hundred quiet lines is the one somebody
                                    // is looking for.
                                    let bad = is_error_line(&line);
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .px(FRAME_PAD)
                                        .text_color(match bad {
                                            true => danger,
                                            false => cx.theme().muted_foreground,
                                        })
                                        .child(line)
                                }),
                            ),
                        ))
                        // **The cut fades at the top, because the tail is what
                        // was kept.** A command's failure is the last thing it
                        // printed, so the preview is its end and what is hidden
                        // is above it.
                        .when(hidden > 0, |body| {
                            body.child(div().absolute().top_0().left_0().right_0().h(SMOKE_OUT).bg(
                                gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(cx.theme().muted, 0.15),
                                    gpui::linear_color_stop(cx.theme().muted.alpha(0.), 1.),
                                ),
                            ))
                        }),
                )
        }))
        .children((hidden > 0 || open).then(|| {
            fold_pill(
                session,
                target,
                match open {
                    true => "Show less".to_string(),
                    false => format!("Show {hidden} earlier lines"),
                },
                open,
                cx,
            )
        }))
}

/// Whether a line of output is the part somebody went looking for.
fn is_error_line(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    ["error", "failed", "panic", "fatal", "assertion"]
        .iter()
        .any(|mark| lower.starts_with(mark))
}

/// Every edit the step made, one diff after another.
fn diff_detail(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    box_: gpui::Div,
    cx: &App,
) -> gpui::Div {
    let open = t.out_open.contains(&0);
    let changed: usize = t
        .diff_summary
        .iter()
        .map(|(_, plus, minus)| plus + minus)
        .sum();
    // **Offered rather than drawn.** A diff this size is searched and not read,
    // and every line of it is an element in a list that is already virtualising
    // rows for the same reason.
    if changed > LARGE_DIFF && !open {
        return box_.child(
            div()
                .w_full()
                .min_w_0()
                .py(STACK_GAP)
                .px(FRAME_PAD)
                .text_color(cx.theme().muted_foreground)
                .child(format!("Large diff · {changed} changed lines"))
                .child(fold_pill(session, target, "Load diff", false, cx)),
        );
    }

    let root = session.read(cx).chat.root.clone();
    let mut budget = MAX_DIFF_LINES;
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let files = t
        .call
        .content
        .iter()
        .filter(|c| matches!(c, ToolContent::Diff { .. }))
        .count();
    for (key, content) in t.call.content.iter().enumerate() {
        let ToolContent::Diff { path, .. } = content else {
            continue;
        };
        // **A header only where there is more than one file**, and it is the
        // way into the editor: reviewing a diff and wanting to touch it up is
        // one motion, not two. With a single file the row above already names
        // it, and a second copy an inch under the first says nothing twice.
        if files > 1 {
            rows.push(diff_path(session, &root, key, path, cx));
        }
        let hunks = t.diff_rows.get(&key).map(Vec::as_slice).unwrap_or_default();
        rows.extend(diff_rows(hunks, &mut budget, cx));
    }
    let total = rows.len();
    let shown = match open {
        true => total,
        false => total.min(PREVIEW_DIFF),
    };
    let hidden = total - shown;
    rows.truncate(shown);

    box_.child(
        div()
            .relative()
            .w_full()
            .min_w_0()
            .child(scrolled(
                open,
                fold_key(target),
                div().v_flex().w_full().min_w_0().children(rows),
            ))
            // The cut fades at the foot: a diff opens on its change, so what is
            // held back is what comes after.
            .when(hidden > 0, |body| {
                body.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(SMOKE_DIFF)
                        .bg(gpui::linear_gradient(
                            0.,
                            gpui::linear_color_stop(cx.theme().muted, 0.15),
                            gpui::linear_color_stop(cx.theme().muted.alpha(0.), 1.),
                        )),
                )
            }),
    )
    .children((hidden > 0 || open).then(|| {
        fold_pill(
            session,
            target,
            match open {
                true => "Show less".to_string(),
                false => format!("Show {hidden} more lines"),
            },
            open,
            cx,
        )
    }))
}

/// Which file the rows under it belong to, and the way into it.
fn diff_path(
    session: &Entity<ChatSession>,
    root: &Path,
    key: usize,
    path: &str,
    cx: &App,
) -> gpui::AnyElement {
    let shown = path_for_display(root, path);
    // Resolved here, where the project root is known: the Workbench opens
    // whatever path it is handed, and a relative one there resolves against the
    // process working directory.
    let full = onehand_core::editor::resolve_in_root(root, path);
    let session = session.clone();
    crate::controls::action(("diff-path", key))
        .ghost()
        .w_full()
        .min_w_0()
        .h_auto()
        .py(TIGHT_GAP)
        .px(DIFF_TEXT_PAD)
        .rounded_none()
        .label(shown)
        .text_color(crate::theme::hue_ink(cx.theme().blue, cx))
        .on_click(move |_, _, cx: &mut App| {
            session.update(cx, |_, cx| {
                cx.emit(super::session::ChatEvent::OpenFile(full.clone()))
            });
        })
        .into_any_element()
}

/// One diff, as three columns that hold whatever the text does.
fn diff_rows(hunks: &[DiffRow], budget: &mut usize, cx: &App) -> Vec<gpui::AnyElement> {
    let status = crate::theme::status_ink(cx);
    let mut out = Vec::new();
    let mut n = 0usize;
    for line in hunks {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        // **A skipped run is a line of its own, in the ink that means it can be
        // opened.** Blue is the transcript's one word for "there is more behind
        // this", and an elided run is exactly that -- left in the quiet ink it
        // read as a remark about the file rather than as a way into it.
        if let DiffRow::Skipped(count) = line {
            out.push(
                div()
                    .w_full()
                    .min_w_0()
                    .px(DIFF_TEXT_PAD)
                    .bg(crate::theme::hue_ink(cx.theme().blue, cx).alpha(0.06))
                    .text_size(rems(0.71875))
                    .text_color(crate::theme::hue_ink(cx.theme().blue, cx))
                    .child(format!("{count} unchanged lines"))
                    .into_any_element(),
            );
            n += count;
            continue;
        }
        n += 1;
        let (sign, ink, wash) = match line {
            DiffRow::Added(_) => ("+", status.success, Some(status.success.alpha(0.08))),
            DiffRow::Removed(_) => ("−", status.danger, Some(status.danger.alpha(0.08))),
            _ => (" ", cx.theme().muted_foreground, None),
        };
        let text = match line {
            DiffRow::Context(l) | DiffRow::Added(l) | DiffRow::Removed(l) => l.clone(),
            DiffRow::Skipped(_) => unreachable!(),
        };
        out.push(
            div()
                .h_flex()
                // **Top, not centre.** A line that wraps has to keep its number
                // and its sign level with its *first* row, or the column down
                // the side stops being a ruler the moment anything is long.
                .items_start()
                .w_full()
                .min_w_0()
                .when_some(wash, |row, wash| row.bg(wash))
                .child(
                    div()
                        .flex_none()
                        .w(DIFF_NUM_W)
                        .px(DIFF_NUM_PAD)
                        .text_right()
                        .whitespace_nowrap()
                        .text_color(cx.theme().muted_foreground.opacity(0.6))
                        .child(format!("{n}")),
                )
                .child(
                    div()
                        .flex_none()
                        .w(DIFF_SIGN_W)
                        .text_center()
                        .text_color(ink)
                        .child(sign),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pr(DIFF_TEXT_PAD)
                        .text_color(cx.theme().foreground)
                        .child(text),
                )
                .into_any_element(),
        );
    }
    out
}

/// The control that opens a detail and the one that shuts it, which are one
/// control.
///
/// A pill rather than a bare word: it sits *over* the fade at the cut, so it
/// needs a plate of its own or it is read against the text it is covering.
fn fold_pill(
    session: &Entity<ChatSession>,
    target: TranscriptItemId,
    label: impl Into<SharedString>,
    open: bool,
    cx: &App,
) -> gpui::Div {
    let session = session.clone();
    div()
        .w_full()
        .h_flex()
        .justify_center()
        // Opened, the control is outside the scrolling box and needs the rule
        // that says so; closed, it is floating over the fade and must not draw
        // a second edge across it.
        .when(open, |row| row.border_t_1().border_color(cx.theme().border))
        .py(STACK_GAP)
        .child(
            crate::controls::action(("detail-fold", fold_key(target)))
                .ghost()
                .py(PILL_PAD_Y)
                .px(PILL_PAD_X)
                .h_auto()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .label(label.into())
                .on_click(move |_, _, cx: &mut App| {
                    session.update(cx, |s, cx| {
                        s.chat.toggle_tool_output(target, 0);
                        cx.notify();
                    });
                }),
        )
}

fn path_for_display(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

// ── plan / TodoWrite ────────────────────────────────────────────────────────

/// The agent's checklist.
///
/// Folds like every other card, and force-opens while an entry is in progress
/// — a plan is only worth the space it takes while it is being worked through,
/// and a finished twenty-item list between two answers is twenty rows of
/// history nobody is reading. Both rules are the model's (`PlanItem::is_open`),
/// the same pair a tool card follows.
fn plan(
    session: &Entity<ChatSession>,
    p: &PlanItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = p.is_open();
    let done = p
        .entries
        .iter()
        .filter(|e| e.status == PlanStatus::Completed)
        .count();
    let rows = p
        .entries
        .iter()
        .take(if open { MAX_TODO_ITEMS } else { 0 })
        .map(|entry| {
            // Pending draws a dot rather than an icon: there is no glyph for
            // "not started" that is not just noise, and the row still needs to
            // occupy the marker column so the contents stay aligned.
            // **The box never changes size, only what is in it.** A plan is
            // worked through while it is on screen, so an entry moving from
            // pending to running to done is the one thing here guaranteed to
            // happen under the reader's eye — and a marker that grew or shrank
            // as it changed would reflow every line beneath it each time.
            let ink = match entry.status {
                PlanStatus::Completed => crate::theme::status_ink(cx).success,
                PlanStatus::InProgress => crate::theme::status_ink(cx).warning,
                PlanStatus::Pending => cx.theme().muted_foreground,
            };
            div()
                .h_flex()
                .items_center()
                .gap(PART_GAP)
                .w_full()
                .min_w_0()
                .h(BUTTON_H)
                .child(
                    div()
                        .flex_none()
                        .size(PLAN_BOX)
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius_tag(cx))
                        .border_1()
                        .border_color(ink)
                        .text_color(ink)
                        .map(|box_| match entry.status {
                            PlanStatus::Completed => {
                                box_.child(Icon::new(IconName::Check).size_3())
                            }
                            // Running is the border alone, because a second
                            // glyph in this column would have to be learned
                            // before the column could be read at all.
                            PlanStatus::InProgress => box_,
                            // A dot rather than an empty box: nothing at all
                            // reads as a box that failed to draw its tick.
                            PlanStatus::Pending => {
                                box_.child(div().size(PLAN_DOT).rounded_full().bg(ink))
                            }
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        // A struck-through line is finished work: the eye skips
                        // it and lands on what is left, which is the only part
                        // of a plan anyone is reading it for.
                        .when(entry.status == PlanStatus::Completed, |row| {
                            row.line_through().text_color(cx.theme().muted_foreground)
                        })
                        .child(entry.content.clone()),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let hidden = if open {
        p.entries.len().saturating_sub(rows.len())
    } else {
        0
    };
    let total = p.entries.len().max(1);
    // The header and checklist are separate regions of the card. The list is
    // intentionally denser inside itself, but it must not pull its first row
    // closer to the title than a tool card pulls detail to its header.
    let body = (!rows.is_empty() || hidden > 0).then(|| {
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .pt(PART_GAP)
            .children(rows)
            .when(hidden > 0, |list| {
                list.child(
                    div()
                        .h(BUTTON_H)
                        .h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{hidden} more")),
                )
            })
    });

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .py(FRAME_PAD)
        .px(TEXT_PAD_X)
        .rounded(radius_block(cx))
        .border_1()
        .border_color(cx.theme().border)
        // A checklist is the agent's working notes, drawn at the size the rest
        // of its working steps are: the entries are read down as a list, not
        // across as prose, and at the answer's size a twenty-item plan is a
        // wall between two paragraphs.
        .text_size(WORK_TEXT)
        .child(
            crate::controls::action(("plan", fold_key(target)))
                .ghost()
                .w_full()
                .min_w_0()
                .h(LINE_H)
                .p_0()
                .on_click({
                    let session = session.clone();
                    move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            s.chat.toggle_tool(target);
                            cx.notify();
                        });
                    }
                })
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(PART_GAP)
                        .w_full()
                        .min_w_0()
                        .child(chevron_slot(Some(open), cx))
                        .child(
                            div()
                                .flex_none()
                                .font_semibold()
                                .text_color(cx.theme().foreground)
                                .child("Plan"),
                        )
                        .child(div().flex_1())
                        // Collapsed, the header is the whole card, so it has to
                        // say what the list said: how much of it is done.
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{done}/{}", p.entries.len())),
                        ),
                ),
        )
        // **The same count as a length.** The figure beside the title is exact
        // and is read a digit at a time; this is the one that answers "nearly
        // done or barely started" without being read at all. A rule's weight
        // rather than a control's, because it is not one — nothing here can be
        // dragged, and a bar thick enough to look draggable says it can.
        .child(
            div()
                .w_full()
                .h(PLAN_BAR_H)
                .mt(STACK_GAP)
                .rounded_full()
                .bg(cx.theme().border)
                .child(
                    div()
                        .h_full()
                        .w(relative(done as f32 / total as f32))
                        .rounded_full()
                        .bg(crate::theme::status_ink(cx).success),
                ),
        )
        .children(body)
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
fn grows(button: Button) -> Button {
    button.small().h(Length::Auto).min_h(CONTROL_ROW).py_1p5()
}

/// The floor a control on a blocking card stands at.
///
/// Carried here as a number because the library hard-codes its row heights in a
/// sizing branch and offers no way to ask for one — and it is worth matching
/// exactly, because these controls sit a few inches from the composer's, and a
/// floor a pixel or two off is a line of controls that no longer agree.
///
/// It is the library's **small** row and not the default one it used to be. The
/// composer's own controls all came down to that height, and a card floating an
/// inch above them with buttons half a rem taller read as a dialog that had
/// landed there rather than as the next thing in the same column. A blocking
/// card is loud enough by being the thing everything is waiting on; it does not
/// also need the biggest buttons on screen.
const CONTROL_ROW: Rems = rems(1.5);

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
pub(super) fn floating_card(cx: &App) -> gpui::Div {
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

/// How much of the window an opened command block may take before it scrolls
/// inside itself.
///
/// **A share of the viewport and not a fixed height**, unlike every other
/// bound in this file: the two things that must stay on screen whatever the
/// command does are the heading that says what is being asked and the buttons
/// that answer it, and what is left between them is whatever the window
/// happens to be tall. A fixed rem bound picked for a laptop leaves half a
/// large screen unused and pushes the buttons off a small one.
const COMMAND_OPEN_SHARE: f32 = 0.5;
/// The block's copy button, and the inset it keeps from the block's corner.
const COPY_SIZE: Rems = rems(1.75);
const COPY_ICON: Rems = rems(0.875);
const BLOCK_INSET: Rems = rems(0.375);
/// The fold control at the foot of a block, and the fade it sits on.
const FOLD_ROW: Rems = rems(1.5);
/// Roughly one mono character at the well's own size — what a line number's
/// column is measured in, since the gutter has to be as wide as the largest
/// number and no wider.
const MONO_ADVANCE: f32 = 0.62;
/// How tall the command block stands while it is folded.
///
/// **The fold is a height, and only then a line count.** Slicing the agent's
/// newlines is what decides *which* lines are drawn, and it is the predictable
/// rule for that -- but it cannot bound one line three thousand characters
/// long, which wraps to a screenful and is still one line. Given the same
/// height as eight short ones, every command folds to the same box whatever
/// shape its text is, and the control that opens it is offered on the same
/// terms.
const FOLD_H: Rems = rems(CODE_TEXT.0 * CODE_LEADING * COMMAND_FOLD_LINES as f32);

/// The command a permission is asking to run.
///
/// **An element of its own because it needs the window.** How far it may open
/// is a share of the viewport, and the keys that answer the card hang off a
/// focus handle the window owns — neither is reachable from a plain builder,
/// and both belong to the one box that holds the command rather than to the
/// pane four levels up.
///
/// What it draws is the agent's text and nothing else: the lines are the
/// newlines the agent wrote, in the order it wrote them, wrapped where the box
/// runs out rather than reflowed. A command edited on its way to the screen is
/// a command approved in one form and run in another.
#[derive(IntoElement)]
struct CommandBlock {
    session: Entity<ChatSession>,
    target: TranscriptItemId,
    /// The whole command, which is what Copy hands back whether or not the
    /// block is folded.
    command: SharedString,
    lines: Vec<SharedString>,
    /// Real lines behind the fold; zero when the block is whole.
    hidden: usize,
    /// How tall the panel this is drawn in was last frame, which is what the
    /// opened block is bounded against. `None` before the list has measured
    /// itself, where the window is the only answer there is.
    well: Option<gpui::Pixels>,
    /// Whether the command has more lines than the block draws unopened, which
    /// stays true once it has been opened and `hidden` has gone back to zero.
    /// Asked of the model rather than worked out from `hidden` here: where the
    /// fold falls is a rule about the command, and a second spelling of it at
    /// this call site is a second place for it to move.
    long: bool,
    total: usize,
    expanded: bool,
}

impl RenderOnce for CommandBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = fold_key(self.target);
        let folded = self.hidden > 0;
        let long = self.long;
        // **Always a gutter, on every command.** It was drawn only past the
        // second line, on the reasoning that a single line has nothing to be
        // told apart from -- which is true of the numbering and false of
        // everything else the column does. A block with no gutter is a
        // different-looking card, and which card a permission gets was decided
        // by whether the agent happened to put a newline in it: two grants an
        // inch apart in one transcript, drawn as two kinds of thing, neither of
        // them the reader's doing. The width is the digits of the count, so the
        // one-line case costs a single character.
        let gutter = rems(MONO_ADVANCE * CODE_TEXT.0 * self.total.to_string().len() as f32);
        // **A share of the panel this is drawn in, not of the window.** What
        // the bound is for is the card's own heading staying on screen with the
        // command it belongs to, and the card is in the conversation -- so with
        // a dock open, half the window is taller than the whole panel and an
        // opened command pushes *Permission required* off the top, which is the
        // one thing the share was put here to stop. The window is the fallback
        // for the frame before the list has measured itself, where it is the
        // only answer there is.
        let ceiling =
            self.well.unwrap_or_else(|| window.viewport_size().height) * COMMAND_OPEN_SHARE;
        // The command scrolls inside a `gpui::list` row, so it needs a handle
        // of its own and a mask over it: a bubble listener runs too late there,
        // the transcript having already spent the same wheel delta scrolling
        // itself. Without this an opened command is a box the wheel slides the
        // conversation behind.
        let scroll = window
            .use_keyed_state(("perm-command-scroll-state", key), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();
        // **Whether the *box* ran out of height, which is not the question the
        // fold asks.** The fold counts the newlines the agent wrote, and that
        // is right for a fold: measured in drawn rows instead it would close a
        // two-line command on a narrow pane and leave a ten-line one open on a
        // wide one, which is a fold nobody can predict. But a single line three
        // thousand characters long answers *no* to that question and fills the
        // well anyway -- so every affordance hung off the fold went away in the
        // one case where the text runs past the bottom edge with nothing
        // holding it back. One line, one `curl`, no gutter to number, no lines
        // held back to unfold, and the reader with no way to tell that what
        // they can see is not all of it.
        //
        // Last frame's, like everything else measured here. The first frame of
        // a command draws without these and the second has them, which is a
        // frame nobody can see.
        let overflows = scroll.max_offset().y > gpui::px(0.);
        // Text out of sight *below what is drawn*, by either route: lines the
        // fold is holding back, or a box scrolled somewhere above its own end.
        let more_below = folded || scroll.max_offset().y - scroll.offset().y.abs() > gpui::px(1.);

        plain_box(cx)
            .relative()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .py_2p5()
            .pl_3()
            // The copy button's own column, kept clear of the text rather than
            // laid over it: a button that covers the first line of a command
            // covers the part of it somebody is most likely to be reading.
            .pr(COPY_SIZE + BLOCK_INSET + BLOCK_INSET)
            .child(
                div()
                    .id(("perm-command-scroll", key))
                    .v_flex()
                    .w_full()
                    // **Folded, it is a height; opened, it is a share of the
                    // panel.** Both are bounds on drawn rows rather than on the
                    // agent's newlines, which is the only kind of bound that
                    // holds for a command of one very long line -- eight real
                    // lines of a base64 blob is still a screenful of wrapped
                    // rows, and one line of it is too.
                    .max_h(match self.expanded {
                        true => ceiling,
                        false => FOLD_H.to_pixels(window.rem_size()).min(ceiling),
                    })
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(self.lines.into_iter().enumerate().map(|(n, line)| {
                        let row = div().h_flex().items_start().gap_3().w_full();
                        row.child(
                            div()
                                .flex_none()
                                .w(gutter)
                                .text_right()
                                // The numbers are a ruler and have to stay one
                                // column: wrapped, they would renumber
                                // themselves down the side of the command.
                                .whitespace_nowrap()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}", n + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // A blank line in a script is a line, and an
                                // empty box is no rows tall — which slides
                                // every number after it up against the wrong
                                // line of the command.
                                .child(match line.is_empty() {
                                    true => SharedString::from(" "),
                                    false => line,
                                }),
                        )
                    })),
            )
            // Over the text and under the copy button, which is added after it:
            // the mask takes the wheel in the capture phase and nothing else,
            // so a press still reaches whatever is drawn on top of it.
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("perm-command-mask", key)))
            .child(
                crate::controls::action(("perm-copy", key))
                    .ghost()
                    .absolute()
                    .top(BLOCK_INSET)
                    .right(BLOCK_INSET)
                    .size(COPY_SIZE)
                    .icon(Icon::new(IconName::Copy).size(COPY_ICON))
                    .tooltip("Copy the whole command")
                    // **Drawn on every command, never waiting to be hovered.**
                    // It was hidden until the pointer arrived on anything short,
                    // as one more thing beside a command that fits whole -- but
                    // a control that appears under the pointer is a control
                    // nobody finds who was not already reaching for it, and this
                    // is the card where the text is most likely to be wanted
                    // somewhere else: read elsewhere, pasted into a shell, kept
                    // for the record of what was approved. A command that
                    // scrolls cannot be selected out by dragging either, since
                    // the drag that reaches its end is the drag that moves the
                    // box.
                    .on_click(move |_, _, cx: &mut App| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            self.command.to_string(),
                        ));
                    }),
            )
            // **What says the command does not end where the box does**, and it
            // answers that question rather than the fold's. Lines held back
            // behind a fold and a box scrolled short of its own end are the
            // same fact to a reader, and the second of them used to draw
            // nothing at all -- the justification being that a scrollbar was
            // already saying it, which was true of the blocking body beside
            // this and never of this. It goes as soon as the end is reached, so
            // it is never a gradient laid over the last line of a command
            // somebody is being asked to approve.
            .when(more_below, |block| {
                block.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(FOLD_ROW + FOLD_ROW)
                        .bg(gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(cx.theme().muted.alpha(0.), 0.),
                            gpui::linear_color_stop(cx.theme().muted, 0.75),
                        )),
                )
            })
            // Drawn over the fade rather than under it, and **only once the
            // block has been opened**. Folded, the way to the rest of the
            // command is the control at the corner and the scrollbar would be a
            // second, quieter answer to the same question -- one that moves the
            // text without ever saying how much there is. Opened, it is the only
            // thing that says how far this runs.
            .when(overflows && self.expanded, |block| {
                block.child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
            })
            // **Offered wherever anything is out of sight, by either route.**
            // Lines the fold is holding back and a box that has run out of
            // height are the same fact to a reader, and gating this on the line
            // count alone left a command of one very long line with no way to
            // open it at all. Where nothing is hidden there is still no control,
            // for the reason there never was: a fold that reveals nothing is a
            // button that has to be pressed to learn it does nothing.
            .when(
                more_below || self.expanded && (long || overflows),
                |block| {
                    block.child(
                        crate::controls::action(("perm-fold", key))
                            .ghost()
                            .absolute()
                            .bottom(BLOCK_INSET)
                            .right(BLOCK_INSET)
                            .h(FOLD_ROW)
                            .px_2()
                            .rounded(cx.theme().radius)
                            // On its own plate, because it sits over the end of
                            // the command: the fade underneath it is the text
                            // it would otherwise be read against.
                            .bg(cx.theme().muted)
                            // The count is what there is more *of*, so it is
                            // said only where lines are what is being held
                            // back. A single line that wraps to a screenful has
                            // no second line to promise, and *Show all · 1
                            // lines* counts the wrong thing and miscounts it.
                            .label(match (self.expanded, self.total > 1) {
                                (true, _) => "Show less".to_string(),
                                (false, true) => format!("Show all · {} lines", self.total),
                                (false, false) => "Show all".to_string(),
                            })
                            .on_click({
                                let (session, target) = (self.session, self.target);
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |s, cx| {
                                        s.chat.toggle_permission(target);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                },
            )
    }
}

/// The path a permission's command would run in, shortened from the front.
///
/// **From the front, because a path is read from its tail**: the last
/// components are what tell two checkouts of one project apart, and they are
/// exactly what trimming the end throws away. The whole of it is in the
/// tooltip, so the line identifies the directory and the hover confirms it.
fn command_cwd(root: &Path) -> Option<(SharedString, SharedString)> {
    let full = root.to_string_lossy().into_owned();
    if full.is_empty() {
        return None;
    }
    // `~` where the home directory is: a line that is mostly somebody's user
    // name is a line spent saying where the machine keeps its accounts.
    let shown = std::env::home_dir()
        .and_then(|home| root.strip_prefix(&home).ok())
        .map(|rest| format!("~/{}", rest.to_string_lossy()))
        .unwrap_or_else(|| full.clone());
    Some((
        crate::rail::ellipsize_front(&shown, MAX_CWD),
        SharedString::from(full),
    ))
}

/// Characters of the working directory the meta line carries.
const MAX_CWD: usize = 48;

// ── the record a settled exchange leaves ────────────────────────────────────
//
// **What is left of a question or a grant once it is answered is one line.**
// While either is waiting it is the block everything stops for, and it is not
// drawn here at all -- it is pinned above the composer, where the answer is
// given. Afterwards there is nothing left to decide, and what the transcript
// owes a reader is the fact: the agent asked this, the user said that.
//
// Drawn as a card it was the loudest thing in the conversation for the rest of
// the conversation's life -- a heading, a rule, a body, a footer and a shadow,
// repeated per exchange, so three questions in a row cost most of a screen to
// say three sentences. As rows they join the block of steps around them, which
// is also where they belong: answering a question is one of the things that
// happened during that piece of work.

/// The settled record of a grant.
fn permission_record(
    session: &Entity<ChatSession>,
    p: &PermItem,
    choice: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let denied = p
        .req
        .options
        .iter()
        .find(|option| option.name == choice)
        .is_some_and(|option| option.weight() == PermissionWeight::Deny);
    let status = crate::theme::status_ink(cx);
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_permission(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("perm-record", fold_key(target)).into(),
        // A refusal is not a failure -- nothing went wrong, somebody decided
        // -- so it is not the danger cross a failed step carries. It is the
        // struck-out words and the pill that say what happened.
        match denied {
            true => RowMark::Refused,
            false => RowMark::Done,
        },
        user_icon(),
        match denied {
            true => "Denied",
            false => "Allowed",
        },
    )
    .object(Some(Object::plain(activity::first_line_trunc(
        p.command(),
        160,
    ))))
    .meta(Some(
        pill(
            choice.to_string(),
            match denied {
                true => status.danger,
                false => cx.theme().muted_foreground,
            },
            cx,
        )
        .into_any_element(),
    ))
    .fold(Some(p.expanded));
    let mut row = row;
    row.struck = denied;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(p.expanded, |block| {
            // The whole command and where it would have run: the two facts the
            // row cut down to one line, in the one place that has room for
            // them.
            block.child(detail_well(
                plain_box(cx)
                    .v_flex()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .py(STACK_GAP)
                            .px(FRAME_PAD)
                            .text_color(cx.theme().foreground)
                            .child(onehand_core::chat::redact(p.command())),
                    )
                    .children(command_cwd(&session.read(cx).chat.root).map(|(shown, _)| {
                        div()
                            .w_full()
                            .min_w_0()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .py(STACK_GAP)
                            .px(FRAME_PAD)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("in {shown}"))
                    })),
            ))
        })
        .into_any_element()
}

/// The settled record of a question.
fn ask_record(
    session: &Entity<ChatSession>,
    a: &AskItem,
    answer: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let pairs = ask_pairs(a);
    // One question is already said whole by the row; several are a list worth
    // opening, and the count is what says there is one.
    let many = pairs.len() > 1;
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_ask(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("ask-record", fold_key(target)).into(),
        RowMark::Done,
        user_icon(),
        "Asked",
    )
    .object(Some(Object::plain(match many {
        // The prompt that introduced the form, with the answers in the
        // column beside it -- a strip of question titles would be the
        // form's own chrome rather than what came of it.
        true => a.req.message.to_string(),
        false => pairs
            .first()
            .map(|(question, _)| question.to_string())
            .unwrap_or_else(|| a.req.message.to_string()),
    })))
    .meta(Some(match many {
        true => pill(
            format!("{} answers", pairs.len()),
            cx.theme().muted_foreground,
            cx,
        )
        .into_any_element(),
        false => pill(answer.to_string(), cx.theme().muted_foreground, cx).into_any_element(),
    }))
    .fold(many.then_some(a.expanded));

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(many && a.expanded, |block| {
            block.child(detail_well(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    // **No rules between the pairs.** They are one answer given
                    // in several parts, not several records -- ruled apart they
                    // read as separate exchanges with the agent, which is the
                    // one thing a form is not.
                    .children(pairs.into_iter().map(|(question, chosen)| {
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap(PART_GAP)
                            .w_full()
                            .min_w_0()
                            .h(BUTTON_H)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(question),
                            )
                            .child(div().flex_none().max_w(relative(0.5)).child(pill(
                                chosen,
                                cx.theme().foreground,
                                cx,
                            )))
                    })),
            ))
        })
        .into_any_element()
}

/// Each question of a settled form paired with what was chosen for it.
///
/// A typed answer and a picked one come back the same shape, because from the
/// far side of the exchange they are the same thing: what the user said. Which
/// of the two it was is carried by weight, not by a different control.
fn ask_pairs(a: &AskItem) -> Vec<(SharedString, SharedString)> {
    a.req
        .fields
        .iter()
        .enumerate()
        .map(|(f, field)| {
            let question = field
                .title
                .clone()
                .or_else(|| field.description.clone())
                .unwrap_or_else(|| format!("Question {}", f + 1));
            let typed = a.custom.get(f).map(String::as_str).unwrap_or("").trim();
            let chosen = match typed.is_empty() {
                false => typed.to_string(),
                true => {
                    let picks = a.picked.get(f).cloned().unwrap_or_default();
                    let choices = match &field.kind {
                        ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.as_slice(),
                        ElicitKind::Text => &[],
                    };
                    picks
                        .iter()
                        .filter_map(|&i| choices.get(i).map(|c| c.label.to_string()))
                        .collect::<Vec<_>>()
                        .join(" · ")
                }
            };
            (
                SharedString::from(question),
                SharedString::from(match chosen.is_empty() {
                    true => "skipped".to_string(),
                    false => chosen,
                }),
            )
        })
        .collect()
}

/// The mark on a row that is about the person rather than the agent.
fn user_icon() -> SharedString {
    use gpui_component::IconNamed as _;
    IconName::User.path()
}

pub(super) fn permission(
    session: &Entity<ChatSession>,
    p: &PermItem,
    target: TranscriptItemId,
    well: Option<gpui::Pixels>,
    cx: &App,
) -> impl IntoElement + use<> {
    if let Some(choice) = p.resolved.as_deref() {
        return permission_record(session, p, choice, target, cx);
    }
    let idx = live_index(target);
    let (shown, hidden) = p.shown_lines();
    let block = CommandBlock {
        session: session.clone(),
        target,
        command: SharedString::from(p.command().to_string()),
        lines: shown
            .into_iter()
            .map(|l| SharedString::from(l.to_string()))
            .collect(),
        hidden,
        well,
        long: p.is_long(),
        total: p.command_lines().len().max(1),
        expanded: p.expanded,
    };
    // The only word the protocol offers about *what* is being asked for. An
    // unrecognised kind has no word, so the slot stays empty rather than
    // printing one this build made up.
    let kind = (p.req.kind != ToolKind::Other).then(|| tool_label(p.req.kind));
    let cwd = command_cwd(&session.read(cx).chat.root);

    floating_card(cx)
        .v_flex()
        // The card's own padding goes to the rows inside it, because the
        // footer's rule has to run edge to edge and a rule inside a padded box
        // stops short of the corners it is squaring off. The corners clip for
        // the other half of that: without it the rule ends in a square nib a
        // pixel outside the rounded edge above it.
        .p_0()
        .overflow_hidden()
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .flex_1()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size_4()
                                .flex_none()
                                .text_color(crate::theme::status_ink(cx).warning),
                        )
                        // **The weight and the mark carry it, not the size.**
                        // This and the question card are the two blocks where
                        // nothing proceeds until the user acts, and they were
                        // set at the answer's own size to say so. What that
                        // bought was a heading that was the largest text on
                        // screen over the largest controls on screen, floating
                        // an inch above a composer where everything had come
                        // down a step.
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_semibold()
                                .child("Permission required"),
                        ),
                )
                // What kind of work it is, opposite the heading — the same
                // slot and the same voice the question card puts its counter
                // in, because the two cards are read as one family and this is
                // the corner a reader has already learned to check.
                .children(kind.map(|word| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(word)
                })),
        )
        .child(
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .pb_3p5()
                .child(block)
                .children(cwd.map(|(shown, full)| {
                    div()
                        .id(("perm-cwd", fold_key(target)))
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("in {shown}"))
                        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                })),
        )
        // A card replayed from the archive carries an rpc id no running adapter
        // issued: showing controls would invite an answer nobody is waiting
        // for. An answered one never reaches here -- it is a row.
        .map(|card| match idx {
            None => card.into_any_element(),
            Some(idx) => permission_keys(
                card.child(permission_footer(session, p, idx, cx)),
                session,
                p,
                idx,
                cx,
            ),
        })
        .into_any_element()
}

/// Enter allows once, Esc denies — and only while the card holds the caret.
///
/// **Taken on the card's own handle rather than bound as app actions**, which
/// is what the question card does and for the same reason: both keys are ones
/// somebody is as likely to be pressing in the composer an inch below, and an
/// app binding would reach over it exactly the way the panel shortcuts are
/// meant to.
///
/// Which option either key means is [`onehand_core::acp::PermissionWeight`]
/// and never the position in the list: an agent is free to send its grants in
/// any order, and a key that answered by position would grant *always* on a
/// card that happened to list it first. An option no card offers is no key at
/// all, since there is nothing to send.
fn permission_keys(
    card: gpui::Div,
    session: &Entity<ChatSession>,
    p: &PermItem,
    idx: usize,
    cx: &App,
) -> gpui::AnyElement {
    let Some(focus) = session.read(cx).perm_focus(idx).cloned() else {
        return card.into_any_element();
    };
    let pick = |weight: PermissionWeight| {
        p.req
            .options
            .iter()
            .find(|option| option.weight() == weight)
            .map(|option| option.id.clone())
    };
    let (allow, deny) = (
        pick(PermissionWeight::AllowOnce),
        pick(PermissionWeight::Deny),
    );
    let session = session.clone();
    let card_focus = focus.clone();
    card.id(("perm-card", idx))
        .track_focus(&focus)
        .on_key_down(move |event, window, cx| {
            let keystroke = &event.keystroke;
            // A modified key is somebody else's: Ctrl+1 switches sessions and
            // Shift+Enter is a newline in whatever holds the caret.
            if keystroke.modifiers.modified() {
                return;
            }
            let chosen = match keystroke.key.as_str() {
                // **Only while the card itself holds the caret**, which is the
                // question card's rule and matters more here. Every button in
                // the footer is a library `Button`, and a focused one already
                // turns Enter into its own click -- so answering here as well
                // races it, and this listener runs first because a click is
                // settled on the key going *up*. Somebody who has tabbed to
                // Deny and pressed Enter would have granted the call: the grant
                // lands, and Deny's own click arrives afterwards to find the
                // permission already answered and is dropped. Unguarded, the
                // key that means no is how yes gets said.
                "enter" if card_focus.is_focused(window) => allow.as_deref(),
                "enter" => None,
                // Esc is safe in the other direction and needs no such guard:
                // no button on this card denies by being focused, and the worst
                // it can do is refuse a call twice.
                "escape" => deny.as_deref(),
                _ => None,
            };
            if let Some(option) = chosen {
                answer_permission(&session, idx, option, cx);
            }
        })
        .into_any_element()
}

/// The strip that answers the card.
///
/// **A strip of its own over a rule that spans the card**, which is the
/// question card's footer drawn again and deliberately so: both end the one
/// block everything is waiting on, and left inside the body's padding the
/// buttons read as the last row of the card rather than as what closes it.
///
/// **The buttons wrap before a label is cut.** A narrow pane drops the key
/// hints first — they name keys that still work — and then lets the row fold
/// onto a second, still right-aligned line. A truncated *Always allow* is a
/// grant nobody can read the reach of, which is the one thing on this card
/// that must never happen.
fn permission_footer(
    session: &Entity<ChatSession>,
    p: &PermItem,
    idx: usize,
    cx: &App,
) -> impl IntoElement + use<> {
    // Deny first and the narrowest grant last, against the reading direction
    // of the sentence: the button under the thumb at the end of the row is the
    // one that expires with this call, and the widest grant never sits there.
    let mut options: Vec<(usize, &onehand_core::acp::PermissionOption)> =
        p.req.options.iter().enumerate().collect();
    options.sort_by_key(|(_, option)| match option.weight() {
        PermissionWeight::Deny => 0,
        PermissionWeight::AllowAlways => 1,
        PermissionWeight::AllowOnce => 2,
    });

    div()
        .v_flex()
        .w_full()
        .child(div().w_full().h_px().bg(cx.theme().border))
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .w_full()
                .px_4()
                .py_2p5()
                .child(
                    div()
                        // The first thing to give way when the row runs out of
                        // room, because the keys it names keep working whether
                        // or not they are printed. Squeezed to nothing, the
                        // buttons wrap onto a line of their own rather than
                        // losing a character of a label.
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Enter allow · Esc deny"),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_end()
                        .flex_wrap()
                        .gap_2()
                        .flex_none()
                        .children(options.into_iter().map(|(i, option)| {
                            let (id, session) = (option.id.clone(), session.clone());
                            let weight = option.weight();
                            grows(crate::controls::action(("perm", i)))
                                // One primary per card, and it is the grant
                                // that expires with this call. "Always allow"
                                // is the same word with a far longer reach:
                                // drawn as loudly as "allow once" it is the
                                // one that gets clicked by muscle memory, and
                                // it is the one that cannot be taken back from
                                // the card. It stays reachable, in the neutral
                                // outline — a decision, not a reflex.
                                .map(|b| match weight {
                                    PermissionWeight::AllowOnce => b.primary(),
                                    PermissionWeight::AllowAlways => b.outline(),
                                    PermissionWeight::Deny => b.ghost(),
                                })
                                .label(option.name.clone())
                                .on_click(move |_, _, cx: &mut App| {
                                    answer_permission(&session, idx, &id, cx);
                                })
                        })),
                ),
        )
}

/// Settle the card at `idx` on `option`.
///
/// One function behind the buttons and the keys for the reason the question
/// card has one: they are the same decision, and a second copy is where a key
/// comes to answer a card the click would have refused.
fn answer_permission(session: &Entity<ChatSession>, idx: usize, option: &str, cx: &mut App) {
    let option = option.to_string();
    session.update(cx, |s, cx| {
        s.chat.answer_permission(idx, &option);
        cx.notify();
    });
}

// ── the agent asking a question (`AskUserQuestion` / an MCP form) ───────────

pub(super) fn ask(
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    if let Some(answer) = a.resolved.as_deref() {
        return ask_record(session, a, answer, target, cx);
    }
    let idx = live_index(target);
    let live = idx.is_some();
    let counter = (live && a.req.fields.len() > 1).then(|| {
        format!(
            "Question {} of {}",
            a.active_field() + 1,
            a.req.fields.len()
        )
    });

    floating_card(cx)
        .v_flex()
        .p_0()
        // **The card's own padding goes to the rows inside it.** The heading
        // and the tab strip are separated by a rule that has to run edge to
        // edge, and a rule inside a padded box stops short of the corners it is
        // squaring off -- which reads as a line somebody drew rather than as
        // the edge of a region.
        //
        // The corners clip what is inside them for the same reason from the
        // other end: the strip's hairline and the footer's both run the full
        // width, so without it each ends in a square nib a pixel outside the
        // rounded edge above it.
        .overflow_hidden()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .child(Icon::new(IconName::Info).size_4())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_semibold()
                        .child(a.req.message.clone()),
                )
                // Where the reader is in the form, in words, because the tab
                // strip says which question is open but not how many are left:
                // three numbered tabs read as three steps only once you have
                // counted them.
                .children(counter.map(|line| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(line)
                })),
        )
        // A question replayed from the archive carries an rpc id no running
        // adapter issued: showing controls would invite an answer nobody is
        // waiting for. An answered one never reaches here -- it is a row.
        .map(|card| match idx {
            None => card.into_any_element(),
            Some(idx) => ask_form(card, session, a, target, idx, cx),
        })
}

/// Take one row of the question showing at `field`.
///
/// **One function behind the click and the key**, because they are the same
/// gesture: a row aimed at with the pointer and a row walked to with the arrows
/// both end in the same answer, and two copies of "what picking does" is how a
/// number key comes to leave the typed answer sitting in a box the click would
/// have emptied.
fn ask_take(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    row: onehand_core::chat::AskRow,
    quick: bool,
    window: &mut Window,
    cx: &mut App,
) {
    use onehand_core::chat::AskRow;

    match row {
        // The typed answer is a row of the list, so reaching it is reaching its
        // box: the keyboard hands the caret over rather than answering for the
        // user, which is the one row that cannot be settled by arriving at it.
        AskRow::Custom => {
            let state = session.read(cx).ask_input(idx, field).cloned();
            if let Some(state) = state {
                let handle = state.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                session.update(cx, |s, cx| {
                    if let Some(item) = s.chat.ask_at_mut(idx) {
                        item.cursor = item.row_count(field).saturating_sub(1);
                    }
                    cx.notify();
                });
            }
        }
        AskRow::Choice(option) => session.update(cx, |s, cx| {
            // Picking is the user choosing the agent's wording over their own,
            // so the box goes with it -- model and widget together, since
            // neither clears the other.
            s.clear_ask_input(idx, field, window, cx);
            if let Some(item) = s.chat.ask_at_mut(idx) {
                item.toggle(field, option);
                item.cursor = option;
            }
            // A one-question single-select has nothing left to decide, so it
            // commits on the press.
            if quick {
                s.chat.answer_ask(idx, false);
            }
            cx.notify();
        }),
    }
}

/// Pass on the question showing at `field`, and settle the form if it was the
/// last one.
///
/// What is sent then is whatever the *other* questions hold: a form skipped
/// through to the end with nothing filled in is a refusal, and one with answers
/// above the skipped question is those answers. Declining a form that carries
/// work already done would throw it away at the last press.
fn ask_skip(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    window: &mut Window,
    cx: &mut App,
) {
    session.update(cx, |s, cx| {
        s.clear_ask_input(idx, field, window, cx);
        let done = s
            .chat
            .ask_at_mut(idx)
            .is_none_or(|item| item.skip_field(field));
        if done {
            let answered = s.chat.ask_at_mut(idx).is_some_and(|item| item.has_answer());
            s.chat.answer_ask(idx, !answered);
        }
        cx.notify();
    });
}

/// Move the form on from `field`, or submit it where that was the last
/// question.
fn ask_advance(session: &Entity<ChatSession>, idx: usize, field: usize, cx: &mut App) {
    session.update(cx, |s, cx| {
        let last = s.chat.ask_at_mut(idx).is_none_or(|item| {
            let last = item.is_last(field);
            if !last {
                item.go_to(field + 1);
            }
            last
        });
        if last {
            s.chat.answer_ask(idx, false);
        }
        cx.notify();
    });
}

/// The highest row a single digit reaches, and so the last one that is offered
/// a key at all.
///
/// **The handler reads one keystroke, not a typed number.** There is nowhere to
/// hold a half-entered figure and nothing that could say when one had ended, so
/// a row past the ninth has no key and must not be drawn carrying one. Printed
/// anyway, `10` was a hint for a press that cannot be made — and worse than
/// inert on the card that answers as soon as a row is taken, where reaching for
/// it lands on `1` and commits the first choice instead.
const ASK_KEY_ROWS: usize = 9;

/// The key hint at the right-hand end of a row, and the number that reaches it.
///
/// **A row a keyboard can reach says so on the row.** The hints at the foot of
/// the card name the arrows and Enter, which is the walk; this is the jump, and
/// a jump has to be to something the user can already see a name for. It is
/// held off shrinking because it is two characters at most and the words beside
/// it are the agent's -- a paragraph of description would otherwise squeeze the
/// one part of the row that is fixed-length.
///
/// `None` past the ninth row, which is a row with no key rather than a row with
/// an unusable one: nothing is drawn, and the words beside it take the space.
fn ask_key_hint(n: usize, cx: &App) -> Option<impl IntoElement + use<>> {
    if n > ASK_KEY_ROWS {
        return None;
    }
    Some(ask_key_mark(n, cx))
}

fn ask_key_mark(n: usize, cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .min_w(ASK_HINT_SIZE)
        .h(ASK_HINT_SIZE)
        .px(ASK_HINT_PAD)
        .h_flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(format!("{n}"))
}

/// One numbered step of the tab strip.
///
/// **The number is the point, until the question has an answer.** A strip of
/// titles says these are three things; a strip of *numbered* titles says they
/// are three things in an order, with a first and a last -- which is the only
/// question a reader has about a form they are partway through. Once a question
/// is answered its number has done that job and the tick replaces it: what is
/// left to do is then readable as the tabs still carrying digits, without
/// counting anything. The two never show together, because one 18px circle
/// holds one mark.
fn ask_tab_mark(index: usize, active: bool, answered: bool, cx: &App) -> impl IntoElement + use<> {
    let (fill, ink) = match (active, answered) {
        // Open: the strongest of the three, because it is the one the choices
        // below belong to.
        (true, _) => (cx.theme().primary, cx.theme().primary_foreground),
        // Answered and not open: filled, quietly, so what is left to do is
        // readable as the ones that are *not* filled.
        (false, true) => (cx.theme().accent, cx.theme().accent_foreground),
        (false, false) => (cx.theme().transparent, cx.theme().muted_foreground),
    };
    div()
        .flex_none()
        .size(ASK_TAB_MARK)
        .rounded_full()
        .bg(fill)
        .when(!active && !answered, |mark| {
            mark.border_1().border_color(cx.theme().border)
        })
        .h_flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(ink)
        .map(|mark| match answered {
            true => mark.child(Icon::new(IconName::Check).size_3()),
            false => mark.child(format!("{}", index + 1)),
        })
}

/// The mark at the head of a choice row.
///
/// Round for a single-select and square for a multi-select, which is the one
/// convention this app inherits rather than invents: a reader who has met a
/// form before already knows that a circle means *instead of* and a box means
/// *as well as*, and nothing else on the row says it. Drawn from two divs
/// rather than an icon because the bundled set has neither shape as a control
/// -- its `circle-check` is an outcome, not a thing waiting to be chosen.
///
/// **Its ring is ink and not the hairline**, which is the difference between a
/// control and an edge. Drawn in the hairline it vanished the moment the row
/// was hovered: a ghost button's hover fill is derived from the same step of
/// the ramp the hairline sits on, so in the dark palette the two land within a
/// shade of each other and the ring is painted onto its own background. A mark
/// that disappears under the pointer disappears exactly when it is being aimed
/// at.
fn ask_choice_mark(on: bool, single: bool, cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .size(ASK_CHOICE_MARK)
        .h_flex()
        .items_center()
        .justify_center()
        // Half a pixel over the hairline everything else on the card is drawn
        // with, which is the whole of the difference between an edge and a
        // control: this ring is the thing being aimed at, and at one pixel it
        // reads as the seam of the row rather than as the mark inside it.
        .border(ASK_MARK_RING)
        .border_color(match on {
            true => cx.theme().primary,
            false => cx.theme().muted_foreground,
        })
        .map(|mark| match single {
            true => mark.rounded_full(),
            false => mark.rounded(cx.theme().radius),
        })
        .when(on, |mark| {
            mark.child(
                div()
                    .size(ASK_CHOICE_DOT)
                    .bg(cx.theme().primary)
                    .map(|dot| match single {
                        true => dot.rounded_full(),
                        false => dot.rounded_sm(),
                    }),
            )
        })
}

/// The live form.
///
/// **One question at a time.** A multi-question form renders as a numbered tab
/// strip with only the active field's choices below it: stacking every question
/// made the card taller than the pane and the overflow was lost off the top. A
/// single single-select form is the *quick* shape — clicking a choice answers
/// on the spot, with no Submit to hunt for.
///
/// **The forward button is what moves the form on**, not the pick. A
/// single-select press used to walk itself to the next open question, on the
/// reasoning that answering one question is asking for the next; what that cost
/// once the card grew a footer is the button in it — a form that has already
/// moved on leaves *Next* pointing at a question the user is now looking at, so
/// the one control the footer exists to offer is the one control there is never
/// a moment to press. The tab strip is still how an answer is gone back to.
///
/// **The keys belong to the card and only to the card.** Numbers, the arrows,
/// Enter and Esc are every one of them a key somebody is as likely to be typing
/// into the composer an inch below, so they are taken on the card's own focus
/// handle rather than bound as app actions — a binding would reach over the
/// composer exactly the way the panel shortcuts are meant to, which is the
/// opposite of what a form wants.
/// The four parts of a question card are drawn against the same five facts:
/// which session it belongs to, the item, where it is in the transcript, which
/// question of the form is open, and whether the card is the quick kind that
/// commits on the click itself.
///
/// Held together rather than threaded through four signatures, which is what
/// they were when this was one function: the same five arguments in the same
/// order at every call, and a fifth part added later would have taken them
/// again. Everything else each part needs is derived from these inside it.
struct AskForm<'a> {
    session: &'a Entity<ChatSession>,
    a: &'a AskItem,
    idx: usize,
    /// The question the strip has open, and the one every row below it answers.
    active: usize,
    /// A one-question single-select: it answers on the click and carries no
    /// footer to hunt for.
    quick: bool,
}

impl AskForm<'_> {
    /// The strip of questions across the top, on a form that has more than one.
    fn tabs(&self, cx: &App) -> gpui::Stateful<gpui::Div> {
        let (session, a, idx, active) = (self.session, self.a, self.idx, self.active);
        div()
            .id(("ask-tabs", idx))
            .h_flex()
            .gap_1()
            .w_full()
            .px_3()
            .pt_2()
            .overflow_x_scroll()
            .children(a.req.fields.iter().enumerate().map(|(f, field)| {
                let session = session.clone();
                let (open, answered) = (f == active, a.field_answered(f));
                let label = field
                    .title
                    .clone()
                    .or_else(|| field.description.clone())
                    .unwrap_or_else(|| format!("Question {}", f + 1));
                crate::controls::action(("ask-tab", f))
                    .ghost()
                    .small()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .h(ASK_TAB_H)
                    .px_2p5()
                    .rounded_none()
                    // **Underlined rather than filled.** A filled tab in a
                    // strip that already carries a filled number in each of its
                    // own tabs is two fills arguing about which one means
                    // "here"; the rule under the open tab lands on the strip's
                    // own hairline and reads as the one continuing into the
                    // body below it.
                    .border_b_2()
                    .border_color(match open {
                        true => cx.theme().primary,
                        false => cx.theme().transparent,
                    })
                    .child(ask_tab_mark(f, open, answered, cx))
                    .child(
                        div()
                            // Only a *tab* label is cut short; a choice never
                            // is. It is held to one line as well as to a width:
                            // a title long enough to wrap turns the strip three
                            // rows tall while the row height stays at one, so
                            // the second and third lines are drawn over the
                            // choices below.
                            .max_w(ASK_TAB_W)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .truncate()
                            .text_sm()
                            .when(open, |label| {
                                label.font_semibold().text_color(cx.theme().foreground)
                            })
                            .when(!open, |label| label.text_color(cx.theme().muted_foreground))
                            .child(label),
                    )
                    // Jumping back restores what that question already holds,
                    // which costs nothing to arrange: every answer is kept per
                    // field, so the tab only has to move the view.
                    .on_click(move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.go_to(f);
                            }
                            cx.notify();
                        });
                    })
            }))
    }

    /// The open question's choices, one row each.
    fn choice_rows(&self, cx: &App) -> Vec<gpui::AnyElement> {
        let (session, a, idx, active, quick) =
            (self.session, self.a, self.idx, self.active, self.quick);
        let field = a.req.fields.get(active);
        let single = matches!(field.map(|f| &f.kind), Some(ElicitKind::Select(_)));
        let choices = field
            .map(|field| match &field.kind {
                ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.clone(),
                ElicitKind::Text => Vec::new(),
            })
            .unwrap_or_default();
        let picked = a.picked.get(active).cloned().unwrap_or_default();
        let cursor = a.cursor_row(active);

        choices
            .into_iter()
            .enumerate()
            .map(|(o, choice)| {
                let session = session.clone();
                let on = picked.contains(&o);
                // The one thing on this card that has to be *read* before anything
                // can happen, so it is set at the size everything else meant to be
                // read is, with its description tight underneath rather than a line
                // away: the two are one answer, and spaced apart the description
                // reads as belonging to whichever row it is nearer.
                let destructive = choice.is_destructive();
                let mut words = div().v_flex().gap_0p5().flex_1().min_w_0().child(
                    div()
                        .w_full()
                        .when(destructive, |label| {
                            label.text_color(crate::theme::status_ink(cx).danger)
                        })
                        .child(choice.label.clone()),
                );
                if let Some(description) = choice.description.clone() {
                    words = words.child(
                        div()
                            .w_full()
                            .text_size(WORK_TEXT)
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    );
                }
                grows(crate::controls::action(("ask-choice", o)))
                    .ghost()
                    .px_3()
                    .py_2()
                    .w_full()
                    .min_h(ASK_ROW_MIN)
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    // **A taken choice is named by its border, and a destructive
                    // one by the danger step of that same border.** Taking it is
                    // still one press, so the tint is not a refusal -- it is the
                    // one row on the card that cannot be pressed a second time to
                    // undo, and the only place to say so is the row itself.
                    .border_color(match (on, destructive) {
                        (true, true) => crate::theme::status_ink(cx).danger,
                        (true, false) => cx.theme().primary,
                        // The keyboard's own position, which has to be visible
                        // without being an answer: an arrow press moves this and
                        // settles nothing, so it is the hairline lifted to full
                        // ink rather than a third colour.
                        (false, _) if cursor == Some(onehand_core::chat::AskRow::Choice(o)) => {
                            cx.theme().ring
                        }
                        (false, _) => cx.theme().border,
                    })
                    // **One child, holding the row's own layout.** A `Button` wraps
                    // whatever the call site gives it in a content box of the
                    // library's own -- centred, with the gap its `Size` chose -- so
                    // a flex written out here lands on a box with exactly one thing
                    // in it and decides nothing at all. That is why the mark sat a
                    // quarter-rem from the words it belongs to while this said
                    // `gap_2p5`, and why aligning it to the top of a two-line
                    // choice did nothing either.
                    // **The mark and the hint sit on the row's middle, not on its
                    // first line.** Both are one shape against a text column that
                    // is one line or three depending on what the agent wrote, so
                    // pinned to the top they line up with the label on a
                    // described choice and with nothing at all on a bare one --
                    // the column of marks down the left comes out ragged for a
                    // reason that is about the wording rather than about the
                    // control.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .w_full()
                            .child(ask_choice_mark(on, single, cx))
                            .child(words)
                            .children(ask_key_hint(o + 1, cx)),
                    )
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        ask_take(
                            &session,
                            idx,
                            active,
                            onehand_core::chat::AskRow::Choice(o),
                            quick,
                            window,
                            cx,
                        );
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>()
    }

    /// The question itself, above the choices that answer it.
    ///
    /// On a multi-question form the tab carries the field's heading and nothing
    /// else said what was actually being asked -- a strip reading "Migration",
    /// "Also generate", "Anything else" over three unexplained options is a
    /// form answered by guessing. The description is the question and the title
    /// is the tab's word for it, so the description leads and the title stands
    /// in where the agent sent only one of the two.
    ///
    /// A single-question form is the exception and prints nothing here: its
    /// question *is* the card's heading, so a second copy under it asks twice.
    fn question(&self) -> Option<String> {
        let field = self.a.req.fields.get(self.active)?;
        (self.a.req.fields.len() > 1)
            .then(|| field.description.clone().or_else(|| field.title.clone()))
            .flatten()
    }

    /// The free-text box, where the question offers one.
    ///
    /// The choices above it are what the agent thought of; this is the answer
    /// it did not, and a form that shows only the first is a question the user
    /// cannot actually answer.
    fn custom_row(&self, cx: &App) -> Option<gpui::Div> {
        let state = self
            .a
            .has_custom(self.active)
            .then(|| {
                self.session
                    .read(cx)
                    .ask_input(self.idx, self.active)
                    .cloned()
            })
            .flatten()?;
        let cursor = self.a.cursor_row(self.active);
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_3()
                .w_full()
                .px_3()
                .min_h(ASK_CUSTOM_ROW_MIN)
                .rounded(cx.theme().radius_lg)
                .border_1()
                // **Dashed** rather than solid, which is the one thing that
                // separates it from the rows above without taking it out of
                // the list: the agent's wording is fixed and this line is not
                // yet written.
                .border_dashed()
                .border_color(match cursor == Some(onehand_core::chat::AskRow::Custom) {
                    true => cx.theme().ring,
                    false => cx.theme().border,
                })
                .child(
                    Icon::new(crate::icons::Icon::SquarePen)
                        .size_4()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground),
                )
                // The row is the border, so the field inside it draws none: two
                // rings around one input read as two inputs, and the inner one
                // lands a hair inside the outer with the gap between them
                // reading as a mistake.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&state).appearance(false)),
                )
                .children(ask_key_hint(self.a.row_count(self.active), cx)),
        )
    }

    /// The strip that closes the card: what the keyboard can do, then Skip and
    /// the forward button.
    fn footer(&self, cx: &App) -> gpui::Div {
        let (session, idx, active, quick) = (self.session, self.idx, self.active, self.quick);
        // The forward button is about *this* question: a form is walked through
        // one at a time, so arming Submit off an answer three tabs back would
        // offer to send while the question on screen is blank.
        let can_advance = self.a.field_answered(active);
        let last = self.a.is_last(active);

        div()
            // **A strip of its own, over a rule that spans the card.** The
            // buttons here end the block everything is waiting on, and left
            // inside the body's padding they read as the last row of the
            // list rather than as what closes it. The rule is the same
            // pixel the tab strip's is, for the same reason: it is where a
            // region ends.
            .child(div().w_full().h_px().bg(cx.theme().border))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .px_4()
                    .py_2p5()
                    // What the keyboard can do, said where the keyboard's
                    // work ends. The numbers are on the rows themselves --
                    // this is the walk, which has nowhere else to be named.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match quick {
                                true => "Enter choose",
                                false => "↑↓ move · Enter choose · Esc skip",
                            }),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .children((!quick).then(|| {
                                crate::controls::action(("ask-skip", idx))
                                    .secondary()
                                    .small()
                                    .label("Skip")
                                    .on_click({
                                        let session = session.clone();
                                        move |_, window: &mut Window, cx: &mut App| {
                                            ask_skip(&session, idx, active, window, cx);
                                        }
                                    })
                            }))
                            .child(
                                crate::controls::action(("ask-submit", idx))
                                    .primary()
                                    .small()
                                    .map(|submit| match can_advance {
                                        true => submit,
                                        // Nothing is picked yet, so the
                                        // pointer would be promising a
                                        // press that does nothing.
                                        false => crate::controls::resting(submit),
                                    })
                                    .disabled(!can_advance)
                                    .label(match last {
                                        false => "Next →",
                                        true => "Submit",
                                    })
                                    .on_click({
                                        let session = session.clone();
                                        move |_, _, cx: &mut App| {
                                            ask_advance(&session, idx, active, cx);
                                        }
                                    }),
                            ),
                    ),
            )
    }

    /// The card's own key handling, hung on the focus handle it was built with.
    ///
    /// Separate from the boxes above because it answers a different question:
    /// those say what the form looks like, this says what a keystroke landing
    /// anywhere on it means.
    fn keys(&self, body: gpui::Div, focus: gpui::FocusHandle) -> gpui::Stateful<gpui::Div> {
        let (idx, active, quick) = (self.idx, self.active, self.quick);
        let session = self.session.clone();
        let card = focus.clone();
        body.id(("ask-card", idx))
            .track_focus(&focus)
            .on_key_down(move |event, window, cx| {
                let keystroke = &event.keystroke;
                // A modified key is somebody else's: Ctrl+1 switches sessions and
                // Shift+Enter is a newline in whatever holds the caret.
                if keystroke.modifiers.modified() {
                    return;
                }
                let session = session.clone();
                // **The free-text box takes every key while it has the caret.** It
                // is inside the card, so a key press there reaches this listener on
                // its way out -- and the answers this card is shortest about are
                // digits, which is exactly what somebody writing their own answer
                // types. Unguarded, a "1" in that box jumps to the first choice and
                // empties the line being written.
                let typing = session
                    .read(cx)
                    .ask_input(idx, active)
                    .map(|state| state.read(cx).focus_handle(cx))
                    .is_some_and(|handle| handle.is_focused(window));
                if typing {
                    return;
                }
                match keystroke.key.as_str() {
                    "up" | "down" => {
                        let delta = if keystroke.key == "up" { -1 } else { 1 };
                        // The walk takes the caret back off whichever row was last
                        // clicked, or the cursor and the focus point at two
                        // different rows and Enter answers the one the eye is not
                        // on.
                        window.focus(&card, cx);
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.move_cursor(active, delta);
                            }
                            cx.notify();
                        });
                    }
                    // **Only where the card itself holds the caret.** A choice row
                    // is a library `Button`, and a focused one already turns Enter
                    // into its own click -- so answering here as well is two
                    // answers, which on a multi-select is the choice toggled on and
                    // straight back off. The row that has the caret settles itself;
                    // this is the walk's Enter, for the cursor the arrows moved.
                    "enter" if card.is_focused(window) => {
                        let row = session
                            .read(cx)
                            .chat
                            .ask_at(idx)
                            .and_then(|item| item.cursor_row(active));
                        if let Some(row) = row {
                            ask_take(&session, idx, active, row, quick, window, cx);
                        }
                    }
                    "enter" => {}
                    // Esc passes on this question rather than refusing the whole
                    // form: the card is walked one question at a time, and the key
                    // that means "not this one" has to mean it at the same scale
                    // the Skip button beside it does.
                    "escape" if !quick => ask_skip(&session, idx, active, window, cx),
                    // A number jumps to the row carrying it, the typed answer's box
                    // included -- and a number nobody offered does nothing at all,
                    // which is `row` refusing rather than rounding to the nearest.
                    key => {
                        let Some(n) = key.parse::<usize>().ok().filter(|&n| n >= 1) else {
                            return;
                        };
                        let row = session
                            .read(cx)
                            .chat
                            .ask_at(idx)
                            .and_then(|item| item.row(active, n - 1));
                        if let Some(row) = row {
                            ask_take(&session, idx, active, row, quick, window, cx);
                        }
                    }
                }
            })
    }
}

/// The question card: the strip of questions, the open one's choices, the box
/// for an answer nobody offered, and what closes it.
///
/// Assembly only. Each part is built by [`AskForm`], because a card that draws
/// four separable regions in one function is one where a change to any of them
/// is read against the other three.
fn ask_form(
    card: gpui::Div,
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    idx: usize,
    cx: &App,
) -> gpui::AnyElement {
    let form = AskForm {
        session,
        a,
        idx,
        active: a.active_field(),
        quick: a.is_quick(),
    };
    let multi_field = a.req.fields.len() > 1;
    let rows = form.choice_rows(cx);
    // The quick card commits on a click and deliberately carries no footer to
    // hunt for -- but typing is not a click, so the footer appears the moment
    // there are words with no other way out. Skip is not added with it:
    // refusing the whole question is a thing that card has never offered, and
    // writing an answer is not the moment to start.
    let typed = a
        .custom
        .get(form.active)
        .is_some_and(|c| !c.trim().is_empty());

    let body = card
        .children(multi_field.then(|| form.tabs(cx)))
        // The strip's rule spans the card and not just the tabs: the tabs are a
        // label on the body below them, and it is the body that has an edge.
        .when(multi_field, |card| {
            card.child(div().w_full().h_px().bg(cx.theme().border))
        })
        .child(
            div()
                .v_flex()
                .gap_3()
                .w_full()
                .px_4()
                .pt_3p5()
                .pb_4()
                .children(form.question().map(|line| {
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(relative(ASK_PROMPT_LEADING))
                        .child(line)
                }))
                // Only the choices scroll. The tab strip is how the *other*
                // questions are reached and the box below is where an answer
                // nobody offered is written, so both are controls and both stay
                // on the card beside the footer rather than inside the region
                // that can be scrolled away from.
                .children((!rows.is_empty()).then(|| BlockingBody::new(target, rows).flush()))
                .children(form.custom_row(cx)),
        )
        // The rule over the footer is the footer's own, drawn as its first
        // child. Added again here it came out twice, two pixels apart, which
        // reads as a border that failed rather than as an edge — and the
        // permission card beside it draws one.
        .when(!form.quick || typed, |card| card.child(form.footer(cx)));

    // No handle means no card on screen to hold the keys -- the boxes and the
    // handle are built in the same pass, so this is only ever the frame a
    // question first appears in.
    match session.read(cx).ask_focus(idx).cloned() {
        Some(focus) => form.keys(body, focus).into_any_element(),
        None => body.into_any_element(),
    }
}

// ── notice ──────────────────────────────────────────────────────────────────

/// A line the session says about itself.
///
/// A remark stays a caption. A **failure does not**: the two loudest things
/// this app can say — the turn errored, the agent is gone and here is the key
/// that brings it back — used to draw at the smallest size in the palest
/// colour the theme has, quieter than the file path in the tool card above
/// them. Something that ends the conversation cannot be whispered, so a
/// failure takes the alert icon, the danger tint and the body size.
///
/// It is **not** one of the muted machine wells, and deliberately so: those say
/// "a machine produced this", and the tint is what carries the meaning here. A
/// failure sits on a wash of its own danger colour, in the body face, which is
/// what stops it reading as one more line of the answer without claiming to be
/// quoted output.
fn notice(text: &str, level: NoticeLevel, cx: &App) -> impl IntoElement + use<> {
    if level != NoticeLevel::Error {
        // **A line down the middle of the column, with nothing around it.** A
        // remark is the transcript speaking about itself rather than either
        // side speaking, and the centre is the one lane neither of them uses —
        // so it needs no box, no rule and no mark to be told apart from the
        // conversation running past it.
        return div()
            .h_flex()
            .items_center()
            .justify_center()
            .w_full()
            .h(LINE_H)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().min_w_0().truncate().child(text.to_string()))
            .into_any_element();
    }

    // A failure is a banner: the full width of the column, its own edge, and
    // the mark anchored to the first line so a message that wraps to three
    // lines does not carry the icon down the middle of itself.
    div()
        .h_flex()
        .items_start()
        .gap(TEXT_PAD_Y)
        .w_full()
        .min_w_0()
        .py(PART_GAP)
        .px(FRAME_PAD)
        .rounded(radius_block(cx))
        .border_1()
        .border_color(crate::theme::status_ink(cx).danger)
        .bg(cx.theme().danger.opacity(0.1))
        .text_color(crate::theme::status_ink(cx).danger)
        .child(
            div()
                .size(MARK_SLOT)
                .flex_none()
                .h_flex()
                .items_center()
                .justify_center()
                .child(Icon::new(IconName::TriangleAlert).size(MARK_SIZE)),
        )
        .child(div().flex_1().min_w_0().child(text.to_string()))
        .into_any_element()
}

// ── activity strip ──────────────────────────────────────────────────────────

/// Split the transcript into runs: either a single item, or a stretch of
/// adjacent completed steps that folds into one line.
///
/// Grouping is core's call (`onehand_core::chat::activity`), so the two front
/// ends cannot disagree about semantic sections or summaries. This view adds
/// one presentation rule: attention states never disappear into a strip.
pub fn runs(items: &[(TranscriptItemId, &ChatItem)]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for &(target, item) in items {
        match (is_activity(item), out.last_mut()) {
            // Still the same stretch of work: extend it.
            (true, Some(Run::Activity { members })) => members.push(target),
            (true, _) => out.push(Run::Activity {
                members: vec![target],
            }),
            (false, _) => out.push(Run::Single(target)),
        }
    }
    out
}

/// Whether an item is part of what the agent *did* rather than what it said.
///
/// **Every status, and no kind boundary.** What a cluster is bounded by is the
/// agent's own words: everything between two paragraphs is one thing it did
/// between saying two things, however many kinds of work that took and whatever
/// state each is in. Splitting on the kind instead gave three reads and a
/// command two headers with nothing between them, which is two claims about one
/// stretch of work; splitting on the status put a running step outside the
/// cluster it belonged to and left it there once it finished.
///
/// A settled question and a settled grant are in it too. While either is
/// waiting it is pinned above the composer and not in the transcript at all;
/// what is left afterwards is a record of one exchange, which is one line of
/// the same list a step is a line of.
fn is_activity(item: &ChatItem) -> bool {
    match item {
        ChatItem::Tool(_) | ChatItem::Thought(_) => true,
        ChatItem::Permission(p) => p.resolved.is_some(),
        ChatItem::Ask(a) => a.resolved.is_some(),
        _ => false,
    }
}

/// The kind a stretch of one cluster's members shares, for the rows drawn
/// inside the opened frame.
///
/// The cluster's own line no longer needs this — it names kinds of work in its
/// sentence and carries the *status* in its mark — but the frame under it is
/// still a list grouped by what the work was.
/// The kind a run of adjacent members shares, for the rows the frame lists —
/// and `None` for a step that stands on its own whatever is beside it.
///
/// **Only reads merge.** Three files looked at in a row are one thing the agent
/// did, and `Read 3 files` opening into the three paths is what a reader wants
/// of them. Two edits are not: each carries its own diff, which is the thing
/// somebody opened the block to see, and folding them behind one row puts the
/// diff two clicks away to save a line. The same goes for two commands, whose
/// output is the point of each.
pub fn section_group(item: &ChatItem) -> Option<activity::ActivityGroup> {
    match item {
        ChatItem::Tool(tool)
            if activity::presentation(tool).kind == activity::ActivityKind::Inspect =>
        {
            activity::group(item)
        }
        _ => None,
    }
}

pub enum Run {
    Single(TranscriptItemId),
    /// Every adjacent activity between two of the agent's paragraphs, which is
    /// one cluster and draws one muted line.
    Activity {
        members: Vec<TranscriptItemId>,
    },
}

/// A cluster of activity, drawn as one muted line that opens into a frame.
///
/// **The line is the whole of what the transcript shows by default.** Everything
/// the agent did between two of its own paragraphs is one thing it did, and a
/// reader skimming wants one answer about it — what sort of work, and did
/// anything break. That fits in a sentence, and a sentence needs no border, no
/// fill and no rule: it sits on the reading surface at the same left edge as
/// the prose either side of it, in the ink a marginal note is set in.
///
/// **The frame only exists once it is asked for.** Drawn always, it was a box
/// the height of a paragraph standing between two paragraphs, for steps nobody
/// had asked to see — the detail claiming the space of the answer. Opened, it
/// is the investigation, and then it can have every edge it needs.
pub fn cluster(
    plan: &super::viewport::ActivityPlan,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    body: Vec<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .v_flex()
        // **The line shrinks to its sentence, and this is what lets it.** A
        // column flex stretches its children across by default, and the line is
        // a library `Button` -- which centres its own content and offers no way
        // out of it. Stretched, the sentence came out down the middle of the
        // column with the prose above and below it starting at the left edge,
        // which reads as a caption for the paragraph rather than as a note
        // beside it. The frame below is unaffected: it asks for the full width
        // itself.
        .items_start()
        .w_full()
        .min_w_0()
        .gap(STACK_GAP)
        .child(cluster_line(plan, open, on_click, id, cx))
        .when(open, |cluster| {
            cluster.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .rounded(radius_block(cx))
                    .border_1()
                    .border_color(cx.theme().border)
                    // **The edge and nothing else.** A fill would make this a
                    // second surface inside the reading one, which is a slab of
                    // another colour standing between two paragraphs for as
                    // long as it is open -- and it is not needed: the border
                    // already says where the detail begins and ends, and what
                    // separates the rows inside it is their own hairlines. It
                    // also keeps the hover fill on those rows legible, which
                    // against a surface already a step off the reading one had
                    // half the contrast to work with.
                    //
                    // The rules between its rows run the full width, so without
                    // this each ends in a square nib a pixel outside the
                    // rounded edge above it.
                    .overflow_hidden()
                    .children(body),
            )
        })
        .into_any_element()
}

/// The collapsed line, which is also the control that opens the frame.
fn cluster_line(
    plan: &super::viewport::ActivityPlan,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    cx: &App,
) -> gpui::AnyElement {
    let summary = &plan.summary;
    let status = crate::theme::status_ink(cx);
    // **A run of text, not a row of columns.** The sentence is read as a
    // sentence, so the verbs sit inside it rather than in a column of their
    // own — and the whole thing shrinks to what it says instead of ruling a
    // line across the column.
    let mut sentence = div()
        .h_flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .children(summary.running.as_ref().map(|part| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .child(format!("{}{}", part.verb, part.rest))
        }));
    for (n, part) in summary.done.iter().enumerate() {
        let lead = match (n, summary.running.is_some()) {
            (0, false) => "",
            (0, true) => " · ",
            _ => ", ",
        };
        sentence = sentence
            .child(div().flex_none().child(lead))
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .child(part.verb.clone()),
            )
            .child(div().min_w_0().truncate().child(part.rest.clone()));
    }

    crate::controls::action(id)
        .ghost()
        .group("cluster-line")
        .h(rems(1.75))
        // **Shrink to the sentence.** A control the width of the column is a
        // bar, and a bar is a thing in the transcript rather than a note in the
        // margin of one. Past the column it truncates instead.
        .w_auto()
        .max_w_full()
        .min_w_0()
        .px(STACK_GAP)
        // Pulled back out by exactly the padding, so the first glyph of the
        // sentence is flush with the prose above it while the hover fill still
        // stands clear of the text on both sides.
        .ml(rems(-STACK_GAP.0))
        .py_0()
        .rounded(radius_control(cx))
        .on_click(on_click)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap(STACK_GAP)
                .min_w_0()
                .text_size(CLUSTER_TEXT)
                // **Muted, lighter than the prose, and placed by size alone.**
                // This line has been through a status mark, a brighter ink on
                // its verbs and a heavier weight -- each added so it could be
                // found while skimming, and each one also making it compete
                // with the answer above it, which nothing here may do. Size is
                // the channel left: it says *where* a reader is in the document
                // without saying how much the thing wants from them, and the
                // light weight is what keeps a larger size from reading as a
                // louder one. What went wrong is still said in words, in the
                // ink that means it.
                //
                // **A weight is a request, like a family.** It lands only
                // where the resolved face carries that cut; where it does not,
                // the platform hands back the nearest it has -- or, asked for
                // something far enough off, a different family altogether,
                // which would put this one line in a typeface of its own.
                // Nothing here depends on it: the size and the ink carry the
                // line on their own, and this is the third channel, not the
                // first.
                .font_weight(gpui::FontWeight::EXTRA_LIGHT)
                .text_color(cx.theme().muted_foreground)
                .group_hover("cluster-line", |line| {
                    line.text_color(crate::theme::meta_ink(cx))
                })
                .child(sentence)
                // How many went wrong, said in the ink that means it and
                // nowhere near the mark, which reads the cluster's *ending*.
                .children((summary.errors > 0).then(|| {
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(status.danger)
                        .child(match summary.errors {
                            1 => " · 1 error".to_string(),
                            n => format!(" · {n} errors"),
                        })
                }))
                // **Mono, and each side in the ink it means.** A diff's two
                // numbers are the one pair on this line a reader takes in
                // without reading — how much came and how much went — and in
                // one muted colour they were two figures to be told apart by
                // the sign in front of them.
                //
                // Mono because they are the numbers that change while the line
                // is on screen: a proportional face slides every word before
                // them sideways each time a digit is added.
                //
                // **A side that is zero is not drawn**, which colour is what
                // forces: `−0` set in the danger ink is the colour of something
                // having gone when nothing did.
                // **The total, after the counts and before the arrow.** It is
                // the one number on this line that is about the *work* rather
                // than about the files, so it goes last of the three -- and
                // only where something actually reported one, because a
                // cluster of steps that never said how long they took would
                // otherwise claim to have taken no time at all.
                .children((summary.seconds > 0).then(|| {
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .child(elapsed(summary.seconds))
                }))
                .children((summary.added > 0 || summary.removed > 0).then(|| {
                    div()
                        .h_flex()
                        .items_center()
                        .gap(TIGHT_GAP)
                        .flex_none()
                        .whitespace_nowrap()
                        .font_family(cx.theme().mono_font_family.clone())
                        .children((summary.added > 0).then(|| {
                            div()
                                .text_color(status.success)
                                .child(format!("+{}", summary.added))
                        }))
                        .children((summary.removed > 0).then(|| {
                            div()
                                .text_color(status.danger)
                                .child(format!("−{}", summary.removed))
                        }))
                }))
                .child(chevron_slot(Some(open), cx)),
        )
        .into_any_element()
}

/// A stretch of one kind of work inside an opened cluster.
///
/// **A row that stands for a section and a row that is one step are the same
/// row.** Collapsed they are indistinguishable, and the only difference is what
/// each opens into: one unfolds a command and its output, the other unfolds the
/// steps it stands for — children at a shorter height, set in to where the
/// parent's verb starts, carrying no frame and separated only by hairlines.
pub fn activity_group(
    section: &super::viewport::Section,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    rows: Vec<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let (name, icon) = activity_identity(section.group);

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(
            ActivityRow::new(id, RowMark::of_run(section.outcome), icon, name)
                .object(Some(Object::plain(section.summary.clone())))
                .meta((section.outcome.errors > 0).then(|| {
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).danger)
                        .child(match section.outcome.errors {
                            1 => "1 error".to_string(),
                            n => format!("{n} errors"),
                        })
                        .into_any_element()
                }))
                .fold(Some(open)),
            on_click,
            cx,
        ))
        .when(open, |block| {
            block.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .pl(DETAIL_INSET)
                    .pr(ROW_PAD_X)
                    .pb(FRAME_PAD)
                    .children(rows),
            )
        })
        .into_any_element()
}

/// The hairline that separates one row of a block from the next.
pub fn rule(cx: &App) -> gpui::Div {
    div().w_full().flex_none().h_px().bg(cx.theme().border)
}

/// The drawing that stands for a kind of work, in the column every row keeps
/// for one.
fn group_icon(group: activity::ActivityGroup) -> SharedString {
    activity_identity(group).1
}

/// Stable identity for a semantic activity section: its name, and the asset
/// path of its icon.
///
/// The summary changes with every member that joins a run; the name and icon
/// do not. Keeping both on the group lets a reader identify the kind of work
/// before reading its counts.
///
/// A path rather than an icon name because the set these are drawn from is not
/// one enum: the bundled library set has no pencil of any kind, so *Changed*
/// comes from the app's own checked-in assets while its five neighbours do not.
/// The path is what both kinds resolve to anyway.
fn activity_identity(group: activity::ActivityGroup) -> (&'static str, SharedString) {
    use gpui_component::IconNamed as _;
    match group {
        activity::ActivityGroup::Explored => ("Explored", IconName::Search.path()),
        // A pencil on a page. The library's nearest shape, `Replace`, is a
        // find-and-replace mark -- one box swapped for another, which is what
        // this group is *not*: it edited files in place.
        activity::ActivityGroup::Changed => ("Changed", crate::icons::Icon::SquarePen.path()),
        activity::ActivityGroup::Ran => ("Ran", IconName::SquareTerminal.path()),
        activity::ActivityGroup::Verified => ("Verified", IconName::CircleCheck.path()),
        activity::ActivityGroup::Reasoned => ("Reasoned", IconName::Info.path()),
        activity::ActivityGroup::Other => ("Other", IconName::Settings2.path()),
    }
}

/// A semantic, counted summary of the work in one activity run.
///
/// Per-target phrases make the folded form almost as expensive to scan as its
/// expanded rows (`Inspected a · Inspected b · Inspected c`). Counts preserve
/// what happened while letting one glance answer how much happened.
pub fn activity_summary(members: &[&ChatItem]) -> String {
    let mut counts = [0usize; 10];
    let mut reason_secs = 0u64;
    for item in members {
        let kind = match item {
            ChatItem::Thought(th) => {
                counts[8] += 1;
                if let Some(secs) = th.elapsed_secs {
                    reason_secs += secs;
                }
                continue;
            }
            ChatItem::Tool(tool) => {
                let p = activity::presentation(tool);
                if p.kind == activity::ActivityKind::Change {
                    // One MultiEdit is one tool step but can touch many files;
                    // the summary names files, so count its distinct paths.
                    counts[3] += tool.diff_summary.len().max(1);
                    continue;
                }
                p.kind
            }
            _ => continue,
        };
        let index = match kind {
            activity::ActivityKind::Inspect => 0,
            activity::ActivityKind::Search => 1,
            activity::ActivityKind::Fetch => 2,
            activity::ActivityKind::Change => 3,
            activity::ActivityKind::Test => 4,
            activity::ActivityKind::Check => 5,
            activity::ActivityKind::Build => 6,
            activity::ActivityKind::Run => 7,
            activity::ActivityKind::Reason => 8,
            activity::ActivityKind::Other => 9,
        };
        counts[index] += 1;
    }

    // **Nouns, with no verb on any of them.** The row this lands in already
    // carries one, in full ink and a weight up, two columns to the left -- so
    // a phrase that opened with its own gave `Ran · Ran 7 commands`, which is
    // the same word twice within an inch and the count pushed a column right
    // for it.
    let mut phrases = Vec::new();
    let counted = [
        (0, "file", "files"),
        (1, "search", "searches"),
        (2, "resource", "resources"),
        (3, "file", "files"),
        (4, "test", "tests"),
        (5, "check", "checks"),
        (6, "target", "targets"),
        (7, "command", "commands"),
        (9, "tool", "tools"),
    ];
    for (index, singular, plural) in counted {
        let count = counts[index];
        if count > 0 {
            phrases.push(format!(
                "{count} {}",
                if count == 1 { singular } else { plural }
            ));
        }
    }
    if counts[8] > 0 {
        phrases.push(match reason_secs > 0 {
            true => format!("{reason_secs}s reasoning"),
            false => "reasoning".to_string(),
        });
    }

    let mut label = phrases.join(" · ");
    if label.is_empty() {
        label.push_str("activity");
    }
    label
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

    fn of(status: ToolStatus) -> Self {
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
        let slot = div()
            .size(KIND_ICON)
            .flex_none()
            .h_flex()
            .items_center()
            .justify_center();
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
struct ActivityRow {
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
    struck: bool,
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
struct Object {
    dir: Option<SharedString>,
    name: SharedString,
}

impl Object {
    /// A path, split; anything else whole.
    fn path(value: impl Into<SharedString>) -> Self {
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

    fn plain(value: impl Into<SharedString>) -> Self {
        Self {
            dir: None,
            name: value.into(),
        }
    }
}

impl ActivityRow {
    fn new(
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

    fn object(mut self, object: Option<Object>) -> Self {
        self.object = object;
        self
    }

    fn meta(mut self, meta: Option<gpui::AnyElement>) -> Self {
        self.meta = meta;
        self
    }

    fn fold(mut self, fold: Option<bool>) -> Self {
        self.fold = fold;
        self
    }
}

fn activity_row(
    row: ActivityRow,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let interactive = row.fold.is_some();
    let id = row.id.clone();
    let content = div()
        .h_flex()
        .items_center()
        .gap(PART_GAP)
        .w_full()
        .min_w_0()
        .py(ROW_PAD_Y)
        .px(ROW_PAD_X)
        // **A disc in the ink the state means, and nothing else in the slot.**
        // A tick and a cross are two drawings to read at a size where both are
        // a handful of strokes; a disc is one shape wherever it appears, so
        // what the column carries is a colour — and a colour is read without
        // being looked at. Running is the exception a static shape cannot
        // cover.
        .child(row.mark.draw(cx))
        .child(
            div()
                .size(KIND_ICON)
                .flex_none()
                .h_flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(Icon::empty().path(row.kind))
                        .size(KIND_ICON)
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_size(VERB_TEXT)
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
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(object.name),
                )
        }))
        .child(div().flex_1().min_w_0())
        .children(row.meta)
        .child(
            div()
                .size(CHEVRON_SLOT)
                .flex_none()
                .h_flex()
                .items_center()
                .justify_center()
                .children(row.fold.map(|open| {
                    Icon::new(match open {
                        true => IconName::ChevronDown,
                        false => IconName::ChevronRight,
                    })
                    .size(CHEVRON_MARK)
                    .text_color(cx.theme().muted_foreground.opacity(0.8))
                })),
        );

    match interactive {
        true => crate::controls::action(id)
            .ghost()
            .w_full()
            .min_w_0()
            .h_auto()
            .p_0()
            .rounded_none()
            .child(content)
            .on_click(on_click)
            .into_any_element(),
        // Nothing to open, so nothing to press: a hover fill and a pointer on a
        // row that does not answer is a promise the row cannot keep.
        false => div()
            .id(id)
            .w_full()
            .min_w_0()
            .child(content)
            .into_any_element(),
    }
}

/// The counts at the end of a row that changed a file.
///
/// **`−0` is not drawn**, which colour is what forces: a zero set in the ink
/// that means "this went" is the colour of a loss that did not happen.
fn line_counts(added: usize, removed: usize, cx: &App) -> Option<gpui::AnyElement> {
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
            .text_size(OBJECT_TEXT)
            .children(
                (added > 0).then(|| div().text_color(status.success).child(format!("+{added}"))),
            )
            .children(
                (removed > 0).then(|| div().text_color(status.danger).child(format!("−{removed}"))),
            )
            .into_any_element(),
    )
}

/// A duration, at the coarseness somebody reads it at.
///
/// **Seconds up to a minute, then minutes.** A step that took four hundred and
/// twelve seconds is a step that took seven minutes, and the extra two digits
/// are two digits the eye has to divide before it means anything.
fn elapsed(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        _ => format!("{}m {}s", secs / 60, secs % 60),
    }
}

/// A word at the end of a row, in the quiet ink or in one that means something.
fn row_note(text: impl Into<SharedString>, ink: gpui::Hsla, cx: &App) -> gpui::AnyElement {
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
pub(super) fn pill(label: impl Into<SharedString>, ink: gpui::Hsla, cx: &App) -> gpui::Div {
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
fn tool_label(kind: ToolKind) -> &'static str {
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

/// A stable element id per item. History and live indices overlap, so the
/// source has to be part of the key or two items share one id.
fn fold_key(target: TranscriptItemId) -> usize {
    match target {
        TranscriptItemId::History(i) => i * 2,
        TranscriptItemId::Live(i) => i * 2 + 1,
    }
}

/// The live-items index, or `None` for a history item.
///
/// Only live items are answerable: a permission replayed from the archive
/// carries an rpc id that no running adapter ever issued.
fn live_index(target: TranscriptItemId) -> Option<usize> {
    match target {
        TranscriptItemId::Live(i) => Some(i),
        TranscriptItemId::History(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onehand_core::acp::{ToolCall, ToolKind, ToolStatus};
    use onehand_core::chat::{Md, Thought};

    fn thought(secs: u64) -> ChatItem {
        ChatItem::Thought(Thought {
            md: Md::parse("…"),
            started: None,
            elapsed_secs: Some(secs),
            expanded: false,
        })
    }

    fn tool(kind: ToolKind, title: &str) -> ChatItem {
        tool_with_status(kind, title, ToolStatus::Completed)
    }

    fn tool_with_status(kind: ToolKind, title: &str, status: ToolStatus) -> ChatItem {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind,
            status,
            content: Vec::new(),
        }))
    }

    fn targets(items: &[ChatItem]) -> Vec<(TranscriptItemId, &ChatItem)> {
        items
            .iter()
            .enumerate()
            .map(|(i, item)| (TranscriptItemId::Live(i), item))
            .collect()
    }

    /// Every named step, at the app's own rem size.
    fn steps() -> Vec<(&'static str, f32)> {
        vec![
            ("HAIR_GAP", HAIR_GAP.0),
            ("TIGHT_GAP", TIGHT_GAP.0),
            ("STACK_GAP", STACK_GAP.0),
            ("PART_GAP", PART_GAP.0),
            ("FRAME_PAD", FRAME_PAD.0),
            ("BLOCK_GAP", BLOCK_GAP.0),
            ("TURN_GAP", TURN_GAP.0),
            ("TEXT_PAD_X", TEXT_PAD_X.0),
            ("TEXT_PAD_Y", TEXT_PAD_Y.0),
            ("ROW_PAD_Y", ROW_PAD_Y.0),
            ("ROW_PAD_X", ROW_PAD_X.0),
            ("STATUS_DOT", STATUS_DOT.0),
            ("KIND_ICON", KIND_ICON.0),
            ("CHEVRON_SLOT", CHEVRON_SLOT.0),
            ("PILL_PAD_Y", PILL_PAD_Y.0),
            ("PILL_PAD_X", PILL_PAD_X.0),
            ("PILL_H", PILL_H.0),
            ("BUTTON_H", BUTTON_H.0),
            ("LINE_H", LINE_H.0),
            ("MARK_SLOT", MARK_SLOT.0),
            ("MARK_SIZE", MARK_SIZE.0),
            ("DIFF_NUM_PAD", DIFF_NUM_PAD.0),
            ("DIFF_SIGN_W", DIFF_SIGN_W.0),
            ("DIFF_TEXT_PAD", DIFF_TEXT_PAD.0),
            ("PLAN_BOX", PLAN_BOX.0),
            ("THUMB_W", THUMB_W.0),
            ("THUMB_H", THUMB_H.0),
            ("SMOKE_DIFF", SMOKE_DIFF.0),
            ("SMOKE_OUT", SMOKE_OUT.0),
            ("DETAIL_OPEN_H", DETAIL_OPEN_H.0),
        ]
    }

    /// Nothing in the transcript is sized off the scale.
    ///
    /// The point of a scale is that the *next* value is chosen from it rather
    /// than measured by eye, and nothing about a rem constant stops somebody
    /// writing `rems(0.7)`. This is what says so out loud — and it fails on the
    /// value's name, so the failure names the step that left the ladder rather
    /// than printing a number.
    #[test]
    fn every_step_is_on_the_scale() {
        // Half a step is a step nobody can see, so the ladder skips no rung it
        // does not use and admits no rung between two it does.
        const SCALE: &[f32] = &[
            2., 4., 6., 8., 10., 12., 14., 16., 18., 20., 24., 28., 32., 40., 44., 62., 64., 92.,
            320.,
        ];
        for (name, rems) in steps() {
            let px = rems * 16.;
            assert!(
                SCALE.iter().any(|step| (step - px).abs() < 0.01),
                "{name} is {px}px, which is not a step of the scale"
            );
        }
    }

    /// What is inside a thing is closer than what surrounds it, at every level.
    ///
    /// This is the one rule the whole arrangement rests on: it is what makes a
    /// turn boundary readable without a rule across the column, and the first
    /// thing to break when one gap is nudged to fix the look of one block.
    #[test]
    fn the_gaps_nest_and_so_do_the_corners() {
        let ladder = [
            ("HAIR_GAP", HAIR_GAP.0),
            ("TIGHT_GAP", TIGHT_GAP.0),
            ("STACK_GAP", STACK_GAP.0),
            ("PART_GAP", PART_GAP.0),
            ("BLOCK_GAP", BLOCK_GAP.0),
            ("TURN_GAP", TURN_GAP.0),
        ];
        for pair in ladder.windows(2) {
            let [(inner, a), (outer, b)] = pair else {
                unreachable!()
            };
            assert!(a < b, "{inner} must stay under {outer}");
        }

        // **A row's words start after its padding, its state disc and its kind
        // icon**, and whatever that row opens is set in to the same place —
        // written as a sum in one spot and a number in the other, the two
        // drifted the first time either column moved.
        assert_eq!(
            DETAIL_INSET.0,
            ROW_PAD_X.0 + STATUS_DOT.0 + PART_GAP.0 + KIND_ICON.0 + PART_GAP.0
        );
        // The marks and the type, each inside what holds it: a disc inside the
        // slot that keeps its column (which is what lets a spinner take its
        // place), the arrow quietest of the three, and what a row did it *to*
        // a step under what it says it did.
        let inside = [
            ("STATUS_DOT", STATUS_DOT.0, "KIND_ICON", KIND_ICON.0),
            ("CHEVRON_MARK", CHEVRON_MARK.0, "KIND_ICON", KIND_ICON.0),
            ("OBJECT_TEXT", OBJECT_TEXT.0, "VERB_TEXT", VERB_TEXT.0),
        ];
        for (inner, a, outer, b) in inside {
            assert!(a < b, "{inner} must stay under {outer}");
        }
    }

    #[test]
    fn every_activity_group_has_its_own_name_and_icon() {
        let groups = [
            activity::ActivityGroup::Explored,
            activity::ActivityGroup::Changed,
            activity::ActivityGroup::Ran,
            activity::ActivityGroup::Verified,
            activity::ActivityGroup::Reasoned,
            activity::ActivityGroup::Other,
        ];
        let identities = groups.map(activity_identity);
        let mut names = identities.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        // The identity already carries the asset path, which is the fact this
        // test is about: two groups drawing the same SVG is the collision worth
        // catching, whatever the two names in front of it are -- and the six
        // are no longer even drawn from one enum.
        let mut icons = identities
            .iter()
            .map(|(_, icon)| icon.clone())
            .collect::<Vec<_>>();
        icons.sort_unstable();
        icons.dedup();

        assert_eq!(names.len(), groups.len());
        assert_eq!(icons.len(), groups.len());
    }

    /// **A cluster of one is still a cluster.** It was the exception —
    /// a lone step drew itself bare — which meant the transcript had two ways
    /// of saying the same thing and which one a reader got depended on whether
    /// the agent happened to do a second thing afterwards.
    #[test]
    fn one_step_is_still_a_cluster() {
        let items = vec![thought(3)];
        let runs = runs(&targets(&items));
        assert!(
            matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 1),
            "a lone step is a cluster of one"
        );
    }

    /// **A cluster is bounded by the agent's words and by nothing else.** Kinds
    /// of work used to bound it too, so three reads and a command between one
    /// paragraph and the next drew two headers — two claims about one stretch
    /// of work, with nothing between them to explain the seam.
    #[test]
    fn every_kind_of_work_between_two_paragraphs_is_one_cluster() {
        let items = vec![
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
            tool(ToolKind::Execute, "cargo build"),
            tool(ToolKind::Edit, "src/a.rs"),
            thought(2),
        ];
        let runs = runs(&targets(&items));
        assert!(
            matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 5),
            "{} runs, wanted one",
            runs.len()
        );
    }

    /// **A step's status does not decide which cluster it is in.** Running work
    /// used to sit outside, and move in when it finished — so the row count
    /// changed every few seconds mid-turn, and a step that had been a card
    /// became a line in a list somebody was already reading.
    #[test]
    fn what_a_step_is_doing_does_not_move_it_between_clusters() {
        let items = vec![
            tool_with_status(ToolKind::Read, "Read src/pending.rs", ToolStatus::Pending),
            tool_with_status(ToolKind::Read, "Read src/live-a.rs", ToolStatus::InProgress),
            tool_with_status(ToolKind::Read, "Read src/failed.rs", ToolStatus::Failed),
            tool(ToolKind::Read, "Read src/done-a.rs"),
            tool(ToolKind::Read, "Read src/done-b.rs"),
        ];
        let runs = runs(&targets(&items));
        assert!(
            matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 5),
            "{} runs, wanted one",
            runs.len()
        );
    }

    #[test]
    fn a_prompt_breaks_a_run() {
        // A user prompt is not activity, so the steps either side of it belong
        // to different turns and must not be folded into one strip.
        let items = vec![
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
            ChatItem::notice("interrupted"),
            tool(ToolKind::Read, "Read src/c.rs"),
        ];
        let runs = runs(&targets(&items));
        assert!(
            matches!(
                runs.as_slice(),
                [
                    Run::Activity { members: before },
                    Run::Single(_),
                    Run::Activity { members: after }
                ] if before.len() == 2 && after.len() == 1
            ),
            "the notice between them is what makes two clusters of one"
        );
    }

    #[test]
    fn summary_aggregates_repeated_steps() {
        let items = [thought(3), thought(3), thought(4), thought(5), thought(6)];
        let bodies: Vec<&ChatItem> = items.iter().collect();
        assert_eq!(activity_summary(&bodies), "21s reasoning");

        let reads = [
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
        ];
        let bodies: Vec<&ChatItem> = reads.iter().collect();
        assert_eq!(activity_summary(&bodies), "2 files");

        let failed_reads = [
            tool(ToolKind::Read, "Read src/a.rs"),
            tool_with_status(ToolKind::Read, "Read src/b.rs", ToolStatus::Failed),
        ];
        // **The state is no longer in the summary**, and neither is the verb.
        // The row this lands in carries both, two columns to the left.
        let bodies: Vec<&ChatItem> = failed_reads.iter().collect();
        assert_eq!(activity_summary(&bodies), "2 files");

        let multi_edit = ChatItem::Tool(ToolItem::new(ToolCall {
            id: "multi-edit".into(),
            title: "Edit project config".into(),
            description: None,
            kind: ToolKind::Edit,
            status: ToolStatus::Completed,
            content: vec![
                ToolContent::Diff {
                    path: "package.json".into(),
                    old: Some("{}".into()),
                    new: "{\"type\":\"module\"}".into(),
                },
                ToolContent::Diff {
                    path: "tsconfig.json".into(),
                    old: Some("{}".into()),
                    new: "{\"module\":\"NodeNext\"}".into(),
                },
            ],
        }));
        assert_eq!(activity_summary(&[&multi_edit]), "2 files");
    }

    /// **A path is split so the name can lead.** A reader scanning a column of
    /// them is looking for the file; the directory above it is only there for
    /// the times two names are the same, and at one weight it takes the eye
    /// first every time it is long.
    #[test]
    fn a_row_splits_a_path_and_leaves_a_command_whole() {
        let file = Object::path("crates/app/src/chat/pane.rs");
        assert_eq!(file.dir.as_deref(), Some("crates/app/src/chat/"));
        assert_eq!(file.name.as_ref(), "pane.rs");

        let bare = Object::path("README.md");
        assert_eq!(bare.dir, None);
        assert_eq!(bare.name.as_ref(), "README.md");

        // A command holds slashes and is not a path: cut at the last one it
        // would quote something nobody ran.
        let command = Object::path("cargo test -p onehand --manifest-path ./Cargo.toml");
        assert_eq!(command.dir, None);
        assert!(command.name.contains("cargo test"));
    }

    /// Output is read for the line that went wrong, so that line is the one
    /// the ink follows — not the row it sits in.
    #[test]
    fn the_line_that_went_wrong_is_the_one_marked() {
        assert!(is_error_line("error[E0433]: failed to resolve"));
        assert!(is_error_line("  FAILED: 1 test"));
        assert!(is_error_line("panicked at src/lib.rs:4"));
        assert!(!is_error_line("test result: ok. 34 passed"));
        assert!(!is_error_line("   Compiling onehand v0.1.0"));
    }

    /// The mark a run's row carries reads its ending, not its worst moment.
    #[test]
    fn a_fixed_failure_is_not_an_alarm() {
        use onehand_core::chat::{Outcome, RunOutcome};
        let mark = |outcome| RowMark::of_run(RunOutcome { outcome, errors: 0 });
        assert!(matches!(mark(Outcome::Clean), RowMark::Done));
        assert!(matches!(mark(Outcome::Recovered), RowMark::Recovered));
        assert!(matches!(mark(Outcome::Failed), RowMark::Failed));
        assert!(matches!(mark(Outcome::Running), RowMark::Running));
    }

    /// **Two clusters never stand next to each other.**
    ///
    /// It is the rule the whole arrangement rests on: a cluster is bounded by
    /// the agent's words, so two of them with nothing between is one stretch of
    /// work claiming to be two — and drawn, that is two muted lines a reader has
    /// to work out the seam between.
    #[test]
    fn two_clusters_never_stand_next_to_each_other() {
        let items = vec![
            ChatItem::User(onehand_core::chat::UserMsg::text("go")),
            tool(ToolKind::Read, "a.rs"),
            thought(2),
            tool(ToolKind::Execute, "cargo build"),
            tool_with_status(ToolKind::Read, "b.rs", ToolStatus::InProgress),
            ChatItem::Agent(Md::parse("done")),
            tool(ToolKind::Edit, "a.rs"),
            ChatItem::notice("interrupted"),
            tool(ToolKind::Read, "c.rs"),
        ];
        let runs = runs(&targets(&items));
        let mut previous_was_cluster = false;
        for run in &runs {
            let cluster = matches!(run, Run::Activity { .. });
            assert!(
                !(cluster && previous_was_cluster),
                "two clusters with nothing between them"
            );
            previous_was_cluster = cluster;
        }
        // And what does bound one is prose, a prompt, or a notice — three
        // boundaries here, so four clusters.
        assert_eq!(
            runs.iter()
                .filter(|run| matches!(run, Run::Activity { .. }))
                .count(),
            3
        );
    }

    /// **A line of this project's own code fits the reading column.**
    ///
    /// The column used to be set by prose alone, which left a diff 74 columns
    /// wide against the 100 `rustfmt` writes at — so nearly every line of
    /// nearly every diff wrapped, in the one block somebody opens the
    /// transcript to read when something has broken. The cap is derived from
    /// that instead, and this is what keeps it derived: shift any inset between
    /// the column and the text and the sum moves, rather than the diff quietly
    /// getting tighter.
    #[test]
    fn a_hundred_columns_of_this_projects_code_fits() {
        // Every inset between the edge of the column and a diff's first
        // character, at the deepest place one is drawn: inside a child row's
        // own detail.
        // Every inset between the edge of the column and a diff's first
        // character: the row's own detail inset, the frame's right margin, the
        // detail box's border, and the diff's number and sign columns.
        let chrome = DETAIL_INSET.0
            + ROW_PAD_X.0
            + 2. / 16.
            + DIFF_NUM_W.0
            + DIFF_SIGN_W.0
            + DIFF_TEXT_PAD.0;
        let text = CODE_TEXT.0 * MONO_ADVANCE * DIFF_COLUMNS;
        assert!(
            CONTENT_COLUMN.0 >= chrome + text,
            "{DIFF_COLUMNS} columns need {:.2}rem and the column caps at {:.2}rem",
            chrome + text,
            CONTENT_COLUMN.0
        );
    }
}
