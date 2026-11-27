# Proposal: a board of its own columns

A proposal, not a requirement. It records the target architecture agreed so far for a board whose
columns a person defines, each doing its own work, with one piece of work carried across them,
and the interface agreed for it.
When a step is built, its behaviour moves into the owning documents and this file loses it.

| Step | What | Spec |
|---|---|---|
| 1 | The board's drive and file of its own, and the migration; nothing new on screen | [spec](../superpowers/specs/2026-11-27-board-columns.md) |
| 2 | Entries name the tasks: the ordered start, a stop keeping the entry, *Remove from board*, held tasks | same |
| 3 | Columns a person defines, in core: manual and workflow, starting, moving on, limits, a next run on the same task | same |
| 4 | Columns defined in Settings | same |
| 5 | The board page draws the columns | same |
| 6 | The hand-over and the history; the proposal moves into the owning documents | same |

## Why

Today a board is drawn from one workflow: a column per step (`board::columns`,
`crates/core/src/board.rs`), a task placed by the step its last run is at, and only To do kept
(`project::Board { workflow, team, todo }`, `crates/core/src/project.rs`). So the columns are
whatever the workflow's steps are, a step of a few seconds takes a column, a wait for a person
cannot be a column without a run held open for it, and a run's end is taken for the work's end: a
done run closes the issue (`board::after_end`), and a stop sends it back to the Backlog.

The aim: a board has columns a person defines, each with work of its own or none; a workflow is how
one column does its work, and its steps do not shape the board; one piece of work crosses the
columns keeping who it is, its code and its history.

## What the code already gives

- **A task already outlives a run.** `Task { brief, setup, runs, origin, … }`
  (`crates/core/src/task.rs`) keeps every run, and its place (checkout, or worktree and branch) is
  shared by all of them, so a later run sees the work an earlier one left.
- **Each run carries its own workflow snapshot** (`workflow::Run::template`), so nothing in a
  task's shape ties its runs to one workflow. Retry and *Retry with current settings* read the last
  run, so a retry of a column's run runs that column's workflow again.
- **A task runs one run at a time** and its place is locked per worktree (`task::queue`), so one
  piece of work never runs twice at once.
- **Ids are handed in.** A task's id and a run's id come with the command that makes them
  (`Task::retry(id, …)`), so a caller can record an id before the thing it names exists.
- **A one-step template made on the spot has precedent:** `Task::check`, a command step with no
  session, never saved.

## What breaks if nothing changes

- `task::history::over_cap` removes finished tasks past 200 a project. A board task whose last run
  is done reads as finished, so one waiting in a manual column could be removed.
- `Task::closes_session`, `board::after_end` and `Signal::BoardEnded` treat a done run as the end
  of the board's work.
- A task is cut its worktree only at its first start (`TasksDrive::start_off_screen`,
  `task/tasks/off_screen.rs`); there is no way to add a run to an existing task other than Retry
  and Resume, and `Run::retry_of` carries outputs over by comparing steps, which is wrong between
  two different workflows.
- `Signal::BoardEnded { project, issue, done, pull }` names no run, so an end that arrives after
  the work moved cannot be told from the current one.

## The model

### What carries the work across: the task

No new entity. The board keeps **where** each piece of work is; the task keeps **what was done and
on which code**.

- **The board** owns its columns (their order, work, limits, how work starts and moves on) and, in
  each column, an ordered list of entries. An entry is the working name for one issue's place on
  the board: `{ issue, task: Option<task id>, run: Option<run id> }`. It is the source of truth for
  where the work stands. "Entry" is not proposed for the glossary yet.
- **The task** is made the first time its issue enters a column that has work. Every later
  entering of a column with work adds **a new run to the same task**, with that column's workflow
  snapshot, starting at its first step and carrying no outputs. It is not a retry: a sibling of
  `Task::retry` that does not go through `Run::retry_of`.
- **A manual column makes no run.** An entry there is the board's data alone: no session held, no
  budget running, nothing a restart cuts off.
- **Before its first column with work**, an issue is a number in a column, as To do holds numbers
  today.

