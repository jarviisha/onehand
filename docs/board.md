# Board

A project's board: the issues of the project's own tracker laid out as work, across columns a
person defines. A column is a person's or runs a workflow on each issue in it. One issue crosses
the columns from left to right with one task, so its worktree, its runs and their history go
with it. A person talks an issue through with an agent, puts it in the first column, and the
board starts runs and moves the issue on as each column says.

A board is set up per project, in the project's settings, and exists only for a project that runs
on this machine. A project on a host has none: its issues are not kept here.

## Columns and entries

A board is its columns, in order, between the **Backlog** and **Done** (`board::Board`,
`board::model`). Neither end is a column: the Backlog is not kept, and Done is a list of its own.

- **A column** (`board::Column`) has an id of its own, stable across a rename, a name, who works
  it (`Work::Person`, or `Work::Workflow` naming a workflow by id), when its run starts
  (`Starts::OnEntry` or `Starts::OnPress`, read only on a workflow column), how an issue ready to
  leave it goes (`MovesOn::ByItself` or `MovesOn::ByPerson`), and an optional limit of at least 1:
  how many issues it holds at most, those waiting to move on counted.
- **An entry** (`board::Entry`) is one issue's place in a column, in the person's order: the issue,
  the task working it once a workflow column took it, the run the board last asked for, when it
  entered the column, and whether its run there ended done. An issue is on one entry at most.
- **A new board** is made with two columns (`Board::made`): *To do*, a person's column moving its
  issues on by itself, then *Work*, running the workflow picked, starting on entry and moving on by
  itself. Its team is 2. Anything past that is the person's to define.
- **The Backlog** is the project's open issues a brief can be made from (`Issues::workable`) that
  no entry names and no done task of the board holds until it is dismissed or let go of as
  history (`board::backlog`, `board::taken`), so an issue kept in step with a forge whose done task
  fell off Done does not come back.
- **Done** holds the board's finished issues, newest first, 50 at most (`DONE_KEPT`).
- **What the board allows** is core's (`Board::any_shape`): at least one column, ids unique, names
  said and not used twice whatever their case, limits of at least 1, an issue on one entry, and a
  run or a finished run only in a workflow column. A change that would leave the board otherwise
  is refused.

## Starting and moving on

**A look** (`board::flow::look`, carried by `BoardDrive::look`) decides it all at once, over the
issues as just read:

- An issue in a column, closed and worked by no task, is dropped from it, with no word: that is
  what closing it asks. One with a task stays.
- **An entry is ready to move on** at once in a person's column, and in a workflow column once its
  run there ended done (`Entry::done`). A column that moves on by itself moves each ready entry
  into the next, or into Done from the last column. The columns are taken last first, so the room
  a move leaves is seen by the column before it, and an entry moves at most once a look.
- **A column takes an entry while it holds fewer than its limit.** A move by itself into a column
  that starts on entry also needs a place on the team and, when the workflow's first step is
  limited, room there (a run waiting for room counted, and a start on its way). Without them the
  entry waits where it is, saying so.
