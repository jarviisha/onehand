# Piece 5: workflows as a page of their own

- Status: proposal, wave 2. The Issues half of this piece is built and lives in its owning
  documents: the page in [DESIGN.md](../../../DESIGN.md) (*Pages without a session*, the rail, the
  reworded first principle) and where an issue's runs are seen in
  [unattended.md](../../unattended.md); its checks are rows of the by-hand list in
  [workflows.md](../../workflows.md). What remains here is the **Workflows page**. Part of
  [the proposal](README.md).
- Contracts it touches: DESIGN.md (the rail, *Pages without a session*, Settings ▸ Workflows).

## Goal

Room to write workflows, without squeezing them into a settings dialog.

## Today

- **Workflows** are a page of the Settings dialog: a capped list, and a form below it, one hairline
  box per step.
- **Tasks**, **Issues** and **Workspace overview** are pages of the agent pane, opened from the
  rail, with the header reading their name and both docks put away. A page is the agent pane's
  only while a person picked it, as DESIGN.md's first principle now says.

## The Workflows page

A rail row *Workflows* below *Issues*, highlighted like it. The list as Settings has it, *Run…* on
each, and the editor as a list of steps, each showing its prompt, its gates or command, what it
keeps, and where a failure sends it back (`on_fail`), with the arrows drawn in the margin. No
canvas, no dragging between lanes. Settings ▸ Workflows goes; the project check commands move to
the project page, beside *Run check*, where the command is run. *Run…* opens the one launcher the
rail and the keymap command open (asking for the project first, below); the rail keeps its way in,
for a start from inside a session, and no second form is built.

Glossary: no new words; *page* is not a glossary term.

## What the page has to settle

- **Which project a run starts on.** A page across the workspace has no project on screen. A
  workflow's *Run…* asks for the project first, defaulting to the one selected in the rail, and
  the launcher then opens on it as today.
- **An unsaved workflow when the page changes.** The editor's draft lives on the page's entity, not
  in the render, so leaving for another page and coming back finds it as it was. Closing the window
  or picking another workflow with changes unsaved asks, in a modal, whether to drop them.

## Questions for the decision

1. ~~Where do project check commands live once Settings ▸ Workflows goes?~~ On the project page,
   beside *Run check*.

## Done when

The Workflows page: as the list above says; its checks are written when it is next in line.

## Documents to change when built

- `DESIGN.md`: *Layout* (the rail row, the page), Settings.
- `docs/workflows.md`: where workflows are written.