Rejected, with reasons, under [Considered and left](#considered-and-left).

### Columns

| | Kind | Holds | Meaning |
|---|---|---|---|
| Backlog | role | nothing stored | the tracker's workable open issues no column holds (`board::backlog`, as today) |
| A manual column | work: none | entries | a person does the work and moves the entry on |
| A workflow column | work: a workflow by id | entries | entering it may start a run of that workflow on the entry's task |
| Done | role | entries, newest first, capped | the board's work is over: the issue is closed as `board::after_end` decides today, and the task leaves the board |

- **To do becomes an ordinary manual column**: ordered, filled by a person, the next column pulling
  from it. Every column is ordered.
- **Agent and Command columns come later**, as a one-step template made on the spot, as
  `Task::check` is, so the engine runs them; whether that template is validated as a saved one is
  decided then.
- **A workflow column snapshots the workflow when its run starts.** An entry waiting there takes
  the version on offer at its start; a run under way keeps its own. A column whose workflow is gone
  or unreadable refuses starts and says why; its entries stay.

### Entering, starting, moving on

Three decisions, each a setting of the column:

- **Entering** a column is a move of the entry: by a person (drag or menu, one rule for both), or by
  moving on from the column before.
- **Starting**: a workflow column starts a run *on entry when there is room*, or *when a person
  presses Start*. Entering never has to mean running.
- **Moving on**: when its run ends done, the entry moves to the next column *by itself*, when that
  column has room, or *waits for a person*. An entry waiting for room stays in its column and says
  so. Only the next column; no branching rules.

What the existing acts mean on a board task:

| Act | Today | Proposed |
|---|---|---|
| Stop, Take over | the task leaves the board, the issue goes back to the Backlog | the run ends as it does; the entry stays in its column, offering Start again |
| Retry | a new run of the task's last workflow | unchanged: a new run of the column's workflow, on the same task |
| Resume, Extend | as on any task | unchanged; Extend still waits for a team place |
| Dismiss | the task is history, it leaves the board | open (below) |
| A move while a run works | no such move | refused; Stop first |
| A move back to an earlier column | no such move | a new run there, on the same worktree, commits and pull request as they are; a pull request step takes the one open on the branch |

**A late end does not move the wrong entry.** A run's end reaches the board with its run id; the
board acts on it only when the entry's `run` is that id, and drops it otherwise. Since a move is
refused while a run works, a mismatch only comes from a column removed or the board changed.

**A done run is not done work.** A run ending done ends the board's work only in a column whose
next is Done and that moves on by itself; otherwise the entry stays or moves on. The issue is
closed on entering Done, not when a run ends.

### Handing over between columns

- **The code is the worktree**, owned by the task, as today.
- **Words go by one prompt variable**: the last answer of the task's previous run, empty for the
  first. Its name and whether validation refuses it where it is always empty are open.
- Named outputs across workflows (`{output.<id>}` reaching another run) are left: they would touch
  validation and the retry carry-over.
- A person editing the work between two columns is seen as a retry sees it: the new run's first
  mark records the work as it now is.

### The worktree

- **It belongs to the task**, cut when the task is made, at its first column with work. A column
  whose work reads only (an analysis) still gets one when it is first.
- **A board needs workflows that work on a worktree**, as `board::usable` says today, for every
  workflow column.
- **Nothing removes it unasked**: *Remove worktree…* after the pull request merged, as today. An
  entry entering Done, or leaving the board, removes nothing.

## Limits

Four, each released at a different moment, so none is folded into another:

| Limit | Counts | Released when | Owner |
|---|---|---|---|
| Column limit | every entry in the column, waiting to move on included | the entry leaves the column | the board (its own data) |
| Team size | the board's runs holding a place, and starts on their way | the run gives its place up | the board, reading the tasks |
| Step limit (room) | runs of one workflow id at a step, per repository | the run leaves the step or ends | `TasksDrive` (`task::rooms`), unchanged |
| Place | one task per checkout | the run's agent and command are over | `TasksDrive` (`task::queue`), unchanged |

- **A column limit needs no reservation.** The entry is written into the column before its run is
  asked for, so it is counted from that moment; today's `starting` and `landed` exist because the
  count is derived from tasks.
- **Team size still needs starts on their way** told apart from starts a crash lost (below). That
  is a set the board's drive keeps in memory, emptied by a restart.
- **The first step's room gap stays** as known-gaps records it: a run asks for room once its task
  is kept. The column limit narrows a pull; the room ledger still holds the step.

## Drives, carriers, links

- **`BoardDrive`** (proposed, beside its model in `crates/core/src/board.rs`), held on `Shared`, one
  for the process. It decides: the columns, every entry's place and order, which entry starts,
  when one moves on, what the boot's reconciling finds, the board file's writes and what a failed
  one puts back. It takes over the board from `ProjectsDrive` (`bind_board`, `set_team`,
  `add_todo`, `move_todo`, `take_todo`, `pull`, `start_todo`, `landed`); the project keeps
  everything else.
- **`TasksDrive`** keeps tasks, places, room and the runs' starts and ends. It gains one command:
  a new run on an existing task with a given workflow, starting at its first step.
- **`RunDrive`** is unchanged: it runs one workflow.
- **`board::link`** joins them, tested with both drives real: a board start becomes a task start or
  a next run; a run's end becomes the board's input.
- **The carrier** (`crates/app/src/board.rs`) writes the file, carries starts into a window that
  holds the project, and decides nothing. No drive holds another, and no hub holds them all.

What crosses (names are working names):

| From → to | What |
|---|---|
| UI → `BoardDrive` | commands: place an entry (column, before), start, define and order columns |
| `BoardDrive` → carrier | effects: write the board file; start a task (`task id`, `run id`, issue, workflow); start a next run (`task id`, `run id`, workflow); close an issue; signals said once |
| `TasksDrive` → `board::link` → `BoardDrive` | inputs: where a start landed (kept, refused, window gone); a run ended (`task id`, `run id`, outcome); the tasks read at boot |
| carrier → `BoardDrive` | inputs: the board file written or not; the library read; the issues read |

What may start work: an entry entering a column that starts on entry, a press of Start, room
freeing in a column or on the team, a run ending done in a column that moves on by itself, and the
boot's reconcile. Nothing else, and only while a window holds the project, as today.

### Identities

- **Board**: one per project, by `ProjectId`.
- **Column**: an id of its own, stable across a rename.
- **Entry**: by issue number, unique on its board.
- **Task and run**: their ids, minted by the caller and written into the entry before the start is
  asked for.

### Kept and not kept

| Kept, in the board file | In memory only |
|---|---|
| the columns and their settings; each column's entries in order, with `task` and `run`; the team size | starts on their way; which entries wait for room; the drive's view of the tasks |

The task file is unchanged: `Origin::Board { project, issue }` keeps its shape, and the column lives
in the board file alone.

## Writes, crashes, restart

The board file is written by the board's carrier, the tasks by `task::files::Writer`, on two
threads with no order between them. So the order is made by the drive:

1. The entry is written with its new `run` id, and its write answered.
2. Only then is the start asked of the tasks.

What each failure leaves, and what the next boot does:

| Case | Left | At boot |
|---|---|---|
| Crash after the move, before the task is kept | an entry naming a run no task has | the entry reads *did not start*, offering Start; nothing runs by itself |
| Task kept, board write lost | cannot happen: the board writes first | |
| The run ended, the move on not written | an entry whose run is done | a column that moves on by itself moves it: a new run, not a resume |
| Entry moved to Done, issue not closed | an open issue in Done | closed again; `board::after_end` already checks it is open |
| A run under way | an interrupted run | under *Needs attention* in its column, for Resume; nothing resumes by itself |
| A task naming the board that no entry names | an orphan | reconciled by issue number: put back in the column of its last run's workflow, else in the first workflow column |

- **Two windows** share one drive in one process (the instance lock); the open question is only
  which window carries a start, as today.
- **A column is not removed while it holds entries.** A board's columns changed otherwise leave
  every entry where it is.
- **A workflow edited** reaches only runs that start after it.
- No database, no event log, no promise of exactly once for the forge or the tracker: an act
  outside onehand is made safe to do again (closing an issue already closed does nothing).

## Migration

Today's board becomes: *Backlog*, *To do* (manual, its order today's `todo`), one workflow column
running the bound workflow (starting on entry, moving on by itself), *Done*; the team size kept.

