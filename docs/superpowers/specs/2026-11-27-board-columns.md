# Spec: a board of its own columns

How the board of [docs/proposals/board-columns.md](../../proposals/board-columns.md) is built: the
proposal says what the board does, this file how, in six steps, one pull request each. A plan per
step goes in `docs/superpowers/plans/` before the step is built. The last step deletes this file
and the proposal, their behaviour written into the owning documents by then.

Points marked **builder's call** are left to whoever builds the step, to decide and say in its PR.

## What the code has now

- The board is `project::Board { workflow, team, todo }`, a field of `Project` in `project.toml`
  (`crates/core/src/project.rs:139`), changed by `ProjectsDrive` through `edit_board`
  (`project/drive/board.rs`) as `Thing::Board` / `Value::Board`: every write of a project writes
  the whole file, the failed one put back. The team's track waits for the changing to stop on the
  drive's own `Arm { token }` / `Input::Elapsed`, and `projects::flush` writes what still waits at
  quit.
- Starts on their way are `ProjectsDrive::starting`, from the pull's decision until they land
  (`start_todo`, `landed`).
- The pull is carried in `crates/app/src/board.rs`: it reads the issues off the UI thread, gathers
  from the tasks what `ProjectsDrive::pull` takes (`running`, extends waiting, the first step's
  room, `taken`; `crates/app/src/task/listing.rs`), and starts each issue through
  `control::start_task`, which mints the task's id there. On the way it decides what a refusal
  comes to (`Err(_) if made.is_some_and(…)`), a rule in the carrier this work moves into core.
- Extends waiting for a team place are given one with no window (`give_places`).
- A board task's end is `Signal::BoardEnded { project, issue, done, pull }`, said from `let_go`
  once the drain is over and the last mark pinned (`task/tasks/ending.rs`); `board::ended` in the
  app closes the issue (`board::after_end`) and pulls. The task is busy until then, so no Retry or
  Resume adds a newer run before its end is said.
- What is on the board is derived from the tasks (`board::on_board`, `taken`, `columns`): by
  origin, outcome and dismissal. A dismissed task leaves it; a retried one comes back; tasks of an
  unbound board come back when one is bound again.
- `TasksDrive::loaded` applies the history cap at once (`enforce_cap`), and the cap deletes files.

## The model

`crates/core/src/board.rs` becomes a module: `board.rs` (the decisions it holds now),
`board/model.rs`, `board/file.rs`, `board/drive.rs`, `board/link.rs`, tests beside each.

```rust
/// A project's board, kept in `projects/<id>/board.toml`.
pub struct Board {
    pub schema_version: u32,
    /// How many of the board's runs work at once, 1 to 8.
    pub team: u8,
    /// Between the Backlog, which is not kept, and Done; in order.
    pub columns: Vec<Column>,
    /// Newest first, at most `DONE_KEPT`.
    pub done: Vec<Entry>,
}

pub struct Column {
    pub id: String,          // stable across a rename
    pub name: String,
    pub work: Work,
    pub starts: Starts,      // read only when `work` is a workflow
    pub moves_on: MovesOn,
    pub limit: Option<u32>,  // at least 1
    pub entries: Vec<Entry>, // in the person's order
}

pub enum Work { Person, Workflow { id: String } }
pub enum Starts { OnEntry, OnPress }
pub enum MovesOn { ByItself, ByPerson }

pub struct Entry {
    pub issue: u64,
    /// The task working the issue, once a column with work took it.
    pub task: Option<String>,
    /// The run the board last asked for: written before it is asked.
    pub run: Option<String>,
    /// When it entered its column, seconds.
    pub since: u64,
}
```

Rules, from the step that uses them on:

- **An entry is ready to move on** when its column's work is `Person`, or the task's last run ended
  done in this column. `MovesOn::ByItself` moves a ready entry into the next column when that
  column takes it; the next of the last is Done.
