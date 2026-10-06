# Piece 1: the issue says where its work stands

- Status: proposal, wave 1, the first piece to build. Part of [the proposal](README.md).
- Contracts it touches: [DESIGN.md](../../../DESIGN.md) (the Issues tab line under *Docks*),
  [unattended.md](../../../docs/unattended.md) (the *Runs* paragraph), [tasks.md](../../../docs/tasks.md)
  (the task detail, which this reuses), and for part B the task file's schema.

## Goal

Open an issue and know, first and without going anywhere else, **whether it waits on you and what
to do**; then:

- which workflow is working it, and at which step;
- whether it is running, queued, waiting on a person or ended;
- what the work left: the branch, the pull request, the check, the files changed;
- every earlier run and how each ended;
- the issue's own state beside all of it, never folded into the run's.

## Today

- The issue's detail in the Issues tab has a *Runs* section (`runs_view`,
  `plugins/builtin/workbench-issues/src/view/detail.rs`): one line per task working the issue,
  `workflow · step or outcome`, a waiting one in the warning ink, five at most (`RUNS_SHOWN`), each
  with *Show task*.
- What it is told is `IssueRun` (`crates/plugin-host/src/workbench.rs`): root, number, task id,
  workflow name, `at`, `waiting`, `working`. It is built by `task::issue_runs`
  (`crates/app/src/task.rs`) from `task::rows`, the same rows the Tasks page draws, and broadcast as
  `Request::IssueRuns` whenever a task starts, moves a step or ends.
- Only issues a project keeps (local or kept in step with a forge) get it; a forge's own issue is
  passed over.
- The task detail on the Tasks page has the timeline, the files each visit changed and their diffs;
  the approval waits on the step strip of the run's session; the pull request is only named in the
  report on the issue.
