# Roadmap: from the workflow engine to Tasks

Where the work goes after the configurable pipelines landed (#72). The goal of the next stretch is
**to make the work that is running visible**: one place showing what is running, what needs a
person, and what has finished. Automating issues and pull requests comes back after that, on the
new foundation, rather than first.

[tasks.md](tasks.md) describes the feature and its architecture as one picture.

The order is fixed. Each milestone rests on the one before it, and each pull request carries its
docs. Milestone 1+2 is one milestone for a person but three pull requests in a row, because the
rename, the task store and the page each carry risks of their own.

## Where things stand

Workflows are configurable and saved, one per file. A single engine (`onehand_core::workflow`)
decides every transition, and a single driver (`crates/app/src/task/driver.rs`) runs it. Every
run keeps a snapshot of its workflow and belongs to a task, whose file is written in order by one
thread and kept as history; an interrupted task can be resumed after a restart.

Pull request 1 of milestone 1+2 has landed: "pipeline" has left the code and the screen (the
template is a **Workflow**, its execution a `workflow::Run`), `pipelines/` moves to `workflows/` at
start, and a lock on the config directory keeps a second onehand from starting on it.

Pull request 2 has landed as well: every run belongs to a **Task** kept in `tasks/<id>.json`
whatever its outcome, and `pipeline-runs/` moves there at start. A run records its step visits, each
pinned at its start and end as a commit under `refs/onehand/tasks/…`. Every start, Resume included,
goes through one queue keyed by the checkout git sees, and a place is freed only once the work has
stopped.

Pull request 3 has landed too: the **Tasks page** lists every task of the window's projects under
*Needs attention*, *Running*, *Queued* and *Finished*, with a project filter, a rail row with the
count of those needing attention, and the unattended runs read-only. The project page links there
in one line and offers *Run check*, which runs the check command as a task of its own. **Retry**
starts a new run at the default step, carrying over what it can, and says when the work changed or
refuses when another branch is checked out. Finished tasks past the newest 200 of a project are
removed with their mark refs.

Milestone 3 has landed: pressing a row opens the **task detail** in place of the cards, with the
last run's step timeline (each visit's output, and the files it changed with their line diffs read
from its marks), what waits for approval with *Open session*, and the earlier runs. Retry is offered
on a finished task there too, and its dialog has a step menu to start from an earlier step; a
dismissed task retried is live again.

Milestone 4 has landed: every workflow has an **id** and a **version**, set when it is saved, and
Retry finds the newer workflow by id, a rename included. Settings ▸ Workflows imports and exports a
workflow file, the launcher previews the steps and the first prompt before Run, and validation
refuses unknown keys, variables that would always be empty, a worktree workflow that never commits
and two steps with one label.