- **What waits in a column that starts on entry is started**, within the same team and room.
- **An issue entering a workflow column** gets a new task when no task works it, or a next run of
  its own task when one does ([workflows.md](workflows.md#starting-queueing-and-resuming)). The run
  is of that column's workflow, from its first step, on the task's worktree.
- **Moving on by a person**: an entry done in a column that moves on by a person waits there, its
  row offering the move.

**A person** acts on an entry from its row (below), by menu or drag, one rule for both:

- **A move** (`BoardDrive::place`) puts the issue in another column, before a row there or last,
  or in Done. It is refused while its task works (*It is at work: Stop it first*), while its start
  is on its way, and into a full column (*<column> is full*). A person's move is taken on the limit
  alone: an issue moved into a column that starts on entry then waits there for the team. Within
  its own column an issue keeps what it did there; into another it starts afresh, its task kept.
  An issue moved back into a workflow column it already passed starts there as any entry does,
  with a next run on the same worktree, its commits and pull request as they are. Every move is
  followed by a look.
- **Start** (`BoardDrive::start`), on an entry of a column that starts on a press, or on one whose
  start did not land, goes past a full team, which caps only what the board starts by itself. It
  makes a new task when none works the issue or its task was never kept, else a next run.
- **Taking it off the board** (`BoardDrive::remove`): *Back to Backlog* for an issue no task works,
  *Remove from board* with one. The entry goes, from a column or from Done; the task stays as
  history. Refused while its task works or its start is on its way.

**A start is written before it is asked for.** The look, or the press, writes the entry with its
task and run first, and the start is asked only once that write is answered; a write that fails
puts the board back and starts nothing. While it is on its way the start is counted as running
(`BoardDrive::starting`), held in memory and never written.

- **A start refused** (no brief, no worktree, no place, another branch checked out, its worktree
  removed, no window holding the project) before a run was made is said in a notification, *Could
  not start #N from the board: <why>* (`Signal::NotStarted`), and its row reads *Did not start*,
  offering *Start* again. Nothing starts it by itself.
- **A task that could not start once made** (no agent, no session) fails as an unwatched task
  does, said by its own start and no second notification. It holds its issue in its column.
- **An entry naming a task that is not there**, with no start on its way, is kept: it reads as a
  start that did not land, for a person to start again.

**The team** is how many of the board's runs may work at once, 1 to 8 (`Board::team`,
`board::running`). A run waiting for room still counts: it holds a worktree and a place. Step
limits only narrow a look, never raise the team.

- **A task out of time gives its place back** once its agent is at rest: the turn cancelled when
  its time ran out has ended, and the work it left (a build in the background, a subagent, what
  the agent says once they are done) is over. Until then it still counts toward the team. At rest
  it also lets go of a limited step's room, holds its issue, its worktree and its column, stays
  under *Needs attention*, and the board looks again (`RunDrive::gives_place`,
  `TasksLink::holds_team`).
- **Extend on a board task waits for a team place** when the team is full: the task reads *Waiting
  for a team place* and is queued, not needing attention. Places go to waiting extends first, the
  one asked first first, and nothing the board starts by itself starts while one waits
  (`board::extends_given`, `board::extends_waiting`, `Effect::GivePlace`). Given a place, it asks
  room at a limited step again, then carries on in the same session. An Extend pressed before the
  run rests goes on at once, in the team place and the step's room it still holds, and so does one
  whose task no column holds (`board::extend_way`).

## When a run ends

- **A run's end is the board's** when an entry in a workflow column names its task with a run
  asked for there. A Retry or a Resume from the Tasks page is another run of the same task, still
  the column's. The end of a task no such entry names (one in a person's column, in Done, or off
  the board) is not the board's: it moves nothing and closes nothing.
- **A run that ends done** marks its entry done, and the look that follows moves it on as its
  column says.
- **Entering Done closes the issue** when the project's own tracker keeps it, by a move by itself
  or by a person. An issue kept in step with a forge is left open: the pull request's merge closes
  it there (`board::after_end`). Nothing closes it again later: a person may have reopened it.
- **A task a person stopped or took over stays in its column**, holding its issue, and its place on
  the team frees, so the board looks again. A task that ended without finishing (failed, ran out,
  timed out, or lost its agent or session) stays there too, under *Needs attention*, and the board
  does not look on its account: a cause the tasks share (an agent gone, a broken workflow, a
  failing check) would most likely end the next the same way (`board::pulls_next`).
- **A board task's end, like a schedule's, is unwatched:** it fails without a person looking, so
  it says what it came to on the Tasks page and nothing asks for a window.
- **A done board run closes its session**, as a schedule's does (`Task::closes_session`), and its
  conversation stays under past conversations. One that did not finish keeps its session in the
  rail.

## When it looks

A look happens when:

- a column, an entry or the team changes, or a board is made;
- a start lands, a run of the board ends done, or a person stopped or took one over;
- room frees at a step, whoever held it (`RoomFreed`);
- a board task out of time comes to rest (`Effect::Rested`), or Extend is pressed on one;
- the tasks are read at boot, and whenever the workflow library is read: at boot and on every
  reload after a workflow is saved or deleted, by a person or a tool, since a column's workflow may
  only now be on offer.

Moves into a person's column or into Done go with no window. A start goes only while a window
holds the project; there is none to start a task from otherwise. Opening a window on the project
does not look (see [known-gaps.md](known-gaps.md)).

## The page

*Board* in a project's dots menu, and the link on the project's page, open it. Its head says the
project and the team, with *Edit columns*, which opens the project's settings. The columns stand
side by side: *Backlog*, the board's own in their order, *Done*. They scroll sideways (a wheel over
the gaps between them or over their heads, or the scrollbar along the page's foot), and each
column scrolls on its own: a wheel over a column taller than the page moves only its rows. Each
column draws at most `BOARD_SHOWN` rows and says how many more it left out. The board draws every
column. A column's head is its name and how many it holds (*n of limit* with a limit); its `⋯`
offers *Edit column…*, the column dialog, and *Edit workflow* on a column whose workflow is on
offer.

**A row** is an issue's until a task works it, then the task's, which it opens. What it says
under its title is core's (`board::line`, its `Line`'s words):

