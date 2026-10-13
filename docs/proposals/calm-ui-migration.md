# Calm UI migration

- Status: proposal, 2026-10-13, its decisions and readings settled with the user the same day.
  Brings the code in line with the contract `DESIGN.md`, `docs/ui-patterns.md` and the UI
  skills were refreshed to that day: inks by role, attention that is earned, separation by the
  lowest step, sizes by role, page heads and filters, immediate feedback and no motion. The
  rules live in their owners; this file holds the order of the work, the decisions it took, how
  unclear rules are read, and what "done" means. It leaves `docs/proposals/` with the last step.
- Source: a read-only survey of every screen against the refreshed contract, 2026-10-13. Its
  findings are folded into the steps by file; line numbers are left out, as they move.
- Delivery: one branch, one pull request carrying the documents, a commit per step. Each step
  ends on its checks and a short summary; the user walks its acceptance cases (`make showcase`,
  `make dev`) before the next one starts. Nobody but the user launches the window.
- Every step's checks:
  - the `ui-review` checklist answered for what the step touches, its *Calm* section included;
  - `.claude/skills/ui-review/scan.sh` prints nothing left unexplained;
  - `cargo test -p onehand --locked`, and `cargo test -p <crate> --locked` for every other crate
    touched;
  - `make clippy CLIPPY_EXTRA="-- -D warnings"` and `make fmt`;
  - a state transition the step changes gets a unit test; a colour that lands on a new surface
    gets its contrast asserted in `theme.rs`;
  - the documents owning what the step changes move in the same commit (each step's
    *Contract* line).
- Sizes: **S** under about 100 changed lines; **M** up to about 400; **L** more, or a change of
  structure or behaviour.

## Decisions

Answered by the user on 2026-10-13. Each changes what a step does.

### 1. The transcript keeps rows of its own

*Lists* and *Hierarchy* want a row to fill on hover, stand `ROW_H` tall, lead with a `.small()`
icon and take keyboard focus. The transcript's fold lines and activity rows are lines of text:
hover raises their ink, they take the line's height, lead with an `.xsmall()` mark, and nothing
in the transcript takes focus. *Choose the container* leaves the transcript "its specific
treatments in DESIGN.md", but *Transcript* names none of these. A thought's reasoning and a
command's output are content drawn muted, a role the inks do not give.

- **Decided**: DESIGN *Transcript* gets the treatment: its rows are lines, hover raises the
  ink, marks stay `.xsmall()` in one slot; what the agent did (a thought, a command's output, a
  read or search list) reads in the middle ink. Keyboard reach of the folds and Copy goes to
  `docs/known-gaps.md`.

### 2. One primary when layers stack

"At most one primary in a view" meets, on one screen: the Issues page head's *New issue* and
the floating issue's next action (or its review's *Continue*, or the form's *Save*); the
Workflows page's *New workflow* and the open form's *Save*; the composer's *Send*, a pinned
card's answer and the step strip's *Continue*.

- **Decided**: what waits on the person holds the view's one primary while it shows (the
  floating issue, the open form, the pinned card, the run's approval); what it covers steps
  down to `secondary`. Written into *Colour and state*.

### 3. Row actions at rest