- Tasks of the board on it today (`board::taken`): an entry in the work column naming the task and
  its last run. Done ones: Done entries, newest first, to the cap. Stopped or taken over ones are
  off the board, as today. Ones waiting for approval or room were cut off by the restart anyway,
  and land under *Needs attention*.
- **The board moves to a file of its own** under the project's folder (`projects/<id>/`), written
  whole (temporary file, rename) before `board` is dropped from `project.toml`. A crash between
  leaves both; the board file wins and the key is dropped at the next boot.
- **An older build** already drops `board` from `project.toml` when it writes (board.md, *An older
  build*); it never touches the new file, so it sees no board and loses nothing.
- The board file carries a schema version, and a newer one is never written over, as workflows
  are.

Splitting each step of an old workflow into a workflow of its own is not done: it would change
sessions, marks, retry and what a step hands the next.

## The interface

What stays from today's page: columns of one width side by side, scrolling sideways, each
scrolling on its own, 50 rows a column and *N more not shown*; a row's `⋯` and right-click; a drag
doing what the menus do, the menus kept for the keyboard.

### Defining the columns

- **In the *Board* box of *Settings ▸ Projects***, built as *Routed commands* is: a row per column
  in order, its name over its work (*A person*, or *Runs <workflow>*) and its limit; *Move up*,
  *Move down*, an edit mark and *Remove…*; *Add column…* at the box's end. Backlog and Done stand
  at the two ends, fixed. *Team* stays the track of 1 to 8.
