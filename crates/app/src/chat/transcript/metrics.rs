use gpui::{App, Rems, rems};
use gpui_component::ActiveTheme;
use onehand_core::chat::COMMAND_FOLD_LINES;

/// Diff lines drawn per tool card, **shared across all its hunks** — a
/// MultiEdit touching twenty files must not cost twenty times the budget.
pub(super) const MAX_DIFF_LINES: usize = 200;
/// Lines of a mono output well before the tail is dropped.
pub(super) const MAX_MONO_LINES: usize = 60;
/// Plan entries drawn before the list is truncated.
pub(super) const MAX_TODO_ITEMS: usize = 50;
/// Attachment rows drawn under a prompt before the rest are counted.
pub(super) const MAX_ATTACHMENT_ROWS: usize = 8;
/// Height a fenced code block in prose is allowed before it scrolls inside
/// itself instead of pushing the rest of the answer off screen.
pub(super) const MAX_CODE_BLOCK_H: Rems = rems(22.5);
/// Height the body of a blocking card may occupy before it scrolls inside
/// itself.
///
/// Set a step under the transcript's other bounded card because what it holds
/// back is different: that one is a detail somebody chose to unfold, this one is
/// standing between the conversation and everything after it. The number is what
/// leaves the card's header, its body and the buttons that answer it on one
/// screen together at the sizes around them — which is the whole point of
/// bounding it, and is why it is a height rather than a count of lines or rows.
pub(super) const MAX_BLOCKING_BODY_H: Rems = rems(16.);
/// The question card's side padding, which its choices' scroll frame reaches
/// back through to put the thumb on the card's edge.
pub(super) const ASK_INSET: Rems = rems(1.);
/// The size every well of machine text is set at — a tool's output, a diff, a
/// live terminal, and the fenced blocks inside an answer.
///
/// **One size, because they are one thing.** All four are "a machine produced
/// this", and the two that arrived by different routes had drifted apart: the
/// wells this file draws were a step below the meta labels naming them, while
/// the markdown renderer set its blocks from a fixed pixel size of its own. The
/// same command run by the agent therefore read at one size in a tool card and
/// another when quoted back in prose.
pub(super) const CODE_TEXT: Rems = rems(0.8125);
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
pub(in crate::chat) const TEXT: Rems = rems(0.875);
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
pub(super) const CLUSTER_TEXT: Rems = TEXT;
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
pub(super) const WORK_TEXT: Rems = rems(0.8125);
/// A fenced block inside an answer: its own size and leading.
///
/// **Split from the size the other wells share, and only here.** A tool's
/// output, a diff and a live terminal are quoted *machine* text inside chrome;
/// a fenced block is something the agent chose to show in the middle of a
/// sentence, read at the pace of the prose around it. It sits a hair under the
/// others and breathes more between lines for that reason.
pub(super) const FENCE_TEXT: Rems = rems(0.78125);
pub(super) const FENCE_LEADING: f32 = 1.75;
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
pub(super) const PARAGRAPH_GAP: Rems = PART_GAP;
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
pub(in crate::chat) const TURN_GAP: Rems = rems(2.);
/// Between two blocks of one turn.
pub(in crate::chat) const BLOCK_GAP: Rems = rems(0.75);
/// Between the parts of one block.
pub(super) const PART_GAP: Rems = rems(0.5);
/// Between two rows that are read as one list, and between a line and the
/// caption belonging to it.
pub(in crate::chat) const TIGHT_GAP: Rems = rems(0.25);
/// A caption sitting directly over the thing it captions, where anything wider
/// would read as two lines rather than one two-line thing.
pub(super) const HAIR_GAP: Rems = rems(0.125);
/// The air between two small stacked things — an attachment and its neighbour,
/// a bubble and the row of controls under it.
pub(super) const STACK_GAP: Rems = rems(0.375);
/// Left and right inside a frame that lays its contents out in columns.
pub(super) const FRAME_PAD: Rems = rems(0.75);
/// The wider frame inset, for a box whose text runs its full width rather than
/// sitting in columns: a bubble, a well, a callout.
pub(super) const TEXT_PAD_X: Rems = rems(0.875);
/// The vertical half of that pair, a touch under the horizontal for the reason
/// every line of type is — a line box already carries its own leading.
pub(super) const TEXT_PAD_Y: Rems = rems(0.625);

