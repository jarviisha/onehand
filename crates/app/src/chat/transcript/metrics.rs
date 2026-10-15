use gpui::{Rems, rems};

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
/// An answer's first two heading levels over its body; below them a heading
/// is told apart by weight alone, so a deep one never prints smaller than the
/// prose it names.
pub(super) const HEADING_1: f32 = 1.25;
pub(super) const HEADING_2: f32 = 1.125;
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

/// Where a folding block's body starts: under the words of its fold line,
/// past the chevron (an `xsmall` icon, 0.75rem) and the gap after it (`gap_2`).
pub(super) const FOLD_INDENT: Rems = rems(0.75 + 0.5);
/// One line of machine text.
pub(super) const LINE_H: Rems = rems(1.25);

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
/// Closer than anything else in the transcript, the scale's smallest step: the
/// control belongs to the bubble rather than following it, and any wider it
/// read as a separate thing that happened to be beneath.
pub(super) const BUBBLE_TAIL_GAP: Rems = rems(0.125);

/// How much of the panel a permission's command box may take, on a panel too
/// short for [`COMMAND_H`]: the heading that says what is asked and the
/// buttons that answer it have to stay on screen with it.
pub(super) const COMMAND_SHARE: f32 = 0.5;
/// The library scrollbar's width, held clear for it beside text it scrolls.
pub(super) const SCROLLBAR_W: gpui::Pixels = gpui::px(16.);
/// The fade over the foot of a command box that scrolls on past it.
pub(super) const COMMAND_FADE: Rems = rems(3.);
/// Roughly one mono character at the well's own size — what a line number's
/// column is measured in, since the gutter has to be as wide as the largest
/// number and no wider.
pub(super) const MONO_ADVANCE: f32 = 0.62;
/// One line of the secondary size, at the reading leading.
pub(super) const LINE_SM: Rems = rems(TEXT_SM.0 * crate::tokens::LEADING);
/// How tall a permission's command box stands before it scrolls: eight lines,
/// which leaves the heading, the buttons and enough of a script to recognise
/// on one screen. A height and not a line count, so one line three thousand
/// characters long is held to the same box as eight short ones.
pub(super) const COMMAND_H: Rems = rems(LINE_SM.0 * 8.);

// The question and permission cards pinned above the composer.
/// A pinned card's side padding, which a question's choices' scroll frame
/// reaches back through to put the thumb in it: wide enough that the thumb
/// has room of its own beside the rows.
pub(super) const CARD_PAD: Rems = rems(1.);
/// Left and right inside a frame that lays its contents out in columns.
pub(super) const FRAME_PAD: Rems = rems(0.75);
/// Width a question's tab label is elided at. Only a *tab* is ever elided.
pub(super) const ASK_TAB_W: Rems = rems(8.75);