- **A column takes an entry** while it holds fewer than its limit. A move *by itself* into a column
  that starts on entry is taken only when its run can start now: the team has room, and the
  workflow's first step has room when it is limited, as `ProjectsDrive::pull` counts it now. A
  person's move is taken on the limit alone; the entry then waits for the team in its column.
  Today's pull is this rule: To do is a `Person` column moving on by itself into one workflow
  column starting on entry.
- **A run's end is the board's** when an entry outside Done names its task and the run that ended
  is the task's last run. It is not matched on `Entry::run`, since a Retry or a Resume from the
  Tasks page runs another run of the same task, which is still the column's.
- **A start a crash lost**: an entry whose `task` names no task, with no start on its way. A task
  that exists has landed, whatever `Entry::run` says (a queued start called off pops its run and
  says no end).
- **The Backlog** is the workable open issues that no entry names and no done task of the board
  holds until it is dismissed or the history cap removes it, so a forge-kept issue whose done
  task fell off Done does not come back.
- `DONE_KEPT` is `BOARD_SHOWN` (50), **builder's call** to name apart. Done says how many it holds
  past what it draws, counting the done tasks it no longer keeps as entries.

## The drive

`BoardDrive`, one for the process, held on `Shared`. It owns every board: reads go to its model,
every change goes through its commands. It never reads the clock or mints an id: `Now` comes with
what stamps, and a start's task id comes from the carrier (`task::new_id`), handed in with the
command or input that decides the start (**builder's call**: a closure or a list of ids).

**Its writes reuse `crate::saving::Saves`**, one thing per board, so a write and the change queued
behind it, and a failure putting back the last state known saved and dropping what was queued
since, are the rules projects' writes already follow. A start held for a write that failed is not
asked; the next look starts from the state put back.

Effects, each with how its late answer is handled:

| Effect | Answered by | Late answer |
|---|---|---|
| `Write { project, board }` | `Input::Written { project, result }` | Record, as `Saves::landed` |
| `Arm { token }` (the team's track) | `Input::Elapsed { token }` | Drop by token |
| `Changed { project }` | none: the carrier redraws the windows holding the project | |
| `ReadIssues { project }` | `Input::IssuesRead { project, result }` | Drop: the next look reads again |
| `Start { project, issue, task, workflow }` | `Input::Landed { project, issue, task, kept, result }` | Record: a task kept is the board's whatever moved since |
| `GivePlace { task }` | none | |
| `CloseIssue { project, issue }` | none: a failure is said by the issues' writer | |
| `Held { tasks }` (step 2) | none | |
| `Signal` | | |

What the board asks of other drives: an extend given a team place is `RunDrive`'s, one per run,
and an issue closed is its issues file's drive's, one per file, each with an owner other than the
board's; so the board's carrier carries `GivePlace` as `task::give_place` and `CloseIssue` as
`onehand_issues::write`, as drives.md allows for drives of different owners, and as the app does
now. The held tasks (step 2) go to `TasksDrive`, held on the same `Shared`, through
`board::link`, tested with both drives real.

- **A start is asked only after the write that names it is answered** (from step 2, when entries
  name tasks). In step 1 the write is To do's, as today.
- **A start on its way** is held in the drive's memory from the look that decides it until it
  lands, counted towards the team and, when the first step is limited, its room; a restart empties
  the set.
- **A look takes what the tasks say**, read by the carrier when the issues' read lands, as it reads
  them now (`task::board_running`, `board_extends_waiting`, `board_taken`, `room_at`): a report of
  reads, deciding nothing, handed in with `Input::IssuesRead` beside the workflow from the library,
  whether a window holds the project, and fresh task ids. Starts need a window; giving places to
  extends does not.
- **`NotStarted`** is said in a window holding the project when the refusal lands, else nowhere.
- `BoardDrive::flush` writes what waits on the team's track; the quit calls it beside
  `projects::flush`.

`Signal::BoardEnded` becomes `{ project, issue, task, run, outcome }`, and `pulls_next` moves into
`board`: what an end pulls is the board's rule.

## Steps

### Step 1: the board's drive and file, nothing new on screen

- **The migration is a blocking step at boot**, `board::migrate_blocking(projects_dir)`, run beside
  the other migrations in `shell/boot.rs` before either drive exists: a project whose
  `project.toml` holds `board` and has no `board.toml` gets one written whole; only then is the
  project written without the key. Both found: the file wins, the key is dropped. A crash between
  leaves both, and the next boot finishes. The field is renamed (`old_board`), read by the
  migration alone, so every reader of `Project::board()` fails to compile and moves to
  `BoardDrive`; `Thing::Board` and `Value::Board` leave `ProjectsDrive`.
- **Step 1's file** holds what today's board does in the model's shape: `team`, a To do column
  (`todo`, `Person`, `ByItself`) with an entry per issue and no task, the work column (`work`,
  the bound workflow, `OnEntry`, `ByItself`) with no entries, Done empty, `schema_version = 1`. Only
  that shape is accepted. A file with a newer version is unreadable: the board says why and is
  never written.
- **`BoardDrive` takes the board over from `ProjectsDrive`** with the same answers: `set_board`,
  `bind_board`, `bind_copy`, `unbinding_asks`, `set_team`, the To do commands, `pull`,
  `start_todo`, `landed`, `extends_given`. *No board* deletes the file.
- **What is on the board stays derived from the tasks**, as today (`on_board`, `taken`, `columns`),
  so dismissing, retrying and binding again behave as they do now.
- **The carrier carries and reports only.** The refusal rule moves into `Input::Landed`: refused
  before the task was kept, the issue is back in the Backlog and `NotStarted { issue, why }` is
  said once; refused after, the task holds the issue and its own start said why. `kept` is whether
  the task is there once the start has landed, a read the carrier reports. `control::start_task`
  takes the id it is given.
- Tests: in core, the drive's answers as `ProjectsDrive`'s were (moved with it), a failed write
  put back, a late landing, the migration (key only, file only, both, an unreadable file never
  written); the carrier's contract tests (effects in order and once, a write's failure reaching the
  drive, the flush at quit).
- Documents: `docs/board.md` (*What is kept*, *An older build*: an older build sees no board once
  the key is gone, and To do changed in it meanwhile is lost to the file); `docs/drives.md` (the
  table, rule 10).

### Step 2: entries name the tasks

- `schema_version = 2`. **Upgrading a version 1 file adopts the board's tasks once**: each task
  `board::taken` holds gets an entry in the work column, done ones in Done (newest first, to
  `DONE_KEPT`). From then on entries are what is on the board; no boot adopts a task again, so a
  task removed from the board stays off it.
- **The ordered start**: the look writes the entry with its `task` and `run` (a new task's run id
  is its task id), and asks `Start` once the write is answered.
- **Every boot** drops an entry naming no task with no start on its way: its issue is back in the
  Backlog, as a crash after a pull leaves it today. (Kept and offered *Start* again from step 3.)
- **The end rule** (above): a done run in a column moving on by itself moves on, so in step 2's one
  shape to Done. The issue is closed when an entry enters Done, by `after_end`'s rule, and not
  again at boot: a person may have reopened it on purpose.
- **Stop and take over keep the entry** in its column; its team place frees, so the board looks
  again (a stopped run pulls, as a done one does).
- **Remove from board**, a `BoardDrive` command refused while the task works: the entry goes, the
  issue is back in the Backlog, the task stays as history. Dismiss is refused on a held task, with
  why.
- **Held tasks**: `Effect::Held` after every change of an entry's task, carried by the link to
  `TasksDrive::held`. At boot the boards are read before the tasks, and `TasksDrive::loaded` takes
  the held set, so the cap it applies at once never removes a held task. `over_cap` takes the set.
- **Deliberate changes**, said in `docs/board.md` and the changelog:
  - a task off the board (removed, or in Done) that a person retries or resumes is theirs: its end
    is not the board's, and its issue is as the board left it;
  - *No board* deletes the file: its tasks run on as the person's, and a board bound later starts
    empty;
  - a stop leaves the issue in its column.
- Step 2 shows: a stop leaving the issue in its column; *Remove from board* in a board row's `⋯`
  (its words **builder's call**, kept with the Tasks page's acts); Dismiss refused on a board task.
- Documents: `docs/board.md` (*When it ends*, *What is kept*), `docs/tasks.md` (held tasks).

### Step 3: columns a person defines, in core

- The model's whole shape is accepted. `board::validate`: names not empty and not used twice, ids
  unique, a limit of at least 1, a workflow column's workflow on offer and usable
  (`board::usable`); a refusal in words the form shows.
- Commands: add, edit, remove (refused while it holds entries) and reorder a column; place an entry
  (a column, before an entry or last), refused while its task works; Start on an entry of a column
  starting on a press; the auto rules (start on entry, move on by itself, limits, team).
- **`TasksDrive::next_run` and `Task::next_run`**, for an entry with a task moving into a workflow
  column (`Effect::NextRun { project, issue, task, run, workflow }`, answered by `Input::Landed`):
  - a new run at the first step with no output carried, not through `Run::retry_of` (and not in
    `workflow/run.rs`, already past 800 lines);
  - its setup keeps the last run's `repo`, `dir`, `branch` and `forge`; agent, mode and check
    command as a new task of the board takes them now; preflighted as `Launch::off_screen` then
    `.on_worktree(…)` with the task's own worktree, rather than a second setup builder;
  - refused on a task whose worktree was removed; another branch checked out refuses it as a retry
    is refused (`marks::Against::refuses_retry`);
  - driven off screen (`drive_off_screen`), never taking the screen as a person's Retry does;
  - `dismissed` cleared, then asking for its place through the queue.
- An entry's line, worked out each time as `next_action` is: in a `Person` column, since when; in a
  workflow column, *Waiting for Start*, the task's own line, *Done here*, *Waiting for room in
  <column>*, *Did not start* (now kept, offering Start), and its one button. A column whose
  workflow is gone says so.
- A run keeps the column's name it started in, an optional field of its own (`Run::column`), set
  by `Start` and `NextRun`; a file without it draws no label, and an older build drops it when it
  saves the task, as `docs/tasks.md` allows.
- Tests: the rules in core; a flow through two workflow columns with a `Person` column between,
  on one worktree.

### Step 4: columns defined in Settings

- *Settings ▸ Projects ▸ Board* holds the columns as *Routed commands* holds its rows, and the
  column dialog, as the proposal's *The interface* says. The *Workflow* menu goes: a board is made
  in step 1's shape on the workflow picked first (**builder's call**: the words of making one).
