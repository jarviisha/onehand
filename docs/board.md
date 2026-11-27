# Board

A project's board: the issues of the project's own tracker laid out as work, and a team of tasks
that pulls them one by one. A person talks an issue through with an agent, moves it to *To do*,
and a task of the board's workflow starts on it when the team has room.

A board is set up per project, in the project's settings, and exists only for a project that runs
on this machine. A project on a host has none: its issues are not kept here.

## What a person gets

- **Making a board.** *Settings ▸ Projects ▸ Board*, with no board, has a *Workflow* menu of the
  library's workflows (*Board team* is the built-in made for a board): picking one makes a board of
  To do and one column running it. A workflow that works in the checkout cannot run on a board,
  since several tasks work at once and a checkout holds one; the reason is said under the menu
  and nothing is made (`board::usable`). *Team* sets how many tasks work at once, 1 to 8, 2 at
  first.
- **Its columns** are defined in the same box: a row each, moved up and down, edited in the column
  dialog (its name, a person's or a workflow's, when a workflow column starts, how an issue moves
  on, a limit), removed while empty, and added with *Add column…* (`Shell::save_column`, then
  `BoardDrive::{add_column, edit_column, move_column, remove_column}`). A name is a column's own,
  whatever its case; a limit is a whole number of at least 1; a workflow column's workflow must be
  on offer and run on a worktree; a column turned to a person's while a task works there is
  refused. Each refusal stays in the dialog. The board page still draws the one shape a board is
  made in.
- **Removing it.** *Remove the board…* asks first (*Remove the board from <project>?*): its columns
  and To do go, the issues stay open in no list, and tasks already started run on as the person's
  own; a board made later starts with none of them.
- **Editing.** The board's head has *Edit workflow*, and a step's column a `⋯` with *Edit step*:
  both open the Workflows page on the board's workflow, the second with that step open, asking
  first when the form there holds changes not saved. *Settings ▸ Projects ▸ Board* has an *Edit
  workflow* on each workflow column's row. A built-in workflow opens read-only, with its
  *Duplicate to edit*; that duplicate, saved for the first time, takes the place of the workflow
  it copies wherever the board runs it, all else kept, and a toast says *<project>'s board now uses <copy name>*. Only a copy of a built-in
  workflow rebinds: one of the person's own is edited in place, and a copy of it leaves the board
  as it is. No other board changes, and the page forgets the board on any save, once another
  workflow is opened, or when the form is closed.
  A board whose workflow is gone draws no *Edit workflow*.
- **The page.** *Board* in a project's dots menu, and the link on the project's page (*Board · N
  running · M to do*), open it. Columns side by side: *Backlog*, *To do*, a
  column per step of the workflow, *Done*. They scroll sideways (a wheel over the gaps between
  them or over their heads, or the scrollbar along the page's foot), and each column scrolls on
  its own: a wheel over a column taller than the page moves only its rows. Each column draws at most `BOARD_SHOWN` rows and says
  how many more it left out. A task's row opens it on the Tasks page, and its `⋯` and right-click
  offer *Show task*, then what the Tasks page's row offers it but *Dismiss* (*Open session*,
  *Stop*, *Resume*, *Retry…*, as its state allows), then, while it does not work, *Remove from
  board*: its issue goes back to the Backlog and the task stays as history (`BoardDrive::remove`). Under its title it says *Needs attention*, *Running*
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
  dropped on the Backlog leaves To do (`BoardDrive::place_todo`). A drop that changes nothing
  writes nothing, and the board pulls after one that does. A task's row does not drag.
- **A step's column** holds the tasks the board's work column names that are at that step,
  however their last run came out: working, ended and needing attention, or stopped. A task whose step left the workflow gets a column after the others, named by the
  step's id. A step with a limit says it in its head, `<label> · <n> of <limit>`, with n the
  tasks holding room there. **Done** holds the finished ones the board keeps, newest first, 50 at
  most (`DONE_KEPT`). A column can run its own
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
- **A task out of time gives its place back** once its agent is at rest: the turn cancelled when
  its time ran out has ended, and the work it left (a build in the background, a subagent, what
  the agent says once they are done) is over. Until then it still counts toward the team. At rest
  it also lets go of a limited step's room, holds its issue, its worktree and its place, stays in
  its step's column under *Needs attention*, and the board pulls (`RunDrive::gives_place`,
  `TasksLink::holds_team`).
