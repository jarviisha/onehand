# DESIGN.md: onehand UI contract

The whole-app UI contract for `onehand`: the principles every view follows, the window's frame,
the transcript, and the visual rules every screen is drawn with. `docs/screens.md` says what
each region holds and in what order, a feature's own document (`docs/tasks.md`,
`docs/workflows.md`) its screens' detail, and `docs/ui-patterns.md` the patterns every screen
repeats, how they hold under strain and how they behave. Code never cites this file.

This contract defines onehand's own visual language: quiet navigation, clear content hierarchy,
compact controls and predictable alignment. Its values (each size, each colour) are the
code's, written once and listed by the size census, which fails when one is added or changed
(T-10, T-18); they change only when the owner asks. A departure from a rule is a bug, put right
or recorded in `docs/known-gaps.md` with its reason within the change that makes it.

## How a rule reads

The visual system and the patterns are written as rules, each with an id that is never reused:
`T` tokens and type, `C` controls and icons, `K` containers and composition, `L` layout under
strain, `S` states and feedback, `W` keys and words. `T` and `C` are here; the rest are in
`docs/ui-patterns.md`, and `C-16` to `C-21`, on control sizes, stand there beside the layouts
they size.

- **When** names the situation, never a screen. What only one screen can meet is that
  screen's fact, and is written in `docs/screens.md`.
- **Then** is what to do. A table under a rule's title is part of its *Then*.
- **Unless** is the rule's own exception. There are no others.
- **Checked by** names what holds the rule, the strongest that can:

| Checker | What it is |
|---|---|
| lint | a call banned outside a named place, in `clippy.toml` |
| guard | a scan of the sources, in `crates/app/src/guards.rs` and `guards/visual.rs` |
| test | a test named here: a view drawn under strain, a contrast, a value |
| picture | the changed view driven in the headless session and read by a reviewer that did not write the change (the `ui` skill, *The reviewer*) |
| owner | taste: a value, a place for a thing; settled in the running app before the documents |
| review | a judgment on the whole view, by that same reviewer from the diff and the pictures |

A rule's id stays out of the code: no comment, test name, lint reason or drawn word names one.
The rules point at the tests, never the reverse. Where no rule decides, do as VS Code does: the
palette and the selection grey were chosen that way.


## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Content leads; navigation recedes.** Neutral surfaces carry the layout. Hue identifies a
   state, a link, a label's kind or code in prose, never a decorative section background.
   Focus and selection are greys; the palette's one blue is the primary action. Attention is earned: the signals that call for the person
   (a state's hue, a count or badge of what waits, a dot, weight past what the role takes) mark
   only what needs them or has just changed, so a view at rest is calm. Hierarchy and identity are not attention: a heading keeps
   its weight, a list its muted count, a label its kind's hue. Nor is a mode switched on: the
   control that switches it may carry a state's hue on its own mark while the mode is on, and
   nowhere else. All colours are theme tokens with
   legible contrast on their actual surfaces in both modes.
3. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
4. **Same role, same treatment.** Across app and feature crates, equivalent rows, headers and controls
   share size, spacing, corners and states. A valid token alone does not make a consistent view.
5. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a bar or a title.
6. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.
7. **Calm and immediate.** What a person changes shows at once, and the work behind it follows.
   Nothing moves for show: the motions are a `Spinner` on work still running and, in the
   transcript, the band crossing the words of what runs, beside what the
   library's own overlays and controls do (*Components*). `docs/ui-patterns.md` (*Immediate feedback*, *Motion*) holds how.

## Layout

One window hosts one workspace. A title bar holding the rail's toggle, the workspace's name, the
palette, the docks' toggles and Settings' runs along the top; nothing runs along the foot. The title bar and
the rail stand on the window's fill, unboxed. Beside the rail, every
other region is a box of its own on that fill: the agent pane and the Workbench side by side, the
Workbench the right half of a split in the `DockArea`'s centre, and under both, across their whole
width, the terminal, the bottom dock. The boxes are parted from each other and from the window's
edge by the region gap, which shows the window's fill (*Type, space and shape*). The diagram is
plain ASCII and box-drawing, so it stays aligned in any mono font.

```
┌────────────────────────────────────────────────────────────────────────────────────┐
│ rail W Workspace v            [ Search sessions ^K ]           term wb set  _ [] x │
│ + New session ^N ┌────────────────────────────┐ ┌────────────────────────────────┐ │
│ Overview         │ Title (...)     past close │ │ Editor Markdown ...   max hide │ │
│ Tasks         2  │ workflow > step >     Stop │ │ open file v               tree │ │
│ Issues           │                            │ │                                │ │
│ Workflows        │                            │ │                                │ │
│ Schedules        │        agent pane          │ │           Workbench            │ │
│ Sessions  Q 2 v  │      (centre, flexes)      │ │     (beside the agent pane)    │ │
│ ──────────────── │                            │ │                                │ │
│ v project  main  │ ┌──────── composer ──────┐ │ │                                │ │
│   o session      │ │ +  Fast  model    Send │ │ │  open file              tree   │ │
│     Idle . agent │ └────────────────────────┘ │ │                                │ │
│ + Add project... │   branch             mode  │ │                                │ │
│                  └────────────────────────────┘ └────────────────────────────────┘ │
│                  ┌───────────────────────────────────────────────────────────────┐ │
│                  │ shell 1                                          +  max hide  │ │
│                  │ terminal (bottom dock, under both boxes)       shell 1        │ │
│                  │                                                 shell 2       │ │
│                  └───────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────────┘
```

What each region holds, and in what order, is `docs/screens.md`'s, under the region's name:

- **Title bar**: the rail's toggle, the workspace's name and its menu, the
  palette over sessions, projects and commands, the terminal's, the Workbench's and Settings' toggles, and the window's own
  controls where the app decorates it.
- **Rail**: app chrome outside the dock, on the window's fill, from *New session* down to the sessions.
- **Agent pane header**: the conversation's name, its menu and the session's controls, a worker's
  standing among them; chrome alone over a page that names itself in its head.
- **Step strip**: a run's steps under the header, while it drives the session on screen.
- **Empty session**: a heading over the composer, both centred, while a session holds nothing.
- **Composer**: the input surface under the transcript, and what waits pinned above it.
- **Docks**: the Workbench beside the agent pane and the terminal under both, each a box opening on
  its bar.
- **Pages without a session**: a project's page, the overview, Tasks, Issues and Workflows.
- **Settings**: a page of its own, a nav column beside the sub-page it shows.
- **Persistence**: what the layout saves.
- **Nothing else in the title bar, nothing along the window's foot, and no right toolbar.**
  Transient status goes in a toast; modals are `Dialog`s.

## Transcript

- **Two sides.** The user's prompt is the library's `Muted` bubble (a grey fill, the `2xl`
  corner), against the right edge, cut at ten lines under a fade with *Show all N lines* beneath it. Everything the agent produces starts on a shared left axis
  across the column, as the library's ghost bubble draws it. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read at the reading size; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, a subagent's line (where the agent handed work off; it never opens in place), answered
  permissions and questions (a line each, those in a row sharing one hairline edge with no fill),
  notices (a remark is a divider: the words centred between two hairlines across the column, in
  the muted ink) and errors (the neutral card, flat: the alert mark in the danger ink beside the
  words, *Turn failed* over them when a turn ended on it, the restart key for a lost agent), the files the turn changed,
  and, while it runs, the running line (a mark for what the turn is doing, which never turns: a pulse while the agent
  works, a bell in the warning ink while it waits on the person, a pen while it writes; its clock,
  *N running*, what the agent says it is doing, still and in the warning ink while it waits on the
  person; a step that runs is named by its own row and not again here). A workflow's steps and a stopped turn are notices, never prompts.
- **Folding blocks open from one line**, the chevron first, with no box: no edge, no fill and no
  rule beside what they hold, which sits indented under the line's words (a cluster, a thought, a plan, a turn's changes). What runs is open until done, the
  plan stays open, and a reader's fold wins. An opened cluster is capped, saying what it holds
  back. Wells are filled with no edge, their text at `CODE_TEXT`.
- **What a row opens into is one box, never taller than sixteen lines** (`DETAIL_OPEN_H`): a
  command's output, a read's or a search's lines, a file's content, a diff. A command stands on
  top, wrapped so it reads as typed and cut at four lines with *Show all N lines*, over a
  hairline. Under it the lines scroll inside the box in both directions, the vertical bar on the
  box's right edge however far they are pushed sideways and the horizontal one along its foot; a
  command's opens at its end, where its result is, and a read's or a search's at its start. Under a
  box holding more than it shows: how many lines (and how many it draws, where it caps them), and
  *Open*, which shows them whole in a dialog as a table's does. A new file is its content,
  numbered, with no fill and no sign; a changed file's fills run the box's width. Copy floats in
  the box's top corner, on its fill and clear of the bar. Nothing inside a box grows it.
