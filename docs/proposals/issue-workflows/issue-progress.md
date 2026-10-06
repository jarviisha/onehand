# Piece 1: the issue says where its work stands

- Status: proposal, wave 1, the first piece to build. Part of [the proposal](README.md).
- Contracts it touches: [DESIGN.md](../../../DESIGN.md) (the Issues tab line under *Docks*),
  [unattended.md](../../../docs/unattended.md) (the *Runs* paragraph), [tasks.md](../../../docs/tasks.md)
  (the task detail, which this reuses), and for part B the task file's schema.

## Goal

Open an issue and know, without going anywhere else:

- which workflow is working it, and at which step;
- whether it is running, queued, waiting on a person or ended, and **what the person should do next**;
- what the work left: the branch, the pull request, the check, the files changed;
- every earlier run and how each ended;
- the issue's own state beside all of it, never folded into the run's.

## Today

- The issue's detail in the Issues tab has a *Runs* section (`runs_view`,
  `plugins/builtin/workbench-issues/src/view/detail.rs`): one line per task working the issue,
  `workflow · step or outcome`, a waiting one in the warning ink, five at most (`RUNS_SHOWN`), each
  with *Show task*.
- What it is told is `IssueRun` (`crates/plugin-host/src/workbench.rs`): root, number, task id,
  workflow name, `at`, `waiting`, `working`. It is built by `task::issue_runs`
  (`crates/app/src/task.rs`) from `task::rows`, the same rows the Tasks page draws, and broadcast as
  `Request::IssueRuns` whenever a task starts, moves a step or ends.
- Only issues a project keeps (local or kept in step with a forge) get it; a forge's own issue is
  passed over.
- The task detail on the Tasks page has the timeline, the files each visit changed and their diffs;
  the approval waits on the step strip of the run's session; the pull request is only named in the
  report on the issue.