*Lists* shows a row's actions on hover, on the selected row and on keyboard focus. Several
screens draw them always (Tasks rows; a project's past conversations, *Delete…*; Workflows
rows, *Run…*; the Plugins rows, *Install*, *Enable*, *Update*; the attachment manager's ×), and
the rail's quick actions over a session's meta line show for the pointer only.

- **Decided**: *Lists* everywhere, the actions in a trailing slot that keeps its width. A
  row under *Needs attention* keeps its one next action drawn: that attention is earned. A
  switch showing a state is content, always drawn. Actions laid over a row's meta line (the
  rail's *Stop*, *Send the last prompt again*, *Close*) show for the pointer and the keys, but
  not on the session shown, whose meta line they would hide for as long as it shows. Feature
  documents keep saying what a row offers, not when it shows.

### 4. What the library draws itself

The library draws some states with an alpha or a motion the contract rules out: a refusing
button (its ink at `opacity(0.5)` on a `0.15` fill; *Send* shows it whenever the field is
empty), a loading button (`0.8`), a picker's caret (`0.75`), a `Tag` under the pointer (`0.9`),
the `Switch` thumb (a 150ms slide) and the underline `TabBar` (a 200ms slide).

- **Decided**: the library's own control states (refusing, loading, a caret, a badge's
  hover, a switch's thumb) go under *Exceptions*, as its overlays already are. The issue's
  `TabBar` gives way to `tab_chip`s, as every other set of tabs in the docks already draws.

### 5. What immediate feedback covers

The rule wants a change shown in its frame, saved after it off the UI thread and in order, and
a failed save put back to the last value known saved, with one toast. Today the workspace file
and `onehand.toml` are written on the UI thread (twelve and four callers); a failed write keeps
the change on screen; renames of one conversation can land out of order; a model, effort, mode
or *Fast* pick the agent refuses stays on screen with nothing said; opening or closing a dock is
never saved; a shortcut shows only once written.

- **Decided**, five clauses written into *Immediate feedback*:
  1. A pick the agent applies shows at once; the agent refusing it is its failed save, put back
     to the value the agent last confirmed, with one toast.
  2. What is put back is the one change, not the file: the pin, the order, the theme, each to
     its own last saved value.
  3. A dragged size and a dock's open state are saved but never put back, since the layout
     would move under the pointer; the toast still says the save failed.
  4. A rename and a shortcut show at once; a form that writes a file (a workflow, an issue, an
     agent) waits on its *Save*.
  5. The rule lives in core: an ordered writer per file and a "shown, saved, newest change"
     state beside `Reading`, tested without a GUI; the app keeps the spawn and the toast.

### 6. The composer's chips

`chip()` mixes two sizes: 12px words at the 24px height, on a 6px inset. *Send*, *Stop* and
*Queue* beside it are `.small()`, with 14px words. *Components* makes a picker's trigger
`secondary`; only the model and effort chips carry a caret.

- **Decided**: `.small()` quiet chips, every picker with its caret: the composer's own
  treatment, listed under *Exceptions* beside its card, since a filled trigger on every chip
  would crowd the field.

### 7. Icons beside words

"Add an icon only when it helps recognition or names an action without visible text." Today
every menu row carries one (`Redo` stands for resending, restarting and refreshing), worded
buttons carry one (*New issue*, *New session*, *New workflow*, *New terminal*, *Continue*,
*Official*), and so do the Issues tab's status words.

- **Decided**: no icon beside a word on a button, in a menu or on a status line. Marks
  found before their word stay: the nav rows (the rail's, Settings'), folder and state marks,
  a file's kind in a tree, the transcript's kinds (decision 1). Where an icon stays, one shape
  per action.

### 8. The Tasks page

The project filter narrows all four cards but stands in *Needs attention*'s title. The cards
always stack, though *Boxes* sets boxes side by side while the column holds two. Each row is a
box inside its card. The task detail is a box with every action inline, in the page's wide
column, where *Details* wants a trail with one primary, one other and `⋯`.

- **Decided**: the filter in a filter bar above the cards, with no head (the cards hold the
  counts, and the page has no action of its own); the cards two by two while the column holds
  two; rows as list rows; the detail by *Details* at `PAGE_PROSE`, its parts plain sections.
  DESIGN *Layout* and `docs/tasks.md` follow.

### 9. The Issues page's scroll

*Pages* says "Its body scrolls as one column". The Issues page keeps its head, filter bar and
column names in place and scrolls the rows alone, so the filters stay in reach beside the
floating issue. The mock-up scrolls the whole column.

- **Decided**: keep it, written into *Pages*: a page with a filter bar keeps its head and
  filters in place while its content scrolls.

### 10. The terminal's dot

DESIGN puts an accent dot on the header's terminal button "while its dock is closed over a live
shell". An idle shell keeps it lit for good, which *Attention is earned* no longer allows.

- **Decided**: the dot shows while a hidden shell runs a command, by the busy test closing
  a tab already uses, in `link`, the ink of what runs.

### 11. The mock-up after the migration

`crates/app/examples/design_mockup.rs` draws the rail, the Issues page and a floating issue from
fixed data.

- **Decided**: delete it at close-out, with `shell::install_look`, which only it uses. Once
  those screens follow the contract, `make showcase` shows the real ones, and a static copy
  drifts into a second reference.

## Readings

How the steps read rules that are unclear or pull two ways, agreed with the user on
2026-10-13. Each step writes the readings it applies into the owning document as it lands.

- **Inks.** A quiet ghost's words take `secondary_foreground`; an icon-only ghost rests muted;
  any ghost on its hover or selected fill takes that fill's paired ink. The danger ink is
  semantic, so a danger word on a `secondary` fill (Settings) keeps it, its contrast asserted.
  The rail keeps its own fills (`sidebar_accent`, `rail_chosen`), as the token reference and
  *Hierarchy* say; DESIGN's `accent` and `list_hover` lines name the rows elsewhere.
- **What floats** keeps the floating treatment (an edge, the floating fill, `lift`); "Do not
  combine a border, contrasting fill and shadow" is about groups in the flow. A fill on a
  bordered box is for hover and selection only, so a project tile's hover stays.
- **Machine text** is `CODE_TEXT` in mono wherever it stands, a path or a reference inside a
  meta line included. A person's words are never mono: a question's wording, a project's name.
- **Sizes on a line.** An action on a line takes the line's size: `.xsmall()` on a meta line,
  `.small()` on a chrome line. A pinned card is a box, so `.small()`; its tab row is not a
  region's strip. The library's menus keep their icon size, as they keep `radius`; "a menu
  item" in *Control sizes* is a row the app draws. A filter bar is `.tall()` wherever it
  stands, the Issues tab's and the Plugins mode's included.
- **Strips.** The pane header, the step strip, the dock strips, the editor's file tabs, the
  Markdown reader's head, the issue's trail, the Issues form's bar and the tab's back row are
  strips. None draws a line under it, as the header does not. A control that shows a hidden
  region may lead (the show-rail button, the tree and list toggles).
- **A dock's body** keeps its strip's text axis (`px_3`); a list in it runs at `px_1`, so its
  rows' text meets that axis; a live grid (the terminal, Neovim) is inset `px_3 py_2`.
- **Tables** run their rows edge to edge inside their box, at the box's inset (`px_3`), each
  row `ROW_H`.
- **Page heads.** A head's count stands where a strip's name stands: chrome size, muted. A
  filter's trigger names what it is set to at rest too (*Any label*, *All projects*). The
  overview is named *Overview*, as the rail names it.
- **Failing.** A press that fails while its view keeps its content is an `error` toast; a start
  that leaves its panel without what it shows (a shell or Neovim that would not start) is a
  `status_line` in the danger ink. Where a feature document says a refusal is said beside its
  control, the pattern wins (the tooltip) and the document follows.
- **Focus.** A panel floating beside the list it steps through (the issue) leaves focus on the
  list, so ↑ and ↓ keep stepping. A pinned card keeps its own rule: it takes the caret only
  when nothing holds it, and Esc answers it. The task detail and the Workflows form are layers
  in their page, so Esc closes them.
- **Calm.** A kind's hue is identity and stays (a label's kind; a diff's added, removed and
  modified). A state's hue marks only what needs the person now: a past failure, a recovered
  run, a finished plan step, an open issue, a signed-in account, a found agent and a plugin's
  hooks draw without one.