| Where it stands | Its line |
|---|---|
| In a person's column | `#N · <age>`, since it entered |
| In a column that starts on a press, not started | *Waiting for Start* |
| In a column that starts on entry, not started | *Waiting for a team place* |
| Its start on its way | *Starting* |
| Its run here under way, or ended | the Tasks page's words for its task |
| Done here, moved on by a person | *Done here* |
| Done here, the next column full | *Waiting for room in <column>* |
| Its start never landed | *Did not start*, in the warning ink |
| Not started, its column unable to start it | why (*Its workflow, <id>, is gone*, or `board::usable`'s reason), in the warning ink |

What it offers is core's too (`board::offers`), each with why it refuses:

- **The act its line names**, drawn as a quiet button beside its `⋯` and first among its moves:
  *Start* (waiting for Start, or did not start), or *Move to <column>*, *Move to Done* (done here,
  moved on by a person). `board::line::waiting` counts these.
- **The moves**: to the next column, back to the one before, to Done, to each other column, then
  *Move up* and *Move down* in its column, refusing at its ends.
- **Back to Backlog**, or **Remove from board** with a task, in the danger ink.
- A task's row offers the Tasks page's acts first, *Show task* leading, but *Dismiss*: a task the
  board holds is let go of from the board.

Start, every move and taking it off the board refuse while its task works (*It is at work: Stop it
first*) or its start is on its way (*It is starting: wait for it to start*); a move into a full column refuses; a start its column
cannot make refuses with its reason. The moves stay listed, so the menu keeps its shape. A
Backlog row offers *Move to <first column>*, refusing while that column is full; a Done row offers
only *Back to Backlog* or *Remove from board*. A press goes through `BoardDrive::{place, start,
remove}`, and what refuses it is said in a notification.

**Dragging** does what the row menus do, which stay for the keyboard. Every row but one whose task
works, and those of Done, drags: dropped on a row it lands just before it, dropped elsewhere on a
column it lands last, dropped on the Backlog it leaves the board, and dropped on Done it goes to
Done. A drop that changes nothing writes nothing (`Board::placed`). A refused drop says why in a
notification.

## Setting it up

*Settings ▸ Projects ▸ Board*:

- **Making a board.** With no board, a *Workflow* menu of the library's workflows (*Board team* is
  the built-in made for a board): picking one makes a board of To do and a column running it
  (`BoardDrive::bind`). A workflow that works in the checkout cannot run on a board, since several
  tasks work at once and a checkout holds one; the reason is said under the menu and nothing is
  made (`board::usable`).
- **Its columns**, a row each in order, at most 20 listed: its name over who works it (*A person*,
  *Runs <workflow>*, *(gone)* after the id of a workflow no longer on offer) and how it starts,
  moves on and holds where that differs from the first choice. Each row has *Move up* and *Move
  down*, an edit mark opening the column dialog, *Edit workflow* while its workflow is on offer, and
  *Remove*, which removes an empty column without asking and refuses one holding issues (*Move its
  N issues out first*). *Add column…* opens the dialog empty. These go through
  `BoardDrive::{add_column, edit_column, move_column, remove_column}`.
- **The column dialog** (`crates/app/src/dialogs/column.rs`, read by `Shell::save_column`):
  *Name*; *Work*, *A person* or *A workflow*, the second with its *Workflow* menu; *Starts*, *When it
  enters* or *When Start is pressed*, with a workflow; *Moves on*, *By itself* or *A person moves
  it*; *Limit*, empty for none. *Add* or *Save* refuses, saying why in the form, which stays: no
  name, a limit that is not a whole number of at least 1, no workflow picked, a workflow not on
  offer or one a board cannot run, a name another column has, and a column turned to a person's
  while a task works there or a start is on its way. A column that stops running a workflow
  forgets what each of its issues did there. A workflow changed for another reaches only runs that
  start after; a run under way keeps its own.
- **Team** sets how many runs work at once, 1 to 8. Lowering it stops none. A change of the track
  is written once the changing stops, and at quit.
- **Removing it.** *Remove the board…* asks first (*Remove the board from <project>?*): its columns
  and their issues go, the issues stay open in no list, and tasks already started run on as the
  person's own. A board made later starts with none of them (`BoardDrive::unbind`).
- **Editing a workflow.** A column's *Edit workflow*, on the page or in Settings, opens the
  Workflows page on that workflow, asking first when the form there holds changes not saved. A
  built-in workflow opens read-only, with its *Duplicate to edit*; that duplicate, saved for the
  first time, takes the built-in's place in every column of that board that runs it, all else kept,
  and a toast says *<project>'s board now uses <copy name>* (`BoardDrive::bind_copy`). Only a copy
  of a built-in workflow does: one of the person's own is edited in place, and a copy of it leaves
  the board as it is. No other board changes, and the page forgets the board on any save, once
  another workflow is opened, or when the form is closed.

## Across the columns

