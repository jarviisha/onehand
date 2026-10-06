# Piece 1: the issue says where its work stands

- Status: proposal, wave 1, the first piece to build. Part of [the proposal](README.md).
- Contracts it touches: [DESIGN.md](../../../DESIGN.md) (the Issues tab line under *Docks*),
  [unattended.md](../../../docs/unattended.md) (the *Runs* paragraph), [tasks.md](../../../docs/tasks.md)
  (the task detail, which this reuses).

## Goal

Open an issue and know, without going anywhere else:

- which workflow is working it, and at which step;
- whether it is running, queued, waiting on a person or ended, and **what the person should do next**;
- what the work left: the files changed, the check command's last word, the branch, the pull
  request and its status checks;
- every earlier run and how each ended;
- whether the issue itself is settled, which is not the same as a run being done.

## Today

- The issue's detail in the Issues tab has a *Runs* section (`runs_view`,
  `plugins/builtin/workbench-issues/src/view/detail.rs`): one line per task working the issue,
  `workflow · step or outcome`, a waiting one in the warning ink, five at most, each with *Show
  task*.
- What it is told is `IssueRun` (`crates/plugin-host/src/workbench.rs`): root, number, task id,
  workflow name, `at`, `waiting`, `working`. It is built by `task::issue_runs`
  (`crates/app/src/task.rs`) from `task::rows`, the same rows the Tasks page draws, and broadcast as
  `Request::IssueRuns` whenever a task starts, moves a step or ends.
- Only issues a project keeps (local or kept in step with a forge) get it; a forge's own issue is
  passed over.
- Everything else is one press away, but somewhere else: the task detail on the Tasks page has the
  timeline, the files each visit changed and their diffs; the approval waits on the step strip of
  the run's session; the pull request is only named in the report on the issue.
- A run does not keep its pull request: the verdict is looked up when the report is sent
  (`unattended::Verdict`).

## Proposal

### What the issue shows

Under the body, where *Runs* is now, a **Work** section. Its first block is the **latest task**,
drawn in full; earlier tasks and earlier runs are lines under it.

```
Work
  Work an issue · run 2                         Waiting for approval
  Plan ✓ › Implement ✓ › Verify ✓ › Approve ›  Push › Pull request › Status checks
  Next: approve the change, or send it back with a note      [Review…]
  ─
  Branch   onehand/local-12-fix-the-thing   3 commits past main
  Pull request  none yet
  Check    passed on 4f2c1e0, 2m ago
  Files    5 changed  +120 −14                                [Show files]
  ─
  Run 1 · exhausted at Implement · yesterday                  [Show]
  Show task
```

- **Head**: the workflow's name and the run's number, and on the right the task's state in the
  words of the Tasks page's groups (*Running*, *Queued*, *Waiting for approval*, *Waiting on a
  card*, or the outcome).
- **Steps**: the step strip's shape (done steps checked, the current one in full ink), read from
  the run's snapshot, so the issue and the session's strip cannot disagree. Clipped by width.
- **Next action**: one sentence and at most one button. See *The next action* below.
- **What the work left**: branch and commits past its base, pull request and its status checks,
  the check command's last outcome and the commit it passed on (`Marks::verified_at`), and the
  files changed across the run with their counts. Each line is drawn only when it has something to
  say (no *Pull request* line on a workflow without forge steps).
- **Earlier runs** of the same task, newest first, capped, each opening the task detail on that run.
- **Earlier tasks** of the same issue (an issue worked twice on two branches, after a merged pull
  request) as one line each, capped, under the latest.
- *Show task* stays, for the full timeline.

### The next action

A pure function in core, beside `Task::group`, from what a task already keeps and what the app
knows of it (`Working`):

