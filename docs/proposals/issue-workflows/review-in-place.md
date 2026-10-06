# Piece 4: approve, revise, resume or retry next to what is judged

- Status: proposal, wave 1. Builds on [issue-progress.md](issue-progress.md), part B included, and
  on the Issues page of [pages.md](pages.md), where its block is drawn. Part of
  [the proposal](README.md).
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

**The run id is checked before the visit id goes on.** Visit ids count from 1 in every run, so
visit 4 of run 1 and visit 4 of run 2 are both "4". The `Tasks` global refuses an action whose task
has no live run, or whose live run is not the run id the action carries, and only then hands the
visit id to the driver and the engine. A view of a run a Retry has replaced is refused like a view
of a closed visit.

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

Where a task waits for approval, region 2 keeps its height: the summary and *Review…*. Pressing
*Review…* on the issue on the Issues page or in the task detail opens one block **below region 2**
([issue-progress.md](issue-progress.md#one-order-whatever-the-state)), pushing the body down; a
person did that, so it is not content moving under them. The engine never opens or closes it. The
Issues tab draws none: its *Review…* opens the issue on the page.

- **what approving starts**, beside *Continue*, read from the step after the approval in the run's
  snapshot: *Continue starts Implement: the agent edits the code*, *Continue starts Push*. Beside
  *Revise…*, the step under review by name: *Plan runs again with your note*. The person never
  works it out from the strip;
- the answer under review, from the run (`Run::under_review`). A long one opens in full in place;
  when what is shown is cut (the last 60 lines), the block says so above the actions, *showing the
  last 60 of 240 lines*, so nothing is approved half read without knowing it;
- **what the work changed** since the step it approves started: the files with their counts, each
  opening its diff, read off the UI thread as the detail already does. **Drawn only when there is
  something**: an approval after a plan that changed no file draws no files region and no check,
  rather than empty ones;
- **the check**, when a command step ran since: from piece 1 part B's command result (passed or
  failed, its tail, the commit and the fingerprint of the uncommitted work). Both are compared
  with the work now, read off the UI thread: the same says *passed on 4f2c1e0*; another commit
  says *passed on 4f2c1e0, 2 commits before the work now*; the same commit with other uncommitted
  or untracked work says *the work changed since the check passed*. So an old pass is never read
  as a pass of the code under review. A run from before part B has the commit only
  (`Marks::verified_at`): on a worktree with uncommitted work it says *cannot tell whether the
  check covers the work now*, and with no command run at all, *not recorded*;
- **the issue's acceptance**, collapsed, one click from the changes, so the work is judged against
  it without scrolling back to the body;
- *Revise…* and *Continue*, carrying the visit id above.

After *Continue* or *Revise…*, the block answers at once: the action shows it was sent, then what
came of it (*the run moved on to Implement*), and the block keeps its place and its scroll. It
closes only when the person closes it or picks another issue; it never collapses or jumps to the
top under the reader.

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

**The Retry dialog says what it will do before it does it**: the step it starts at and why (the
carry-over rule, or the step the person picked), and what it keeps, one line each: the agent, the
mode, the check command, the timeout and the workflow's version, all from the last run. Where one
of them differs from what Settings says now, the line says so and points at *Retry with current
settings*, so a person who changed Settings is not surprised by a retry that ignores it.

### Retry with current settings

A second action beside *Retry*, never a change to what *Retry* does. Its dialog shows, before
anything starts:

- **what changed**, one line each, old → new: the agent, the mode, the check command, the timeout,
  the workflow's version. *Retry with version N* leaves the *Retry* dialog for this one, so
  *Retry* means only "as the last run was";
- **where it starts**, the earlier of two points:
  - the carry-over rule (`Run::retry_plan`) for the workflow version it runs;
  - **the earliest step it would carry over whose command actually changes.** A command step's
    command is its own `command`, or, when it names none, the setup's check command. Each command
    step before the carry-over start is compared, old command against new, and the first that
    differs is where the retry starts. Not the last command step: in
    `Implement → Verify (project check) → Package (own command) → Push`, a changed project check
    starts the retry at **Verify**, since starting at Package would push work the new check never
    ran on. A step whose own command is unchanged is not a reason to go back;
  a changed agent, mode or timeout moves no start;
- the preflight for that configuration ([preflight.md](preflight.md), kind *Retry with current
  settings*).

**Which workflow it runs: the task's own, by id, at its newest version.** The workflow the last
run's snapshot names (`Template::id`) is looked up among the workflows on offer now, as *Retry with
version N* already does (`Template::newer_than`). An issue's labels or `[unattended] workflow` are
**not** asked again: they choose the workflow of a new task, and a retry is the same task. A
workflow whose id is gone, or that no longer validates, blocks, said in the preflight.

**The rest of current settings** come from where a new task of that kind takes them: for an issue
task, `[unattended]` (agent, mode, timeout) and the project's check command; for a launcher task,
Settings' default agent and the project's check command. *Answer the pull request review* keeps its task's own
configuration, as the label path does.

### Answer the pull request review, as an action

On an issue, or its task, whose latest run is done with a pull request open: **Answer the pull request review**.
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
- Beside *Continue* the block names the step it starts, from the snapshot; a plan approval with no
  file changed draws no files and no check; an answer longer than what is shown says it is cut
  before the actions; a check on a commit older than the worktree's head says how much older.
- After *Continue*, the block shows it was sent at once and the reader's scroll is where it was;
  the block stays open until closed by the person, and a run reaching an approval never opens it.
- A check passed, then a tracked file edited without a commit: the block says *the work changed
  since the check passed*; the same with only an untracked file added. A pre-part-B run on a dirty
  worktree says *cannot tell*. A test per case.
- A failed run's Retry dialog names the step it starts at and the agent, mode, check command,
  timeout and version it keeps; with Settings changed since, the differing lines say so.
- A run failed on a mode not offered: *Retry* fails again the same way, *Retry with current
  settings* shows the old and new mode and runs with the new one.
- With `Verify (project check) → Package (own command)`, a changed project check starts *Retry
  with current settings* at Verify; a changed Package command alone starts it at Package; neither
  changed leaves the carry-over start. A test per case, in core.
- An action carrying a visit id of an earlier run of the same task is refused.
- *Answer the pull request review* starts a new run at the repair step, refused with the label path's words.

## Documents to change when built

- `docs/tasks.md`: the task detail (*Awaiting approval* becomes the review block), *Retry and
  Resume* (the second action and what it re-checks, and what the Retry dialog says it keeps).
- `docs/workflows.md`: the approval step and the driver (actions carry the visit).
- `docs/unattended.md`: *Answering a review* (the action beside the label).
- `DESIGN.md`: the step strip bullet, the Issues tab bullet.
