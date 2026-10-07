# Piece 3: everything a start needs, checked before it claims or cuts anything

- Status: built in steps 4 and 5 except the parts below, which the
  [README](README.md#order-of-work) assigns to step 6. What is built lives in [workflows.md](../../workflows.md#starting-queueing-and-resuming)
  (the checks), [tasks.md](../../tasks.md#retry-and-resume) (Resume and Retry) and
  [unattended.md](../../unattended.md) (the start form, the search, the mode learned).
- Contracts it touches: the same documents, and the launcher in [DESIGN.md](../../../DESIGN.md).

## What is left

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

- *Check the agent* turns *not known yet* into a block or into nothing, without a prompt.
- `preflight` has a test per new row per kind it applies to.