- What a run does not keep is listed in [the proposal](README.md#what-is-read-and-what-is-new): a
  failure's kind, a passed command's output, the pull request it opened.

## Two parts

**Part A, read-only**, is built first and stores nothing new. **Part B** adds the persistence the
richer lines need, with its compatibility rule. Part A draws every line part B fills as *not
recorded* or leaves it out, so B changes what is drawn, not the shape.

## Part A: what the issue shows

Under the body, where *Runs* is now, a **Work** section. Its first block is the **latest task**,
drawn in full; earlier runs and earlier tasks are lines under it.

```
Work                                                   read 2m ago  [Refresh]
  Issue: Open   Run: waiting for approval   Pull request: none yet
  Work an issue · run 2
  Plan ✓ › Implement ✓ › Verify ✓ › Approve ›  Push › Pull request › Status checks
  Next: approve the change, or send it back with a note    [Open session]
  ─
  Branch   onehand/local-12-fix-the-thing   3 commits past main
  Check    passed on 4f2c1e0
  This run changed  5 files  +120 −14                       [Show in task]
  ─
  Run 1 · exhausted at Implement · yesterday                [Show]
  Show task
```

- **The three facts**, on one line: the issue (*Open*, *Closed*), the latest run (its group or
  outcome, in the Tasks page's words), and the pull request (*none yet*, *open, draft*, *open*,
  *merged*, *closed unmerged*, *could not be read*), the last only on a project a forge serves.
  Nothing combines them.
- **Steps**: the step strip's shape, read from the run's snapshot, so the issue and the strip
  cannot disagree. Clipped by width.
- **Next action**: one sentence and at most one button (below).
- **Branch** and commits past its base (`IssueSource::base`).
- **Check**: from `Marks::verified_at` only, *passed on <commit>*, or the last failed command
  visit's `output`. A pass's output is part B.
- **Files changed**, two scopes said apart, because a retry that starts at *Push* changes nothing
  itself:
  - *This run changed*: the run's first start mark to its last end mark;
  - *The branch*: base to the worktree's head, the task's whole work.
  Both read off the UI thread; *Show in task* opens the task detail at that run.
- **Earlier runs** of the task, newest first, capped, each opening the task detail on that run.
- **Earlier tasks** of the same issue, one line each, capped.

### The next action

A pure function in core, beside `Task::group`, from what a task keeps and what the app knows of it
(`Working`), matched exhaustively over the groups and `Outcome`:

| The task | Next action |
|---|---|
| Running | none; the step and how long it has worked |
| Queued | none; the task holding its place. A task never queues for a slot: a full slot refuses the start |
| Waiting, approval | approve or revise; *Open session* until piece 4 brings the actions here |
| Waiting, card | answer the card in the session (*Open session*) |
| Ended, resumable (agent stopped, session gone, cut off) | *Resume* |
| Ended, exhausted or timed out | look at the last visit, then *Retry* (piece 4 says from where) |
| Ended, failed | the failure's text, then *Retry*; part B tells a configuration failure apart, and piece 4 offers *Retry with current settings* for it, since a plain retry keeps the run's setup |
| Done, pull request open | review it on the forge; put the label back to have a review answered |
| Done, pull request merged | none; the issue's own state says whether anything is left |
| Done, pull request closed unmerged | the pull request was closed unmerged; **reopen the pull request** (not the issue) and put the label back to have it answered, as `launch::taking_blocking` says |
| Done, a forge serves the project, the workflow has no pull request step | look at the branch on the forge; the branch is the result. Not *could not be read*: no pull request is the expected end |
| Done, no forge | look at the branch; close the issue when satisfied |
| Done, pull request could not be read | *Refresh* |
| Finished by a person (stopped, taken over, dismissed) | none |

### Refresh, and how old it is

What is read off the network or the disk (the pull request, branch counts, files changed) carries
the time it was read, drawn as *read 2m ago*, and a *Refresh* beside it. It is read:

- when the issue is opened;
- when its task moves (the `Request::IssueRuns` broadcast);
- on *Refresh*;
- when the window regains focus with the issue on screen, if what is shown is older than a minute.

Never on a timer of its own. A pull request merged on the forge after the run ended is seen at the
next of these. What failed to read keeps the last value, drawn as stale with the failure beside it,
so *could not be read* is said on a request that failed, not inferred from silence.

**Each read is keyed to what asked for it, and to when.** A request carries:

- **the issue's identity**: the issues file it is kept in (`issues::file_for`) and its number, the
  pair `task::issue_runs` already matches on. Never the project and number alone: two workspaces
  can open one project and each keep an issue 12 of its own;
- **the task id and the run id** the read is about, so a Retry that keeps the task but starts a new
  run makes every read of the old run stale;
- **a generation**, bumped every time a read is sent, whatever the reason.

Only an answer carrying the current generation is taken; any other is dropped. That covers a
switch to another issue and two refreshes of the same issue answering out of order alike: the
later request's answer wins even when the earlier one lands last.

## Part B: what runs start keeping

| Field | On | Written by | Read before it existed |
|---|---|---|---|
| Failure kind (configuration, forge, other) | the run, as `failure`, **beside** `Outcome::Failed(String)`, which keeps its shape | where the failure is made: the mode refusal, the missing check command, the forge steps | absent → *other* |
| Pull request (number, url) | the run | the engine on `forge_done` from *Pull request* | absent; part A's lookup by branch still answers |
| Command result (passed or failed, exit, the tail of its output, the commit it ran on) | the command step's visit | the engine on `command_finished` | absent; *not recorded for this run* |

The output tail is capped as a visit's output is (the last 60 lines shown).

**Compatibility, as the task file works today.** A task file has no `schema_version`: `Task` is
written and read with serde directly (`task::files`), and no task type refuses unknown keys. So:

- **No existing shape changes.** `Outcome` stays as it is, `{"Failed": "reason"}` included; the
  kind is a separate optional field on the run, `#[serde(default, skip_serializing_if =
  "Option::is_none")]`, as `Setup::mode` and `Setup::forge` were added. Changing `Failed(String)`
  into `Failed { kind, why }` would make every older file unreadable, and is not done.
- **An older file reads** with every new field absent.
- **An older build reading a newer file** ignores the new fields, and drops them the next time it
  saves that task. Nothing refuses it, since nothing can: no version is written today, and adding
  one now would only protect against builds after it. `tasks.md` already accepts no way back to an
  older build in this pre-release; the loss is these fields, never the task.
- **The outcomes in unsent reports** (`IssueSource::unsent`, `PendingReport::outcome`) are
  `Outcome` too, and are covered by the same rule; they gain nothing.

Tests: a task file from before part B, with a failed run and an unsent report, reads, saves and
reads again unchanged; a file with the new fields round-trips; the failure kind absent reads as
*other*.

## What stays

- *Show task* and the task detail stay the full record; this section is a summary of them.
- Approving still happens on the run (piece 4 moves the buttons, not the rule).
- The Tasks page stays where every task in the workspace is seen; this is one issue's view.
- No field is added to `LocalIssue` or the issue's file.
- A forge's issue still gets no section until decision 2 of [the proposal](README.md) is taken.

## Open questions

1. Does the rail's `auto · #N` pill open the issue, now that the issue says more than the task row?
2. Is focus-regained refresh worth its `gh` call per focus? Recommended: yes, behind the
   one-minute age, and only for the issue on screen.

## Done when

Part A:

- Every row of the next-action table is reachable with the mock workflow agent and shows its
  sentence and button.
- A done run on an open issue shows *Issue: Open*, *Run: done*, *Pull request: open*; closing the
  issue changes only the first; merging on the forge changes the third at the next refresh.
- With the network pulled, *Refresh* says the pull request could not be read and keeps the rest.
- Switching issues while a read is in flight never shows the first issue's answer on the second;
  two refreshes of one issue answering out of order show the later request's answer; a Retry
  drops reads of the run before it.
- The next-action function has a test per row.

Part B:

- A task file from before part B, with an unsent report, reads; its lines say *not recorded*.
- A passed check shows its tail; a mode refusal reads as a configuration failure.

## Documents to change when built

- `DESIGN.md`: the Issues tab bullet under *Docks*.
- `docs/unattended.md`: the *Runs* paragraph becomes this section.
- `docs/tasks.md`: the next-action rule, beside the group rule; part B's fields in *The model*.
- `docs/workflows.md`: part B, what the engine records on a command and a pull request.
- `CONTEXT.md`: *Next action*.
