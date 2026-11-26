# Board

A project's board: the issues of the project's own tracker laid out as work, and a team of tasks
that pulls them one by one. A person talks an issue through with an agent, moves it to *To do*,
and a task of the board's workflow starts on it when the team has room.

A board is set up per project, in the project's settings, and exists only for a project that runs
on this machine. A project on a host has none: its issues are not kept here.

## What a person gets

- **Binding.** *Settings ▸ Projects ▸ Board* has a *Workflow* menu: *No board* and the library's
  workflows (*Board team* is the built-in made for a board). A workflow that works in the checkout cannot be bound, since several tasks work at
  once and a checkout holds one; the reason is said under the menu and nothing is bound
  (`board::usable`). Once bound, *Team* sets how many tasks work at once, 1 to 8, 2 at first.
  Choosing *No board* on a board whose To do is not empty asks first (*Remove the board from
  <project>?*): its To do goes, the issues stay open in no list, and tasks already started run on.
- **Editing.** The board's head has *Edit workflow*, and a step's column a `⋯` with *Edit step*:
  both open the Workflows page on the board's workflow, the second with that step open, asking
  first when the form there holds changes not saved. *Settings ▸ Projects ▸ Board* has the same
  *Edit workflow* beside its menu. A built-in workflow opens read-only, with its *Duplicate to
  edit*; that duplicate, saved for the first time, becomes that board's workflow, team and To do
  kept, and a toast says *<project>'s board now uses <copy name>*. Only a copy of a built-in
  workflow rebinds: one of the person's own is edited in place, and a copy of it leaves the board
  as it is. No other board changes, and the page forgets the board on any save, once another
  workflow is opened, or when the form is closed.
  A board whose workflow is gone draws no *Edit workflow*.
- **The page.** *Board* in a project's dots menu, and the link on the project's page (*Board · N
  running · M to do*), open it. Columns side by side, scrolling sideways: *Backlog*, *To do*, a
  column per step of the workflow, *Done*. Each column draws at most `BOARD_SHOWN` rows and says
  how many more it left out. A task's row opens it on the Tasks page, and its `⋯` and right-click
  offer *Show task* and then what the Tasks page's row offers it (*Open session*, *Stop*, *Resume*,
  *Retry…*, *Dismiss*, as its state allows). Under its title it says *Needs attention*, *Running*
  or *Queued* as the Tasks page sorts it, then what it waits on or why it ended in its detail's
  words (*Approving starts <step>*, *The agent asks something in its session*), or *Waiting for
  room at <step>* alone when its step is full.
- **Backlog** is the project's open issues a brief can be made from (`Issues::workable`) that
  are neither in To do nor held by a task of the board (`board::backlog`). A person moves one to
  To do, and back.
- **To do** is in the person's order: first pulled first. A person moves an issue up or down in
  it, or back to the Backlog.
- **Dragging** does what the row menus do, which stay for the keyboard: an issue of the Backlog or
  To do dropped on a To do row lands just before it, dropped elsewhere on To do lands last, and
  dropped on the Backlog leaves To do (`ProjectsDrive::place_todo`). A drop that changes nothing
  writes nothing, and the board pulls after one that does. A task's row does not drag.
- **A step's column** holds the board's tasks at that step: working, or ended and needing
  attention. A task whose step left the workflow gets a column after the others, named by the
  step's id. A step with a limit says it in its head, `<label> · <n> of <limit>`, with n the
  tasks holding room there. **Done** holds finished ones, newest first. A column can run its own
  agent: the step names one, and its task works that step in a session of that agent. A column
  whose step is a review sends tasks back on its own: its agent approves, or returns the task to
  an earlier column with a note, and the task is drawn there again.
- **Pulling.** When the team has room, and the workflow's first step has room when it is limited
  (a task waiting for room there is counted, as is a start on its way), the first issue in To do
  starts a task of the board's workflow, the brief made from the issue
  (`issues::supervised_brief_blocking`). The task is made
  when the issue is pulled, started off screen as a schedule's is (`task::start_off_screen`), so
  an issue in To do is not a task and is not on the Tasks page. The Tasks page's row reads *from
  the board · <workflow>*.
- **A refused pull** before its task is made (no brief, no worktree, no place) puts the issue back
  in the Backlog, with a toast naming it and the reason. It is not retried, and nothing is pulled
  in its place, so a reason every issue shares is said once.
- **A task that could not start once made** (no agent, no session) fails as an unwatched task does,
  said by its own start and no second toast. It holds its issue in its step's column, and nothing
  is pulled in its place.
