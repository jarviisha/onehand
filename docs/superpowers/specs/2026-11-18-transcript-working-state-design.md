# The transcript, redrawn where it reads badly

A clickable mock-up of every state below, each beside what the app draws today, is
`localDocs/2026-11-18-transcript-prototype/index.html` (local, not checked in). Where this
document and the mock-up disagree, this document is right; the mock-up's icons and its
"today" side are approximations.

## Intent

The owner reads long agent turns in the transcript all day and finds four parts of it
hard to read:

- **While a turn works**, the same running step is named three times, in two tenses, under
  two spinners in the accent; the spinner itself is disliked.
- **When a turn ends early**, a step the person stopped reads *failed* in the danger ink, and
  an agent that ran out of room reads *Turn failed*.
- **What a row opens into** cuts its command at the box's edge, shows a search's last lines
  rather than its first, paints a new file green from end to end, and grows without limit.
  The block of a turn's changes repeats itself and prints absolute paths.
- **A subagent's card** lists every step flat inside the conversation, where none of them
  opens, and the conversation reads as two stories at once.

## Goals

- While a turn works, each fact is said once, in the tense that is true.
- No rotating spinner in the transcript: what is alive is told by its words, which a band of
  brighter ink sweeps across. Every slot holds the same mark before and after a step settles.
- An ending says what happened: stopped by the person, stopped at a limit, or failed.
- Nothing a row opens into is taller than one cap; what is past it scrolls inside and can be
  opened whole.
- A subagent is one line in the conversation; what it did is read apart from it.

## Non-goals

- The pinned cards, tables, prose, the prompt bubble, the plan card's layout.
- Spinners outside the transcript.
- A setting that turns motion off. The app has none; recorded as a gap.
- Drawing what the protocol offers and the app drops today (context compaction, a usage
  update's cost, the `switch_mode` kind, unknown updates). Recorded as gaps.
- Workers. A worker is a session of its own and keeps its row in the rail and its band.

## Anti-goals

- A shimmer bright or fast enough to tire a reader beside a long turn.
- A folded cluster that no longer says what runs inside it.
- A float that hides the conversation's tail, where new blocks arrive.
- Any action removed without the owner saying so. One is: *Show every diff* (below).

## Approaches

**The working mark.** Considered: an arc that turns (still a wheel per row); a dot that
breathes (one more mark in the kind's slot); the same spinner in the muted ink (changes the
colour, not the complaint); **the words move and the mark stays (picked by the owner)**.

**Where a subagent's work is read.** Considered: a better card in place (still two stories
in one column, and needs fold addresses for nested steps); the pane swapped to the subagent
with a way back (cannot see both); a Workbench mode (the dock is narrow, often shut, and the
mode is unlike its peers); a modal dialog (blocks the conversation; wrong for work still
running); **a float over the pane (proposed by the owner, approved)**.

**Long detail.** Considered: the show-more controls of today, which grow the box in place
(the cause of the overflow); **one cap for every box, scrolling inside, with *Open* for the
whole (approved)**.

## Design

### 1. While a turn works

**The shimmer.** One builder draws a line of text in the middle ink and sweeps a narrow band
of the foreground ink across it, left to right, about two seconds a pass with a short rest
between passes. No accent. Mono text keeps its family; a truncated line sweeps what is drawn.

| Place | After |
|---|---|
| A running row in a cluster | the kind's icon stays; the present verb and its object shimmer; past ten seconds its own clock stands at the row's end |
| A step waiting its turn (`pending`) | *Waiting* and its object in the muted ink, no shimmer |
| An open cluster's line, a step running | the chevron stays; only the settled summary, in the line's own ink |
| A folded cluster's line, a step running | the chevron stays; *Running X* shimmers, then the settled counts |
| A live thought | the chevron stays; *Thinking…* shimmers |
| A plan's item in progress | its ring as a pending item draws it; its words shimmer |
| The running line | no mark; the clock, then *N running* (and *, M waiting*) in the middle ink; a running step's command is not repeated; *Responding…*, *Working…* and the waiting-for-you lines stay, and shimmer |