- **The column dialog**, at `DIALOG_FORM` with `.tall()` controls: *Name*; *Work*, two choices, *A
  person* and *A workflow*, the second with the workflow menu (capped as today's, a checkout
  workflow refused with `board::usable`'s reason); *Starts* (with a workflow): *When it enters* or
  *When Start is pressed*; *Moves on*: *By itself* or *A person moves it*; *Limit*, empty for none.
  What refuses it, the drive's words, above the footer; *Cancel*, then *Add* or *Save*.
- **Remove…** refuses while the column holds issues (*Move its N issues out first*); an empty one
  goes without asking, being a definition put back as easily.
- **On the board**, a column's head has a `⋯` with *Edit column…*, the same dialog, and *Edit
  workflow* on a workflow column; the page's head has *Edit columns*, opening the Settings box.
  One place edits a column.

### A row

The issue's title over one line saying where it stands, in core's words as `next_action`'s are:

| Where it stands | Its line | Drawn as a button |
|---|---|---|
| In a manual column | *Since <age>*, muted | none |
| In a column that starts on a press | *Waiting for Start* | **Start** |
| Running, queued, waiting for room at a step | as today | none |
| Waiting on a person (an approval, a card), or ended needing attention | as today, the Tasks page's words | the Tasks page's acts |
| Done in its column, a person moving it on | *Done here* | **Move to <next column>** |
| Done in its column, the next one full | *Waiting for room in <next column>* | none |
| Its start never landed (a crash before its task was kept) | *Did not start*, in the warning ink | **Start** |

A column whose workflow is gone says so under its head in a `status_line`, *<workflow> is gone;
nothing starts here*, and its Start buttons refuse.

- **Pressing a row** opens its task's detail, as today; an entry with no task yet opens its issue.
- **A row's `⋯`**: *Move to <next column>*, *Move back to <previous column>*, *Move to…* (the
  columns), *Move up*, *Move down*, *Back to Backlog*; then the task's acts (*Show task*, *Open
  session*, *Stop*, *Retry…*, *Resume*). While a run works the moves stay listed and refuse,
  *Stop it first*, so the menu keeps its shape.
- **Every row drags**, not only an issue's, except one whose run works. A drop on a row lands just
  before it, as on To do today; a drop on a full column does nothing, its head already saying
  `n of limit`.

