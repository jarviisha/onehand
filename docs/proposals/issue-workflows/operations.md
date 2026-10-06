# Piece 6: unattended runs when there are many issues

- Status: proposal, wave 2. Part of [the proposal](README.md).
- Contracts it touches: [unattended.md](../../../docs/unattended.md) (*Not built, on purpose*, `at_once`,
  the rule that worktrees are left on disk), [tasks.md](../../../docs/tasks.md) (*Worktrees are never
  removed by onehand*).

Every item here is in `unattended.md`'s *Not built, on purpose*, deliberately. This document says
what would change the answer for each, and the smallest build when it does. Building one moves it
out of that list in the same PR.

## 1. A slot kept when a window closes mid-lookup

**Today.** An issue task the tick or a pick has just kept goes into `Unattended::starting`
(`crates/app/src/unattended.rs`), counted against `at_once` until `placed` takes it out once it
waits for or holds its place. `launch.rs` takes it out when `window.update` fails at once. But a
window that closes *during* `task::request`'s place lookup (`task::queue::place_blocking`, run in
the background) leaves the task in `starting`: it never asks, so it never reaches `placed`, and the
slot is held until onehand restarts. Marked `ponytail:` at `crates/app/src/unattended/launch.rs`
(the task kept, then its place asked for).

**Fix.** The root is that `starting` is let go of on the paths that reach a place, and not on the
one that does not. One rule at the one place every outcome of a request passes: when
`task::request`'s lookup comes back to a window that is gone, it calls `unattended::placed` for the
task (and the task reads interrupted, under *Needs attention*, as after a restart). A test drives a
request whose window is dropped before the lookup returns and asserts the cap has room again.

**Size.** Small; worth doing in wave 1 if it is touched anyway.

## 2. Say what holds each slot, and why a run waits

**Today.** *Look for an issue now* says the cap is reached; a refused pick names the issues being
worked. The Tasks page shows queued tasks, but not whether one waits for its place or for a slot.

**Proposal.** One line on the Tasks page's *Queued* card and in Settings ▸ Workspace ▸ Unattended
runs: *Slots: 1 of 1 — #12 · Work an issue (Implement)*, each opening its task. A queued issue task
says *waits for a slot* or *waits for its place, behind …*. Read from `issues_working` and the queue;
nothing stored. This is the *Waiting* row of the preflight ([preflight.md](preflight.md)) shown after
the start.

## 3. A cap on waiting runs

**Today.** A run waiting on a card, an approval or its status checks gives its slot up, keeping its
adapter alive. `at_once` counts working runs only. Unbounded waiting runs means unbounded live
agents.

**When.** When a pile of unanswered runs is seen to cost something (memory, an adapter's own limit).

**Smallest build.** `[unattended] waiting = N` (unset: no cap). Past it the tick claims nothing new
and says why, the way the working cap does. It never stops, parks or answers a waiting run: the
person's answer is still what frees one. Parking a waiting run with no session (resumed when
answered) is the larger alternative already listed under *Parking a run while its status checks
run*, and is not this.

## 4. Clean up after a merge, on request

**Today.** Worktrees and branches are left on disk in every case: a half-done run has work in it.
A merged pull request is named in `unattended.md` as the first signal clear enough to act on.

**Proposal.** Never automatic. On a task whose pull request is merged (piece 1 reads it), the task
detail and the issue offer **Remove worktree…**, a destructive action in the danger tint with a
modal naming the folder and the branch. Before it asks, a check in core, off the UI thread:

- uncommitted or untracked files in the worktree;
- commits on the branch not on the forge's branch, or not in the merged pull request;
- a session of the workspace still open on the worktree.

Any of these is listed in the modal and the removal is refused until the person deals with it; a
session open there is closed first only by the person. The removal itself is `git worktree remove`
and `git branch -d` (never `-D`), and the task's marks stay until the history cap drops them.

## What stays

- Nothing here starts, stops, answers or removes anything a person did not ask for.
- `at_once` keeps meaning working runs.
- A worktree is never removed for a run that did not end merged.

## Done when

- Item 1: the test above, and the `ponytail:` comment gone.
- Item 2: a queued issue task says which it waits for.
- Item 3 and 4: each with its own *Checking it by hand* rows in `unattended.md`.

## Documents to change when built

- `docs/unattended.md`: *Not built, on purpose*, per item; *Config* for `waiting`.
- `docs/tasks.md`: *Worktrees are never removed by onehand* becomes *never removed unasked*.