- **A subagent is one line, read apart in a float.** Its line: a mark of its own in the
  arrow's slot, its name, what it is doing (shimmering), *waiting on you* in the warning ink
  while its own permission or question waits, or how it ended (*done*, *failed* in the
  danger ink, *cancelled*, *interrupted*), and its steps and time at the end. Pressed, its own
  transcript opens in a float over the pane, drawn by the transcript's own renderer: a head (the
  name; its state, steps and time; *n of N* with the previous and next subagent of the turn; a
  way back from a nested one; Close), the task it was given cut at two lines with *Show all*, and
  its runs, capped. The float reads the pane, never the window: as tall as what it holds and at
  most three fifths of the pane, at the pane's top right while the pane is wide enough and
  across its width when it is not. It is not modal, and Escape closes it while it holds the
  keys. While any of the turn's subagents works, a chip stands in the composer's row: their mark,
  then a count per standing, each in the rail's mark and ink for it, most urgent first (waiting on
  the person, failed, running, done, stopped), its words only in its tooltip; pressed, the float
  opens on the most urgent.
- **A turn's changes say each file once.** One file is one line: what happened to it, its path,
  its counts, and the chevron opens its diff. Several are a head (*N files changed*, the counts)
  and a row a file: the path giving way at its start (the folder in the meta ink, the name in the
  foreground, struck when deleted), *new* or *deleted* in the muted ink, the share bar on a
  changed file only, and the counts in one column at the right edge. Each diff opens on its own
  row, in the box. A finished turn ends on Copy (its closing paragraph) and how long it took, said as
  *Failed after …* where the turn ended on a failure and *Stopped after …* where it was stopped
  or cut short.
- **How a turn ended is said, not alarmed.** A cancel, whoever asked for it, leaves one *Stopped*
  divider, and a step it caught running reads *stopped* at its row's end in the muted ink, its
  cluster counting *N stopped* in the line's own ink; only a cancel stops a step, and anything
  else that ends one unsettled (a lost agent, a turn that moved on) still fails it. A turn the
  agent cut short (its output limit, its request limit, a refusal) is the neutral card with no
  alarm in it: the info mark in the muted ink, what stopped it as the title and what that means
  under it, both in the body ink, and after a limit *Continue* (`secondary`, `.short()`), which
  asks the agent to carry on as a person's prompt would, while no prompt has followed. It is
  still a failed turn to the rail and to whatever drives the session. A failed step's *exit N*,
  or *failed* where no code came, is in the danger ink while its turn runs and the muted ink once
  the turn is over.
- **Its rows are lines, not list rows**: a fold line or an activity row is as tall as its text,
  answers the pointer by raising its ink rather than with a fill, leads with an `.xsmall()` mark
  in one slot (the kind of work in every state, so nothing moves as it settles), and takes no focus.
  **What runs is told by its words**, never by a turning mark: a running row's verb, in the present
  tense, and what it is done to, a live thought's *Thinking…*, a folded cluster's step in hand, a
  plan's item in hand and what a working subagent is doing read in the ink of the line they stand on
  while a narrow band of the foreground ink crosses their opening, with no accent, and lift with
  that line under the pointer. A step is named by one verb and one path wherever it is named: its
  row and its folded cluster's line say the same. An opened cluster's line says only what has
  settled (*Working* while nothing has), since the step in hand has its own row; a step reported
  as pending reads as running, since an agent may hold one there until it ends, and one that has
  run ten seconds shows its own clock at the row's end. A plan's mark never turns: a check once
  done, a ring for the item in hand whether or not the turn runs, a dash for one not begun.
  A path reads from the project's root,
  and from `~` outside it. A
  path or a command is mono; the agent's own wording, a folder and what the agent did (a
  thought, a command's output, a read or search list) read in the middle ink. One copy control
  (`.short_icon()`, quiet, named in its tooltip, over a box's top corner; a permission's on the line
  under its command, beside where it runs; a prompt's under its bubble, with *Fork from here* beside it, shown while
  the prompt is pointed at or a control holds focus) and one show-more control serve every block. Text
  that scrolls keeps a gutter for its scrollbar.
- **A table keeps its columns readable**: each is as wide as its text down to a floor, and past
  the column the table scrolls sideways in its own track while the vertical wheel stays the
  conversation's. Under every table, Copy (as Markdown) and *Open*, which shows it whole in a
  dialog as wide as the window allows.
