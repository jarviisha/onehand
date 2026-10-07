# Piece 6: unattended runs when there are many issues

- Status: items 1 to 4 built (item 1 in wave 1, the rest in step 6); what they became lives in
  [unattended.md](../../unattended.md) and [tasks.md](../../tasks.md). Part of
  [the proposal](README.md).
- Contracts it touches: [unattended.md](../../../docs/unattended.md) (*Not built, on purpose*, `at_once`,
  the rule that worktrees are left on disk), [tasks.md](../../../docs/tasks.md) (*Worktrees are never
  removed by onehand*).

Every item here is in `unattended.md`'s *Not built, on purpose*, deliberately. This document says
what would change the answer for each, and the smallest build when it does. Building one moves it
out of that list in the same PR.

## 1. A slot kept when a window closes mid-lookup

Built: `task::request` lets the cap go when its window is gone by the time the place lookup
returns. It left this proposal and the *Not built* list of `unattended.md`.

## 2. Say who holds each slot

Built: the slots line in a refusal, the preflight and Settings, and the task a
queued one waits behind ([unattended.md](../../unattended.md)).

### A queue for slots, not in this proposal

Turning a full slot into a wait is a change to the start, not a status line, and is left for its
own proposal. It would have to settle, at least:

- **when the claim happens**: before the wait (the label is gone, the issue is told a run started
  while nothing runs) or after (the issue can be taken by another start meanwhile);
- **what holds the slot**: whether a task waiting for a slot counts against the cap of waiting
  runs (item 3);
- **cancelling**: *Stop* on a task waiting for a slot, and what the issue is told;
- **a restart**: whether a slot wait survives it, given that nothing starts by itself after one;
- **the order**: first in first out across windows and projects, against the pinned-project order
  the tick uses today.

## 3. A cap on waiting runs

Built: `[unattended] waiting` ([unattended.md](../../unattended.md)).

## 4. Clean up after a merge, on request

Built: *Remove worktree…* ([unattended.md](../../unattended.md#the-report),
[tasks.md](../../tasks.md)).

## What stays

- Nothing here starts, stops, answers or removes anything a person did not ask for.
- `at_once` keeps meaning working runs, and a full slot keeps refusing.
- A worktree is never removed for a run that did not end merged.

## Done when

Items 1 to 4 are built. What is left here is the queue for slots above, which is not part of this
proposal.
