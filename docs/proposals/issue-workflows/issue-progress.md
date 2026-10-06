# Piece 1: the issue says where its work stands

- Status: proposal, wave 1. Part A is built; part B is not. Part of [the proposal](README.md).
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

- Part A is built: the Issues tab draws an issue in five regions, its newest task's progress and
  next action above the body, the branch and pull request below it, and its earlier runs and tasks
  in *Before*. What it is told is `IssueWork` (`crates/core/src/task/work.rs`), keyed by
  `IssueKey`, broadcast as `Request::IssueWork`.
- What a run does not keep is listed in [the proposal](README.md#what-is-read-and-what-is-new): a
  failure's kind, a passed command's output, the pull request it opened.

## Two parts

**Part A, read-only**, is built first and stores nothing new. **Part B** adds the persistence the
richer lines need, with its compatibility rule. Part A draws every line part B fills as *not
recorded* or leaves it out, so B changes what is drawn, not the shape.

## Part A: built

Part A is built and lives in its owning documents: the order and what each region says in
[DESIGN.md](../../../DESIGN.md) (the Issues tab bullet under *Docks*) and
[unattended.md](../../unattended.md) (*An issue says where its work stands before what it says*),
the next-action rule beside the group rule in [tasks.md](../../tasks.md), *Next action* in
[CONTEXT.md](../../../CONTEXT.md), what it leaves out in [known-gaps.md](../../known-gaps.md), and
its checks as rows of the by-hand list in [workflows.md](../../workflows.md). What the Issues page
adds to the short form (the steps still to come, the check, the files changed and the commits past
the base in region 4, and *Open in Issues*) is built too, and lives in the same documents. What
remains here is part B.

## Part B: what runs start keeping

| Field | On | Written by | Read before it existed |
|---|---|---|---|
| Failure kind (configuration, forge, other) | the run, as `failure`, **beside** `Outcome::Failed(String)`, which keeps its shape | where the failure is made, by one rule: **a configuration failure is a preflight block found late** (the agent no longer configured, a mode not offered, an empty check command, a workflow that no longer validates); a forge step or `gh` failing is *forge*; anything else *other*. A test maps every block row of the preflight to *configuration* | absent → *other* |
| Pull request (number, url) | the run | the engine on `forge_done` from *Pull request* | absent; part A's lookup by branch still answers |
| Command result (passed or failed, exit, the tail of its output, the commit it ran on and the fingerprint of the uncommitted work, untracked files included, as `workflow::facts` takes it for a mark) | the command step's visit | the engine on `command_finished` | absent; *not recorded for this run* |

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

1. ~~Does the rail's `auto · #N` pill open the issue?~~ Yes, on the Issues page, as built
   ([unattended.md](../../unattended.md)). *Review…* keeps opening the run's session, whose step
   strip holds the approval, until the page can judge an answer in place (piece 4).
2. ~~Is focus-regained refresh worth its `gh` call per focus?~~ Yes, behind the one-minute age,
   and only for the issue on screen.

## Done when

Part A's lines are the by-hand rows in [workflows.md](../../workflows.md) and the tests beside
`task::work`. Part B:

- A task file from before part B, with an unsent report, reads; its lines say *not recorded*.
- A passed check shows its tail; a mode refusal reads as a configuration failure.

## Documents to change when built

- `docs/tasks.md`: part B's fields in *The model*.
- `docs/workflows.md`: part B, what the engine records on a command and a pull request.
