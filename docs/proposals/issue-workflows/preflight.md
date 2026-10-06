# Piece 3: everything a run needs, checked before it claims or cuts anything

- Status: proposal, wave 1. Part of [the proposal](README.md).
- Contracts it touches: [workflows.md](../../../docs/workflows.md) (*Starting, queueing and resuming*),
  [unattended.md](../../../docs/unattended.md) (*Config*, *The brief*), the launcher in
  [DESIGN.md](../../../DESIGN.md).

## Goal

A run that cannot finish is found out before it claims an issue, cuts a branch or starts an agent,
and the person sees what is missing in one list, at the place they start it from.

## Today

The checks exist; they are spread out and some come late.

| What | Checked today | When |
|---|---|---|
| The workflow is valid | `workflow::validate` | every keystroke of the launcher, before a run starts; problems in the danger ink, *Run* starts nothing |
| A checkout workflow for an issue | `unattended::launch::workflow` | before the claim |
| The check command, for a workflow that runs it | the project's row, the pick refused | before the claim |
| `gh` present and signed in, on a project GitHub serves | per project, on its row in the warning ink | at switch-on, window open, every tick |
| The slot (`at_once`) | `unattended::over_cap` in `task::request` | before the claim, and on Resume and Retry |
| Another run on the issue | `launch::taking_blocking` | before the claim |
| The agent offers the configured mode | the driver, when the agent first comes up | **after** the claim: the first issue is spent, and the feature pauses |
| The place is free | the queue | at the start; a taken place queues, which is not a failure |

## Proposal

### One preflight, one list

A pure function in core, `preflight`, takes what the start knows (the workflow, the project's
check command, the project's forge and its sign-in as last seen, the slot count, the tasks on the
issue, the agent spec and the modes last learned for it) and returns a list of findings, each:

- **what** (one of the rows below);
- **blocks** or **warns**: a block stops *Run*, a warning is said and the run may start;
- **where it is fixed**: the Settings page or the project menu entry that fixes it.

| Check | Blocks when | Warns when |
|---|---|---|
| Agent | the agent spec is gone | the configured mode was never seen offered by this agent (not learned yet) |
| Mode | the agent offered modes and this is not one | |
| Check command | a step runs the check command and the project has none | the workflow has no command step at all, so nothing verifies the work |
| Place | a worktree workflow on a folder that is not a git repository, or a detached HEAD with no forge | a checkout workflow while a person's own session works in the same checkout |
| Base | | where the branch will be cut from (`origin/<default>`, or the branch checked out) — said always, as information |
| Forge | the workflow has forge steps and `gh` is missing or signed out on a project GitHub serves | the workflow has forge steps and no forge serves the project: they will pass at once, the branch is the result |
| Issue | a run already works the issue; its pull request was closed unmerged | |
| Waiting | | the slot is full (it will wait, and for which runs); the place is taken (it will queue, behind which task) |
| Limits | | always said: the timeout (the run's, after `[unattended] timeout` is put over it) and the misses allowed |

The launcher and the issue picker draw the list where the launcher draws its problems today, blocks
in the danger ink and warnings muted, and *Run* refuses while a block remains. The tick runs the
same function before its claim; a block on the tick is said on the project's row, as `gh` problems
are now, and nothing is claimed.

### The mode, learned without spending an issue

Modes are known only from the agent's `session/new`. Two changes:

- **Remember what an agent offered** the last time it came up anywhere (per agent spec, in memory
  for the process, and in the config directory so a restart keeps it). The preflight blocks on a
  remembered list, and only warns when there is none.
- **Check the agent**, a button beside the warning, starts the adapter, reads `session/new`, and
  closes it, off the UI thread, saying what it found. Nothing is prompted. This is the one check
  that costs an agent start, so it is never run unasked.

With both, the first-issue-spent case in *The brief* of `unattended.md` happens only when nobody
has ever started that agent and nobody pressed *Check the agent*; the pause it leads to stays as it
is.

## What stays

- `workflow::validate` stays the workflow's own check, run where it runs now; the preflight calls
  it and lists its problems first, it does not repeat them.
- A taken place still queues and a full slot still waits; the preflight says so, it does not refuse.
- Nothing in the preflight reaches the network in a render: `gh`'s sign-in is the state last seen
  (`Connections` already keeps it), and *Check again* there refreshes it.

## Open questions

1. Is the remembered mode list kept on disk, or for the process only? Recommended: on disk, beside
   the instance lock; a stale list warns rather than blocks when the agent's version changed.
2. Does the Tasks page's *Resume* and *Retry* run the preflight too? Recommended: yes, the same
   function; a Retry dialog lists what it finds above its buttons.

## Done when

- With an agent whose mode was learned and is not offered, the issue picker refuses *Run* before
  any claim, naming the mode and what was offered, and the issue keeps its trigger label.
- With `gh` signed out, a workflow with forge steps is refused on a GitHub project and warned on a
  project no forge serves.
- A full slot and a taken place are both said, and the run still starts and waits.
- `preflight` has a test per row of the table.

## Documents to change when built

- `docs/workflows.md`: *Starting, queueing and resuming*.
- `docs/unattended.md`: *Config* (what refuses a run, and when), *The brief* (the first issue spent).
- `DESIGN.md`: the launcher bullet.
- `CONTEXT.md`: *Preflight*.