- **Extend on a board task waits for a team place** when the team is full: the task reads *Waiting
  for a team place* and is queued, not needing attention. Places go to waiting extends first, the
  one asked first first, and **no To do issue starts while one waits** (`board::extends_given`,
  `board::extends_waiting`, the look's `GivePlace`, which counts starts on their way, and
  the reservation in `BoardDrive`'s look). Given a place, it asks
  room at a limited step again, then carries on in the same session. An Extend pressed before the
  run rests goes on at once, in the team place and the step's room it still holds, and so does one
  whose project no longer has a board (`board::extend_way`).
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

- **A run that ends done moves its task to Done**, and Done closes the issue when the project's
  own tracker keeps it. An issue kept in step with a forge is left open: the pull request's merge
  closes it there (`board::after_end`). Nothing closes it again at a later start: a person may have
  reopened it.
- **A task a person stopped or took over stays in its step's column**, holding its issue, and its
  place on the team frees, so the board pulls. A task that ended without finishing (failed, ran
  out, timed out, or lost its agent or session) stays there too, under *Needs attention*, until the
  person acts on it. **A task leaves the work column only by *Remove from board***, or by being
  done, and while it stands there it cannot be dismissed: the Tasks page refuses it, saying to
  remove it from the board first. A task in Done is history like any other: dismissed, or let go
  of by the history cap, it is no longer drawn there.
- **A task off the board is the person's.** One removed from it, or one in Done, that a person
  retries or resumes runs as any task: its end moves nothing on the board and closes nothing.
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
- a board task out of time comes to rest (`Effect::Rested`), or Extend is pressed on one;
- the tasks are read at boot, and whenever the workflow library is read: at boot and on every
  reload after a workflow is saved or deleted, by a person or a tool, since a board's workflow
  may only now be on offer.

And only while a window holds the project; there is none to start a task from otherwise. Opening
a window on the project does not pull (see [known-gaps.md](known-gaps.md)).

A run waiting for room still counts toward the team: it holds a worktree and a place. Step limits
only narrow the pull, never raise the team size.

The starts on their way, issues taken out of To do whose task is not yet kept, are counted as
running so the team is not overfilled. They live in `BoardDrive` (`BoardDrive::starting`, let go of as each lands) and
are never written.

## What is kept

- **`board.toml`**, beside the project's `project.toml` in its folder, holds the board, written
  whole: its `schema_version`, `team`, and its columns, To do holding the issue numbers in order
  and the work column the workflow's id (`board::Board`, read by `board::file`, edited by
  `BoardDrive::{bind, unbind, set_team, add_todo, move_todo, place_todo, drop_todo}`). A team size
  changed by the track is written once the changing stops, and at quit. *No board* removes the
  file. A file of a newer version, or of a shape this build does not know, is not read: the board
  page and Settings say why, and nothing writes over it.
- **What is on the board is what its entries name.** The work column's entries name a task and
  the run the board asked for, Done's the task. A start is written before it is asked: the issue
  leaves To do for the work column in a write, and its task starts only once that write lands; a
  write that fails puts the issue back and starts nothing. An entry naming a task that is not
  there, and whose start is not on its way, is kept, to read as a start that did not land: a
  person starts it again, and nothing starts it by itself.
- **A board task** is an ordinary task with `Origin::Board { project, issue }`, kept like any
  other. Where it stands in the work column comes from its last run (`board::columns`). The
  tasks the board's columns name are held: the history cap passes over them, the cap applied as the
  tasks are read included (`TasksDrive::held`, told by the board's drive, `Effect::Held`).
- **A board written before its columns named their tasks** (version 1) takes them up the first
  time the tasks are read: those on it in the work column, oldest first, the done ones in Done,
  newest first (`BoardDrive`'s `TasksRead`), and is written as version 3. The Backlog leaves out
  the issues its entries name and those of its done tasks no longer in Done, until they are
  dismissed or capped.

**Columns of any shape** are kept and decided in core, though the page and Settings still show
the one shape a bound board has (version 3; versions 1 and 2 read as they were). A column is a
person's or runs a workflow; a workflow column starts on entry or on a press, and a column moves
an issue ready to leave it on by itself or waits for a person; it may hold a limit. An issue is
ready in a person's column at once, and in a workflow column once its run there ended done
(`Entry::done`). A look (`board::flow::look`) drops the closed issues no task works, moves the
ready ones on, from the last column to the first, into a column with room (one that starts on
entry only with a place on the team and room at its workflow's first step), and starts what waits
in a column that starts on entry, within the same room. An issue entering a workflow column with
a task already working it gets a next run of that task (`TasksDrive::next_run`); one with none,
a new task. A start pressed goes ahead past a full team, which caps only what the board starts by
itself. A run that ends done marks its entry done, and the look its read brings moves it on;
entering Done closes the issue. Each row's line is core's (`board::line`).

**Changing the workflow** for another leaves the tasks the old one started on the board: they
still hold their issues out of the Backlog, count as running against the team, and stand in
columns named by their step's id when the new workflow has no such step, until they end and a
person removes them from the board. *No board* lets them go.

**A restart** keeps To do, which only holds numbers, and pulls it again once the tasks and the
library are read. A task that had started comes back cut off, under *Needs attention*, for the
person's Resume; it is not counted as running, so the team has room for another.

## An older build

A board an earlier build kept in `project.toml` is moved into `board.toml` at the first start of
this build, before any project is read (`board::file::migrate_blocking`): the file is written
first, and the key dropped only then, so a move that fails keeps the key, which every write of the
project keeps until the next start tries again. A board file already there wins, and the key goes.

An older build that knows the board sees none once the key is gone, and never touches
`board.toml`; To do changed in it meanwhile is lost to this build. A build that does not know the
board at all skips the file of a board task it cannot read (`Origin::Board` is unknown to it). The
issues, which are the project's own, are untouched.