- `docs/screens.md`' Settings paragraph.

### Step 5: the board page draws the columns

- The page draws the board's columns, each entry's line and button, the moves in a row's `⋯` and
  by drag for every row but one whose task works, *Edit column…* in a column's head and *Edit
  columns* in the page's head.
- `board_page.rs` (746 lines of code, its tests inline) is split as it grows: its tests to a file
  of their own first, then **builder's call** along which seam, a column and a row being the
  likely ones.
- `docs/screens.md`' Board paragraph.

### Step 6: the hand-over and the history, and the end of the proposal

- A prompt variable, the last answer of the task's previous run (its last agent visit's kept
  answer), empty for a first run; its name **builder's call**; `workflow::validate` accepts it
  anywhere.
- The task detail labels each run by `Run::column` (*Run N · <column>*) and says *On the board at
  <column>*; the Tasks page's line for a held task between runs; the project page's link counts
  what waits for a person.
- CONTEXT.md (*Board*, *To do*, *Backlog*, *Team size*, *Lane*'s note, *Column*), `docs/board.md`
  whole, `docs/workflows.md`, `docs/known-gaps.md`; this spec and the proposal deleted.

## Every step

- Before its pull request: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`; a review of the branch; the documents the step changes.
- Our enums are matched whole (`guards.rs`). Nothing in core is `pub` that nothing outside names.
- No file past 800 lines of code; `board/drive.rs` is split by its commands once it nears it.
  `project/drive.rs` (798) shrinks as the board leaves it.
