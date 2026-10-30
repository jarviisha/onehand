# The workers' list: one place to read a lead's workers

## Why

The owner ran a lead with four workers (2026-10-30) and found the UI confusing to the point of
not being usable. What went wrong, as seen in that run:

- Every worker was named after its prompt's first words, so three read *Turn 2* and one *Turn 1*;
  the lead's own report spoke of jobs a, b, c, d and nothing on screen matched them. The only clue
  was the folder inside a permission card.
- The same list was drawn twice at once: the deck slid up over the lead's transcript, and the
  workers' region beside it held a block per worker.
- The region opened the first worker (an idle one) and stacked the two waiting on the person and
  the failed one under it, the failed one pushed off the bottom.
- The lead's rail row read *Failed* in the danger ink because one worker had failed, while the
  lead itself was fine.
- A folded block's permission card cut its command to one line with a scrollbar of its own, so the
  person approved a command they could not read; both cards said *Alt+Y*.

A throwaway prototype of three layouts was shown (`.superpowers/prototypes/lead-workers/`, not in
git); the owner picked the list-plus-detail one (variant A) and settled the rest in conversation.

## What the person does with a worker

The owner opens a worker mainly to **read what it is doing**, and sometimes to **write to it**.
They want to see every worker at a glance while the picked one's transcript runs. Answering a
permission is done on the worker's own detail, not the reason the region exists.

## Goals

- Each worker is told apart by a name that says its job.
- One list of every worker stays in view, the picked worker's transcript running under it.
- What needs the person stands out without anything moving or opening on its own.
- A permission card shows the command it asks to run.

## Non-goals

- No change to what a worker is, how it opens, or the tools a lead has, beyond a name.
- No new place for workers in the rail (the prototype's variant C was not picked).
- Workers' answers do not come to the lead's composer (variant B was not picked).
- No saving the region's layout or picked worker across a restart (still a known gap).

## Anti-goals

- The screen changes by itself when a worker starts waiting: no row jumps, no worker is picked,
  nothing opens. The owner said so in those words: *nothing happens*.
- A command approved without being readable.
- The same thing drawn in two places at once **as a list**. (The strip above the lead's composer
  is a one-line summary, and stays; see below.)

## Decided by the owner

### The workers' region: a list over a detail

The region keeps its place (the Workbench's, never beside it), its strip (*Workers of* the
lead's name became *Workers · N*, see *Builder's calls*), maximize and hide, and the rules for when
it opens, follows the lead and closes. What is under the strip changes:

- **The list**, pinned at the top, never scrolled away by the transcript under it. A row per
  worker, two lines: its state mark, its name, the state's word in its ink, its age; under them,
  what it waits on in the warning ink (*asks: * and the command's first line), else what it is
  doing, else its state's detail (a failure's reason), muted. The picked row is in the selected
  step. Pressing a row picks it.
- **The order** is by urgency: failed, needs input, running, done, idle. The list is re-sorted
  only when the person picks another worker or the region opens; while it shows, a row that
  changes state changes its mark, words and ink in place and does not move.
- **The detail** of the picked worker under the list: a head (mark, name, the folder's last
  component, *Write to <name>…*, *Stop* while a turn runs, `⋯` with *Give back to the lead* while
  offered, *Send the last prompt again* after a failure, *Close*), then the oldest card it waits on,
  then its whole transcript, following the newest output while the person has not scrolled up.
- **No composer by default.** *Write to <name>…* opens one at the foot of the detail, under the
  transcript, with *Send* and *Queue*; sending or `Esc` closes it and gives the transcript its
  room back.
- **Picked by default**: the most urgent worker when the region opens. A worker asked for anywhere
  else (the keys, a toast, the remote bridge, the strip) is picked instead.

### When a worker starts waiting

Nothing moves. Its row turns to *Needs input* in the warning ink with the command's first line
under it; the strip above the lead's composer counts it; the rail's badge counts it. The card is in
that worker's detail, seen when the person picks the row.

### The strip above the lead's composer

The tucked deck goes: no sliding list over the transcript. In its place, one line above the
lead's composer: *N workers*, a count per state that has any in its mark and ink, most urgent
first, and at the end the most urgent waiting worker, *<name> needs you*, when one waits.
**It stays while the region shows.** Pressing it opens the region (or, while it shows, picks the
most urgent worker). `Ctrl+Shift+Y` does the same; the deck's `↑`/`↓`/`Enter`/`Esc` go with the
deck.

### The rail

The lead's row keeps its own state, mark, words and age; it no longer takes its workers' most
urgent state. Beside its second line, a badge in the warning ink, *N workers*, counts its workers
waiting on the person (needs input or failed); none, no badge. *N need attention* above the
sessions keeps counting each such worker. Workers stay unlisted under the lead, as now.

### Permission cards

- A card shows its command whole up to a bound of lines (about ten); a longer one ends on
  *Show all N lines*, which shows the rest in place. No scrollbar inside the card.
- `Alt+Y`/`Alt+N` answer only the card in the picked worker's detail (or, in the lead's own
  conversation, the lead's pinned card as now). No other card shows the keys.

### Worker names

A worker is named for its job. The lead names it when it opens the worker; a worker opened
without a name is named for its folder's last component (`onehand-sim-b`), never for its prompt.

## Constraints

- Every rule in `AGENTS.md`: tokens from `cx.theme()` and the shared sizes, controls through
  `crate::controls`, events matched exhaustively, a named cap per list, files under ~800 lines.
- The region's cap stays four, which is also how many sessions a lead may have open.
- `docs/screens.md` (the rail paragraph on workers, the composer's deck paragraph, the workers'
  region bullet) and `docs/control.md` (`open_session`, *A worker*) change in the same PRs as the
  code that builds them. `docs/known-gaps.md` loses nothing.
- Focus tests stay deferred to issue #267.

## Builder's calls (the agent's, not the owner's)

- **The `name` argument.** `open_session` takes an optional `name` from a session's agent only
  (an outside program naming it is refused like `dirs`), trimmed, at most 40 characters, not
  empty; it is what the rail, the region and the toasts show, and `name:` in the answer echoes it.
  The fallback is the folder's last component. The `leading-worker-agents` skill passing a name
  (`a · review #265`) is the skills repository's work, not this one's.
- **The strip's title** becomes *Workers · N*: the lead's name is already in the header beside it,
  and it was the longest, most cut text on screen.
- **The folder in the detail head** is its last component, the whole path as its tooltip.
- **The list's second line** truncates with an ellipsis at the row's width, as rail rows do.
- **Freezing the order** is held by the region's own state (the order taken when it opened or a
  row was last picked), not by core: the urgency rule itself lives in core beside the rail's.
