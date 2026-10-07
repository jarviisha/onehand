# Piece 3: everything a start needs, checked before it claims or cuts anything

- Status: built in step 4 except the parts below, which the [README](README.md#order-of-work)
  assigns to steps 5 and 6. What is built lives in [workflows.md](../../workflows.md#starting-queueing-and-resuming)
  (the checks), [tasks.md](../../tasks.md#retry-and-resume) (Resume and Retry) and
  [unattended.md](../../unattended.md) (the start form, the search, the mode learned).
- Contracts it touches: the same documents, and the launcher in [DESIGN.md](../../../DESIGN.md).

## What is left

### Two kinds of start (step 5)

| Kind | Configuration judged | Place and base | What it already refuses |
|---|---|---|---|
| **Retry with current settings** | the task's workflow by id at its newest version; agent, mode, timeout and check command from where a new task of its kind takes them (piece 4) | as *Retry* | as *Retry*, and a workflow id no longer on offer |
| **Answer a pull request review** | the task's own snapshot and setup | the task's worktree, fast-forwarded to the forge's branch | no status checks step or no forge, pull request closed unmerged, branch gone its own way |

A Retry blocked by its own setup then offers *Retry with current settings* as the place it is
changed. Answering a review runs the preflight before its claim, like the other starts; its
refusals become findings of the *Issue* check, which needs the pull request's state as a fact.

### Check the agent (step 6)

**Check the agent**, a button beside *not known yet*, starts the adapter, reads `session/new` and
closes it, off the UI thread, saying what it found. Nothing is prompted. It is the one check that
costs an agent start, so it is never run unasked. What it learns goes where any sighting of the
agent's modes goes.

### The remaining informs rows (step 6)

| Check | Applies to | Informs |
|---|---|---|
| Place | new run, new issue run | a checkout workflow while a person's own session works in the same checkout |
| Forge | runs with forge steps | no forge serves the project: the forge steps pass at once, the branch is the result |
| Limits | all | the timeout and the misses allowed, from the snapshot that will run |
| Place taken | all | the start will queue, behind which task (the queue's notification says it today, once queued) |

The first needs a fact nothing tracks yet: which sessions work in which checkout.

## Done when

- A Retry of a run whose setup names a mode the agent no longer offers offers *Retry with current
  settings*.
- Answering a review is refused before its claim by the preflight, with each of today's refusals
  as a finding.
- *Check the agent* turns *not known yet* into a block or into nothing, without a prompt.
- `preflight` has a test per new row per kind it applies to.
