use gpui::{App, Rems, rems};
use gpui_component::ActiveTheme;
use onehand_core::chat::COMMAND_FOLD_LINES;

/// The transcript's reading size: what the agent and the person say.
///
/// A step over the app's chrome, because a conversation is read the way a page
/// is, line after line, while the rows and controls around it are aimed at.
/// Fed to the markdown renderer's heading base as well, so an answer's headings
/// are scaled off the prose they sit in rather than off the app's base.
pub(in crate::chat) const TEXT: Rems = rems(1.);
/// One step under the reading size: the record of how an answer got made.
/// Activity lines, a thought's reasoning, code and every well of machine text
/// take it, so what the agent did reads quieter than what it said.
pub(in crate::chat) const TEXT_SM: Rems = rems(0.875);
/// Line height as a multiple of the size, for prose and wells alike: set once
/// on the column as a ratio, so a well a step smaller keeps the same rhythm.
pub(in crate::chat) const LEADING: f32 = 1.6;
/// An answer's first two heading levels over its body; below them a heading
/// is told apart by weight alone, so a deep one never prints smaller than the
/// prose it names.
pub(super) const HEADING_1: f32 = 1.25;
pub(super) const HEADING_2: f32 = 1.125;
/// A state's ink thinned to a fill: a diff line's green or red, an error's
/// banner. Thin enough that the text on it keeps its own ink.
pub(super) const STATE_TINT: f32 = 0.1;
/// How tall a fenced block in an answer stands before it is clipped, rather
/// than pushing the rest of the answer off the screen; its Copy reaches the
/// part that is cut.
pub(super) const MAX_CODE_BLOCK_H: Rems = rems(22.5);
/// Space between the paragraphs of one answer: under the space between whole
/// blocks, because two paragraphs are parts of one block.
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

/// A control the pointer has to hit.
pub(super) const BUTTON_H: Rems = rems(1.5);
/// One line of machine text.
pub(super) const LINE_H: Rems = rems(1.25);
/// The glyph on a well's copy button.
pub(super) const MARK_SIZE: Rems = rems(0.75);

/// What a collapsed detail shows before it says there is more.
///
/// **A diff from the top and an output from the bottom**, because that is where
/// each keeps its point: a diff opens on the change, and a command's failure is
/// the last thing it printed.
pub(super) const PREVIEW_DIFF: usize = 12;
pub(super) const PREVIEW_OUT: usize = 5;
/// How tall an opened detail stands before it scrolls inside itself.
pub(super) const DETAIL_OPEN_H: Rems = rems(20.);
/// Changed lines past which a diff is offered rather than drawn.
///
/// **A thousand-line diff is not read, it is searched** — and drawing it costs
/// one element per line in a list that is already virtualising rows. Past this
/// the row says how big it is and waits to be asked.
pub(super) const LARGE_DIFF: usize = 400;

/// The reading column, its side inset included, centred in the panel.
///
/// Set by prose: about as far as a line can run before the eye loses its
/// place coming back to the next one. What is wider than that — a diff, a
/// command's output — scrolls sideways inside its own well rather than
/// widening the column for every paragraph around it.
pub(in crate::chat) const CONTENT_COLUMN: Rems = rems(44.);
/// The widest a prompt's bubble grows, so a long one wraps well short of the
/// agent's left edge and the two sides stay told apart.
pub(super) const BUBBLE_MAX: Rems = rems(28.);
/// The corner ladder below the theme's card corner: a mark, a control, a
/// block. Wells and the user's bubble take the card corner itself.
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
pub(super) fn radius_block(cx: &App) -> gpui::Pixels {
    cx.theme().radius
}
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
pub(super) const FOLD_H: Rems = rems(TEXT_SM.0 * LEADING * COMMAND_FOLD_LINES as f32);

// The question and permission cards pinned above the composer.
/// The question card's side padding, which its choices' scroll frame reaches
/// back through to put the thumb on the card's edge.
pub(super) const ASK_INSET: Rems = rems(1.);
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
/// Left and right inside a frame that lays its contents out in columns.
pub(super) const FRAME_PAD: Rems = rems(0.75);
/// A status pill: as tall as the words in it and no taller.
pub(super) const PILL_H: Rems = rems(1.125);
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
