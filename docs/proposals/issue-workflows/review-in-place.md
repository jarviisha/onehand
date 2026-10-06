# Piece 4: approve, revise, resume or retry next to what is judged

- Status: proposal, wave 1. Builds on [issue-progress.md](issue-progress.md). Part of
  [the proposal](README.md).
- Contracts it touches: [tasks.md](../../../docs/tasks.md) (the task detail, *Retry and Resume*),
  [workflows.md](../../../docs/workflows.md) (the approval step), [unattended.md](../../../docs/unattended.md)
  (*Answering a review*), the step strip in [DESIGN.md](../../../DESIGN.md).

## Goal

A person looking after several issues judges and answers each one where its work is shown, without
opening its session and reading the transcript again.

## Today

- **Approval** is answered on the step strip of the run's session: *Review…* (the kept answer, from
  the run), *Revise…* (a note, refused empty), *Continue*. The task detail shows *Awaiting approval*
  with the answer's last 60 lines and *Open session*, and says it is answered there.
- **The diff** of each step visit is in the task detail; the check command's output is in the
  visit that ran it. The answer, the diff and the check are three places.
- **Resume** is offered for a resumable outcome, **Retry** for the rest, with a dialog that says
  where the retry starts and what it carries. Nothing says which of the two, or a configuration
  change, is the right move for a given ending.
- **A review is answered** by putting the trigger label back on the issue whose pull request is
  open; the task is retried from its repair step with the review as a revision note.

## Proposal

### One review block

Where a task waits for approval, the task detail and the issue's *Work* section draw one block:

- the answer under review, from the run (`Run::under_review`), as today;
- **what the work changed** since the step it approves started: the files with their counts, each
  opening its diff, read off the UI thread as the detail already does;
- **the check**, when a command step ran since: passed or failed, and its last lines;
- *Revise…* and *Continue*, the same actions the strip has, through the same driver path.

The rule does not change: approving is a person's act, and it reaches the run through the driver,
which may be in another window; the action goes to the window whose session drives the run, as the
strip's does. A run whose driver is not live (cut off while waiting) shows *Resume* instead, and
the approval comes back once it is up.

### Which way out, said

When a task has ended, the block under its head says which of the ways out fits, from the outcome,
before the buttons:

| Ended | Said | Offered |
|---|---|---|
| Agent stopped, session gone, cut off | the run can go on where it was, with its marks | *Resume* (primary), *Retry* |
| Exhausted at a step | the step missed too often; what its last visit ended on | *Retry* from that step (primary), *Retry* from an earlier one |
| Timed out | how long it worked against its timeout | *Retry*, and where the timeout is set |
| Failed on configuration | what is missing and where it is set (piece 1's reason kind) | the setting, then *Retry* |
| Failed on the forge | the forge's reason | *Retry* (it starts at the forge step) |
| The work changed by hand since | the retry dialog's *Changed* note, before it is opened | *Retry* |

This is the next-action function of [issue-progress.md](issue-progress.md#the-next-action), drawn
with its reason; one function, two views.

### Answer the review, as an action

On an issue, or its task, whose latest run is done with a pull request open: **Answer the review**.
It runs the path a re-added trigger label runs (`launch::taking_blocking` → fetch and fast-forward
→ `crate::task::retry` from `Template::repair_step` with `core::review_note`), with the same
refusals (a workflow without status checks, no forge, a pull request closed unmerged, a branch that
went its own way). The trigger label keeps working; this is a second door to the same path, not a
second path.

## What stays

- Nothing approves, answers a card or retries by itself.
- *Revise…* refuses an empty note; a revision is not a miss.
- The step strip keeps its controls; the block is the same actions in a second place.
- Retry's carry-over rule (`Run::retry_plan`) is unchanged; the block says what it will do.

## Open questions

1. Is *Continue* allowed from the issue when the run's session is in another window? Recommended:
   yes, as the strip's is; the action goes through the `Tasks` global to the live driver.
2. Does *Answer the review* need a confirmation? Recommended: no; it starts a run like *Run
   workflow…*, which asks none, and the preflight shows first.

## Done when

- With the mock workflow agent at an approval, *Continue* and *Revise…* from the task detail and
  from the issue move the run exactly as the strip's do, and the strip follows.
- The review block shows the plan, the files changed and the check output together.
- Each ending in the table above shows its sentence and its primary action.
- *Answer the review* on a done task with an open pull request starts a new run at the repair step,
  and is refused with the same words the label path uses.

## Documents to change when built

- `docs/tasks.md`: the task detail (*Awaiting approval* becomes the review block), *Retry and Resume*.
- `docs/unattended.md`: *Answering a review* (the action beside the label).
- `DESIGN.md`: the step strip bullet (the same actions also in the detail), the Issues tab bullet.
