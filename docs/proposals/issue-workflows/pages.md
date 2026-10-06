# Piece 5: issues and workflows as pages of their own

- Status: proposal, wave 2. **Needs a design decision first**: it changes a principle of
  [DESIGN.md](../../../DESIGN.md). Part of [the proposal](README.md).
- Contracts it touches: DESIGN.md (*Chat is the centre*, the rail, *Pages without a session*, the
  Workbench's Issues mode, Settings ▸ Workflows).

## Goal

Room to work through many issues and to write workflows, without squeezing either into a dock or a
settings dialog.

## Today

- **Issues** are a Workbench mode: a list and an issue's detail in the right dock, beside the
  conversation, at the dock's width.
- **Workflows** are a page of the Settings dialog: a capped list, and a form below it, one hairline
  box per step.
- **Tasks** and **Workspace overview** are already pages of the agent pane, opened from the rail,
  with the header reading their name and both docks put away.

## The conflict

DESIGN.md's first principle is *Chat is the centre*: the conversation is the only region that
flexes, and docks never crowd it. The two pages that exist (overview, Tasks) are already an
exception in practice: they take the agent pane, not a dock. Two more pages make the exception a
pattern, and the principle should say so rather than be bent quietly.

## Options

| | A: pages in the agent pane | B: keep the dock, widen it | C: keep as is |
|---|---|---|---|
| Issues | a rail row *Issues*, a page with the list on the left and the detail on the right; the Workbench mode stays for a glance beside a session | the Issues mode gets a maximized two-column layout | no change |
| Workflows | a rail row *Workflows*, a page: list, and the editor in the page's column | stays in Settings | stays in Settings |
| DESIGN.md | *Chat is the centre* becomes: the conversation is the centre **while a session shows**; pages take the agent pane and put the docks away | unchanged | unchanged |
| Cost | two pages, two rail rows, the principle reworded | a layout per width | none |

**Recommended: A.** It follows the shape the overview and Tasks already have, keeps the dock mode
for reading one issue beside a conversation, and moves workflows out of a dialog, which is the wrong
place for something a person writes and runs from.

## If A is chosen

- **Rail**: *Issues* and *Workflows* below *Tasks*, highlighted like it; *Issues* carries no count
  (*Tasks* already counts what needs a person).
- **The Issues page**: the list across the workspace's projects (`issues::Across` exists for the
  overview's card), filters by project, label, open or closed, and by progress (piece 1's state:
  working, waits on me, done not settled, nothing yet); the issue on the right, with piece 1's *Work*
  section in full and diffs opened in place.
- **The Workflows page**: the list as Settings has it, *Run…* on each (the launcher on the project
  on screen), and the editor as a list of steps, each showing its prompt, its gates or command, what
  it keeps, and where a failure sends it back (`on_fail`), with the arrows drawn in the margin.
  No canvas, no dragging between lanes. Settings ▸ Workflows keeps only the project check commands,
  or moves them to the project menu.
- **Glossary**: no new words; *page* is not a glossary term.

## Questions for the decision

1. Is *Chat is the centre while a session shows* the principle you want, or should pages stay
   limited to overview-like summaries?
2. Does the Issues Workbench mode stay once the page exists? Recommended: yes, as the glance beside a
   session.
3. Where do project check commands live once Settings ▸ Workflows goes?

## Documents to change when built

- `DESIGN.md`: principle 1, *Layout* (rail rows, pages), *Docks* (the Issues mode's role), Settings.
- `docs/workflows.md`: where workflows are written.
- `docs/unattended.md`: where an issue's runs are seen.