- **Words.** Hints, empty lines and toasts lose their periods everywhere, the feature
  documents' quotes included. *Keep* answers every confirmation. *Task* is the glossary's, so
  the running line counts tools, not tasks.
- **File size.** A step that adds to a file past about 800 lines (`ask.rs`, `tool.rs`,
  `viewport.rs`) or at the line (`workspace_page.rs`, the Issues `view.rs`, `dialogs.rs`)
  splits it first, along a seam the survey found.
- **Motion.** A state that leaves on a timer (*Copied*) is not motion.

## Order

Shared defaults first, then what is always on screen, then the pages and the plugins, then the
rarer screens; saves last, as they move nothing on screen.

| # | Step | Size | Rests on |
|---|---|---|---|
| 0 | Foundation: inks, shared defaults, a guard | M | 4 |
| 1 | Rail | L | 3, 7 |
| 2 | Strips: the pane header, the step strip, the docks' strips | M | 2, 7, 10 |
| 3 | Composer | M | 6 |
| 4 | Transcript and pinned cards | L | 1, 2, 7 |
| 5 | Issues page: frame, head, filters, table | M | 2, 9 |
| 6 | The issue: detail, work, review, the Issues tab, the form | L | 4, 7 |
| 7 | The overview and a project's page | M | 3, 7 |
| 8 | Tasks and the task detail | L | 3, 8 |
| 9 | Workflows | M | 2, 3 |
| 10 | Settings | M | |
| 11 | Workbench modes: Files, Markdown, Editor, Neovim, Plugins | L | 3, 7 |
| 12 | Dialogs | M | |
| 13 | Saves and waits | L | 5 |
| 14 | Close-out | S | 11; every row of *Reconciliation* |

## Steps

### 0. Foundation (M)

- **Inks.** `meta_ink` and `middle_ink(full, meta)` move into the plugin host, the app
  re-exporting them, from the stashed Issues trial (*issues page trial on the calm contract*),
  whose two hunks apply to `main` as they are; the middle ink is asserted on the reading surface
  and, under its ink, on `accent`. Every other pairing is asserted by the step that draws it.
- **Shared defaults** every caller already goes through:
  - `.quiet_icon(on, cx)`: an icon with no words rests in the muted ink and takes its fill's ink
    when hovered or `on`, which the header's dock buttons pass; every icon-only ghost goes
    through it but the rail's filter, whose state step 1 settles;
  - `choice` and `switch` take a `ControlSize` (`XSmall`, `Small`, `Tall`) rather than the
    library's `Size`, which held its default; each caller keeps its size until its step;
  - `.refuses(why)` takes the reason it shows as its tooltip, so no refusal goes without one;
    its 25 callers say why (the Workflows *Save*, the dialogs' *Run* and *Create worktree*,
    `Confirm`'s go, the question card's *Next*, *Unbind* among them);
  - `status_line(message, ink)` takes the ink of the state it reports and stands at the strip's
    inset; `toast` moves into the plugin host, the app re-exporting it, so a plugin reports a
    failed press as the app does.
- `text_area` stays as it is: the library's `Textarea` takes no size, and its text inset (the
  default's 10px) is out of a wrapper's reach; the token reference says so.