| The task | Next action |
|---|---|
| Running | none; the step and how long it has worked |
| Queued | none; what it waits for (the place, or a slot; see [operations.md](operations.md)) |
| Waiting, approval | approve or revise (piece 4 brings these here; until then *Open session*) |
| Waiting, card | answer the card in the session (*Open session*) |
| Ended, resumable (agent stopped, session gone, cut off) | *Resume* |
| Ended, exhausted or timed out | look at the last visit, then *Retry* (piece 4 says from where) |
| Ended, failed on configuration (a mode not offered, no check command, a workflow that cannot run) | fix the configuration; names what, and where it is set |
| Ended, failed on the forge | the forge's reason, then *Retry* |
| Done, a pull request open | review it on the forge; put the label back to have a review answered |
| Done, no forge | look at the branch; close the issue when satisfied |
| Finished by a person (stopped, taken over, dismissed) | none |

"Failed on configuration" needs the failure to say so. Today `Outcome::Failed` carries a string;
this piece adds a reason kind beside it (configuration, forge, other) set where the failure is
made, never parsed out of the text.

### Two states, not one

The issue's head already shows *Open* or *Closed*. Beside it, when the latest task is done, the
issue reads one of:

- **Settled**: the issue is closed, or the pull request merged.
- **Done, not settled**: every step passed and the issue is still open, with why: *a pull request
  waits for review*, or *the branch waits for you* where no forge serves the project.
- Nothing more while a run works or after one ended otherwise; the next action says it.

A run *Done* on a still-open issue is the normal case, not a warning: it is the hand-over to a
person.

### Where the data comes from

| Line | Read from | Cost |
|---|---|---|
| State, steps, next action | the task and its last run (`Task`, `Run::visits`, `Run::outcome`), the app's `Working` | in memory, per frame like `task::rows` |
| Earlier runs and tasks | the task's runs; the tasks whose `IssueSource` names this issue | in memory |
| Branch and commits past base | the task's place and `IssueSource::base`, `git rev-list --count` | background, when the issue opens |
| Files changed | the run's first start mark to its last end mark (`task::marks`), the same read the task detail does per visit | background, when the issue opens |
| Check | `Marks::verified_at` and the last command visit's outcome | in memory |
| Pull request and status checks | the forge, by branch, as the verdict and `taking_blocking` already ask | background, when the issue opens, behind the connector; *could not tell* said, never *none* |

Nothing is stored on the issue. The one thing that may be worth keeping on the run is the pull
request's number once the forge step opened it, so that line needs no search by branch; that is a
field on the run, written by the engine on `forge_done`, not on the issue.

`IssueRun` grows into the summary the tab draws, or the tab asks for a task's summary through a
new `Request` when an issue opens; the second keeps the broadcast small. Either way the summary is
built in core from a `Task`, and the plugin draws it.

## What stays

- *Show task* and the task detail stay the full record; this section is a summary of them.
- Approving still happens on the run (piece 4 moves the buttons, not the rule).
- The Tasks page stays where every task in the workspace is seen; this is one issue's view.
- A forge's issue still gets no section until decision 2 of [the proposal](README.md) is taken.

## Open questions

1. Is a diff opened from the issue drawn in the Issues tab, or does *Show files* open the task
   detail at that run? Recommended: the task detail, until piece 5 gives the issue room.
2. How often is the pull request looked at while the issue is open on screen? Recommended: when
   it opens and when its task moves; never on a timer of its own.
3. Does the rail's `auto · #N` pill open the issue, now that the issue says more than the task row?

## Done when

- Every row of the next-action table is reachable with the mock workflow agent and the
  `mock_ui_agent`, and shows the sentence and the one button the table says.
- A done run on an open issue reads *Done, not settled*; closing the issue turns it *Settled*
  without a run moving.
- Pulling the network while an issue with a pull request is open says the pull request could not
  be read, and draws everything else.
- No field is added to `LocalIssue` or the issue's file.
- The core function behind the next action is matched exhaustively over `Outcome` and the groups,
  with a test per row of the table.

## Documents to change when built

- `DESIGN.md`: the Issues tab bullet under *Docks*.
- `docs/unattended.md`: the *Runs* paragraph becomes this section.
- `docs/tasks.md`: the next-action rule, beside the group rule.
- `CONTEXT.md`: *Settled*, *Next action*.