/// A status pill: as tall as the words in it and no taller.
pub(super) const PILL_H: Rems = rems(1.125);
/// A control the pointer has to hit.
pub(super) const BUTTON_H: Rems = rems(1.5);
/// One line of machine text.
pub(super) const LINE_H: Rems = rems(1.25);
/// The fixed box every mark in a row is drawn inside. Drawings are smaller than
/// it; what the box buys is that a spinner, a tick and a cross take each other's
/// place without the words beside them moving.
pub(super) const MARK_SLOT: Rems = rems(1.);
/// The drawing inside that box.
pub(super) const MARK_SIZE: Rems = rems(0.75);
/// An activity row's own insets and columns.
///
/// **Its padding is not the frame's.** A row is a line of a list rather than a
/// box with something in it, so it stands off the frame's edge by more than the
/// frame's own hairline inset and sits tighter top to bottom than a card would.
pub(super) const ROW_PAD_Y: Rems = rems(0.5);
pub(super) const ROW_PAD_X: Rems = rems(0.875);
/// The disc that carries a row's state, and the only thing in its slot.
pub(super) const STATUS_DOT: Rems = rems(0.375);
/// The drawing for a kind of work, and the box it sits in — one and the same,
/// because unlike a state there is only ever one shape here.
pub(super) const KIND_ICON: Rems = rems(0.875);
/// The verb, in the reading face; what it was done to, in the machine one.
pub(super) const VERB_TEXT: Rems = rems(0.8125);
pub(super) const OBJECT_TEXT: Rems = rems(0.75);
/// The arrow, and the slot holding its column while it turns.
pub(super) const CHEVRON_MARK: Rems = rems(0.6875);
pub(super) const CHEVRON_SLOT: Rems = rems(0.75);
/// How far a glyph drops to sit on the line its text does.
///
/// **An optical correction, and the one number here that is not on the scale.**
/// A centred box and centred *type* are not the same place: the renderer puts a
/// line's baseline at `(line_height − ascent − descent) / 2 + ascent`, and since
/// a face's ascent is the larger of the two the baseline lands below the middle
/// of the box — so lowercase text sits about a tenth of an em low inside its own
/// line, and an icon centred against that box comes out looking that much high.
/// Every row here puts a mark beside words, so every one of them needs it.
///
/// A tenth of an em at the sizes on these rows is a pixel, which is why it is
/// written as one rather than as a fraction of a size that changes: half a pixel
/// would not survive the rounding, and two would be a visible drop.
pub(super) const GLYPH_DROP: Rems = rems(0.0625);

/// Where a row's verb starts, and so where whatever it opens is set in to.
///
/// **A sum, written once and read twice.** It is the row's padding, the state
/// disc, the kind icon and the two gaps between them — and what unfolds beneath
/// a row lines up with the words that opened it rather than with the marks
/// beside them. Written out as a number in the second place, the two drifted
/// the first time either column moved.
pub(super) const DETAIL_INSET: Rems =
    rems(ROW_PAD_X.0 + STATUS_DOT.0 + PART_GAP.0 + KIND_ICON.0 + PART_GAP.0);

/// The detail box: machine text, at the leading a list of it wants.
pub(super) const CODE_LH: f32 = 1.7;
/// A diff's three columns.
pub(super) const DIFF_NUM_W: Rems = rems(2.75);
pub(super) const DIFF_NUM_PAD: Rems = rems(0.5);
pub(super) const DIFF_TEXT_PAD: Rems = rems(0.625);

/// What a collapsed detail shows before it says there is more.
///
/// **A diff from the top and an output from the bottom**, because that is where
/// each keeps its point: a diff opens on the change, and a command's failure is
/// the last thing it printed.
pub(super) const PREVIEW_DIFF: usize = 12;
pub(super) const PREVIEW_OUT: usize = 5;
/// How far the cut fades into the box, at whichever end the cut is.
pub(super) const SMOKE_DIFF: Rems = rems(4.);
pub(super) const SMOKE_OUT: Rems = rems(2.75);
/// How tall an opened detail stands before it scrolls inside itself.
pub(super) const DETAIL_OPEN_H: Rems = rems(20.);
/// Changed lines past which a diff is offered rather than drawn.
///
/// **A thousand-line diff is not read, it is searched** — and drawing it costs
/// one element per line in a list that is already virtualising rows. Past this
/// the row says how big it is and waits to be asked.
pub(super) const LARGE_DIFF: usize = 400;

