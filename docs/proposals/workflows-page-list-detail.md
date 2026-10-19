# Workflows page: list beside its editor

Status: agreed in conversation on 2026-10-19, not built. When built, its rules move into
`docs/screens.md`, `docs/workflows.md` and `docs/ui-patterns.md`, and this file leaves
`docs/proposals/` in the same PR.

## Why

The Workflows page today stacks the editor under the list on one `PAGE_PROSE` column: *New
workflow* or *Edit* grows a form below every row, so the page reads as a list and a form at
once, and the form scrolls away from the list it came from. The form itself is one flat run of
fields (name, description, where it works, misses, timeout, a loose variables hint, then a box
per step, six *Add … step* buttons, problems, *Save*), with nothing grouping the fields by what
they are for. Each step box is open in full, so three steps already fill a screen. The user
finds it inconsistent and unpleasant to work in.

## Goals

- The list and the editor are two regions side by side, never one stacked above the other.
- The editor groups its fields by purpose, each group under a heading.
- A workflow of several steps reads at a glance: one line per step, opened one at a time.

## Non-goals

- No change to the workflow model, its validation, its file format, the store, import/export,
  the launcher or the start form. `crates/core` is not touched.
- No change to what a step of each kind asks for: the fields inside an open step stay as they
  are today (`workflows_page/step.rs`), only laid out differently.
- No new features (search, sorting, drag to reorder, undo).

## Anti-goals

- The editor stretched edge to edge on a wide window: fields too long to read.
- A second, competing list-and-detail rule. The page reuses `list_detail`
  (`crates/plugin-host/src/list_detail.rs`) and the *List and detail* pattern.
- Losing an unsaved draft without asking.

## The page

- **Width.** Workflows stops being a `PAGE_PROSE` column page and takes the pane's width for a
  list beside its detail, laid out by `list_detail`: side by side while the pane holds both,
  otherwise one at a time, the editor under a `back_link` naming *Workflows*.
- **The list (left).** A narrow column at a fixed starting width (dragging it is the builder's
  call; `ListWidths` allows it). Its head is cut down to fit the column (user's choice): *Workflows*
  at the title size with the count under it in meta ink, and at the end *New*, the page's one
  primary, stepping down to `secondary` while an editor is open, and a ⋯ holding *Import…*. The
  page's head stays in the body, so the pane header carries chrome alone, as on a column page.
  The about sentence goes; what it said (shipped workflows are read-only, duplicate one to change
  it) moves to the facts line of a built-in workflow's editor (my call). Then the rows, capped at `TEMPLATES_SHOWN` and saying what was left out, as today, in
  two slots: the person's own, then *Built in*. A row's name is its whole content; picking it
  opens it on the right and marks it selected. ↑ and ↓ step the selection while the list holds
  focus.
- **The editor (right).** Centred in the region beside the list, up to `PAGE_PROSE` wide, with
  equal gutters, so hiding the list centres it rather than stretching it.
- **On arrival nothing is selected.** The right region shows one centred meta line
  (*Pick a workflow to see and change it, or make a new one*) (user's choice).
- The selection, the open draft and the list's scroll are kept while the page is left and shown
  again (the draft already is, per window).

## The editor

- **Head**: the workflow's name at the title size, a line of facts under it in the middle ink
  (how many steps, where it works), and at the end *Run…* and a ⋯ menu holding what the row's
  hover actions hold today: *Duplicate*, *Export…*, *Delete…* (my call: rows lose their hover
  actions, since picking a row now opens it).
- **Three groups**, each a plain section of the form (a heading in the section's semibold, the
  section gap, no box of its own), in this order:
  1. *About*: Name and Description, side by side where they fit.
  2. *How it runs*: Where it works (the switch), then Misses allowed and Timeout side by side.
  3. *Steps*: the variables hint (`{brief}` …) moves to the meta line beside this heading;
     then the steps; then one *Add step* button opening a menu of the step kinds, replacing the
     six buttons.
- **Foot**: problems in the danger ink above *Cancel* and *Save*, as today; *Save* refuses with
  the first problem and spins while it writes.
- **A built-in workflow** opens read-only: the same groups, every field disabled, no *Save*; its
  head offers *Duplicate to edit* as the editor's primary (my call, in line with "shipped ones
  are read-only: duplicate one to change it").
- **New workflow** adds a draft row (*Untitled*) at the top of the person's slot, selected, with
  an empty editor on the right; *Cancel* on an unchanged draft removes the row. *Import…* opens
  the imported file the same way, as a new draft.
- **Esc** closes the editor and leaves nothing selected, asking first when the draft holds
  changes (*Keep*, *Discard*), as today.
- **Picking another row with changes unsaved asks first** (user's choice), with the same
  *Keep*/*Discard* modal; *Keep* leaves the old row selected and the draft as it was.
- **Selected is open** (my call): picking a row, by pointer or by ↑ and ↓, opens it, as the
  Issues page steps its list; there is no separate *Edit*. Stepping off a draft with changes asks
  as picking does. *Cancel* does what Esc does: closes the editor and clears the selection,
  asking first when the draft holds changes.
- **Duplicate** opens the copy as a new draft row, as *Import…* does (my call). An unchanged
  *Untitled* row goes when another row is picked. The selection is kept by id, not by place.
- **Run…** refuses, its tooltip saying to save first, while the draft holds changes or was never
  saved (my call): the launcher runs what is stored.
- **An unreadable workflow** keeps its row with *Cannot be read* in the warning ink; picking it
  shows the reason on the right and, for the person's own file, *Delete…* (my call), so a broken
  file can still be removed from the app.

## The steps

- The steps stand in one box, a row each, parted by hairlines. A row reads: the step's number,
  its kind as a small badge, its label (its id when it has none), then muted facts (where a
  failure goes back to, *on failure ↑ 1*; *keeps answer* for an agent step that keeps it), then a
  chevron and the step's ⋯ (*Move up*, *Move down*, *Remove*, as today).
- **One step open at a time.** Picking a row opens it under its line and closes the one that was
  open. A step just added opens. Opening a step does not move the page's scroll away from it.
- **An open step** shows Kind, Id and Label on one line (wrapping when narrow), then the fields
  of its kind exactly as today.
- **A step with a problem** shows a danger dot and the problem on its row, so it is seen without
  opening it (from the step each problem already names in core's validation; core is untouched).
- The margin column with *to N* / *from N* arrows goes; the row's *on failure ↑ N* says the same
  thing (my call: *from N* is not repeated on the target row).

## Constraints

- Follow the `ui` skill and its checklist; sizes, inks and corners from the theme and tokens,
  `.tall()` controls as a page form, buttons through `crate::controls::action`, menu rows through
  `controls::menu_item`.
- No document path or section is cited from code comments.
- `workflows_page.rs` is 449 lines; split along a seam (the list, the head, the empty state)
  before any file passes about 800.
- The owning documents are updated in the same PR: the *Workflows* bullet in `docs/screens.md`,
  the page paragraph and the by-hand rows about the page in `docs/workflows.md`, and Workflows'
  place among the page kinds in `docs/ui-patterns.md` (it leaves the `PAGE_PROSE` column-page
  list for a list beside its detail).
- One PR, docs included.

## Left to the builder

- The list's starting width and whether it can be dragged.
- How the open step's chevron and the row's hit area look, within the contract.
- Whether the row badge for a kind reuses an existing badge or chip.
- The exact meta wording of the step row's facts and the empty state, within the *Words* rules.
- How the split is cut across files.
