# Proposal: issues and workflows as one way of working

- Date: 2026-10-06.
- Status: proposal. Nothing here is a requirement until a task asks for it; once a piece is built,
  its document moves into the owning documents and leaves this folder. The contracts are
  [CONTEXT.md](../../../CONTEXT.md), [DESIGN.md](../../../DESIGN.md), [workflows.md](../../../docs/workflows.md),
  [tasks.md](../../../docs/tasks.md) and [unattended.md](../../../docs/unattended.md).
- Builds on: issues × workflows (PR #89), which gave the Issues tab *Run workflow…* and a *Runs*
  section, and workflow labels beside the trigger label.

## The gap

The parts are there. An issue can be searched, labelled, kept in step with a forge and have its
conflicts settled. A workflow has approval steps, a check command, Resume and Retry, a diff per step
visit and the forge steps up to a pull request. What is missing is the way through them: to start
an issue, follow it, judge what came back and send it round again, a person moves between the
Issues tab, the launcher, the Tasks page, the task detail, the step strip and the run's session.

The goal of the whole proposal, in one sentence: **from an issue, a person can start the work,
follow it, judge what it left and send it back, without going looking for it.**

## The pieces

| # | Piece | Document | Wave |
|---|---|---|---|
| 1 | The issue says where its work stands | [issue-progress.md](issue-progress.md) | 1 |
| 2 | An issue written to be worked, and the brief shown before it is | [issue-brief.md](issue-brief.md) | 1 |
| 3 | Everything a run needs, checked before it claims or cuts anything | [preflight.md](preflight.md) | 1 |
| 4 | Approve, revise, resume or retry next to what is judged | [review-in-place.md](review-in-place.md) | 1 |
| 5 | Issues and workflows as pages of their own | [pages.md](pages.md) | 2 (a design decision first) |
| 6 | Unattended runs when there are many issues | [operations.md](operations.md) | 2 |
| 7 | Workflows past a straight line | below | 3 |

**If only one is built next, it is 1.** It uses the engine as it is, reads only what tasks already
keep, and is the piece the other three in wave 1 hang off: 3 shows its answer before the run, 4
acts inside the view 1 draws, and 2 shapes what 1 reports against.

## Waves

| Wave | Scope | Done when |
|---|---|---|
| **1: one issue, end to end** | 1, 2, 3, 4 | From one issue a person starts a run, sees where it is, judges its verdict and sends it back or lets it go, without opening the Tasks page or the run's session unless they choose to |
| **2: many issues** | 5, 6, plus filters on progress | A person sees which issues need them and which can go on alone, and why a run is waiting for a slot |
| **3: richer workflows** | structured step outputs, steps that run only on a condition, an agent per step | The cases a straight-line workflow cannot serve, each named by a real workflow that needs it |

Following the one-PR-per-feature rule, each piece is one pull request with its document changes
in it; piece 1 may be split if it passes a reviewable size, along the line its document draws.

## Rules every piece keeps

These are the contracts the pieces must not bend; each document says where it comes close.

- **Progress is read, never stored twice.** What an issue shows of its work is worked out from its
  tasks and their runs, the way the Tasks page's groups are (`Task::group`, `task::rows`). No field
  on an issue repeats a run's step or outcome; a second copy is a copy that disagrees.
- **A run's outcome is not the issue's state.** *Done* means every step passed. Whether the issue
  is settled is the issue's own state (open or closed) and, where a forge serves it, its pull
  request's (merged, closed, open). Each piece that shows one shows the other beside it and never
  folds them into one word. See [issue-progress.md](issue-progress.md#two-states-not-one).
- **Core decides, the app draws.** Every rule (what the next action is, what a preflight finds,
  what an issue is missing) is a pure function in core with its tests, as `Task::group` is.
- **Nothing reaches the network or the disk in a render.** A pull request's state, a diff, a
  preflight's `gh` call are read on the background executor, and the view says while they are read.
- **A parked ask still waits for a person**, and answering a card is still not taking over. Nothing
  here answers a card or approves a step on anybody's behalf.
- **Bounded, and said.** Every new list has a named cap and says what it left out.
- **Vocabulary.** The words are the glossary's: run, task, step visit, outcome, verdict, report,
  brief, check command, place, slot, Resume, Retry. New words this proposal needs are listed below
  and go into the glossary in the PR that builds them, not before.

## Words this proposal would add

| Word | Meaning | Avoid |
|---|---|---|
| **Settled** (an issue) | The issue is closed, or its pull request merged; what a person is after, as against a run being done | resolved, fixed, complete |
| **Next action** | The one thing a task waits on a person for, worked out from its group and outcome: answer a card, approve, resume, retry, fix the configuration, look at the verdict | todo, call to action |
| **Preflight** | What onehand checks before a run claims an issue or cuts a worktree, and what it found | precheck, validation (that is the workflow's own) |
| **Acceptance** (in a brief) | How a person will judge the work, written into the issue; read by the agent, never checked by onehand as a gate | definition of done, criteria on their own |

## Wave 3, in short

Not specified yet, deliberately: each needs a workflow somebody actually wants before its shape is
worth deciding. `workflows.md` already lists the straight line as not built. The candidates:

- **Structured step outputs**: a step that keeps a value (a list of files, a yes/no) other steps or
  gates can read, rather than only prose.
- **A step that runs only when**: a condition on an earlier step's kept value or a fact (code
  changed, a label on the issue).
- **An agent per step**: today one session carries every step. A step on another agent means a
  second session on the same place, and a hand-over of what the first kept.

## Decisions to take before wave 1

1. Whether the issue's progress (piece 1) is drawn in the Issues tab first and moves to the page of
   piece 5 later, or waits for piece 5. Recommended: the tab first; piece 5 then mounts the same
   view in more room.
2. Whether forge issues (not kept in onehand) get piece 1 too. Today the *Runs* section shows only
   issues a project keeps, since a forge's numbers are its own. Recommended: kept issues first.
3. Whether issue templates (piece 2) are per project files or shipped with onehand. Recommended:
   shipped, with a project's own `.github/ISSUE_TEMPLATE/` read when it has one.