/// The little pill that opens and closes a detail.
pub(super) const PILL_PAD_Y: Rems = rems(0.125);
pub(super) const PILL_PAD_X: Rems = rems(0.75);

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
pub(in crate::chat) const CONTENT_COLUMN: Rems = rems(59.);
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
pub(super) const DIFF_COLUMNS: f32 = 100.;
/// The `+` / `-` column of a diff: one mono character and the air either side.
///
/// A column of its own rather than a character glued to the front of the line,
/// which is what it had been. Glued, the sign shifted every line of the file
/// one place right of where the file has it and left a removed line and the
/// added line replacing it starting at two different columns — so the one thing
/// a diff is read for, what changed between two nearly identical lines, was
/// being compared across an offset the diff itself introduced.
pub(super) const DIFF_SIGN_W: Rems = rems(0.875);
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
pub(in crate::chat) fn radius_tag(cx: &App) -> gpui::Pixels {
    corner(cx.theme().radius, -4.)
}
/// A button, a status pill, an inline mark, a callout's open side.
///
/// **A pill is a corner and not a capsule**, which is the one place this ladder
/// overrules the shape a pill usually has. Fully round, it was the only
/// silhouette in the transcript that curved — a lozenge at the end of a row of
/// square boxes, reading as a badge stuck onto the row rather than as one of
/// its columns.
pub(super) fn radius_control(cx: &App) -> gpui::Pixels {
    corner(cx.theme().radius, -2.)
}
/// Everything that is a box with content in it: an activity block, a well of
/// machine text, a diff, a banner, a plan, a thumbnail.
///
/// **One step for all of them**, because they are one thing — a bounded region
/// of the column — and a ladder that gave a well and the block around it two
/// different corners would be spending a rung on a distinction nobody reading
/// it is making.
pub(in crate::chat) fn radius_block(cx: &App) -> gpui::Pixels {
    cx.theme().radius
}
/// A user's bubble, and nothing else.
///
/// **The one corner in the transcript that is not nearly square.** Everything
/// else here is a bounded region of a document and takes the tight ladder for
/// that reason; this is the one block shaped like a thing somebody *said*, and
/// the roundness is what says so before a word of it is read. Its own tail
/// corner comes back down to a control's, which is what points the shape at the
/// side the prompt came from.
pub(super) fn radius_bubble(cx: &App) -> gpui::Pixels {
    corner(cx.theme().radius_lg, 6.)
}
/// Width a question's tab label is elided at. Only a *tab* is ever elided.
pub(super) const ASK_TAB_W: Rems = rems(8.75);
/// The numbered circle at the head of a question's tab.
///
/// Sized to the digit rather than to the row: a strip of four tabs carries four
/// of these, and a circle as tall as the label beside it is a bullet the eye
/// reads before the word it belongs to.
pub(super) const ASK_TAB_MARK: Rems = rems(1.125);
/// The radio or checkbox at the head of a choice row, and the mark inside it
/// once the choice is taken.
///
/// Two numbers because the inner one is not a fraction of the outer: the ring
/// has a border of its own, and a dot derived from the outside measurement
/// would grow into it at one size and float inside it at another.
pub(super) const ASK_CHOICE_MARK: Rems = rems(1.0);
pub(super) const ASK_CHOICE_DOT: Rems = rems(0.5);
/// The ring around that mark, in pixels and not rems: it is a hairline's job
/// done a touch heavier, and a hairline is the one measurement in the window
/// that must not scale with a panel's zoom — doubled, it stops being a line.
pub(super) const ASK_MARK_RING: gpui::Pixels = gpui::px(1.5);
/// The key-hint chip at the end of a choice row: a floor rather than a size, so
/// a two-digit hint grows sideways instead of overrunning its border.
pub(super) const ASK_HINT_SIZE: Rems = rems(1.25);
pub(super) const ASK_HINT_PAD: Rems = rems(0.3125);
/// The height a choice row stands at whatever it holds.
///
/// A floor, not a height. The label and the description under it are the
/// agent's sentences, so the row grows with them; what this answers is the
/// other end — a row whose label is one word, which shrink-wrapped is a target
/// half the size of the one below it.
pub(super) const ASK_ROW_MIN: Rems = rems(3.25);
/// The same floor for the free-text row, a step under it: that row is one line
/// by construction, and standing it at a two-line height would leave a band of
/// empty surface under an input that is nowhere near filling it.
pub(super) const ASK_CUSTOM_ROW_MIN: Rems = rems(2.75);
/// The height of one tab, which is also the strip's.
pub(super) const ASK_TAB_H: Rems = rems(2.25);
/// Leading for the question itself — looser than a control's and tighter than
/// prose, because it is one sentence that has to be read once and is as long as
/// the agent made it.
pub(super) const ASK_PROMPT_LEADING: f32 = 1.4;
/// The box a plan entry's state is drawn in, and the mark inside a pending one.
///
/// A square rather than an icon, because what the column is asking is whether
/// a line is done — and a checkbox is the one shape a reader does not have to
/// be taught. The three states change what is *in* the box and never the box,
/// so a plan does not reflow as it is worked through.
pub(super) const PLAN_BOX: Rems = rems(0.875);
pub(super) const PLAN_DOT: Rems = rems(0.25);
/// The bar under a plan's heading. A rule's weight rather than a control's:
/// it is read in one glance beside a count that already says the same thing.
pub(super) const PLAN_BAR_H: Rems = rems(0.1875);
/// An attached file's plate beside a prompt.
///
/// **A fixed plate, not a bounded picture.** A preview is here to be
/// recognised — it answers "which screenshot did I send" and nothing more, and
/// the file itself is one click away in whatever opens it. Bounded only in
/// height, a row of them came out a different width each and the row read as
/// ragged rather than as a set; at one size they are a strip of cards, and a
/// file with no picture takes the same plate with its name in it.
pub(super) const THUMB_W: Rems = rems(5.75);
pub(super) const THUMB_H: Rems = rems(3.875);
/// How much of the row a user prompt may take before it wraps.
///
/// The prompt is the one block that shrinks to what was typed: a one-line
/// question stretched edge to edge is indistinguishable from an answer, and
/// telling the two apart at a glance is the whole reason it sits on the other
/// side. Past this it wraps rather than crowding the answer beneath it.
///
/// The wider figure is for a narrow panel, where the column is already short
/// enough that holding a fifth of it clear costs more than the edge is worth.
pub(super) const USER_BUBBLE_MAX: f32 = 0.78;
pub(super) const USER_BUBBLE_MAX_NARROW: f32 = 0.9;
/// The bubble's own padding, and the one pair here off the spacing scale.
///
/// **A corner has to be cleared before it can be padded.** At the scale's own
/// steps the text either crowded the 14px curve at the two ends of every line
/// or stood a whole step clear of it and left the bubble looking hollow; these
/// two are the pair that sits the words just outside the arc. Named and kept
/// together so the reason travels with them, and deliberately not entered in
/// the scale's own list: they are an optical fit to a radius, the way
/// [`GLYPH_DROP`] is an optical fit to a baseline.
pub(super) const BUBBLE_PAD_Y: Rems = rems(0.6875);
pub(super) const BUBBLE_PAD_X: Rems = rems(0.9375);
/// Between the bubble and the control offered under it.
///
/// Closer than anything else in the transcript: the control belongs to the
/// bubble rather than following it, and at any of the scale's steps it read as
/// a separate thing that happened to be beneath.
pub(super) const BUBBLE_TAIL_GAP: Rems = rems(0.1875);

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
pub(super) const CONTROL_ROW: Rems = rems(1.5);