- **Calm once it is past**: a failure a later prompt has moved past keeps its mark, title and
  words in the muted ink; a settled *N failed* drops the danger ink; a refusal's badge is
  neutral. An error, and the failure a turn ended on, is the waiting cards' neutral shell, flat
  and on the page's fill: the alert mark in the danger ink, *Turn failed* in it too where a turn
  ended on it, then the words in the body ink, what it attached folded under *Show details* in mono; a lost agent's names the
  key that restarts it. A failed turn holds until a prompt goes out; a restart does not answer it. An
  error that failed no turn (one answering a *Stop*, a resume that fell back) is past at once.
- **The pinned cards** (a permission, a question) share one neutral shell: a hairline edge
  (`border`) at `radius_lg`, no wash, the tone only in the mark and the title (a permission's
  warning ink). While they wait they stand on the composer's fill (`theme::raised`) with its lift
  (`composer_lift`), as wide as the composer, their foot tucked under it (`TUCK`) so the composer
  reads as laid over them, without motion: every fill inside a card is a step tuned against the
  page, so the card takes no grey of its own. The ones waiting behind peek over the shown one's
  top edge as slivers, at most two, and its head says *1 of N waiting*. The transcript's end clears
  the pinned cards as it clears the composer, so the rows they stand over can be scrolled clear of
  them. Back in the history the shell is on the page's fill and
  flat. It holds a head, the body, the answers at the end at `.short()` (a hairline under the head,
  padded as a box whose body scrolls, its cut end fading out while more lies below), the
  dismissing one a quiet ghost. A choice is a bare row at rest, on the pointer's fill under the
  pointer and the hover step (`list_hover`) under the keys, and on the selected step when taken, a radio or checkbox drawn in ink at
  its head (the library's rings vanish on the selected step) and its number with Alt as a bare `Kbd`, a menu row's key, at least
  a field's height, the scrolling list of them between two hairlines, one over it and one under it,
  the last row clear of the lower one once scrolled to its end;
  a typed answer sits under the list, with the answers and no line between them,
  as a field with its `Kbd`: edged in `input` on the card's own fill, two
  lines tall at rest and growing as it is written (Shift+Enter a newline), ringed while the keys
  are on it; what the field shows is the answer, a word an input method is still composing
  included. A permission's command sits in a well (`muted`). A permission's answers say their
  kind, *Allow once*, *Always allow*, *Deny*, *Always deny*, the agent's own words for each in its
  tooltip; the agent's words stand on the button only for a kind the protocol does not name, or
  where two answers are of one kind. Each answer in a pinned card's footer shows its key beside its words, always in
  sight, as that key reaches the card from anywhere in the conversation.
  The card holds the one primary while it shows. A question card always ends on *Skip*; one
  asking a single pick-one question answers on the pick, its primary only for a typed answer.

## Type, space and shape

#### T-01 Text takes the size its role takes

| Role | Size |
|---|---|
| Meta, hints, counts, a small label | `.text_xs()`, in `muted_foreground` when it is meta |
| A caption inside a small object, a badge's words | `MICRO_TEXT`, a step under the meta size |
| Machine text | `CODE_TEXT`, in `mono_font_family` |
| Chrome: a panel's rows, boxes, forms and strips, a menu's rows, a `.tall()` control's words | `CHROME_TEXT`, a step under the library's control size |
| A control's words (`.short()`) | `.text_xs()` |
| Reading text: the transcript, the composer's field, a document | the chrome size on the reading leading: the transcript's `TEXT`, `CHROME_TEXT` elsewhere |
| The transcript's record: activity, thoughts, notices, a card's question | a step under the reading size, the transcript's `TEXT_SM` |
| A heading in reading text | a step up the scale per level, two at most; then the body's size |
| A page's title, in its head | `CHROME_TEXT`, semibold |
| The title of one thing read in full (an issue, a task) | `.text_lg()` |
| The display size, for the one heading of an empty view | `.text_xl()` |

- Checked by: test `the_sizes_are_the_listed_ones` for each constant's value; review for the role.

#### T-02 Weight carries hierarchy before size

- When: a name, a title or a heading has to stand apart from the text around it.
- Then: `font_medium` for a name in a header, and for a list page's row name and the column read
  down the list; `font_semibold` for a page's title and any other title or heading. Nothing
  bolder. Line height is gpui's, but `LEADING` for reading text and `LEADING_TIGHT` for a
  wrapped title.
- Checked by: review.

#### T-03 A row's weight is its kind's, never its being chosen

- When: a navigation row or a tab is the chosen one.
- Then: its fill alone says so (T-23); its weight stays its kind's.
- Checked by: picture.

#### T-04 Neutral text takes one of three inks, by role

- When: text stands on a neutral surface (the reading surface, a dock, the rail, a floating
  surface, a well).
- Then: `foreground` for content and the current location; `theme::meta_ink` for a detail beside
  a name on its line (its folder, its number, its kind); `muted_foreground`, still readable, for
  labels, counts, hints, placeholders and a row's second line. No fourth neutral ink.
  K-03 says which part of a row, a box or a strip takes which.
- Checked by: tests `the_inks_read_on_every_neutral_surface` and
  `the_middle_ink_reads_and_stays_between_the_two_it_divides` for the contrast; review for the role.

#### T-05 Text on a fill takes the ink paired with that fill

- When: text stands on a fill rather than a neutral surface.
- Then: a button's variant and the library's controls set their own; a chosen row takes
  `chosen_ink`; a badge takes its `Tint`'s ink.
- Checked by: tests `the_primary_word_reads_on_the_primary_fill`, `a_tint_reads_on_its_own_fill`,
  `the_chosen_row_reads_and_stands_off_a_hovered_one`.

#### T-06 Text that carries meaning takes its semantic ink

- When: text says a state, is a link, names what runs, or is code in prose.
- Then: a state takes `status_ink()`; a link `link`; what runs `info`; code in prose `hue_ink`.
- Checked by: test `status_ink_is_readable_on_every_surface_it_lands_on`; review for the meaning.

#### T-07 A quieter ink is a mixed step, never an alpha

- When: text or a mark has to read quieter than its neighbours.
- Then: it takes a step mixed from two tokens (`mix_oklab`). No alpha goes on the text or on its
  container: see-through text reads differently on every fill under it.
- Unless: what a cut draws (L-02).
- Checked by: review.

#### T-08 Mono for machines, the interface face for people, both shipped

- When: any text is drawn.
- Then: code, paths, terminal output and diffs use `mono_font_family`; anything a person wrote
  uses the default family. Both faces ship with the app (`crate::fonts`), so text looks and
  centres the same everywhere; `[font].monospace` may name another installed mono family.
- Checked by: review.

#### T-09 Space takes one of gpui's steps

- When: a gap, an inset or a padding is set.
- Then: one of `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6`, `8`, picked by the relationship (K-05),
  not by what fits one screenshot. A named spacing constant equals one of them.
- Checked by: review; guard `a_size_is_written_once_in_a_constant` holds a number written anywhere else.

#### T-10 A size is in rems, has a name, and is written once

- When: a size the steps lack is needed.
- Then: a named constant in rems, its reason in its doc comment: in `onehand_ui::tokens` when a
  second module needs it, else beside its use. Fixed chrome heights stay outside the zoom
  wrapper.
- Unless: what zoom must not reach is in pixels: a stroke (a line, a ring, a mark), a shadow, a
  bound measured against the window, an inset cancelling one the library draws in pixels, and a
  grid that zooms on its own (the terminal's).
- Checked by: guard `a_size_is_written_once_in_a_constant`; test `the_sizes_are_the_listed_ones`,
  which shows every size added or changed by name.

#### T-11 What a person presses takes `radius`

- When: a part is pressed, picked or typed in: a control, a field, a menu or rail row, a tree
  row, a tab and the open tab's fill in a tab bar, a chip, and the fill a choosable item shows
  on hover or when chosen.
- Then: `radius`.
- Unless: a part joined to its container's edge takes its corner there; the library's small
  parts drawn inside a control keep its half corner (C-10).
- Checked by: test `corners_are_four_for_controls_and_six_for_containers` for the values; review.

#### T-12 What wraps a block of information takes `radius_lg`

- When: a part wraps a block of information or an area holding several actions: a well, a box
  on a page (`card_box`), a card, the input surface and a card pinned to it, a strip waiting on
  it, a popup, a floating panel, a dialog, a toast, a group of settings, and a region's box.
- Then: `radius_lg`, a step rounder. The library's own dialogs and toasts follow.
- Checked by: test `corners_are_four_for_controls_and_six_for_containers` for the values; review.

#### T-13 Only a dot or a status mark is `rounded_full`

- When: a corner is fully rounded.
- Then: the part is a dot or a status mark. A small mark that is neither (a letter tile) takes
  `radius`.
- Checked by: review.

#### T-14 A badge is the shared builder's

- When: a tag stands on its own (a state, a count, *Default*).
- Then: `badge`: a `Tag` at `radius`, its tint's fill edged halfway from its fill to its ink
  (`badge_edge`), its words at the micro size and medium weight on a slim inset.
- Unless: inside a line of meta text a label is written in its kind's ink instead, unframed.
- Checked by: review.

#### T-15 Regions part by the gap

- When: a region of the window is drawn.
- Then: its box is wrapped in a hairline (`border`) and stands on the window's fill, its own
  fill a step off it, parted from its neighbours and the window's edge by `REGION_GAP`, which
  shows the window's fill.
- Checked by: tests `the_regions_stand_a_step_apart`, `a_box_stands_apart_from_the_window`.

#### T-16 Lines part content inside a region

- When: two parts inside one region have to be told apart.
- Then: a 1px `border`: under a strip, parting it from what it heads; down the side of a list
  facing its detail, on the box's own fill; under a page's head, where the page's own content
  starts. A list page's line over its foot is a step fainter (`faint_rule`), so the
  edge-to-edge list reads as one surface. A box on a page takes a step stronger edge, a
  control's (`input`); a group of settings keeps the hairline. Tabs in a tab bar are parted by
  space, not lines, the open one a rounded `secondary` fill with its name in the content ink.
- Unless: a folding block has no edge at all (*Transcript*).
- Checked by: review.

## Colour and state

The palette is onehand's own, a light one and a dark one under one set of rules. Pairs under
WCAG AA are in `docs/known-gaps.md`; `crate::theme`'s tests hold each at what it reaches.

#### T-17 A colour is a theme token, read at the call site

- When: anything is given a colour.
- Then: `cx.theme()`'s slot named for the role (T-21). A step no slot holds is derived in
  `crate::theme` or `onehand_ui::ink` from the slots, with its contrast asserted there, and a
  step between two tokens is mixed perceptually (`mix_oklab`), so both modes derive it by one
  rule. Never a value in a view.
- Checked by: lint (the colour constructors are banned outside the palette); guard
  `a_colour_is_written_in_the_palette_alone`.

#### T-18 The palette is written once

- When: a palette value is added or changed.
- Then: in `crate::theme`'s `palette`, which `crate::theme::install` lays over the component
  library's two theme configs, so a library part and the app's own read one palette. A slot
  the palette leaves alone (a status hue, the ring, a chart's series) stays the library's.
- Unless: a program's colours are its own: the terminal's sixteen stay VS Code's
  (`onehand_terminal_ui::ansi`). A chart's series and a label's kind take the library's base hues.
- Checked by: test `the_sizes_are_the_listed_ones`, which lists every palette slot; owner for
  the value.

#### T-19 Surfaces are neutral greys, a visible step apart

- When: a surface is filled.
- Then: on the dark palette the reading surface is the deepest, a charcoal still clear of
  black, and every region's box is drawn on it; the window's fill, under the title bar, the
  rail and the gaps between the boxes, a step over it; a floating surface a step over the
  page, told apart by its fill and its edge. The light palette turns it over: the boxes white
  on a pale grey window. A nested container inherits its surface unless it has a role of its
  own: a well, a selection or floating content.
- Checked by: tests `the_regions_stand_a_step_apart`, `a_box_stands_apart_from_the_window`,
  `a_floating_surface_stands_off_what_it_floats_on`.

#### T-20 A hue on screen means something

- When: a part takes a hue rather than a grey.
- Then: it says a state, a link, a label's kind or code in prose, or it is the primary action,
  the palette's one blue. Focus and selection are greys.
- Unless: the control that switched a mode on may carry a state's hue on its own mark while the
  mode is on, and nowhere else.
- Checked by: test `hover_stays_neutral`; picture.

#### T-21 A role takes its token

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text: every region's box |
| `sidebar` | the window's fill: the title bar, the rail and the gaps between the boxes |
| `theme::dock_surface` | the Workbench and the terminal: the reading surface, as every box is |
| `muted` / `muted_foreground` | quiet fills (every well), meta text |
| `theme::meta_ink` | the middle ink: a detail beside a name, between prose and meta |
| `border` | every hairline: a region's box's and a settings group's edge |
| `input` | a field's edge, and a box's on a page |
| `ring` | keyboard focus (T-24); the input surface while a file is dragged over it |
| `chosen_fill`, `chosen_ink` | the one item chosen among several: a row, a rail entry; the selected grey (`accent`), and the content ink on it |
| `accent` | the selected grey: the chosen item (through `chosen_fill`), and what the keys rest on in a popup and the pointer on a ghost, as the library draws them |
| `link` | a link: the library draws it in the prose ink, underlined |
| `info` | what runs |
| `list_hover` | hover on a pickable row; on the rail `sidebar_accent` |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text, and `closed` for what is over and settled |
| `popover` | floating surfaces |
| `theme::raised` | the input surface's card |
| `secondary` | a document's code on a dock, a tab bar's open tab |

- Checked by: review; the slots' names that mislead are in `.claude/skills/ui/tokens.md`.

#### T-22 Hover is a fill, a neutral step from the surface under it

- When: the pointer is over something pickable.
- Then: a row takes the hover step (`list_hover`; on the rail `sidebar_accent`). A ghost button
  takes the library's own hover; a secondary button its pointed-at step. Hover changes a fill,
  never an ink or a layout (K-04).
- Checked by: test `hover_stays_neutral`; picture.

#### T-23 The chosen item stands on the selected grey

- When: one item among several is the chosen one, or the keys rest on a row inside a popup.
- Then: `chosen_fill`, a step past the hover step and solid (a row's fade and the controls over
  a row are painted in it), its name in `chosen_ink`; in a popup the same step (`accent`). A
  hovered selected row stays recognisably selected.
- Checked by: test `the_chosen_row_reads_and_stands_off_a_hovered_one`; picture.

#### T-24 Keyboard focus is the ring, drawn only when the keyboard moved it

- When: a control holds keyboard focus.
- Then: the ring (`ring`, a grey), and only when the keyboard put the focus there.
- Unless: a field, and the input surface, are edged in `input` and show focus by the caret
  alone, the edge unchanged; a list whose arrow keys move the selection itself draws no ring
  (K-17).
- Checked by: picture.

#### T-25 At most one primary in a view

- When: a view offers actions.
- Then: at most one takes `primary`; a view needs none (a page that only sets values has
  none). Where layers stack, what waits on the person holds it while it shows (a floating
  item, an open form, a pinned card, an approval), and what it covers steps down to `secondary`.
- Checked by: review.

#### T-26 A group is shown by one device, not three

- When: parts that belong together have to read as a group.
- Then: a box on a page is a border; a region's box is a fill in a hairline; a well and a
  bubble are fills; an ordinary section is unboxed (K-01). A border, a contrasting fill and a
  shadow are never combined just to make a group visible.
- Checked by: test `a_waiting_card_keeps_every_fill_and_line_drawn_on_it` for the card; review.

#### T-27 One control carries a lift

- When: a pick-one track is drawn.
- Then: a sunk fill (`muted`) with no edge, the pick raised a step lighter under a tight lift
  (the page fill by day, the selected step by night, where the page is darker than the well).
  No other control is lifted.
- Checked by: test `a_segment_pick_is_raised_off_its_track`.

## Components

#### C-01 Reuse the library before building

- When: a control, an overlay or a container is needed.
- Then: in this order: a builder `.claude/skills/ui/catalog.md` lists, a gpui-component part
  (`Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`, `InputState`, `Editor`, `TextView`, gpui's
  own `list`), a builder of your own. A second place that needs something drawn inline lifts it
  into a shared builder in that change.
- Unless: the app owns what the library cannot draw: the transcript renderers, the icon
  registry, the terminal panel, per-panel zoom and the input surface's popup.
- Checked by: review.

#### C-02 A button is made by `action`, a field by the field builders

- When: a button or a field is drawn.
- Then: buttons through `action`; fields through `text_input`, `search_input` and `text_area`.
  The builders set the pointer and the library's corner.
- Checked by: lint (`Button::new` and `cursor_pointer` are banned outside the shared builders);
  review for a field.

#### C-03 A field is the library's, unchanged by focus

- When: a field stands in a form, a dialog, a page's top or a search.
- Then: edged in `input` on the page's fill, its edge unchanged by focus (T-24). A search adds
  the search mark and the clear button.
- Unless: a search that stands on a bar is drawn as that bar's entry in `docs/screens.md` says.
- Checked by: picture.

#### C-04 A button names its variant by its role

| Variant | Role |
|---|---|
| `primary` | the one commitment in a decision area (T-25) |
| `secondary` (edged, on its own fill) | an ordinary action beside it; a set picker's trigger; every button on a page that only sets values |
| `outline` | an action that needs a boundary with less weight |
| `segment` | a choice in a pick-one track, its pick raised off the track's fill (T-27) |
| `choice` | a filter chip: a ghost, selected when taken |
| `danger` | C-09 |
| ghost (`.quiet(cx)`) | the familiar, low-emphasis commands of a toolbar, a strip or a row; an unset picker's trigger (through `picker`) |

- Then: each is drawn as the library draws it; no variant and no size is the library's default
  and is never what a role takes.
- Checked by: review.

#### C-05 A control that cannot act refuses, and says why

- When: a button cannot act now.
- Then: `.refuses(Some(why))`: no press, the arrow cursor, and `why` as its tooltip. It says
  nothing else (S-11).
- Unless: a switch a person cannot flip is `.locked(…)`; a menu item and a field keep the
  library's `disabled`.
- Checked by: lint (`Disableable::disabled` is banned outside `Refuses` and `Locked`).

#### C-06 A ghost never starts a line alone

- When: a ghost button would stand at the start of its line with nothing before it.
- Then: it takes the secondary fill instead (a form's picker, a fold toggle): without a fill
  its padding sets its words in from the edge the text above and below starts on, and nothing
  drawn says where it begins.
- Unless: a ghost ending a line or centred on it may stand alone.
- Checked by: picture.

#### C-07 A ghost is known by where it stands

- When: an action stands beside a sentence (a finding, a notice, a status line).
- Then: it takes the secondary fill, since a ghost there reads as more of the text. A ghost
  belongs in a strip, a toolbar, a footer, a row's action slot, beside other controls.
- Checked by: picture.

#### C-08 An action with a menu of alternatives is one control

- When: an action has a menu of other ways to do it.
- Then: `split_action`: the action and its caret half joined edge to edge, one variant and one
  height, the inner corners square, parted by a seam in the colour of the surface under them.
  A caret never stands as a button apart beside the action it qualifies.
- Unless: a caret otherwise ends a picker or a tab, after its words.
- Checked by: review.

#### C-09 Danger is a word, and a fill only to end or confirm

- When: an action destroys something.
- Then: a word in the danger tint. The solid `danger` fill marks two buttons only: the one that
  stops work while it runs, and the one confirming a removal inside its modal (S-13).
- Unless: nothing that destroys nothing takes the danger ink.
- Checked by: review.

#### C-10 The library's own parts are the library's

- When: a gpui-component control or overlay draws a part itself: a refusing or loading button's
  alpha, a picker's caret, a badge under the pointer, a switch's thumb, the motion of its
  overlays, the `sm` radius of its small parts drawn inside a control (a `Kbd`, a checkbox, its
  own tabs), an open dropdown's trigger on its pressed fill.
- Then: it stays as the library draws it.
- Unless: where the library's mark vanishes on a fill the app draws on, a shared wrapper draws
  it in ink (`choice_mark`). The app corrects the library through a shared wrapper only, never
  at a call site.
- Checked by: review.

#### C-11 A control's size names its role, and the role follows the room

- When: a control is given a size.
- Then: one of the library's two. `.tall()` where there is room and the person reads and types
  what the control holds: a page's form, and a dialog holding a form at `DIALOG_FORM`.
  `.short()` in dense chrome, so a control never outweighs what it acts on. An icon alone is
  square at its row's height. `.tall()` is the ceiling. C-16 maps each role to its component
  and size.
- Unless: inside a fixed object smaller than a row of controls (an attachment's plate, a
  thumbnail) an icon-only control takes `.xsmall()` and a caption `MICRO_TEXT`, so neither
  crowds what it sits on. A worded control never takes `.xsmall()`, whose words run to its edges.
- Checked by: review; test `the_sizes_are_the_listed_ones` for `CONTROL_H` and `FIELD_H`.

## Icons

#### C-12 Every icon is an SVG from the library's set

- When: an icon is drawn.
- Then: gpui-component's `IconName`, the library's default subset of Lucide. `crate::icons`
  holds only what that set cannot draw: shapes picked from the kit's complete Lucide catalog,
  and brand marks and forks of bundled shapes, synced from `assets/icons/manifest.toml` by
  `scripts/sync-icons.sh`. Never a Unicode or emoji glyph. Where the bundled set lacks a shape,
  an approximate icon is accepted.
- Checked by: review.

#### C-13 An icon takes its ink from what it says

- When: an icon is given a colour.
- Then: at rest `muted_foreground`; one that carries state a semantic token; one beside text
  that text's colour.
- Checked by: picture.

#### C-14 An icon's size and slot are its role's

- When: an icon stands in a row, a control or a line of text.
- Then: equivalent roles use the same library size through a shared builder, and an optical
  correction belongs there. An icon beside text is no taller than that text's line and sits on
  its centre. Icons down a column share one slot width (`mark_slot`), so the labels after them
  start on one edge.
- Checked by: picture.

#### C-15 An icon is added only when it helps

- When: an icon is about to be added.
- Then: it helps recognition, or names an action that has no visible text; a word reads faster
  than a shape nobody has learned. No icon sits on a tile of its own: a fill behind one belongs
  to the control or row it is part of.
- Checked by: review.

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY
of its own per project. Files lives inside Editor, not as a mode. Every mode is built in.
