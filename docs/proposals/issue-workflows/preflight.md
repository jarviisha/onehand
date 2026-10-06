# Piece 3: everything a start needs, checked before it claims or cuts anything

- Status: proposal, wave 1. Part of [the proposal](README.md).
- Contracts it touches: [workflows.md](../../../docs/workflows.md) (*Starting, queueing and resuming*),
  [tasks.md](../../../docs/tasks.md) (*Retry and Resume*), [unattended.md](../../../docs/unattended.md)
  (*Config*, *The brief*, *Answering a review*), the launcher in [DESIGN.md](../../../DESIGN.md).

## Goal

A start that cannot finish is found out before it claims an issue, cuts a branch or starts an
agent, and the person sees what is missing in one list, where they start it from. The list judges
**the configuration that will actually run**, which for a Resume or a Retry is the run's own, not
what Settings says now.

## Today

The checks exist; they are spread out and some come late.

| What | Checked today | When |
|---|---|---|
| The workflow is valid | `workflow::validate` | every keystroke of the launcher, before a run starts; problems in the danger ink, *Run* starts nothing |
| A checkout workflow for an issue | `unattended::launch::workflow` | before the claim |
| The check command, for a workflow that runs it | the project's row, the pick refused | before the claim |
| `gh` present and signed in, on a project GitHub serves | per project, on its row in the warning ink | at switch-on, window open, every tick |
| The slot (`at_once`) | `unattended::over_cap` in `task::request` | before the claim, and on Resume and Retry: **refused**, never queued |
| Another run on the issue | `launch::taking_blocking` | before the claim |
| The agent offers the configured mode | the driver, when the agent first comes up | **after** the claim: the first issue is spent, and the feature pauses |
| The place is free | the queue | at the start; a taken place queues, which is not a failure |

## Kinds of start

Each kind of start has its own rules today, and the preflight keeps them apart rather than judging
every start by one table. What is checked comes from the kind:

| Kind | Configuration judged | Place and base | What it already refuses |
|---|---|---|---|
| **New run** (launcher) | the workflow picked, Settings' agent, the project's check command | checkout: the project as it is. Worktree: a new `workflow/<title>` cut off the project's `HEAD` (`shell/workflows.rs`) | validation problems |
| **New issue run** (pick or tick) | the workflow by pick, label or default; `[unattended]` agent, mode and timeout | a new worktree cut off `origin/<default>` after a fetch with a forge, else off the branch checked out; a detached HEAD without a forge is refused | checkout workflow, missing check command, slot, issue already worked, mode refused once learned |
| **Resume** | the run's own snapshot and setup, unchanged | the run's existing place; the folder added back if it left the workspace | issue task past the slot |
| **Retry** | the last run's setup (`Run::retry_of` copies it) and its snapshot; a newer version is *Retry with current settings* | the task's existing place; another branch checked out refuses, changed work is said | issue task past the slot |
| **Retry with current settings** | the task's workflow by id at its newest version; agent, mode, timeout and check command from where a new task of its kind takes them (piece 4) | as *Retry* | as *Retry*, and a workflow id no longer on offer |
| **Answer a pull request review** | the task's own snapshot and setup | the task's worktree, fast-forwarded to the forge's branch | no status checks step or no forge, pull request closed unmerged, branch gone its own way |

## Proposal

### One function, a kind in its input

A pure function in core, `preflight(kind, facts)`. `kind` is one of the six above; `facts` is what
the app knows when the start is asked for: the configuration that kind runs with (above), the
project's forge and its sign-in as last seen, the slot count and who holds the slots, the tasks on
the issue, and what is known of the agent's modes (below). It returns findings, each:

- **what** (one of the rows below);
- **blocks** or **informs**: a block stops the start, information is said and the start goes on;
- **where it is changed**: the Settings page or menu entry, or, for a Resume or Retry whose own
  setup is the problem, *Retry with current settings* (piece 4).

**The preflight decides; it does not claim or cut.** Who carries a start out stays as it is: for a
new issue run, `unattended::launch` claims the issue and cuts the worktree, and for a new worktree
run the launcher cuts it (`shell/workflows.rs`); each then keeps the task and calls
`task::request`. Resume and both Retries call `task::request` on a task whose place already
exists, and never claim or cut. Answering a review goes through `unattended::launch` too: it claims
the issue (the label off, a comment saying it answers the review) and fast-forwards the task's
existing worktree, but cuts nothing. The preflight runs first in each of those callers, before
anything of theirs.