/// How much of the window an opened command block may take before it scrolls
/// inside itself.
///
/// **A share of the viewport and not a fixed height**, unlike every other
/// bound in this file: the two things that must stay on screen whatever the
/// command does are the heading that says what is being asked and the buttons
/// that answer it, and what is left between them is whatever the window
/// happens to be tall. A fixed rem bound picked for a laptop leaves half a
/// large screen unused and pushes the buttons off a small one.
pub(super) const COMMAND_OPEN_SHARE: f32 = 0.5;
/// The block's copy button, and the inset it keeps from the block's corner.
pub(super) const COPY_SIZE: Rems = rems(1.75);
pub(super) const COPY_ICON: Rems = rems(0.875);
pub(super) const BLOCK_INSET: Rems = rems(0.375);
/// The fold control at the foot of a block, and the fade it sits on.
pub(super) const FOLD_ROW: Rems = rems(1.5);
/// Roughly one mono character at the well's own size — what a line number's
/// column is measured in, since the gutter has to be as wide as the largest
/// number and no wider.
pub(super) const MONO_ADVANCE: f32 = 0.62;
/// How tall the command block stands while it is folded.
///
/// **The fold is a height, and only then a line count.** Slicing the agent's
/// newlines is what decides *which* lines are drawn, and it is the predictable
/// rule for that -- but it cannot bound one line three thousand characters
/// long, which wraps to a screenful and is still one line. Given the same
/// height as eight short ones, every command folds to the same box whatever
/// shape its text is, and the control that opens it is offered on the same
/// terms.
pub(super) const FOLD_H: Rems = rems(CODE_TEXT.0 * CODE_LEADING * COMMAND_FOLD_LINES as f32);