Not there yet:
- The workflow mock agent (`crates/core/examples/mock_workflow_agent.js`) and the checklist in
  [workflows.md](workflows.md#checking-it-by-hand) are in the repository; a full pass of the
  checklist, the Tasks page rows included, is still owed.
- How many old tasks the cap removed is counted since onehand started, not kept across restarts.
- An issue or a pull request cannot be the source of a run. The unattended run on `main` is still
  the older one-turn kind.

## Milestones

| # | Pull request | Scope | What a person gets |
|---|---|---|---|
| 0 | Engine check (small) | A mock agent for pipelines and a manual test checklist | Proof that a pipeline runs and stops as it should |
| 1+2 | Tasks (three pull requests) | The rename to Workflows; the task model, history, step visits and the per-checkout queue; the Tasks page, the check as a task and Retry | One place showing what runs, what needs them, and what finished |
| 3 | Task detail (medium) | Step timeline, outputs, diffs, what awaits approval, retries | Understanding a task and stepping in from it |
| 4 | Workflow library (medium) | Template versions, import and export, a preview of the run's configuration | Managing and reusing many workflows |
| 5 | Issues and pull requests (large) | An issue as a source; integration steps; reports that retry | Unattended work on issues, rebuilt on the new foundation |

### 0. Engine check

- **A mock agent for pipelines**, beside `crates/core/examples/mock_ui_agent.js`. It answers a plan,
  edits files and commits to suit the gates. On request it misses a gate or makes the check command
  fail, so every path can be walked in seconds without an API key.
- **A manual test checklist**:
  - Stop while a command runs;
  - a gate missed until the run is exhausted;
  - a failed command going back to its earlier step;
  - Approve and Revise;
  - a restart mid-step, then Resume;
  - a template edited while a run uses its snapshot.

This goes first so that driver bugs are found before a UI depends on the states it reports.

### 1+2. Tasks

Three pull requests, in this order: **1** is the one-instance lock, the rename and its migration; **2** is items 2 to 4,
the tasks, their history, visits and marks, and the queue; **3** is items 5 and 6, the page, the
check as a task and Retry. Item 7 is spread over all three, each bringing its own docs.

1. **Rename Pipelines to Workflows**, in the code as well as on screen. That covers modules, types,
   the config directory (`pipelines/` → `workflows/`, with its data moved over), the glossary and
   the docs. Behaviour does not change.
2. **The task model and its store.**
   - A **Task** is the work.
   - A **Run** is one execution of it. Run absorbs today's *Pipeline run* now, and the unattended
     run in milestone 5. Until then unattended runs are shown on the page through a thin
     conversion, with their data and code left as they are.
   - A task records its source, its runs, the session each run used, and its outcome.
   - Stored through `pipeline::files::Writer`, with what is in `pipeline-runs/` migrated.
   - Finished tasks are kept as history: the most recent N per project (around 200). The page says
     when that cap has cut something.
3. **A run as a list of step visits, with a mark at each visit's start and end that can rebuild a
   diff.** Going back to a step is a new visit, so nothing is written over. Each mark carries a commit
   object made from a temporary index and pinned under `refs/onehand/` (see the decisions below),
   so that milestone 3 can show diffs for every run from now on. A digest can compare two states
   but cannot rebuild a diff, and a checkout's work is left uncommitted.
4. **One running run per checkout or worktree, with a queue behind it.** A run started where one is
   already working waits as *Queued* and starts when the place is free. Resume and Retry are
   starts too.
5. **The Tasks page**, in the agent pane beside the workspace overview.
   - Groups: *Needs attention*, *Running*, *Queued*, *Finished*.
   - A filter by project.
   - A row opens its session, and Stop and Resume work from the row.
   - The project page's list of unfinished runs becomes a link here, so there are not two lists that
     can disagree.
6. **Single commands as tasks, and retry.**
   - A single command, such as the project's check, runs as a task with an outcome.
   - Retry starts a new run of the task (see the decisions below).
7. **The docs**, the glossary and the UI contract for the page.

### 3. Task detail

- Each run's step timeline, with each step's output and its diff, taken from the marks of 1+2.
- What waits for approval.
- The runs before this one.
- Stop, Resume, Retry and Open session.

### 4. Workflow library

- Template versions.
- Import and export.
- A preview of the configuration a run would start with.
- Fuller validation.

The rename is already done by 1+2.

### 5. Issues and pull requests

- An issue as a task's source.
- Integration steps: push, open a pull request, wait for CI, repair failing checks, answer a
  review.
- The outcome reported on the issue, retried until it lands, with the run's record kept until
  then.
- Branches prefixed by where the issue lives, so the same number on two trackers never shares a
  branch.
- A concurrency cap across the workspace, before more than one issue is taken up automatically.
- The one-turn unattended run is replaced.
- The review findings from #70 are this milestone's acceptance criteria. On top of them, a push
  must carry the commit the check passed on (`verified_at`).

## Decisions taken

- **A task is the work; a run is one execution of it.** A retry is a new run, and the earlier runs
  stay in the task's history.
- **A workflow is optional.** A task is anything with a lifecycle and an outcome: a workflow run,
  an unattended run, or a single command.
  - An unattended run shows read-only until milestone 5.
  - A plain agent session is not a task, and it does not appear on the Tasks page.
- **A run keeps a snapshot of its configuration.** Editing a template never changes a run that
  already started.
- **Retry reuses the previous run's snapshot by default.** If the template has changed since, the
  newer one is offered.
  - It starts at the step where the previous run stopped, carrying the outputs of the steps before
    it, such as an approved plan.
  - An earlier step can be chosen instead.
- **Resume is not retry.** Resume carries on the same run, with its marks kept.
- **Needs attention is explicit, and every entry comes with an action.** It holds two kinds, with
  different actions:
  - **waiting**: a live run waiting for approval, or on a card its session parked. It keeps its
    place, and offers Open session and Stop;
  - **ended**: a failed, exhausted, timed out or interrupted run. Its place is free, and it offers
    Resume (interrupted only), Retry and Dismiss.

  An ended task stays there until a person retries, resumes or dismisses it. Dismissing moves it to
  *Finished* with its outcome kept.
- **The place belongs to the task.** Every run of a task works in the same checkout or worktree
  and on the same branch, so a retry sees the work the earlier run left.
- **A workflow is the template.** `PipelineRun` becomes a run of a task; there is no "workflow
  run", and "pipeline" leaves the code and the screen.
- **A mark records the work, not a second snapshot.** "Snapshot" names only the configuration a
  run keeps. The work at a step visit's start and end is a mark that also carries a commit object.
- **A run records step visits, not steps.** A step reached twice is two visits, each with its own
  id, times, marks, output and result; its refs are `refs/onehand/tasks/<task>/<run>/<visit>/…`.
- **That commit object is made from a temporary index, not `git stash create`.** `stash create`
  leaves untracked files out and makes a commit nothing points at, which `git gc` prunes. The
  index is filled with `git add -A`, and the commit is pinned under `refs/onehand/`. The ref is
  deleted when its task falls out of the history cap.
- **A task's outcome is its last run's outcome**, in one enum shared with unattended runs, plus
  *Dismissed*.
- **Which outcomes need attention:** exhausted, failed, timed out, and interrupted (the agent
  stopped or the session went). Stopped by a person and taken over go straight to *Finished*,
  because a person already acted. Resume is offered only for an interrupted run.
- **The Tasks page holds tasks only.** A card parked by a plain session is signalled on the rail,
  as it is today.
- **The glossary describes the code as it stands.** It is brought up to date now; the task terms
  go in with the code that brings them.
- **Every task holds its place's lock, a single command included**, so a check waits behind a
  workflow that is still editing. A plain session holds no lock, and starting a task beside one
  gives no warning.
- **A queued task survives a restart as interrupted.** It waits under *Needs attention* for
  Resume, because nothing starts an agent by itself after a restart.
- **A retry carries a step over only while its configuration, and everything it reads, is
  unchanged.** Going from the first step, a step's output and its approval carry over while the
  step and the steps it names, approves or falls back to are the same in both workflows. The first
  that differs, and everything after it, runs again, approvals included. A kept id with a new
  prompt is a different step.
- **Every start goes through the queue, Resume and Retry included.** A place is held while a run
  is queued, running or waiting, and given up only once its agent's turn is over and its command
  has exited, never at the press of Stop.
- **Only one onehand runs on a config directory.** A lock on a file in it is taken at boot; a
  second instance says so and exits. It covers the place locks, the migration and the ordered
  writer at once, which are all one process's today, and it comes before the migration.
- **A retry checks the work against the previous run's last mark.** Another branch checked out
  refuses it, naming the branch; changed work is said in the dialog and outputs still carry over.
- **Stop on a queued row calls off that start only.** A resume or retry goes back to ended, its
  earlier run untouched; a task that never ran a step finishes as stopped by a person, with no
  empty run.
- **A place is the checkout git sees.** The lock is keyed by the canonical top level of the
  worktree, so projects that are folders of one checkout, and a checkout reached through a
  symlink, share one lock.
- **onehand never removes a task's worktree.** Removing it could lose unpushed commits. The
  merged pull request of milestone 5 is the first clear signal to clean up.
- **The history cap counts tasks against the project they were started from**, never a worktree
  added as a project. A task pushed out of the cap is deleted with its refs, and the page says
  how many were removed.
- **Finished is read-only on the Tasks page**, except *Retry* in the task detail.
- **The Tasks page has a rail row beside the overview**, with the count of *Needs attention*
  (hidden at zero), and a keymap command with no default key.
- **The page covers the window's workspace**, filtered by project.
- **A single command is the project's check command**, nothing else, until there is a real need.
- **Moving `pipelines/` and `pipeline-runs/` can be stopped at any point and run again.** It runs
  at every boot until nothing is left to move. Each file is written in full before its old copy is
  removed, a migrated task keeps the old run's id so a file found twice is one task, a file this
  build cannot read is left untouched, and an old directory goes only once it is empty. There is no
  way back to an older build, which this pre-release accepts; the release notes say so.
- **The pipeline mock agent takes its orders from the brief** (`miss`, `fail-check`), as
  `mock_ui_agent.js` takes `fast`. The manual checklist is a section of the workflows doc and
  moves with it when it is renamed.
- **A template has an id and a version counter, and no old version is kept.** A run's snapshot
  already holds the one it ran. A new file takes its file name as id, a shipped one
  `builtin:<name>`; the save alone sets both, so a duplicate or an import always gets a new id.
- **Import and export are a template file to and from disk**, nothing more. An import opens in the
  form and is written only at Save.
- **The preview lives in the launcher, before Run**, collapsed by default.
- **Fuller validation refuses** unknown TOML keys, a variable that would always be empty, a
  worktree workflow with no step gated `committed`, and a label used twice. A file with an
  unknown key reads as unreadable and is never written over.
- **The per-checkout lock comes before any cap on concurrency.** Two runs editing one checkout is
  the failure possible today. A workspace-wide cap arrives with milestone 5, before issues are
  taken up automatically.