- **A task the board holds** is one an entry in a column names (`Boards::held`, told to the tasks
  as `Effect::Held`): the history cap passes over it and it cannot be dismissed. One in Done is
  history like any other. See [tasks.md](tasks.md).
- **Handing over.** The code goes by the worktree, which the task owns. Words go by one prompt
  variable, `{handover}`: what the task's run before this one last answered
  ([workflows.md](workflows.md#prompts)). A person editing the work between two columns is seen as
  a retry sees it: the new run's first mark records the work as it now is.
- **The task detail** labels each run by the column it ran in (*Run N · <column>*, `Run::label`),
  and its line, like its row on the Tasks page, starts *on the board at <column> · * while the task
  is not at work (`Boards::column_holding`), so a long ending cut short never hides it.
- **The project page's link** reads *Board · N running · M to do · K waiting for you*, each part
  left out at zero (*Board*, *Board · 2 to do*): N the board's runs holding a team place, M the
  first column's issues no task works yet, K the entries waiting on a person, those offering an act
  (`board::line::waiting`). These waits are the board at rest by the person's design, not
  something gone wrong: they are not *Needs attention* and are not counted on the rail.
- **A worktree is never removed unasked**: *Remove worktree…* after the pull request merged, as for
  any task. An issue entering Done, or leaving the board, removes nothing.

## What is kept

- **`board.toml`**, in the project's folder beside its `project.toml` (`projects/<id>/`), holds
  the board, written whole (`board::file`): its `schema_version`, `team`, its columns with their
  entries in order, and Done. *Remove the board…* removes the file. What is in memory only: the
  starts on their way and the drive's view of the tasks.
- **A file of a newer version, or of a shape this build does not know**, is not read: the board
  page and Settings say why, and nothing writes over it (`BoardLoad::Unreadable`).
- **A board task** is an ordinary task with `Origin::Board { project, issue }`, kept like any other.
  Where it stands is the board file's alone; its run keeps the column's name it started in
  (`Run::column`) and its hand-over (`Run::handover`).
- **A restart** keeps every column and entry, and looks again once the tasks and the library are
  read. A run that was under way comes back cut off, under *Needs attention*, for the person's
  Resume; it is not counted as running, so the team has room for another. Nothing resumes by
  itself.

**Earlier versions** of the file are still read (`Board::shape`):

- **Version 1**, a board before its entries named their tasks (To do and one work column, no
  entries past To do), takes its tasks up the first time the tasks are read: those on it in the
  work column, oldest first, and the done ones in Done, newest first (`BoardDrive`'s `TasksRead`).
  It is then written as version 3. Until then it starts nothing.
- **Version 2**, the same two columns with entries naming their tasks, is read as version 3 at
  once (`Board::read_up`): it is a version 3 board in one shape, and changes as any does.
- **Version 3** is any board of the shape above.

**A board an earlier build kept in `project.toml`** is moved into `board.toml` at the first start of
this build, before any project is read (`board::file::migrate_blocking`), as a version 1 board of
To do and one work column. The file is written first, and the key dropped only then, so a move
that fails keeps the key, which every write of the project keeps until the next start tries again.
A board file already there wins, and the key goes.

An older build that knows the board only in `project.toml` sees none once the key is gone, and never
touches `board.toml`; To do changed in it meanwhile is lost to this build. A build that reads an
earlier version of the file finds a newer one unreadable and leaves it as it is. A build that does
not know the board at all skips the file of a board task it cannot read (`Origin::Board` is unknown
to it). An older build saving a task drops `Run::column` and `Run::handover`. The issues, which are
the project's own, are untouched.

## Not built

- **Agent and Command columns.** A column is a person's or runs a saved workflow; a one-step
  column made on the spot, as the project's check is, is not offered.
- **One board per project**, and none spanning projects or repositories. A project on a host has
  none.
- **Several runs at once on one issue.** A task is at one step of one run at a time; a move is
  refused while its run works.
- **No move log.** When an issue entered its column is kept; the time it spent in each before is
  not.
- **No named outputs across workflows.** `{handover}` is the only word one column's run hands the
  next; `{output.<id>}` reaches only the run's own steps.
- **No branching.** An issue moves on to the next column only; the column order and a person's move
  cover the rest.
- **No agent runs the board.** Starting, team size, limits and moving on are the engine's, decided
  in core; no coordinator agent dispatches, and no rule lives in a prompt.
- **Only a person puts an issue on the board.** An agent writes into the Backlog with
  `create_issue`; no tool moves an issue past it.
- **The board's tasks are the ones it started.** A task started any other way (the launcher, a
  schedule, `start_issue`, *Run check*) is not on the board, even when it runs a column's
  workflow, and its issue stays in the Backlog.
- **No column merges.** The end of the line is a pull request or an approval a person gives;
  merging is the person's, or a later forge step's.