- **Where the name lives.** A name the lead gives is the conversation's custom title
  (`Chat::custom_title`): saved like a rename, shown in history and the resume picker, and a
  person's *Rename…* wins over it as over any title. *Reset title* on a worker goes back to the
  folder's name, not the prompt's. The folder fallback is computed where a worker is shown and
  never saved.
- **The rail's other signals.** Only the lead's row itself stops borrowing its workers' state. The
  *Needs attention* filter, *N need attention* and a folded project's badge keep reading the
  folded state, so a lead whose worker waits still passes the filter and still marks its folded
  project.
- **Disconnected and connecting.** A disconnected worker ranks with failed (they share a mark and
  an answer) and counts toward the badge; connecting ranks with running and does not.
- **Keys in the region.** Opening the region by pointer still takes no caret. `Ctrl+Shift+Y`
  (and the strip pressed by key) puts the caret on the list: `↑`/`↓` move the pick, `Enter` opens
  *Write to…*, `Esc` gives the caret back to the lead's composer. While the caret is anywhere in
  the region, `Alt+Y`/`Alt+N` answer the picked worker's card; elsewhere in the lead's
  conversation, the lead's. This amends the region's *takes none* line in `docs/screens.md` only
  for the key path.
- **Which cards show the whole command.** Every permission card (the transcript's, the lead's
  pinned one, a worker's), since the shared card draws them all; the pinned card keeps a bound of
  its own above the composer and ends on *Show all N lines* past it.
- **Steps**, one PR each: (1) the name argument and its fallback, core and control docs; (2) the
  rail row's own state and badge; (3) the region as list over detail, the on-demand composer, the
  card's whole command and the one card holding the keys; (4) the strip in place of the deck.
  The plan may merge (3) and (4) if splitting them leaves a build with both deck and list.
