# Roadmap: from the pipeline engine to Tasks

Where the work goes after the configurable pipelines landed (#72). The goal of the next stretch is
**to make the work that is running visible**: one place showing what is running, what needs a
person, and what has finished. Automating issues and pull requests comes back after that, on the
new foundation, rather than first.

The order is fixed. Each milestone is its own pull request with its docs, and each one rests on the
milestone before it.

## Where things stand

Templates are configurable and saved, one per file. A single engine (`onehand_core::pipeline`)
decides every transition, and a single driver (`crates/app/src/pipeline/driver.rs`) runs it. Every
run keeps a snapshot of its template, its file is written in order by one thread, and an unfinished
run can be resumed after a restart.

Not there yet:
- No one has tested Stop, retry, approval and restart in the running app.
- There is no task model, and a run's record is deleted when it finishes, so there is no history.
- Runs do not queue, and two runs can edit the same checkout at once.
- An issue or a pull request cannot be the source of a run. The unattended run on `main` is still
  the older one-turn kind.

## Milestones

| # | Pull request | Scope | What a person gets |
|---|---|---|---|
| 0 | Engine check (small) | A mock agent for pipelines and a manual test checklist | Proof that a pipeline runs and stops as it should |
| 1+2 | Tasks (large) | The task model, history, the per-checkout queue, the Tasks page, the rename to Workflows | One place showing what runs, what needs them, and what finished |
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

One pull request, landed as commits in this order:

1. **Rename Pipelines to Workflows**, in the code as well as on screen. That covers modules, types,
   the config directory (`pipelines/` → `workflows/`, with its data moved over), the glossary and
   the docs. Behaviour does not change.
2. **The task model and its store.**
   - A **Task** is the work.
   - A **Run** is one execution of it. Run absorbs today's *Pipeline run* and the unattended
     *Attempt*.
   - A task records its source, its runs, the session each run used, and its outcome.
   - Stored through `pipeline::files::Writer`, with what is in `pipeline-runs/` migrated.
   - Finished tasks are kept as history: the most recent N per project (around 200). The page says
     when that cap has cut something.
3. **A snapshot of the work at each step's start and end.** A commit object made with
   `git stash create`, kept in the run, so that milestone 3 can show diffs for every run from now
   on. A digest can compare two states but cannot rebuild a diff, and a checkout's work is left
   uncommitted.
4. **One running run per checkout or worktree, with a queue behind it.** A run started where one is
   already working waits as *Queued* and starts when the place is free.
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

- Each run's step timeline, with each step's output and its diff, taken from the snapshots of
  1+2.
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
  - A plain agent session is not a task. It appears under *Needs attention* only while a card it
    parked waits for an answer.
- **A run keeps a snapshot of its configuration.** Editing a template never changes a run that
  already started.
- **Retry reuses the previous run's snapshot by default.** If the template has changed since, the
  newer one is offered.
  - It starts at the step where the previous run stopped, carrying the outputs of the steps before
    it, such as an approved plan.
  - An earlier step can be chosen instead.
- **Resume is not retry.** Resume carries on the same run, with its marks kept.
- **Needs attention is explicit, and every entry comes with an action.** It holds:
  - a run waiting for approval;
  - a card waiting for an answer;
  - a failed, exhausted or interrupted run.

  A failed task stays there until a person retries, resumes or dismisses it. Dismissing moves it to
  *Finished* with its outcome kept.
- **The per-checkout lock comes before any cap on concurrency.** Two runs editing one checkout is
  the failure possible today. A workspace-wide cap arrives with milestone 5, before issues are
  taken up automatically.
