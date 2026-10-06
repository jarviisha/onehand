# Proposal: issues and workflows as one way of working

- Date: 2026-10-06, revised the same day after a review against the code, and again after a
  review of the layout: what a person must do comes before what the engine knows.
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
| 5 | Issues and workflows as pages of their own | [pages.md](pages.md) | the Issues page 1, the Workflows page 2 |
| 6 | Unattended runs when there are many issues | [operations.md](operations.md) | 2, except its first item |
| 7 | Workflows past a straight line | below | 3 |

How they fit together in the code, with the flows: [architecture.md](architecture.md).

## Order of work

Smaller first steps than the pieces suggest, because pieces 1 and 4 each need data the runs do not
keep yet (see *What is read and what is new* below).

1. ~~**The slot kept when a window closes mid-lookup**~~: a bug, fixed on its own. Built.
2. ~~**Issue progress, read-only** (piece 1, part A)~~: the next action, the step, the runs kept,
   and the header's one primary action, in the Issues tab's short form. Built.
3. ~~**The Issues page** (piece 5, the Issues half): the issues lifted into one owner, then the page
   with the list, the filters on progress and piece 1 in full. From here the dock is the glance and
   the page the place to work.~~ Built.
4. **Brief preview** (piece 2) and **preflight per kind of start** (piece 3), with *Run workflow…*
   opening on the issue it was pressed on.
5. **Review in place** (piece 4) on the page, with the approval guarded by the visit it was read
   from, and the persistence it needs (piece 1, part B). The dock offers *Review…*, which opens
   the page; the full review is never built in the dock first. It also takes **answering a pull
   request review** into the preflight as a kind of its own: step 4 left that kind's refusals
   where they are, since they already come before the claim and judging them in the preflight
   needs the pull request's state, which this step reads anyway.
