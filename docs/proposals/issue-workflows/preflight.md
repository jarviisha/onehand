# Piece 3: everything a start needs, checked before it claims or cuts anything

- Status: built in steps 4, 5 and 6, except *Place taken* below. What is built lives in
  [workflows.md](../../workflows.md#starting-queueing-and-resuming) (the checks and *Check the
  agent*), [tasks.md](../../tasks.md#retry-and-resume) (Resume and Retry) and
  [unattended.md](../../unattended.md) (the start form, the search, the mode learned). Part of
  [the proposal](README.md).

## What is left

| Check | Applies to | Informs |
|---|---|---|
| Place taken | all | the start will queue, behind which task (the queue's notification says it today, once queued) |

It needs git in the render to find the place; it is tracked with step 4's leftovers in issue #113.