### Across the columns

- **The task detail** labels each run by the column it ran in (*Run 3 · <column>*), and its line of
  facts says where the task stands (*On the board at <column>*).
- **No move log** in the first version: time spent in a manual column is not recorded.
- **On the Tasks page**, a board task between two runs stands under *Finished*, its line *<outcome>
  · on the board at <column>*; the history cap passes over it while an entry names it.
- **Waiting on a person by the board's own design** (for Start, for a move) is not *Needs
  attention* and is not counted on the rail: the row shows its button, and the project page's board
  link adds *· N waiting for you*. A run waiting on an approval or a card, or ended needing
  attention, is counted as today.

## What changes in the contracts

- **CONTEXT.md**: *Board* (columns a person defines, not a column per step), *To do* (a manual
  column), *Backlog* ("no column holds"), *Team size*, the note under *Lane* ("its columns are
  steps"); *Column* added. *Task* and *Run* keep their words; a task's runs may now use different
  workflows.
- **docs/board.md**: rewritten for columns, entries, starting and moving on, the acts, the file,
  the migration.
- **docs/tasks.md**: a board task is held by its board (no history cap, not finished while an entry
  names it); a next run on a task; *Earlier runs* as the columns it went through.
- **docs/workflows.md**: the handover variable; a run started on an existing task.
- **docs/drives.md**: `BoardDrive` in the table, `board::link` under rule 10, the board leaving
  `ProjectsDrive`'s row.
- **docs/known-gaps.md**: the gaps of the old board that go (old workflow's tasks still counting,
  rebinding from a copy) and those that stay (no pull when a window opens, the first step's room).
- **docs/screens.md**: the *Board* page (columns, a column's head and `⋯`, rows and their lines,
  moves, empty lines) and the *Board* box of *Settings ▸ Projects* (the column rows and the column
  dialog); the project page's board link; the task detail's run labels and line of facts.
- **DESIGN.md**: nothing new expected; the board stays a page, and nothing in it is a new region.

## Considered and left

- **A new board item entity.** It would redo what a task does (place, runs, marks, retry) and stand
  beside it in the glossary. A task already crosses runs on one place.
- **The issue as what carries the work.** It holds no code and no run history, and writing the
  board's history into it would make the issues file's drive keep the board's state.
- **One workflow split into columns by a key on its steps.** It gives the drawing and per-column
  limits, but a wait for a person is still a run held open, cut off by every restart.
- **A step that waits for the merge.** Same reason: days of a run, a session and a team place held.
- **Outputs named across workflows** in the first version: they reach into validation and the retry
  carry-over.
- **Moving on by rules (branching)** in the first version: the column order and a person's move
  cover it.
- **A database or event sourcing**: stable ids, an ordered write and a reconcile at boot are enough.
- **Editing columns in place on the board** (a column added at its end, a head renamed where it
  stands): no peer in the app, and the board page would become a settings page too.
- **A group of its own on the Tasks page** for a board task between runs, or hiding it there: the
  first adds a box and a branch in `Task::group`, the second loses the way to its history.
- **Counting a board's own waits as *Needs attention***: a person chose a column that waits for a
  press, so its wait is the board at rest, not something gone wrong.

## Open

- The handover variable's name, what "the last answer" of a run is (its last agent visit's kept
  answer), and whether validation refuses it in a workflow that may run first.
- What Dismiss means on a board task: off the board with its issue back in the Backlog, as a stop
  does today, or not offered while an entry names it.
- How many Done entries the file keeps.
- Whether an analysis column first on the board should be allowed to start without a worktree,
  making the task's place come later; not in the first version.
- The project page's board link counts *waiting for you*: whether it replaces *M to do* or stands
  beside it.

## First version

A board of its own columns; entries tied to issues; manual and workflow columns; moving by hand,
or by itself where a column says so; a limit per column; one worktree and one history per issue
across the columns, with the handover variable. Agent and Command columns come later. Not in scope:
several boards on a project, a board across projects, a board on a host, several runs at once on
one issue.