6. Then the Workflows page (piece 5's other half) and the rest of operations (piece 6), with
   three parts step 4 left for it:
   - ***Check the agent*** ([preflight.md](preflight.md#the-mode-known-without-spending-an-issue)):
     the mode remembered from any time the agent comes up covers most starts, and the driver's
     check covers the rest, as before; the button needs a way to start an adapter with no session.
   - **The remaining informs rows** ([preflight.md](preflight.md#one-function-a-kind-in-its-input)):
     a checkout shared with a person's own session, which needs a fact nothing tracks yet; *no
     forge serves the project*; and *Limits*, which the issue's start form already shows.
   - **A project's own issue templates** ([issue-brief.md](issue-brief.md#templates-for-an-issue)):
     no project uses them yet; the body reader step 4 builds takes any template, so this is
     reading the files.

Each step is one pull request with its document changes in it.

**Specs.** Each step gets a spec issue before it is built, written from this folder as it stands
then, one step at a time and in this order. The next step to specify is the first without an
issue below; publishing one writes its number here. A step built moves its text into the owning
documents and its row here names the pull request.

| Step | Spec | Built |
|---|---|---|
| 1 | none: a bug fix, specified in [operations.md](operations.md) item 1 | [#91](https://github.com/jarviisha/onehand/pull/91) |
| 2 | [#90](https://github.com/jarviisha/onehand/issues/90) | [#96](https://github.com/jarviisha/onehand/pull/96) |
| 3 | [#97](https://github.com/jarviisha/onehand/issues/97) | [#104](https://github.com/jarviisha/onehand/pull/104) |
| 4 | [#105](https://github.com/jarviisha/onehand/issues/105) | |
| 5 | | |
| 6 | | |

## Five questions every layout answers

The pieces are judged by what a person can do, not by how much they are shown. Each question is a
*Done when* line in the piece that owns it.

| Question | Owned by |
|---|---|
| An issue with a very long description is opened: is it seen at once that it waits on me? | [issue-progress.md](issue-progress.md#done-when) |
| With many issues: can the ones that need me be found without opening each? | built: the Issues page rows of the by-hand list in [workflows.md](../../workflows.md#checking-it-by-hand) |
| A failed run is retried: is it clear where it starts again and which configuration it keeps? | [review-in-place.md](review-in-place.md#done-when) |
| A session or a diff is opened and left: is the same issue, filter and scroll still there? | built: *Coming back* in the by-hand list in [workflows.md](../../workflows.md#checking-it-by-hand) |
| The window is narrowed or zoomed in: can the action and the current step still be read? | [issue-progress.md](issue-progress.md#done-when); built for the page: *Narrow* in the by-hand list in [workflows.md](../../workflows.md#checking-it-by-hand) |

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
| **1: one issue, end to end** | 1, 2, 3, 4, the Issues page of 5, and item 1 of 6 | From one issue a person starts a run, sees where it is, judges its verdict and sends it back or lets it go, on the Issues page; the dock says where it stands and leads there. One thing still goes elsewhere, on purpose: a card is answered in the run's session |
| **2: many issues** | the Workflows page of 5, the rest of 6 | A person sees who holds each slot, and writes workflows on a page of their own |
| **3: richer workflows** | structured step outputs, steps that run only on a condition, an agent per step | The cases a straight-line workflow cannot serve, each named by a real workflow that needs it |

## Rules every piece keeps

These are the contracts the pieces must not bend; each document says where it comes close.

- **Progress is read, never stored twice.** What an issue shows of its work is worked out from its
  tasks and their runs, the way the Tasks page's groups are (`Task::group`, `task::rows`). No field
  on an issue repeats a run's step or outcome; a second copy is a copy that disagrees.
- **Three facts, shown apart, each where it is used.** The issue's state (open, closed) beside its
  title; the latest run's progress where the work stands; and, where a forge serves the project,
  its pull request's state (open, merged, closed, or *could not be read*) with what the work left. No word
  folds them into one: a run done on an open issue, an issue reopened after its pull request
  merged, and an issue closed without the work are all different, and each is said as its facts.
  A fact with nothing to say yet (no pull request before the step that opens one) is left out; one
  that should be there and cannot be read is said.
- **What to do before what is known.** An issue's view has a fixed order: who it is, where its
  work stands and what to do next, what it asks for, what the work left, the runs before. A state
  change changes what a region says, never the order of the regions, and never moves the selected
  row of a list.
- **One primary action.** Wherever an issue or a task is drawn, at most one action is primary, the
  one the next action names; the others sit quieter, in a place that does not move with the state.
- **Not recorded is not never.** Task history is capped, so an issue with no task found says *no
  run recorded*, never *never run*.
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
| **Next action** | The one thing a task waits on a person for, worked out from its group and outcome: answer a card, approve, resume, retry, change the configuration, look at the verdict. It names the primary action, or none | todo, call to action |
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

1. ~~Whether the issue's progress (piece 1) is drawn in the Issues tab first or waits for piece 5.~~
   Taken: the tab first, in its short form; the Issues page comes in wave 1, right after, and holds
   the full view and the review. The tab and the page are two views of one model, not one view
   shrunk (DESIGN.md's Issues tab and Issues page bullets).
2. ~~The principle the Issues page needs.~~ Taken: *The
   conversation is the centre while a session shows; pages take the agent pane and put the docks
   away. A page never opens over a session by itself; only a person picking it does.*
3. ~~Whether forge issues (not kept in onehand) get piece 1 too.~~ Taken: kept issues only. A
   forge's issue has no issues file, so no `IssueKey`; it goes into `docs/known-gaps.md` when
   piece 1 is built.
4. ~~Whether issue templates (piece 2) are per project files or shipped with onehand.~~ Taken:
   three shipped; a project's own `.github/ISSUE_TEMPLATE/*.md` replaces them whole.

Taken in the reviews of 2026-10-06, and written into the pieces: region 2 follows the newest task
of an issue; *Pull request open* is a progress filter of its own, not part of *Needs attention*;
the forge's review is always *pull request review*, and region 4 is *What the work left*, never
*Results*; before the Issues page exists the tab's *Review…* opens the run's session; *Retry*
loses *Retry with version N* to *Retry with current settings*; the Issues page reads pull
requests once per project, never per row; a configuration failure is a preflight block found late;
a re-added label still starts a new task, and its report names the earlier one left; project check
commands move to the project page. After a second review: a closed issue only gates a new start, never a
live run's actions; a check is judged against the commit and the uncommitted work it ran on; a
capped pull request read says what it did not read; the review block opens below region 2, only on
a press; an issue opened outside the page's filters is pinned, not unfiltered; *Needs attention*
takes an issue when any of its tasks needs it; an issue draft is kept, and asked about before it
is dropped.