**As built, where it departs from the table above.** A step reported as `pending` reads as
running, not *Waiting*: the default agent holds every step at pending until it ends, so the
two cannot be told apart and *Waiting* named most running steps. The running line counts
*N running* only. The shimmer's base is the muted ink with the middle ink at the band's
edges, since middle to foreground is too small a step to see, and the running line stands in
the muted ink with it. The band sweeps a line's first 48 characters rather than all that is
drawn: the pass takes the same time on every line, so over a whole long command it was too fast
to follow and mostly out of sight. An opened cluster with nothing settled yet reads *Working*,
never an empty line. A working subagent's name shimmers too, since its line lost its spinner
with the rest. A plan's item in hand keeps a ring that does not turn, in a running turn and
after it. Nothing turns the motion off.

**Tense.** Core gives a step in flight its present verb (*Running*, *Reading*, *Editing*,
*Searching*, *Fetching*) wherever it is named, and the past verb once it settles. Today the
verb is chosen from the tool's kind alone.

**Small things.** A thought under a second reads *Thought*. One rule shortens a path
everywhere: relative to the project's root; outside it, from `~/`. A thought stays its own
line and does not join a cluster (the owner was shown both and did not ask for the change).

### 2. How a turn ends

- **Stopped.** When a turn ends on a cancel, whoever asked for it (the person here, the
  rail, the remote channel, a lead, a run), a step still running is marked stopped, not
  failed: *stopped* at its row's end in the muted ink, the cluster counts *N stopped* in its
  own ink, the notice reads *Stopped* (in place of *Interrupted*), and the turn's last line
  reads *Stopped after …*. This is a new outcome for a step in core, beside failed, and it is
  saved. A cancelled subagent's running steps are stopped too. Every other path that
  settles a running step today (a lost agent, a turn that ends with steps unsettled, a
  subagent that failed or was interrupted) still fails it. (Builder's call on the reach;
  the owner saw only the person's own Stop.)
- **Stopped at a limit** (output limit, requests per turn) or **refused.** The neutral card,
  flat, a neutral mark, no danger ink: *Stopped at the output limit* / *Stopped at the
  request limit* / *The agent declined to go on*, one sentence under it, and for the two
  limits a *Continue* button (secondary, short) that sends a prompt asking the agent to carry
  on. The turn's last line reads *Stopped after …*. *Turn failed* stays for failures.
  *Continue* shows only in a session a person attends, never in a run's; pressing it is a
  prompt like any other and moves the ending into the past.
  Such a turn is still a failed turn to the rest of the app; see *Decided* below.
- **A failed step the turn moved past.** Its *exit N* drops to the muted ink once the turn
  has settled, as the cluster's *N failed* already does.
- A failed command's row reads *exit N* where the code is known, *failed* otherwise.

**As built** (PR stacked on #357). *Continue* shows in any session whose last turn stopped at a
limit, a run's included: a session does not know a run drives it (recorded in
`docs/known-gaps.md`). A cut-short card's mark is the info mark in the muted ink. A subagent's
failed steps go quiet once its card has ended, as a turn's do once it is over. *Continue* sends
"Continue from where you stopped." as a person's prompt.

### 3. What a row opens into, and a turn's changes

**One box for every detail** (a command's output, a search's results, a file's content, a
diff):

- The command stands on top, wrapped so it reads whole, cut at four lines with *Show all N
  lines*; a hairline parts it from what follows.
- Under it the output, never taller than the cap of **sixteen lines**, scrolling inside in
  both directions. The vertical scrollbar runs down the box's right edge however far the
  lines are pushed sideways; the horizontal one along its foot.
- A search and a file read open on their first lines; a command, failed or not, on its
  last, where its result is.
- Under a box holding more than the cap, a diff's included: how many lines, and *Open*,
  which shows it in a dialog as wide as the window allows, as a table's does. The dialog
  draws up to a named cap and says so when it bites.
- A new file is its content, numbered, with no tint and no `+`. A changed file keeps the
  tints, which run the box's full width.
- Copy floats in the box's top corner, clear of the scrollbar, and takes no column.
- The show-earlier and show-more controls inside a box go: the box scrolls instead.
- A cluster's sentence says *created* where its row does.

**A turn's changes.**

- One file: one line, `› Created path +N`, whose chevron opens the diff. No head over it.
- Several: the head (*N files changed*, the counts), then a row a file: the path (folder in
  the meta ink, giving way at its start, the name in the foreground), *new* or *deleted* in
  the muted ink (a changed file says nothing; a deleted one stays struck), and the counts in
  one column at the right edge. The ratio bar stays on changed files only.
- *Show every diff* is removed, on the owner's word: it pours every diff into the
  conversation at once.
- An opened diff is the box above.

### 4. Subagents

- **In the conversation, one line a subagent**, where the work was handed off: a mark of its
  own (not a chevron, since nothing opens in place), its name, then what it is doing
  (shimmering) or how it ended (*done*, *failed* in the danger ink, *cancelled*,
  *interrupted*), and at the end its steps and its time. It never expands in place.
- **Pressed, its own transcript opens in a float** over the pane, drawn by the transcript's
  own renderer: clusters, rows that open, the boxes above. Its head: the name on a line of
  its own, under it the state, steps and time; at its end, *n of N* with a step to the
  previous and next subagent of the conversation, and Close. Under the head the task it was
  given, two lines, *Show all* for the rest. The subagent's words read in the middle ink at
  the small size; its last answer, which is what the parent receives, in the foreground.
- **The float reads its pane, never the window.** Wide: 440px at the pane's top right, at
  most about three fifths of its height, so the tail and the composer stay in sight. Under
  900px: it covers the pane above the composer. Escape or Close returns. It is not modal:
  the conversation behind scrolls and takes presses.
- **A subagent of a subagent** is a line inside the float; pressed, the float shows it, and
  its head gains a way back to the one it came from.
- **A strip above the composer** while any subagent of the turn works, gone when the last
  one ends: *N subagents*, then a count per state that has any. It counts the subagents
  handed work since the last prompt, which is also what the float's *n of N* steps through
  (for a line of an older turn, that turn's). The order of urgency: waiting on the person (a
  permission or a question of its own is pinned), failed, running, done. Pressed, the float
  opens on the most urgent. It stacks where the lead's strip does.
- **Its time and its whole task are new facts.** A card today keeps no clock and only the
  task's first line. Both are added to the model and to the card's saved line as fields that
  may be absent; a card saved before shows no time and its one line with no *Show all*.
  *Steps* counts tool steps only.
- A subagent's permission and question cards stay pinned over the composer as today.

## Decided: a turn cut short stays a failed turn

A turn that stopped at a limit, or that the agent refused, is a failed turn in the model
today: the rail marks the session failed, a run does not go on to judge work that was never
done, a lead is told it failed, and *Send the last prompt again* is offered. **Section 2
changes the transcript's words and ink only** (approved by the owner on 2026-11-18 with the
rest of this document, on the recommendation given). The model keeps it failed, and the
rail, runs and leads behave as today. A third ending in core, beside finished and failed,
was considered and left out: it is consistent everywhere but touches every reader of a
failed turn, and is a change of its own if it is ever wanted.

## The contract

Each PR carries its own lines of `DESIGN.md` (*Transcript*), `docs/screens.md` (*Subagent
card*, for the fourth), `catalog.md` for a new builder, and `docs/known-gaps.md` for the
gaps named under Non-goals. The sentences that change: "a spinner while it runs" and the
running line's contents; "One copy control … in a column of its own"; "Long output folds and
is capped"; the error card's "*Turn failed* … where a turn ended on it"; the subagent card.

## Order of work

Four PRs, each standing alone, in this order: (1) while a turn works; (2) how a turn ends;
(3) boxes and a turn's changes; (4) subagents, which draws on 1 and 3.

## Left to the builder

- The band's exact width, inks and timing, within "calm"; how the sweep is drawn in GPUI,
  so long as it does not lay the row out again each frame.
- The wording of the prompt *Continue* sends.
- How the float is mounted (an overlay of the pane, not a window-level dialog), and how a
  subagent's items are handed to the transcript renderer as a transcript of their own.
- Named caps for what the float lists, said on screen when they bite.
