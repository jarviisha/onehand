# Proposal: issues and workflows as one way of working

- Date: 2026-10-06, revised the same day after a review against the code.
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
| 3 | Everything a start needs, checked before it claims or cuts anything | [preflight.md](preflight.md) | 1 |
| 4 | Approve, revise, resume or retry next to what is judged | [review-in-place.md](review-in-place.md) | 1 |
| 5 | Issues and workflows as pages of their own | [pages.md](pages.md) | 2 (a design decision first) |
| 6 | Unattended runs when there are many issues | [operations.md](operations.md) | 2, except its first item |
| 7 | Workflows past a straight line | below | 3 |

How they fit together in the code, with the flows: [architecture.md](architecture.md).

## Order of work

Smaller first steps than the pieces suggest, because pieces 1 and 4 each need data the runs do not
keep yet (see *What is read and what is new* below).

1. **The slot kept when a window closes mid-lookup** ([operations.md](operations.md#1-a-slot-kept-when-a-window-closes-mid-lookup)):
   a bug, fixed on its own.
2. **Issue progress, read-only** (piece 1, part A): state, current step, the runs kept, the next
   action. Nothing new stored.
3. **Brief preview** (piece 2) and **preflight per kind of start** (piece 3).
4. **Review in place** (piece 4), with the approval guarded by the visit it was read from, and the
   persistence it needs (piece 1, part B).
5. Then pages (piece 5) and the rest of operations (piece 6).

Each step is one pull request with its document changes in it.

## What is read and what is new

| Today, kept and readable | Not kept, so new persistence |
|---|---|
| a task's runs, their snapshots, setup, step, outcome, visits and marks | why a run failed, as a kind (configuration, forge, other): `Outcome::Failed` is a string |
| a failed command's output, as its visit's `output` | a passed command's output: a passed visit keeps none |
| `Marks::verified_at`, the commit the last passed command ran on | which command passed and when, as a record per command visit |
| the issue's own open or closed | the pull request a run opened: the verdict looks it up by branch when the report is sent |

Piece 1 part A uses the left column only. Everything in the right column is piece 1 part B, with
its compatibility rule: a task file written before it reads with the new fields absent, and the
view says *not recorded for this run*, never a guess.

## Waves

| Wave | Scope | Done when |
|---|---|---|
| **1: one issue, end to end** | 1, 2, 3, 4, and item 1 of 6 | From one issue a person starts a run, sees where it is, judges its verdict and sends it back or lets it go. Two things still go elsewhere, on purpose: a card is answered in the run's session, and a file's diff opens in the task detail, until piece 5 gives the issue room |
| **2: many issues** | 5, the rest of 6, filters on progress | A person sees which issues need them and which can go on alone, and who holds each slot |
| **3: richer workflows** | structured step outputs, steps that run only on a condition, an agent per step | The cases a straight-line workflow cannot serve, each named by a real workflow that needs it |

## Rules every piece keeps

These are the contracts the pieces must not bend; each document says where it comes close.

- **Progress is read, never stored twice.** What an issue shows of its work is worked out from its
  tasks and their runs, the way the Tasks page's groups are (`Task::group`, `task::rows`). No field
  on an issue repeats a run's step or outcome; a second copy is a copy that disagrees.
- **Three facts, shown apart.** The issue's state (open, closed), the latest run's outcome, and,
  where a forge serves the project, its pull request's state (open, merged, closed, or *could not
  be read*). No word folds them into one: a run done on an open issue, an issue reopened after its
  pull request merged, and an issue closed without the work are all different, and each is said
  as its facts.
- **Retry keeps the run's configuration.** `Run::retry_of` copies the last run's setup (agent,
  mode, check command) and, by default, its workflow snapshot. Running with what Settings says now
  is a different action, shown with what changed (piece 4).
- **A full slot refuses; only a place queues.** An issue pick, a Resume or a Retry past `at_once` is
  refused today (`unattended::over_cap` in `task::request`); the queue holds tasks waiting for their
  place only. These pieces say who holds the slot; they do not turn the refusal into a wait. That
  would be its own change (see [operations.md](operations.md#a-queue-for-slots-not-in-this-proposal)).
- **Core decides, the app draws.** Every rule (the next action, a preflight's findings, what an
  issue body lacks) is a pure function in core with its tests, as `Task::group` is.
- **Nothing reaches the network or the disk in a render.** A pull request's state, a diff, a
  preflight's `gh` call are read on the background executor; the view says while they are read and
  how old what it shows is.
- **A parked ask still waits for a person**, and answering a card is still not taking over. Nothing
  here answers a card or approves a step on anybody's behalf.
- **Bounded, and said.** Every new list has a named cap and says what it left out.
- **Vocabulary.** The words are the glossary's: run, task, step visit, outcome, verdict, report,
  brief, check command, place, slot, Resume, Retry. New words go into the glossary in the PR that
  builds them, not before.

## Words this proposal would add

| Word | Meaning | Avoid |
|---|---|---|
| **Next action** | The one thing a task waits on a person for, worked out from its group and outcome: answer a card, approve, resume, retry, change the configuration, look at the verdict | todo, call to action |
| **Preflight** | What onehand checks before a start claims an issue, cuts a worktree or starts an agent, and what it found | precheck, validation (that is the workflow's own) |
| **Acceptance** (in a brief) | How a person will judge the work, written into the issue; read by the agent, never checked by onehand as a gate | definition of done, criteria on their own |

A word for "the issue is done with" (closed, or its pull request merged) was considered and left
out: the cases it would fold together (reopened, closed unworked, pull request closed unmerged, a
forge that cannot be read) are exactly what a person needs told apart.

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
