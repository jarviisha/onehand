# Piece 6: unattended runs when there are many issues

- Status: proposal; item 1 in wave 1, the rest in wave 2. Part of [the proposal](README.md).
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

**Today.** Worktrees and branches are left on disk in every case: a half-done run has work in it.
A merged pull request is named in `unattended.md` as the first signal clear enough to act on.

**Proposal.** Never automatic. On a task whose pull request is merged (piece 1 reads it), the task
detail and the issue offer **Remove worktree…**, a destructive action in the danger tint with a
modal naming the folder and the branch. A check in core, off the UI thread, runs **when the modal
opens and again when it is confirmed**, since anything can change between the two:

- uncommitted or untracked files in the worktree;
- commits on the branch past the head the forge says the pull request merged (below);
- anything of onehand's still using the folder, **across every window**: a task holding or
  waiting for its place there, a session on it, a terminal or a Neovim whose directory is in it, a
  command step still running there.

Any finding is listed and the removal refused until the person deals with it; onehand closes
nothing on the person's behalf.

**The branch, after the forge's merge.** `git branch -d` refuses a branch whose commits are not
reachable from its upstream or `HEAD`, which is the usual state after a squash or rebase merge, and
the forge's branch may already be deleted. So the branch is judged by the forge instead: the
pull request's merged head commit, as the forge reports it, against the local branch's head.

| Local branch head | Forge's remote branch | What happens |
|---|---|---|
| the merged head | any | the worktree is removed, and the branch with `git branch -D`, the modal saying the forge's merged head is why |
| past the merged head | any | the worktree is refused (commits past the merge are work); nothing removed |
| the forge cannot be read | any | refused, said; nothing removed |
| `-D` itself fails | | the worktree stays removed, the branch is kept, and the failure is said |

The task's marks stay until the history cap drops them.

## What stays

- Nothing here starts, stops, answers or removes anything a person did not ask for.
- `at_once` keeps meaning working runs, and a full slot keeps refusing.
- A worktree is never removed for a run that did not end merged.

## Done when

- Item 4: with its own *Checking it by hand* rows, with a
  case for a squash merge, a deleted remote branch, and a terminal open in the worktree in
  another window.

## Documents to change when built

- `docs/unattended.md`: *Not built, on purpose*, for item 4.
- `docs/tasks.md`: *Worktrees are never removed by onehand* becomes *never removed unasked*.
