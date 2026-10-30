# Proposal: one lead session coordinating worker sessions

- Date: 2026-10-29, revised the same day after a review against the code.
- Status: proposal. Nothing here is a requirement until a task asks for it; once a piece is built,
  its document moves into the owning documents and leaves this folder. The contract this touches
  is [control.md](../../control.md).
- Source: a run on another repository (mmerp) on 2026-10-28, where one lead session opened three
  worker sessions through onehand's tools, approved their work stage by stage and merged three
  pull requests. The run is summarised below; the person's own notes on it are outside this repo.

## What happened

The lead prepared before opening anything: it handed out numbers the three branches would
otherwise race for (ADR numbers), checked which shared resources were safe to run in parallel
(test databases were, the end-to-end suite with its fixed database and ports was not), cut one
worktree per job and wrote a `LEAD.md` every worker read first: who works where, the scope of each
job and its border with the others, commands no worker may run, append-only rules for shared
files, and "open a pull request and stop, never merge".

It opened the workers with `list_projects`, `list_agents` and `open_session`, each with a first
prompt of the same frame (the job, read `LEAD.md`, the repository's process, the job's own
constraints, stop at the pull request). Once the person handed it the decisions, it moved every
question and approval to itself over Claude Code's `ListAgents` and `SendMessage`, and approved
each stage: questions, spec (reading the real files and checking claims), plan (recomputing the
numbers in the tests, setting checkpoints), the SHA at each checkpoint (diffing shared files and
trying the branches against each other with `git merge-tree`). Each pull request got the
end-to-end suite and an independent review by a read-only subagent given the risks but not the
lead's conclusions; fixes got a scoped re-review answering ADDRESSED or NOT ADDRESSED per finding.
With the person's permission it merged the three in order, merging `main` into each next branch
and fixing the semantic conflicts git could not see.

## What went wrong

- **Talking to a worker went outside onehand.** `open_session` answers with a sentence, not the
  session; the lead had to guess which of `mmerp-18`, `mmerp-03`, `mmerp-75` held which job, and
  every message after the first used Claude Code's own tools.
- **onehand closing ended the three running sessions and the lead's hold on them.** The
  conversations themselves are kept on disk and read back; what was lost was the open sessions,
  the turn in progress and which session the lead had opened for which job. The work survived
  because each worker had committed. How much of the rest a restart brought back was not
  examined at the time.
- **No model per worker.** All three ran the lead's model; a cheaper one for implementation could
  not be asked for.
- **The person could not follow the workers** without opening each from the rail. onehand.md
  section 8 moves them out of the rail, into a deck tucked behind the lead's composer and a region
  of their own in the Workbench's place.
- **Semantic conflicts surfaced only at merge**: a new scheduled job made a job-counting test
  flaky, and one branch forbidding an edit broke two tests of another. `git merge-tree` sees lines,
  not meaning.
- **The lead was wrong three times, each time by concluding before reading the code.**
- **The disk filled** and took the database down mid-run.

## What was decided

Three layers, each in the place that suits it:

1. **A skill for the lead**, independent of onehand and of any one agent: the method, its
   checklists, and a small file protocol between lead and workers. See [skill.md](skill.md).
2. **onehand as a host for workers**: a way to open a worker on a fresh worktree, a handle on it,
   the next turn and its state; after a restart the lead opens new workers. See [onehand.md](onehand.md).
3. **A workflow for workers**, later and optional.

The rule that **no tool answers a card, a permission or an approval** stays as it is. Approval of
a worker's stage is a record in the protocol's files, which the agents follow; nothing here needs
a tool to press *Continue*.

## The pieces

| # | Piece | Document | Step |
|---|---|---|---|
| 1 | The file protocol and the lifecycle of a worker | written: the `leading-worker-agents` skill, see [skill.md](skill.md); onehand's side in [onehand.md](onehand.md) | 1 |
| 2 | One worker through a whole cycle on the command line, the required cases below included | done on a scratch repository, every case that applies to the command line: see [skill.md](skill.md) | 2 |
| 3 | Opening a worker on a worktree, with the folders it may write and the mode unattended runs use; the handle, the state, the next turn, cancelling, giving back | [onehand.md](onehand.md), [spec.md](spec.md) | 3, a spec |
| 3b | The person's view of the workers: out of the rail, a deck tucked behind the lead's composer, a region in the Workbench's place | [onehand.md](onehand.md), [spec.md](spec.md) | 3 |
| 4 | A model per worker or per stage | per worker in [onehand.md](onehand.md) and [spec.md](spec.md), built with step 3; per stage later | 3, then 4 |
| 5 | A *Worker under a lead* workflow | not written | 5, optional |

Step 1 is written as a personal skill; step 3 has a spec, [spec.md](spec.md), whose PR 1 (the
worktree and its folders) PR 2 (the handle and the open) and PR 3 (`session_state`) are built.

## Order of work

1. **Settle the file protocol and the lifecycle and ownership of a worker** on paper: who writes
   what, how a stage is approved, who owns a worker when the lead or the person steps in.
2. **Prove one worker end to end** with the command-line transport for its whole life, no onehand
   piece needed.
3. **Write the spec** for onehand's part: adding the worktree and the folders it may write, the
   mode and the model per worker, the handle, state, cancelling, sending, giving back.
4. **A model per stage**, later, for agents that change model inside a session.
5. **The optional workflow.**

## Required cases

Every step above that they reach is checked against these:

- **A restart mid-stage**: the app or the lead stops after a worker wrote its status and before
  the lead read it, and again while a turn runs.
- **A stale approval**: a spec is approved, then revised or sent back; the old approval must not
  let the worker go on.
- **A lost answer to a send**: the lead does not hear back from sending a turn and tries again; the
  worker must not run the turn twice. The same for `open_session`: a retry must not open a second
  worker.
- **An old worker that may still run**: after a crash, or with the lead lost, no replacement is
  opened on its worktree until it is confirmed stopped.
- **A fix asked for at approval**: the lead asks for fixes, the worker makes them, and only the
  revision with the fixes is approved.
- **The person steps in**: the person types into a worker's session; the lead must not send over
  them.
- **A finished worker holding a place**: a worker done but still open must not block the next
  batch without anyone knowing why.