| Check | Applies to | Blocks when | Informs |
|---|---|---|---|
| Workflow | new run, new issue run, Retry with a newer version, Retry with current settings | `workflow::validate` finds problems (listed first, not repeated) | |
| Agent | all | the agent the setup names is no longer configured | |
| Mode | runs with a mode | the agent's current offer (below) does not hold it | the mode is not known yet |
| Check command | runs with a command step naming none | the setup's check command is empty | a workflow with no command step: nothing verifies the work |
| Place | new run, new issue run | a worktree workflow on a folder outside git; for an issue run, a detached HEAD with no forge | a checkout workflow while a person's own session works in the same checkout |
| Base | new run (worktree), new issue run | | where the branch is cut from, by that kind's rule |
| Forge | runs with forge steps | `gh` missing or signed out on a project GitHub serves | no forge serves the project: the forge steps pass at once, the branch is the result |
| Issue | new issue run, answer a pull request review | what `taking_blocking` refuses today | |
| Earlier task | new issue run | | the issue's last task ended under *Needs attention*: a new start makes a second task; *Retry* that one instead is offered. A tick has nobody to read this, so its new task's report says it instead: *an earlier task on this issue ended exhausted at Implement; its worktree is kept*. Whether a re-added label should retry rather than start anew is a change to `unattended.md`, not made here |
| Slot | issue runs of every kind | `at_once` reached: refused, naming the runs that hold the slots | |
| Place taken | all | | the start will queue, behind which task |
| Limits | all | | the timeout and the misses allowed, from the snapshot that will run |

The launcher, the issue picker and the Retry dialog draw the list where the launcher draws its
problems today, blocks in the danger ink and information muted; the start refuses while a block
remains. The tick runs the same function, kind *new issue run*, before its claim; a block is said
on the project's row, as `gh` problems are now, and nothing is claimed.

### The mode, known without spending an issue

Modes are known only from the agent's `session/new`. A list once learned does not prove the
adapter still offers it, so what is remembered has an owner and an end:

- **Learned per agent spec**: the command, arguments and environment the spec holds, compared
  whole. Editing the spec forgets what was learned for it.
- **Kept for the process only.** A restart forgets, so an adapter upgraded between runs is never
  judged by an old list. On disk was weighed and left out for that reason.
- **Replaced every time the agent comes up anywhere**, a person's session included.
- **It blocks only when it is current**: learned in this process, from the same spec. Otherwise
  the mode is *not known yet*, which informs, and the driver's check when the agent comes up stays
  the authority, as today.
- **Check the agent**, a button beside *not known yet*, starts the adapter, reads `session/new`
  and closes it, off the UI thread, saying what it found. Nothing is prompted. It is the one check
  that costs an agent start, so it is never run unasked.

The first-issue-spent case in *The brief* of `unattended.md` remains for a tick when nothing has
learned the mode since the start of onehand; the pause it leads to stays as it is.

### Starting from an issue

Today *Run workflow…* on an issue opens the issue picker narrowed to that one issue
(`dialogs::pick_issue` with `only`): a workflow menu, then the issue as a row, and clicking the row
is what starts it. The issue is chosen already, so the form drops the row and reads top to bottom
in the order a person decides:

1. **The workflow**, the menu as today (*By the issue's labels* first), with the picked one's
   description in one muted line.
2. **Where it works**: the worktree and the branch it will be cut as, off which base (the *Base*
   finding), and the agent that will run it.
3. **What blocks**, the preflight's findings for kind *new issue run*, blocks first. While one
   remains, *Run* is disabled and says why beside it.
4. **Instructions for this run**, optional ([issue-brief.md](issue-brief.md#the-brief-shown-and-added-to)).
5. **Preview**, collapsed: the steps, and the first prompt filled with this issue's brief. The
   limits (timeout, misses allowed) are one line above it, not inside.
6. **Run** in the footer, the primary action, with *Cancel* beside it.

Once started, the dialog closes and the person stays on the issue, whose region 2 shows the run
starting with *Open session* among its actions; the session is no longer put on screen in their
place, as the picker does today.
*Open session* leads there, and the rail's pill leads back ([pages.md](pages.md#coming-back)).

The launcher, started from the rail with no issue, keeps its own form; only the issue's start
changes.

## What stays

- `workflow::validate` stays the workflow's own check, run where it runs now.
- A taken place still queues; a full slot still refuses. The preflight says each, it does not
  change either.
- Nothing in the preflight reaches the network in a render: `gh`'s sign-in is the state last seen
  (Settings ▸ Connections keeps it), and *Check again* there refreshes it.

## Done when

- With an agent whose mode was learned in this process and is not offered, the issue picker
  refuses before any claim, naming the mode and what was offered, and the issue keeps its trigger
  label. After the agent spec is edited, the same mode reads *not known yet*.
- A Retry of a run whose setup names a mode the agent no longer offers is blocked in the Retry
  dialog, with *Retry with current settings* offered; changing Settings alone does not clear it.
- A new run on a worktree says it is cut off `HEAD`; a new issue run says `origin/<default>`; a
  Resume says nothing of a base.
- A full slot refuses an issue start and names who holds the slots; a taken place informs and the
  start queues.
- `preflight` has a test per row per kind it applies to.
- *Run workflow…* on an issue opens on that issue with no row to pick, in the order above; a
  block disables *Run* and says why; once started, the issue stays on screen showing the run.

## Documents to change when built

- `docs/workflows.md`: *Starting, queueing and resuming*.
- `docs/tasks.md`: *Retry and Resume* (the preflight in the dialog).
- `docs/unattended.md`: *Config* (what refuses a run, and when), *The brief* (when the first issue
  is still spent).
- `DESIGN.md`: the launcher bullet.
- `CONTEXT.md`: *Preflight*.