- **An issue closed or deleted** is dropped from To do at the next look, with no word: that is
  what closing it asks. One still open that cannot be worked now (one kept in step with a forge
  while the forge is not found) or that a task of the board still holds stays in To do, skipped,
  for a later look.

## Not built

- **One board per project**, and none spanning projects or repositories. A project on a host has
  none.
- **No agent runs the board.** Pulling, team size, step limits and sending back are the engine's,
  decided in core; no coordinator agent dispatches, and no rule lives in a prompt.
- **Only a person moves an issue to To do.** An agent writes into the Backlog with `create_issue`;
  no tool moves an issue past it.
- **The board's tasks are the ones it pulled.** A task started any other way (the launcher, a
  schedule, `start_issue`, *Run check*) is not on the board, even when it runs the board's
  workflow, and its issue stays in the Backlog.
- **No task is dragged between step columns.** A person acts on a board task as on any task: on
  the Tasks page, approving, sending back, Stop, Retry, Resume, taking over.
- **No step merges.** The end of the line is a pull request or an approval a person gives;
  merging is the person's, or a later forge step's.
- **A task is at one step at a time.** Several agents on a task means one per step, one after
  another, never several on one step of one task.
- **A workflow without a step's agent, a review or a limit runs exactly as before**, and so do
  the shipped workflows, the launcher, *Run check*, schedules and `start_issue`.

## When it ends

- **Done closes the issue** when the project's own tracker keeps it. An issue kept in step with a
  forge is left open: the pull request's merge closes it there (`board::after_end`).
- **A task a person stopped, took over or dismissed leaves the board**; its issue returns to the
  Backlog. A task that ended without finishing (failed, ran out, timed out, or lost its agent or
  session) stays in its step's column, under *Needs attention*, holding its issue until the person
  acts on it.
- **A board task's end, like a schedule's, is unwatched:** it fails without a person looking, so
  it says what it came to on the Tasks page and nothing asks for a window.
- **A done board task closes its session**, as a schedule's does (`Task::closes_session`): nobody
  started it by hand, so it leaves the rail, and its conversation stays under past conversations.
  One that did not finish keeps its session in the rail.

## When it pulls

`board::pull` decides, over what the carrier read: as many of To do as the team has room for, the
issues closed or gone dropped, and those not workable now or held by a task (`board::taken`)
skipped. `crates/app/src/board.rs` carries it out. A look
happens when:

- To do or the team changes, or a board is bound;
- room frees at a step, whoever held it, so a task of the board held at the first step by another
  workflow's run or by a task of no board still lets a pull go on (`RoomFreed`);
- a board task ends done, or a person stopped or took it over (`Signal::BoardEnded`, whose
  `pull` core decides). One that ended without finishing pulls nothing, by the owner's choice:
  a cause the tasks share (an agent gone, a broken workflow, a failing check) would most likely end
  the next the same way, one after another until To do is empty, so the board waits for a person
  instead;
- the tasks are read at boot, and whenever the workflow library is read: at boot and on every
  reload after a workflow is saved or deleted, by a person or a tool, since a board's workflow
  may only now be on offer.

And only while a window holds the project; there is none to start a task from otherwise. Opening
a window on the project does not pull (see [known-gaps.md](known-gaps.md)).

A run waiting for room still counts toward the team: it holds a worktree and a place. Step limits
only narrow the pull, never raise the team size.

The starts on their way, issues taken out of To do whose task is not yet kept, are counted as
running so the team is not overfilled. They live in `ProjectsDrive` (`start_todo`, `landed`) and
are never written.

## What is kept

- **`project.toml`** holds the board with the project: `workflow`, `team` and `todo`, the issue
  numbers in order (`project::Board`, edited by `ProjectsDrive::{set_board, set_team, add_todo,
  move_todo, take_todo}`). A team size changed by the track is written once the changing stops.
- **A board task** is an ordinary task with `Origin::Board { project, issue }`, kept like any
  other. Where it stands comes from its last run and the app's view of whether it works
  (`board::columns`, `board::taken`, `board::running`).

**Changing the workflow** (*No board*, then another workflow, or another picked over it) leaves
the tasks the old one started as they are: they still hold their issues out of the Backlog, count
as running against the team, and stand in columns named by their step's id when the new workflow
has no such step, until they end and a person dismisses them.

**A restart** keeps To do, which only holds numbers, and pulls it again once the tasks and the
library are read. A task that had started comes back cut off, under *Needs attention*, for the
person's Resume; it is not counted as running, so the team has room for another.

## An older build

A build that does not know the board drops `board` from `project.toml` the next time it writes the
project, and skips the file of a board task it cannot read (`Origin::Board` is unknown to it). The
issues, which are the project's own, are untouched.