- New builders land with their first caller, named in its step, and enter `catalog.md` then.
- Guard: nothing animates (`with_animation`, `Animation::new`), a plain rule, as nothing does.
- Contract: `reference.md` (the middle ink within a plugin's reach; a ghost's inks; the
  textarea's inset), `catalog.md`, DESIGN *Components* and the `.refuses` examples in
  `docs/ui-patterns.md` and the skills.
- Acceptance: icon-only ghosts muted at rest (the dialog ✕, the Markdown and file-tree toggles,
  the Plugins and issue `⋯`, the transcript's and the composer's Copy and ×, the Issues sync
  controls, every dock strip's) and inked when hovered; the header's Workbench and terminal
  buttons inked while their dock is open; each refusing control's tooltip says why; a status
  line's words on the strip's inset; nothing else moves; both palettes.

### 1. Rail (L)

- **Inks.** Project names and session titles muted at rest, and still muted under the pointer
  (hover changes the fill alone; in the dark palette that is 3.96:1 on the hovered row, accepted
  for a passing state); the chosen row in `sidebar_accent_foreground`, its meta and git line in
  the step mixed under it. A state's word that waits on the person in its state's ink (*Needs
  input*, *Failed*, *Disconnected*); running keeps the accent on its dot. The tile's letter in
  its fill's ink. Weight by role only: the shown and the unread session lose theirs, the
  *Sessions* label takes none. Asserted in `theme.rs`: the chosen meta step, the state words and
  the badges on the rail's three fills.
- **One axis.** `mark_slot` lands here: every mark down the rail in one slot on one centre line
  (the tile, the nav icons, the chevron's icon, the folder, the state marks, *Add project*'s
  `+`), its label on one edge; a session's title and meta line on its project name's axis, the
  indent a slot and its gap. Rows `px_2` in a `px_1p5` list, icon and label `gap_2`, meta `gap_1`,
  the search and *New* `gap_2`; one folder shape, as the chevron says open or folded. A state's
  mark is `.xsmall()`, a step under the title, the size *Control sizes* now names for it.
- **Controls.** The chevron, `+` and `⋯` at `.small()`, the chevron's hit area over its slot;
  *Stop*, *Send the last prompt again* and *Close* stay `.xsmall()` over the meta line. Hover
  actions take the trailing meta's place and never move the name, and show for the keyboard's
  row too (decision 3). A run's pill becomes a control, with its tooltip.
- **Keys.** The keyboard's row draws the ring, apart from the chosen fill, which marks the
  session on screen and a project whose page shows; `docs/known-gaps.md` loses *The rail's
  keyboard row shows after a click too*.
- The *Sessions* header is a label line over the list, its chip and filter `.small()`, the
  filter's icon through `quiet_icon`, on while a filter other than *By project* holds. The
  foot's *Settings* becomes a page row like *Overview*, filled while it shows, with no hairline.
  The Tasks count and the *auto* pills go through `badge` (lands here), on the `inset` step,
  which stands off the rail's three fills.
- Menus: no icons (decision 7); a workspace's folder in the middle ink and in mono. No rail menu
  row runs a keymap command (each acts on its own row's session or project, a command on the
  one on screen), so none shows a key; the header's menu does, in step 2. *Close session* is a
  plain word: the conversation is kept.
- Words: an open project with no session says *No open sessions*; the switcher's tooltip names
  what its menu lists; *Remove from workspace…* keeps its ellipsis only when it asks.
- Contract: DESIGN *Layout* (the rail's foot, inks, keys, the empty line) and *Colour and state*
  (the rail's own fills); *Control sizes* (a state's mark).
- Acceptance: the rail at rest in both palettes, only the current row in full ink; sessions
  needing input, failed, running and done unread; hover and the keys walking the list (the ring
  on the keyboard's row, the fill on the shown session, never two fills); a project folded and
  unfolded by its chevron; marks lined up from the tile to the state dots at 80, 100 and 150
  percent; a 232px rail with a long project name, its branch and pills; a drag reorder; the
  Settings foot shown and marked.

### 2. Strips (M)

- `strip` and `strip_control` land here (from the header's `header_control`): every strip at
  `BAR_H`, `px_3`, `gap_2`, what it names first in full ink at chrome size, its controls
  `.small()` quiet. They draw the pane header, the step strip, the Workbench and terminal
  strips, the editor's file tabs and the Markdown reader's head; steps 5 and 6 bring the issue's
  trail, the Issues form's bar and the tab's back row.
- The pane header: the name at `text_sm`; the overview named *Overview*; the dock buttons' icons
  in their fill's ink while on; the terminal's dot by decision 10; *Restart the agent* shows its
  key; icons by decision 7.
- The step strip: its labels at chrome size, the current one in full ink and no extra weight;
  `.small()` controls; *Continue* without its icon, primary by decision 2; its *Stop* a quiet
  word, as a run can be resumed.
- The docks: the Workbench's modes as `choice` buttons; `tab_select` `.small()`, its caret drawn
  in the builder; `tab_chip`'s chosen ink paired with its fill; the editor's and Markdown's
  controls `.small()`, *Edit source* in a reserved slot. A shell failing to start beside open
  tabs is an `error` toast; an empty dock that cannot start one, a `status_line`. *New terminal*
  without its icon.
- Contract: *Strips* (no line under one; a control showing a hidden region may lead), DESIGN
  *Docks* (the shell tabs capped in width, so none is left out).
- Acceptance: the header and both dock strips side by side, their names and controls on one
  line at 80, 100 and 150 percent; the Workbench narrow (its modes a select) and wide; crowded
  file tabs; a long conversation name giving way first; the dot lit only while a hidden shell
  runs `sleep 30`; a run waiting at an approval (one primary); both palettes.

### 3. Composer (M)

- The popup: `popup_row` on a solid `accent`, `list_hover` under the pointer, so a hovered row
  never out-reads the chosen one; the middle ink mixed on the fill for a detail; `SELECTED_ALPHA`
  and its compositing test go. Row marks `.small()` in a `slot`, in their text's ink; a
  description beside a name in the middle ink in every list; the surface's edge `border`; its
  waiting and empty lines name their work and follow the pattern (*Looking for files…*, *No
  commands match*).
- The attachment manager: its rows are `popup_row`s at `ROW_H`, no row at the library's default
  size, capped at six with *N more*; the unreadable mark in a `slot`.
- The chips by decision 6: one `chip` at one size, a caret on every picker, its glyph in its
  words' ink, the paired ink while open; the model and effort chips `gap_2` apart; *Queue* and
  *Stop* `gap_2`; the narrow stack `gap_2`; the `+` glyph at its button's size.
- The tray: each chip an edge with no fill, `gap_2` apart; *Show all N* at the chips' size.
- The lines above the card on the card's text axis (*N more requests wait*, the reconnect and
  queue lines); the reconnect hint muted; *Queued* without weight.
- *Stop* shows `.loading(true)` from the press until the turn ends.
- Keys: *Send*'s tooltip shows Enter as a `Kbd`; the mode chip's tooltip its cycling key.
- Words: the effort's qualifier in sentence case; the `/` list's title and the `+` menu's row
  say the same thing.
- Contract: DESIGN *Exceptions* (decision 6), *Components* (the popup's rows).
- Acceptance: the `@`, `/`, `+`, model, effort, mode and branch menus walked by the keys while
  the pointer rests on another row; a popup over a pinned card; the composer narrow; *Stop*
  mid-turn, loading until the turn ends; twelve staged files, one unreadable; both palettes;
  zoom.

### 4. Transcript and pinned cards (L)

- First, the three files past the line split along their seams: `tool.rs` (the command block;
  the diff renderer), `ask.rs` (the settled record; the card's keys), `viewport.rs` (run
  planning).
- Rows by decision 1: `fold_line`, `activity_row`, `RowMark` and `file_row` share one slot for
  the mark, the spinner included, so no label jumps as work settles; a folder in the middle ink;
  counts muted and in the UI family; an agent's wording in the UI family; an opened cluster
  capped, with *N more*; what the agent did in the middle ink.
- Calm: a state's wash mixed (`tint`, `tint_over` on the well), `STATE_TINT` gone; a failure a
  later turn has moved past in muted words; a recovered run's *N failed* muted; a plan's
  finished step a muted check, a running one a `Spinner` while the turn runs; code in a plan
  step `hue_ink`; a denied grant's badge neutral; the skipped-run line muted.
- Wells and code at `CODE_TEXT` through `well`, the command gutter recomputed; the permission
  well on a ramp step of its own for a well on a floating surface (the dark palette's floating
  step equals its well), mixed and asserted, with no edge. One copy control (`.xsmall()`,
  quiet, its tooltip, a column of its own) for code blocks, a tool's well, the turn's footer,
  the permission command and the prompt, whose Copy shows on hover and on keyboard focus and
  keeps its width as it says *Copied*. One show-more control. The diff's path header an
  `.xsmall()` quiet action.
- Pinned cards: one shell for both (header, `p_3`, footer), controls `.small()`, the dismissing
  answer a quiet ghost on both (*Deny*, *Skip*). Choices are `choice` rows, the taken one on
  `accent`, a destructive one marked by its danger word, with no edges; the library's `Radio`,
  `Checkbox` and `Kbd`; tabs keep their weight, and selection takes `accent`, not `primary`. The
  free-text field a plain `text_input`; key hints leave the labels for `Kbd` tooltips; *Next*
  carries an icon, not a glyph; one primary by decision 2; the permission card keeps its warning
  mark, the question card drops its icon.
- The prompt: its Copy through `action`; *+N more attachments* in the plural it needs; a
  non-image attachment shows its name on its plate, without an icon.
- The running line counts *N running*, its spinner at the transcript's mark size. The to-bottom
  button `.small()` and quiet on its floating surface. `waiting_hint` keeps the header and
  becomes one meta line with its spinner inside. The restart notice takes its key from the
  keymap.
- Guard: no alpha on a colour outside the palette (fades at `alpha(0.)` and `alpha(1.)` aside),
  a plain rule now that the composer and the transcript hold none.
- Contract: DESIGN *Transcript* (decision 1; the turn's changed files and the running line as
  blocks it holds); `docs/known-gaps.md` (keyboard reach).
- Acceptance: a whole Mock UI turn, fast and slow, no label jumping as work settles; every
  block kind; a failed turn, then a later one (the old failure calms); a permission and a
  question pinned, answered by key (keys shown in tooltips; one primary), then drawn in the
  transcript; a long diff; Copy from each place; zoom; a pane narrower than `NARROW_PANEL`; both
  palettes.

### 5. Issues page (M)

- Starts from the stashed trial, which applies to `main` without conflict, and goes on from
  where it stopped.
- Frame: `p_6`, `text_sm`; the scroll by decision 9; *Reading the issues…* until the first read
  lands, not *0 open* over *No issues yet*.
- Head: the count at chrome size, muted; *Refresh* `.loading(true)` while it reads; *New issue*
  without its icon, stepping down by decision 2.
- Filter bar: `picker` lands here (from the page's `trigger`): every trigger in its variant's
  ink, named for what it is set to at rest.
- Table: `row` lands here (at least `ROW_H`, `px_2`, `gap_2`, `sm`, hover and selection fills
  with their paired inks and the mixed step for meta, the ring for the keys, a trailing slot
  that keeps its width, actions on hover, selection and focus). A table's rows at its box's
  inset; the reference in a fixed slot at `CODE_TEXT` in the middle ink, mixed on the picked
  fill; the title `flex_1`; the priority, the work's words and the project in trailing slots,
  the project muted; labels through `badge`; on the picked row every quieter ink mixed and a
  neutral badge on its own pair; the column head unfilled at `ROW_H`, *Title* over the titles.
- The floating issue: its trail through `strip`; focus stays on the list; *Close*'s tooltip
  shows Esc as a `Kbd`.
- Contract: DESIGN *Layout* (the Issues page's pickers, which it calls chips), *Pages*
  (decision 9), *Keys and focus* (a panel floating beside its list), *Lists* (a table's inset).
- Acceptance: issues of several projects and of one; every filter set and cleared, each naming
  its setting; a list past its cap; an issue picked by pointer, then ↑ and ↓ with it open; the
  page left and shown again (filters, search, selection, scroll kept); a slow first read; one
  primary whatever is open; both palettes; zoom.

### 6. The issue (L)

- The detail: the facts line without the reference and project the trail names; the order
  facts, what it waits on, then tabs; `tab_chip`s for *Description* and *Activity*, the motion
  guard growing to forbid the library's `TabBar`; the title on `LEADING_TIGHT`; a linked
  reference in `link`; the priority a muted mark with its word; the state badge neutral, without
  an icon; the body read as a document (`text_base`, `LEADING`); *No description* a meta line;
  the conflict through `card_box` (into the plugin host, out of `workspace_page.rs`), its word in
  the warning ink, its choices `.small()`; *Files it names* a heading and space, its rows `row`s
  at `CODE_TEXT` without a file icon; activity times in a fixed slot; the project tag goes, as
  the trail names it; a local issue's trail says *Draft*.
- The work: lines become space; *Verify · step 3 of 6*; the actions `gap_2`; *Remove worktree…*
  into `⋯`, in the danger ink; the branch at `CODE_TEXT`. A fold control (the chevron first,
  focusable, refusing when empty) lands here for *N files changed* and *Acceptance*, and serves
  the task detail in step 8. Changed files as `row`s; the diff's well at `CODE_TEXT`.
- The review: through `card_box`; its controls `.small()`, `gap_2`; *Continue* without its icon;
  the check's output through `well`; the cut note in the warning ink, as it needs the person,
  without a period.
- The Issues tab: its search and filters a `.tall()` filter bar, the label trigger a `picker`;
  adjacent controls `gap_2`; the lines under its bars go; the sync footer a meta line whose *Sync
  now* stays in place, `.loading(true)`; status words without icons; the selected row's meta
  mixed on its fill; *working* without its icon; the back row through `strip`.
- The form: `field`s, `gap_4`; *Cancel* and *Save* `.tall()`; *Save* refuses with the problems
  above it; a failed save, publish, link open or copy is an `error` toast.
- Closing an issue goes ahead without asking, as a plain word, since it can be reopened, unless
  a run works on it, when it asks as it does now.
- The Issues `view.rs` splits before it grows.
- Contract: DESIGN *Docks* (the issue's tags), `docs/unattended.md` (the step's wording, *Remove
  worktree…* in `⋯`, *Run*'s refusal, closing).
- Acceptance: an issue on the page and in the tab, open, closed, in conflict, with work running,
  waiting at its review, done; the review opened, revised, continued; *Description* and
  *Activity* switched, nothing sliding; the form with a problem; a failed publish (one toast);
  the tab narrow (list and detail one at a time); both palettes; zoom.

### 7. The overview and a project's page (M)

- The overview: *Waiting on you* and *Working* stay boxes; *Recent conversations* and *Open
  issues* become plain sections; one scrolling column, the inner scroll of *Open issues* gone.
  Rows are `row`s, the issue's reference and the ages in fixed slots, the reference in the
  middle ink. A tile's folder `.small()`, its lines on its name's axis. Boxes and tiles side by
  side one gap apart, sections `gap_6`. The issue filter a `picker` at a box's size, kept while
  the page is left; labels tinted by kind, as on the Issues page.
- A project's page: one column from `p_6`, sections `gap_6`, `text_sm`. A head: its tasks line
  at the start, *Run check* then *New session* (primary) at the end, `.tall()`; *Run check*
  `.loading(true)` while its check runs. The check command a `field`, `.tall()`. Past
  conversations as `row`s, *Delete…* by decision 3; the confirmation's title names the
  conversation; the resume picker capped; the line without a project a `hint`; icons by
  decision 7.
- Contract: DESIGN *Layout* (the overview's sections; a project's page and its head).
- Acceptance: the overview with no project, one and many (tiles wrapping); its filter set, the
  page left and shown again; a project with sessions and without, its check run once and
  loading; a past conversation deleted (*Keep*, then *Delete*); a narrow pane; both palettes.

### 8. Tasks and the task detail (L)

- By decision 8: the project filter a `picker` in a filter bar above the cards, kept while the
  page is left and set by a project page's line; the cards two by two while the column holds
  two; rows as `row`s inside their card, actions by decision 3; empty lines without periods.
- The detail by *Details*, at `PAGE_PROSE`: a trail (a `back_link` to Tasks; Esc as before)
  with one primary, one other and `⋯`, *Remove worktree…* in the `⋯`, so its late arrival shifts
  nothing; the title `text_xl` semibold on `LEADING_TIGHT`; one line of facts; *Way out*,
  *Awaiting approval* and the runs as plain sections.
- Earlier runs, visits and files as `row`s, through the fold control of step 6; times in fixed
  slots; indents on the chevron's axis; paths and kept output at `CODE_TEXT`; a failed read says
  *Reading the diff failed: <why>* in the warning ink; the cut note in the warning ink, without
  a period; *Continue* without its icon.
- `workspace_page.rs`'s page builders (`card_title`, `page_card`) move to a module of their own
  once Tasks is their last other user.
- Contract: DESIGN *Layout* (Tasks), `docs/tasks.md` (the detail's head and parts; its quoted
  lines' periods).
- Acceptance: every card empty, then full past its cap; the filter set from a project's page
  and from the bar, kept after leaving; a finished task's detail: retried, dismissed; a review
  answered; a waiting task approved; a narrow column (the cards stacked); both palettes.

### 9. Workflows (M)

- A head: the count at the start, *Import…* then *New workflow* (primary, stepping down while
  the form is open) at the end. Rows as `row`s, with stable trailing slots and actions by
  decision 3; *Reading the workflows…* and the cap line as meta lines, the folder in mono.
- The form: every field `.tall()` (*Id*, *Label* and *Wait at most* too); pickers `picker`s;
  *Where it works* a `.tall()` `switch`; the *Add* buttons `.tall()`. A step's box is a
  `card_box`: its title semibold, with one `⋯` (*Move up* and *Move down*, refusing at the ends;
  *Remove*); the kind picker its first field; fields `gap_4`; *Gates* a `field`. Notes are hints
  (`text_xs`, no period), prompt variables in mono. *Save* refuses with why, shows a name clash
  above it before the press, and shows `.loading(true)` while it writes, one write at a time.
  *Cancel* asks when the form holds changes (*Keep*, *Discard*); so does closing the window,
  whose question says *Discard*.
- Contract: DESIGN *Layout* (Workflows' head), `docs/workflows.md` (the step's box, *Keep*, the
  label refusal, its quoted lines' periods).
- Acceptance: built-in and own workflows listed; a new workflow with a clashing name, fixed,
  saved by one press; a step moved and removed; the form left by Esc and by *Cancel* while it
  holds changes; both palettes.

### 10. Settings (M)

- A sub-page's title goes, as its nav row names it; its description stays, as a hint. Nav rows
  as the rail's page rows (`ROW_H`, a `slot`, the selected fill and its ink); section headings
  semibold; `list_row` `gap_2`, its second line meta; switches `.small()` at their own geometry;
  the label field and *Add* `.tall()`; workflow pickers `picker`s; *Edit* `.small()`; adjacent
  controls `gap_2`.
- *Check again* and *Test* `.loading(true)`, their labels kept; a signed-in account and a found
  agent muted at rest; *Default* a `badge`, as *Built in* is; the config path and keys in mono;
  every list capped; hints without periods; the scope line no longer repeating the name field.
- A danger word on a `secondary` button keeps the danger ink, asserted on `button_secondary`.
- Shortcuts: *Save*, *Cancel*, *Reset* and *Edit* `secondary` (Settings has no primary); editing
  a shortcut keeps the page and its nav, with no page-sized title; meta lines `text_xs`.
- Contract: DESIGN *Layout* (Settings' sub-pages).
- Acceptance: every sub-page in both palettes and at zoom; an agent tested, made the default,
  edited and removed; a shortcut edited and reset; a connection checked; a long list's cap line.

### 11. Workbench modes (L)

- Files and Markdown share one tree row: a `row` at the dock's axis, icons `.small()` in a
  `slot`, `ROW_INSET` and `TREE_INDENT` into `tokens`. The open file is the tree's selected row,
  as the open document is the list's. A changed file's name in full ink, its letter in its
  state's ink, the marks in one trailing slot. *N more* with its count, core's walk saying how
  many it left out.
- Editor and Markdown: a failed open or save is an `error` toast (*Opening X failed: …*,
  *Saving X failed: …*); a file changed on disk stays a `status_line`; a confirmation's title
  names the file, its danger word the verb of the control that asked.
- Neovim: a start that fails is a `status_line` in the danger ink; its grid inset as the
  terminal's.
- Plugins: row actions `.small()` quiet by decision 3; the switch always drawn, `.small()`,
  refusing with why; the search and filters a `.tall()` filter bar, the installed filters
  through `choice`; the version in the middle ink at `CODE_TEXT`; a plugin turned off keeps its
  name in full ink; *hooks* a neutral `badge`, *set elsewhere* muted; weight by role; meta
  `gap_1`; rows and notes on one axis; the busy and pending lines no longer move the list; a
  hook's command a `well`; empty lines in the pattern's words; *Official* without its icon.
- Contract: *Spacing and density* (a dock's body; a live grid), DESIGN *Docks*.
- Acceptance: a large project's tree (its cap line counting), a changed file; a file that fails
  to open (a toast) and one changed on disk (a line); the Markdown list and reader narrow and
  wide; Neovim missing from `PATH`; Plugins installed, turned on, updated and uninstalled by
  pointer and by keys; both palettes; zoom.

### 12. Dialogs (M)

- `title_row`'s ✕ muted, with its tooltip. A footer's third action stands at its start, apart
  from the dismissing button and the primary at its end: rename's *Use the automatic title*,
  the worktree's *Put it somewhere else…*, retry's *Retry with current settings*.
- The launcher's and the issue picker's inputs are `field`s, `gap_4` apart; pickers `picker`s at
  `.tall()`; problems above the primary in the danger ink; the first prompt's preview a `well`;
  the worktree's path in mono. The issue picker's rows are `row`s (selected on `accent`, the
  reference in the middle ink, *N more*), its waiting and empty lines meta. *Rename* refuses an
  empty name; *Check the agent* keeps its label while loading.
- The retry questions and *Answer the review* stay alert dialogs, as `Confirm` is: a question
  without fields. A failed start says *<What> failed: <why>* in an `error` toast (*Starting on
  issue N failed*, *Making the worktree failed*).
- Contract: *Dialogs* (a third action; a question without fields).
- Acceptance: each dialog opened by pointer and by key, closed by Esc, by its dismissing button
  and by its primary, focus back where it was; a refusing primary's tooltip; a worktree that
  fails; both palettes.

### 13. Saves and waits (L)

- Core, by decision 5: an ordered writer per file (from `task::files::Writer`, handing its
  result back) and a "shown, saved, newest change" state beside `Reading`. Tests: two changes in
  a row land in order; the newest change failing puts back the last saved value; a superseded
  change failing does nothing.
- The workspace file and `onehand.toml` are written through it, off the UI thread: the pin, the
  projects' order, the workspace's name, check commands, the unattended switch, the default
  workflow and its labels, Appearance (put back on failure, its deliberate keep gone), agents,
  shortcuts (shown at once). Dragged sizes and open docks are saved and never put back; opening
  or closing a dock schedules its save; a window closing flushes the pending save.
- A conversation's renames land in order; one that fails puts back the last saved title and
  says so once (*Renaming the session failed: <why>*).
- Picks: core tracks `set_mode` and `set_config_option`; a refusal puts back the agent's last
  confirmed value, with one toast; the remote bridge's picks go the same way (`Chat::choose`).
  The Mock UI agent refuses one pick, to show it.
- The Issues plugin's writes go through the same state (shown at once, in order, put back with
  one toast); a workflow's *Save* writes one at a time.
- Facts for buttons: a task's *Stop* waits on its button until the run gives its place up; the
  shell tells the Issues plugin and the task detail while *Answer the pull request review…* and
  *Remove worktree…* run; *Look for an issue now* waits on its button instead of a warning
  toast on the second press.
- A failed write of a dismissed task, and of the recent workspaces on *Unbind*, says so once.
- Contract: *Immediate feedback* (decision 5's clauses); the `ux-patterns` example, pointing at
  core's state.
- Acceptance: a session renamed twice fast with its file made read-only once (the last saved
  title, one toast); a pin and a reorder with the workspace file read-only (each put back, one
  toast each); a dock dragged wider with the file read-only (it stays, one toast); a pick the
  agent refuses (put back, one toast); a task stopped (waiting until it gives its place up);
  each button above pressed twice.

### 14. Close-out (S)

- Every row of *Reconciliation* done, or moved to `docs/known-gaps.md` with its reason.
- The contract's notes about lagging go: DESIGN's "The contract refresh changes documentation
  only…" and the token reference's "does not change the values below or certify that existing
  call sites conform". `CLAUDE.md`'s "implementation may lag the contract" stays, as a rule.
- The mock-up by decision 11.
- `scan.sh` run against `main` over the whole branch; this file is deleted.

## Reconciliation

Every departure the survey found, by kind, the step that ends it, and how its end shows.

| Departure | Step | Done when |
|---|---|---|
| The middle ink out of a plugin's reach; a detail beside a name in muted (Issues rows, the issue picker, the Plugins version, Markdown's folder, the transcript's folders, the workspace menu) | 0, 1, 4–7, 11, 12 | every such detail in the middle ink |
| Navigation rows in full ink at rest; text on a fill not in its paired ink (rail rows, the tile, the open chips, the dock buttons, `tab_chip`, Markdown's chosen document) | 0–3, 11 | only the current row in full ink |
| Alpha on inks and fills (the popup's chosen row; the transcript's washes) | 3, 4 | the alpha guard is a plain rule |
| Icon-only ghosts inked by hand, or in full ink (dialog ✕, Markdown toggle, Plugins ⋯) | 0 | no call site sets a ghost's ink |
| Attention at rest (the terminal dot; weight on the shown, unread and current rows; recovered runs; past failures; green plan checks and open badges; priority hues; hooks and *set elsewhere* in warning; account and agent in success) | 1, 2, 4, 6, 10, 11 | each as *Readings* (*Calm*) says |
| Boundaries framed or drawn twice (dock strips' lines; the Tasks rows' nested boxes; the task detail's boxes; review and conflict boxes at `radius`; the Issues column head's fill; the work's lines; tray chips' edge and fill; the permission well's edge; card footers' line) | 2–9 | the scan and the checklist's *Calm* find none |
| Spacing off its relationship (adjacent controls at `gap_1`/`gap_1p5`; rows at `px_1`/`px_1p5`/`px_3`; cards at `px_4`; pages at `px_4`; the project page's `gap_3`) | every screen's step | each relationship at its step |
| Marks off one slot and axis (the rail; the transcript's file letters; the overview's leads; Settings' nav; the task detail's indents; the Issues references) | 1, 4, 5, 7, 8, 10 | every column of marks in `slot`s |
| Controls off their role's size (the `.xsmall()` row actions, step strip, editor and Markdown strips, Plugins rows, Issues review and tab; `.small()` in forms, dialog pickers and footers; the library's default on switches, manager rows, the to-bottom button, Copy, the diff path, the free-text field; heights and insets set at call sites) | 0–12 | every control at its role's size, no call-site height or inset on one |
| Strips off `BAR_H` and `px_3` (the step strip, file tabs, Markdown's head, the issue's trail, the form bar, the back row) | 2, 5, 6 | every strip through `strip` |
| Page heads and filters missing or out of order (Tasks, Workflows, a project's page, the overview's filter); filters not kept (Tasks, the overview) | 5, 7–9 | the shared order wherever a head stands |
| A page's name repeated in its body (Settings' sub-pages, *Edit shortcut*) | 10 | none left |
| Pages off one scrolling column (a project's page centred; *Open issues* scrolling inside) | 7 | one column each |
| Lists not drawn as rows (Tasks rows and past conversations as boxes; the detail's rows as bare divs; the issue picker's rows; Workflows rows through Settings' `list_row`) | 5–10, 12 | every list through `row` |
| Row actions always drawn, or not for the keys | 1, 3, 7–9, 11 | decision 3 everywhere |
| The rail's keyboard row sharing the chosen fill | 1 | the ring for the keys; the known gap removed |
| Picker triggers drawn five ways | 5, then 6–12 | every trigger is a `picker` |
| Machine text off `CODE_TEXT` (wells, paths, references, branches, diffs) | 4–12 | the reading in *Readings* |
| Icons beside words | 1–12 | decision 7 everywhere |
| Motion: the issue's `TabBar` | 6 | the guard forbids it |
| Waiting drawn wrong (`waiting_hint`; the Issues page's first read; labels swapped for *Checking…*; actions without `.loading`: *Run check*, both *Stop*s, *Refresh*, *Read them*, *Sync now*, *Check again*, *Test*, *Look for an issue now*, *Remove worktree…*, *Answer the pull request review…*) | 3–7, 10, 13 | every action in flight on its button, its label kept |
| Saves on the UI thread, out of order, neither put back nor said once; picks refused unseen; open docks never saved; a pending save lost on close | 13 | decision 5 |
| Failures said as lines where a press failed (editor, Markdown, Issues writes, a second shell), or only logged (recents, a dismissed task) | 2, 6, 11, 13 | each a toast once |
| Refusals without their reason | 0 | `.refuses(why)` everywhere |
| Keys spelled in labels (card footers, *Close (Esc)*, *Send*'s tooltip, the restart notice) | 3–5 | a `Kbd` from the keymap |
| Words (periods on hints, empty lines and toasts; *Drop and close*; *running tasks*; *Close this issue?*; *Delete this conversation?*; *Workspace* for the overview; lower-case lines; cap lines that count what is shown) | every step | every string the step draws reviewed |
| Files past or at the line when touched (`ask.rs`, `tool.rs`, `viewport.rs`, `workspace_page.rs`, the Issues `view.rs`) | 4, 6, 8 | split along a seam |
| Contract text lagging the code or itself (the Issues page's "chips"; the rail's *Empty*; its fills; "capped" shell tabs; the transcript's unlisted blocks; the skills' *Run check* examples disagreeing) | the step touching each | the documents describe what is drawn |

## Not in this plan

- Keyboard reach of the transcript's folds and Copy (decision 1), recorded at step 4.
- The composer's *Queue* and *Queued* name prompts with the glossary's word for a task waiting
  for its place: a vocabulary question for `CONTEXT.md`, raised on its own.
- A refused menu row cannot say why, as the library's menu row takes no tooltip: recorded in
  `docs/known-gaps.md` at step 1 unless the row can carry one.
- The header's icon buttons without accessible names stay the known gap they are.