- What a run does not keep is listed in [the proposal](README.md#what-is-read-and-what-is-new): a
  failure's kind, a passed command's output, the pull request it opened.

## Two parts

**Part A, read-only**, is built first and stores nothing new. **Part B** adds the persistence the
richer lines need, with its compatibility rule. Part A draws every line part B fills as *not
recorded* or leaves it out, so B changes what is drawn, not the shape.

## Part A: what the issue shows

### One order, whatever the state

An issue's view is five regions, always in this order, so a person coming back to an issue finds
what it waits on before its description, however long that is:

1. **Who it is**: the project, the issue's number, its title, and its state (*Open*, *Closed*)
   beside the title. The issue's state is drawn here and nowhere else.
2. **Where the work stands**: the latest run of the issue's newest task (by when it last moved)
   in one line, the next action's sentence,
   and the one primary action (below). Nothing when no run is recorded and the issue is open,
   except *Run workflow…* as the primary action.
3. **What it asks for**: the body, its acceptance included.
4. **What the work left**: the branch, the check, the files changed, the pull request.
5. **Before**: earlier runs and earlier tasks.

An issue can hold several tasks: a pick on an issue whose last task ended, even under *Needs
attention*, makes a new task beside it (`taking_blocking` refuses only a working one). The newest
task is the issue's work; an older one under *Needs attention* stays in region 5, in the warning
ink, and the preflight says so before a new start (its *Earlier task* row).

A run moving on changes what regions 2 and 4 say; it never reorders the regions, inserts one above
the body that was not there, or scrolls the view. Region 2 keeps its height while a task is working
(one line of progress, one of next action, the actions), so the body does not jump under a reader
when a step ends.

On the Issues page ([pages.md](pages.md)), with room:

```
onehand · #42 · Fix session reconnect                                   Open
Waiting for approval · Plan · step 1 of 6
Approving starts Implement: the agent edits the code.
[Review plan]                                   Open session   Stop   ⋯
─
Description
  Problem, scope, acceptance…
─
What the work left                                        read 2m ago  [Refresh]
  Branch   onehand/local-42-fix-reconnect   3 commits past main
  Check    passed on 4f2c1e0
  This run changed  5 files  +120 −14                            [Show in task]
─
Before
  Run 1 · exhausted at Implement · yesterday                     [Show]
```

In the Issues tab, the short form, in the same order: region 2 drops the step list for the one line
and adds *Open in Issues*; region 4 keeps the branch and the pull request only; region 5 is one line
with a count. The tab never draws the review block (piece 4): its primary action at an approval is
*Review…*, which opens the issue on the page.

```
#42 · Fix session reconnect                         Open
Waiting for approval · Plan · step 1 of 6
[Review…]                             Open in Issues  ⋯
─
Problem, scope, acceptance…
```

These sketches show the order, not a finished design; sizes and inks are the theme's.

### Where each fact goes

- **The issue's state** beside the title (region 1).
- **The run's progress** in region 2: its group or outcome in the Tasks page's words, then the
  step, then where the step stands in the workflow, *Verify · step 3 of 6*. The current step is
  never clipped away: where there is room the strip's shape is drawn after it, read from the run's
  snapshot so the issue and the strip cannot disagree, and where there is not, the one line stays
  and the full list opens from it. No percentage: a step that sends the work back makes one go
  down.
- **The pull request** in region 4, only on a project a forge serves: *open, draft*, *open*,
  *merged*, *closed unmerged*, or *could not be read*. Before the run reaches the step that opens
  one, or for a workflow without one, nothing is drawn; a pull request that should be there and
  cannot be read is said, in the warning ink.
- **Branch** and commits past its base (`IssueSource::base`).
- **Check**: from `Marks::verified_at` only, *passed on <commit>*, or the last failed command
  visit's `output`. A pass's output is part B.
- **Files changed**, two scopes said apart, because a retry that starts at *Push* changes nothing
  itself:
  - *This run changed*: the run's first start mark to its last end mark;
  - *The branch*: base to the worktree's head, the task's whole work.
  Both read off the UI thread; *Show in task* opens the task detail at that run.
- **Earlier runs** of the task, newest first, capped, each opening the task detail on that run.
- **Earlier tasks** of the same issue, one line each, capped.

### The next action

A pure function in core, beside `Task::group`, from what a task keeps, what the app knows of it
(`Working`) and the issue's state, matched exhaustively over the groups and `Outcome`. It returns
the sentence and the primary action; the secondary actions come with it, so the header and the
tab cannot each pick their own.

| The task | Said | Primary | Secondary |
|---|---|---|---|
| No run recorded, issue open | *no run recorded* | *Run workflow…* | *Work here*, *Edit* |
| Running | the step, step N of M, how long it has worked | none | *Open session*, *Stop* |
| Running, at the step that waits for status checks | waiting for the forge's checks, not for you | none | *Open pull request*, *Stop* |
| Queued | the task holding its place. A task never queues for a slot: a full slot refuses the start | none | *Show task* |
| Waiting, approval | what approving starts next (piece 4) | *Review…* | *Open session* |
| Waiting, card | the agent asks something in its session | *Open session to answer* | *Stop* |
| Ended, resumable (agent stopped, session gone, cut off) | it can go on where it was | *Resume* | *Retry…* |
| Ended, exhausted or timed out | the step and what its last visit ended on | *Retry…* (piece 4 says from where) | *Show task* |
| Ended, failed | the failure's text | *Retry…*; for a configuration failure (part B), *Retry with current settings*, since a plain retry keeps the run's setup | *Show task* |
| Done, pull request open | review it on the forge | *Open pull request* | *Answer the pull request review* (piece 4) |
| Done, pull request merged | none; the issue's own state says whether anything is left | none | *Run workflow…* |
| Done, pull request closed unmerged | the pull request was closed unmerged; **reopen the pull request** (not the issue) and put the label back to have it answered, as `launch::taking_blocking` says | *Open pull request* | *Show task* |
| Done, a forge serves the project, the workflow has no pull request step | the branch is the result. Not *could not be read*: no pull request is the expected end | *Open branch* | *Show task* |
| Done, no forge | look at the branch; close the issue when satisfied | none | *Show task* |
| Done, pull request could not be read | what failed | *Refresh* | *Show task* |
| Finished by a person (stopped, taken over, dismissed) | none | none | *Run workflow…* |
| Any task, issue closed | the task's line as above, muted | none | *Reopen issue*, *Show task* |

- *Work here* opens a session on the project's checkout as it is, with no workflow; its menu entry
  says so in one line, so it is not taken for a second way to run a workflow.
- *Edit*, *Publish to …* and *Reopen issue* are always secondary, wherever the state puts them.
- The secondary actions sit in one place that does not move: after the primary one, then ⋯ for
  what does not fit. With no primary action the place is kept empty, not filled by a secondary
  one, so a button is not where another was a moment ago.

### Refresh, and how old it is

What is read off the network or the disk (the pull request, branch counts, files changed) carries
the time it was read, drawn as *read 2m ago*, and a *Refresh* beside it. It is read:

- when the issue is opened;
- when its task moves (the `Request::IssueRuns` broadcast);
- on *Refresh*;
- when the window regains focus with the issue on screen, if what is shown is older than a minute.

Never on a timer of its own. A pull request merged on the forge after the run ended is seen at the
next of these. What failed to read keeps the last value, drawn as stale with the failure beside it,
so *could not be read* is said on a request that failed, not inferred from silence.

**Each read is keyed to what asked for it, and to when.** A request carries:

- **the issue's identity**: the issues file it is kept in (`issues::file_for`) and its number, the
  pair `task::issue_runs` already matches on. Never the project and number alone: two workspaces
  can open one project and each keep an issue 12 of its own;
- **the task id and the run id** the read is about, so a Retry that keeps the task but starts a new
  run makes every read of the old run stale;
- **a generation**, bumped every time a read is sent, whatever the reason.

Only an answer carrying the current generation is taken; any other is dropped. That covers a
switch to another issue and two refreshes of the same issue answering out of order alike: the
later request's answer wins even when the earlier one lands last.

## Part B: what runs start keeping

| Field | On | Written by | Read before it existed |
|---|---|---|---|
| Failure kind (configuration, forge, other) | the run, as `failure`, **beside** `Outcome::Failed(String)`, which keeps its shape | where the failure is made, by one rule: **a configuration failure is a preflight block found late** (the agent no longer configured, a mode not offered, an empty check command, a workflow that no longer validates); a forge step or `gh` failing is *forge*; anything else *other*. A test maps every block row of the preflight to *configuration* | absent → *other* |
| Pull request (number, url) | the run | the engine on `forge_done` from *Pull request* | absent; part A's lookup by branch still answers |
| Command result (passed or failed, exit, the tail of its output, the commit it ran on) | the command step's visit | the engine on `command_finished` | absent; *not recorded for this run* |

The output tail is capped as a visit's output is (the last 60 lines shown).

**Compatibility, as the task file works today.** A task file has no `schema_version`: `Task` is
written and read with serde directly (`task::files`), and no task type refuses unknown keys. So:

- **No existing shape changes.** `Outcome` stays as it is, `{"Failed": "reason"}` included; the
  kind is a separate optional field on the run, `#[serde(default, skip_serializing_if =
  "Option::is_none")]`, as `Setup::mode` and `Setup::forge` were added. Changing `Failed(String)`
  into `Failed { kind, why }` would make every older file unreadable, and is not done.
- **An older file reads** with every new field absent.
- **An older build reading a newer file** ignores the new fields, and drops them the next time it
  saves that task. Nothing refuses it, since nothing can: no version is written today, and adding
  one now would only protect against builds after it. `tasks.md` already accepts no way back to an
  older build in this pre-release; the loss is these fields, never the task.
- **The outcomes in unsent reports** (`IssueSource::unsent`, `PendingReport::outcome`) are
  `Outcome` too, and are covered by the same rule; they gain nothing.

Tests: a task file from before part B, with a failed run and an unsent report, reads, saves and
reads again unchanged; a file with the new fields round-trips; the failure kind absent reads as
*other*.

## What stays

- *Show task* and the task detail stay the full record; regions 2, 4 and 5 are a summary of them.
- Approving still happens on the run (piece 4 moves the buttons, not the rule).
- The Tasks page stays where every task in the workspace is seen; this is one issue's view.
- No field is added to `LocalIssue` or the issue's file.
- A forge's issue still gets no section (decision 3 of [the proposal](README.md): kept issues only).

## Open questions

1. ~~Does the rail's `auto · #N` pill open the issue?~~ Once the Issues page exists, yes, on the
   page ([pages.md](pages.md#coming-back)). Before it, the pill and the tab's *Review…* open the
   run's session, whose step strip holds the approval; the page's PR moves both.
2. ~~Is focus-regained refresh worth its `gh` call per focus?~~ Yes, behind the one-minute age,
   and only for the issue on screen.

## Done when

Part A:

- Every row of the next-action table is reachable with the mock workflow agent and shows its
  sentence, its one primary action and its secondary ones in their fixed place.
- An issue whose body is several screens long, waiting for an approval, shows *Waiting for
  approval* and *Review…* without scrolling, in the tab and on the page.
- A step ending while the body is being read moves nothing under the reader: the regions keep
  their order and region 2 its height.
- A done run on an open issue shows *Open* beside the title, *Done* in region 2 and *open* on the
  pull request in region 4; closing the issue changes only the first; merging on the forge changes
  the third at the next refresh. A run not yet at its pull request step draws no pull request line.
- At the dock's narrowest width and at the largest zoom step, the primary action and the current
  step (*Verify · step 3 of 6*) are still drawn whole; the step list is what gives way.
- With the network pulled, *Refresh* says the pull request could not be read and keeps the rest.
- Switching issues while a read is in flight never shows the first issue's answer on the second;
  two refreshes of one issue answering out of order show the later request's answer; a Retry
  drops reads of the run before it.
- The next-action function has a test per row.

Part B:

- A task file from before part B, with an unsent report, reads; its lines say *not recorded*.
- A passed check shows its tail; a mode refusal reads as a configuration failure.

## Documents to change when built

- `DESIGN.md`: the Issues tab bullet under *Docks*: the issue's header is its title and state, then
  one primary action from the next action, with *Work here*, *Run workflow…*, *Edit* and ⋯ no
  longer drawn side by side at one weight.
- `docs/unattended.md`: the *Runs* paragraph becomes this section.
- `docs/tasks.md`: the next-action rule, beside the group rule; part B's fields in *The model*.
- `docs/workflows.md`: part B, what the engine records on a command and a pull request.
- `CONTEXT.md`: *Next action*.
