# Piece 5: issues and workflows as pages of their own

- Status: proposal. Option A below is chosen (2026-10-06). The **Issues page** is wave 1, built
  right after piece 1's short form; the **Workflows page** is wave 2. Both wait on the reworded
  principle of [DESIGN.md](../../../DESIGN.md) being agreed. Part of [the proposal](README.md).
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

**Chosen: A.** It follows the shape the overview and Tasks already have, keeps the dock mode for
reading one issue beside a conversation, and moves workflows out of a dialog, which is the wrong
place for something a person writes and runs from. B was weighed and left out: a dock widened into
two columns is the page drawn smaller, and still has to give way to the conversation.

## The page and the dock

Two views of the same issues for two situations, not one view at two sizes:

| | The Issues page | The Issues tab (Workbench) |
|---|---|---|
| For | finding, choosing and working through many issues | keeping one issue in view while working in a session |
| Layout | list and detail side by side when the page is wide enough; list, then detail, when it is not | one column: the list or one issue, never both |
| The issue | piece 1 in full, the review block (piece 4), the run form (piece 3) | piece 1's short form: title and state, the work's one line, the primary action, the body |
| Leads to | the run's session, the task detail | *Open in Issues*, the same issue on the page |

What the tab does not draw, it leads to: an approval's *Review…* and *Open in Issues* both open the
page on that issue, with no filter changed. Nothing built for the page is first built smaller in the
tab.

## The Issues page

- **Rail**: *Issues* and *Workflows* below *Tasks*, highlighted like it; *Issues* carries no count
  (*Tasks* already counts what needs a person).
- **Across the workspace.** The list covers every project of the workspace (`issues::Across` exists
  for the overview's card); the issue on the right is the one picked, drawn in piece 1's order.
  Diffs open in place.
- **Narrow.** Below the width that holds both columns, the page shows the list, and picking an
  issue shows it alone with *Back*. *Back* returns to the list with its filters, search and scroll
  as they were, and the issue just read still selected.

### A row says whether to open it

Each row is enough to choose from without opening it:

- **the title**, the strongest line;
- **the project and the issue's number**, muted, since the list spans projects;
- **one line of work**, the next action's state in the Tasks page's words: *Waiting for approval*,
  *Running · Verify*, *Ended · failed*, *Done · pull request open*; the warning ink only for what
  needs the person. *No run recorded* is not drawn on the row, to keep rows without work quiet;
- **labels**, as many as fit after the rest, then a count.

### Filters

- **Open** and **Closed** stay a filter of their own, beside the others.
- **How the list knows a pull request's state.** One read per project, never per row: a connector
  read of the repository's pull requests in one call (`gh pr list --state all`, capped), matched to
  the tasks' branches, on opening the page, on *Refresh*, and on focus regained when older than a
  minute. The list's head says *read Xm ago*; a row not read draws no pull request line, and a
  project whose read failed is said beside the filter.
- **Progress**: *All*, *Needs attention* (the glossary's: waiting on a person, or ended on
  something nobody chose), *Running*, *Queued*, *Pull request open*, *No run recorded*. *Pull
  request open* is a done run whose pull request waits on a person on the forge; it is its own
  choice, so *Needs attention* keeps the glossary's meaning. A run waiting for its pull
  request's status checks is *Running*: it waits on the forge, not on the person, and never counts
  as *Needs attention*.
- **Project** and **label**.

### The list does not move under a person

- A run changing state updates its row where it is. The selected row is never moved, re-sorted
  away or dropped while it is selected.
- Under a progress filter, an issue that stops matching (approved under *Needs attention*, say)
  stays in the list, its line saying where it went (*now Running*), until another issue is picked
  or the filter changes. Then it leaves.
- A new issue that comes to match a filter is added below what is on screen, never above the
  selected row.

### Coming back

The page's state (the selected issue, filters, search, scroll, and an open review's reading
position) lives on the page's entity, not in the render. Opening the run's session, a diff or the
task detail and then picking *Issues* in the rail finds the page as it was left. From a run's session, the rail's `auto · #N`
pill is the way back to its issue: it opens the issue on the page (open question 1 of
[issue-progress.md](issue-progress.md#open-questions), answered yes once the page exists).

## The Workflows page

The list as Settings has it, *Run…* on each, and the editor
as a list of steps, each showing its prompt, its gates or command, what it keeps, and where a
failure sends it back (`on_fail`), with the arrows drawn in the margin. No canvas, no dragging
between lanes. Settings ▸ Workflows goes; the project check commands move to the project page, beside *Run
check*, where the command is run. *Run…* opens the one launcher the rail and the keymap command open
(asking for the project first, below); the rail keeps its way in, for a start from inside a
session, and no second form is built.

Glossary: no new words; *page* is not a glossary term.

## What the pages have to settle

- **Which project a run starts on.** A page across the workspace has no project on screen. An
  issue's *Run workflow…* uses the issue's project. A workflow's *Run…* asks for the project first,
  defaulting to the one selected in the rail, and the launcher then opens on it as today.
- **An unsaved workflow when the page changes.** The editor's draft lives on the page's entity, not
  in the render, so leaving for another page and coming back finds it as it was. Closing the window
  or picking another workflow with changes unsaved asks, in a modal, whether to drop them.
- **One source for the page and the dock, which is new work.** Today each `IssuesView` owns its
  `RootIssues`: the issues, the sync and its own selection and draft. A second view needs the data
  and the sync lifted into one owner in the app per issues file, while selection, filters and
  drafts stay with each view. That refactor is laid out in
  [architecture.md](architecture.md#who-owns-the-issues) and lands before the page, in its PR.
  Issues are named by `IssueKey` (the issues file and the number), never by project and number.
- **Closed issues.** `issues::open_across` lists open issues only, counting closed ones, for the
  overview's card. The page's *Closed* filter needs a reader of its own: the same walk with the
  state as an argument, capped and saying what it left out.

## Questions for the decision

1. ~~Is *Chat is the centre while a session shows* the principle you want?~~ Yes, with A, worded
   in decision 2 of [the proposal](README.md#decisions-to-take-before-wave-1).
2. ~~Does the Issues Workbench mode stay once the page exists?~~ Yes, in the role above.
3. ~~Where do project check commands live once Settings ▸ Workflows goes?~~ On the project page,
   beside *Run check*.

## Done when

The Issues page:

- From the list alone, with twenty issues across two projects, the ones that need the person are
  told apart from the ones running and the ones with no run recorded, without opening any.
- Approving an issue picked under *Needs attention* leaves it in the list, saying *now Running*,
  until another is picked; the selected row never moves while a run under it changes state.
- Opening the run's session from an issue, then *Issues* in the rail, shows the same issue, filters,
  search and scroll. The same after opening a diff or the task detail.
- Narrowed below two columns, the page shows the list, then the issue with *Back*; *Back* restores
  the list as it was. At the largest zoom step a row still shows its title and its line of work.
- In the tab, *Review…* and *Open in Issues* open the page on that issue.

The Workflows page: as the list above says; its checks are written when it is next in line.

## Documents to change when built

- `DESIGN.md`: principle 1, *Layout* (rail rows, pages), *Docks* (the Issues mode's role), Settings.
- `docs/workflows.md`: where workflows are written.
- `docs/unattended.md`: where an issue's runs are seen.
