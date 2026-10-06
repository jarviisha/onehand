# Piece 4: approve, revise, resume or retry next to what is judged

- Status: proposal, wave 1. Builds on [issue-progress.md](issue-progress.md), part B included.
  Part of [the proposal](README.md).
- Contracts it touches: [tasks.md](../../../docs/tasks.md) (the task detail, *Retry and Resume*),
  [workflows.md](../../../docs/workflows.md) (the approval step, the driver),
  [unattended.md](../../../docs/unattended.md) (*Answering a review*), the step strip in
  [DESIGN.md](../../../DESIGN.md).

## Goal

A person looking after several issues judges and answers each one where its work is shown, without
opening its session and reading the transcript again, and never approves something other than what
they read.

## Today

- **Approval** is answered on the step strip of the run's session: *Review…* (the kept answer, from
  the run), *Revise…* (a note, refused empty), *Continue*. The strip calls `driver::approve(uid)`
  (`crates/app/src/task/driver.rs`), which names the session only; the engine's `Run::approved` and
  `Run::revised` check only that the run waits for an approval.
- The task detail shows *Awaiting approval* with the answer's last 60 lines and *Open session*.
- **The diff** of each step visit is in the task detail; a failed command's output is in its
  visit; a passed command keeps no output.
- **Resume** is offered for a resumable outcome, **Retry** for the rest. `Run::retry_of` copies the
  last run's setup (agent, mode, check command) and, unless *Retry with version N* is picked, its
  workflow snapshot, which carries the timeout. A Retry after changing Settings runs the old
  configuration.
- **A review is answered** by putting the trigger label back on the issue whose pull request is
  open; the task is retried from its repair step with the review as a revision note.

## Proposal

### Approval names what it approves

Every approval action, from the strip, the task detail or the issue, carries **the task id, the run
id and the visit id of the approval step's open visit** it was drawn from. The engine takes them
(`Run::approved(visit)`, `Run::revised(visit, note)`) and answers `Idle` unless the run is waiting
for approval **at that visit**. The driver and the `Tasks` global route by task id rather than by
session uid, so a window other than the session's can act.

What that settles:

| Case | Without the id | With it |
|---|---|---|
| Window A reads the plan; B revises; the plan runs again and waits at a new approval visit; A presses *Continue* | A approves the new plan unread | refused: A's visit is no longer open. A reloads and shows the new answer |
| *Continue* pressed twice | the second may land on the next approval | the second names a closed visit, refused |
| *Continue* while the start mark of the next step is being pinned | waits for the pin, as today | the same; a second press during the pin names the closed visit, refused |
| The run cut off while waiting | nothing to approve | refused; *Resume* offered |

A refusal is not an error toast: the block reloads and says *the answer changed since you opened
it*. The strip goes through the same call, so there is one path.

### One review block

Where a task waits for approval, the task detail and the issue's *Work* section draw one block:

- the answer under review, from the run (`Run::under_review`);
- **what the work changed** since the step it approves started: the files with their counts, each
  opening its diff, read off the UI thread as the detail already does;
- **the check**, when a command step ran since: from piece 1 part B's command result (passed or
  failed, its tail, the commit). A run from before part B says *not recorded*;
- *Revise…* and *Continue*, carrying the visit id above.

### Which way out, said

When a task has ended, the block under its head says which way out fits, from the outcome:

| Ended | Said | Offered |
|---|---|---|
| Agent stopped, session gone, cut off | the run can go on where it was, with its marks | *Resume* (primary), *Retry* |
| Exhausted at a step | the step missed too often; what its last visit ended on | *Retry* from that step (primary), from an earlier one in the dialog |
| Timed out | how long it worked against the timeout its snapshot carries | *Retry* keeps that timeout; *Retry with current settings* when the workflow's or `[unattended]`'s changed |
| Failed, configuration (part B) | what was missing, in the run's setup | *Retry with current settings* (primary) |
| Failed, forge | the forge's reason | *Retry* (it starts at the forge step) |
| Failed, other | the failure's text | *Retry* |

This is the next-action function of [issue-progress.md](issue-progress.md#the-next-action), drawn
with its reason: one function, two views.

### Retry with current settings

A second action beside *Retry*, never a change to what *Retry* does. Its dialog shows, before
anything starts:

- **what changed**, one line each, old → new: the agent, the mode, the check command, the timeout,
  the workflow's version (already offered today as *Retry with version N*, folded in here);
- **where it starts**, by the carry-over rule (`Run::retry_plan`) for the workflow, and further:
  a changed check command holds the start at or before the last command step up to it, as
  `Run::recheck` does for changed work, since what the old command passed proves nothing about the
  new one. A changed agent, mode or timeout moves no start;
- the preflight for that configuration ([preflight.md](preflight.md), kind *Retry*).

For an issue task, current settings are `[unattended]` and the workflow its labels or default
choose now; for a launcher task, Settings' agent and the project's check command now. *Answer the
review* keeps its task's own configuration, as the label path does.

### Answer the review, as an action

On an issue, or its task, whose latest run is done with a pull request open: **Answer the review**.
It runs the path a re-added trigger label runs (`launch::taking_blocking` → fetch and fast-forward
→ `crate::task::retry` from `Template::repair_step` with `core::review_note`), with the same
refusals. The trigger label keeps working; this is a second door to the same path, not a second
path.

## What stays

- Nothing approves, answers a card or retries by itself.
- *Revise…* refuses an empty note; a revision is not a miss.
- *Retry* keeps the run's configuration; its carry-over rule is unchanged.
- The step strip keeps its controls, through the same guarded call.

## Done when

- Two windows on one run at an approval: B revises, the plan runs again; A's *Continue* is refused
  and A shows the new answer. A double press of *Continue* approves once. A press during a pin is
  refused once the visit closes. Each is a test against the engine and one against the driver.
- The review block shows the answer, the files changed and the check together; a pre-part-B run
  says *not recorded* for the check.
- A run failed on a mode not offered: *Retry* fails again the same way, *Retry with current
  settings* shows the old and new mode and runs with the new one.
- A changed check command moves *Retry with current settings*' start back to the last command step.
- *Answer the review* starts a new run at the repair step, refused with the label path's words.

## Documents to change when built

- `docs/tasks.md`: the task detail (*Awaiting approval* becomes the review block), *Retry and
  Resume* (the second action and what it re-checks).
- `docs/workflows.md`: the approval step and the driver (actions carry the visit).
- `docs/unattended.md`: *Answering a review* (the action beside the label).
- `DESIGN.md`: the step strip bullet, the Issues tab bullet.
