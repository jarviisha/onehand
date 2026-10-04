# Workflows

A run takes a brief through the steps of a workflow: an agent step prompts the
session and is judged by gates onehand checks itself, a command step runs a command in the work, and
an approval step waits for a person. The words are defined in [CONTEXT.md](../CONTEXT.md).

## Where the code is

| Part | Where |
|---|---|
| Workflow types and serde | [crates/core/src/workflow/template.rs](../crates/core/src/workflow/template.rs) |
| What stops a workflow being saved or run | [crates/core/src/workflow/validate.rs](../crates/core/src/workflow/validate.rs) |
| Prompt variables, onehand's additions, carry-on prompts | [crates/core/src/workflow/prompt.rs](../crates/core/src/workflow/prompt.rs) |
| The two shipped workflows | [crates/core/src/workflow/builtin/](../crates/core/src/workflow/builtin/) |
| The person's workflows on disk | [crates/core/src/workflow/store.rs](../crates/core/src/workflow/store.rs) |
| The engine: one run as pure state | [crates/core/src/workflow/run.rs](../crates/core/src/workflow/run.rs) |
| Marks, facts, gates, the command runner | [crates/core/src/workflow/facts.rs](../crates/core/src/workflow/facts.rs) |
| Tasks, which keep their runs | [crates/core/src/task.rs](../crates/core/src/task.rs) |
| Task files, their ordered writer, the move of `pipeline-runs/` | [crates/core/src/task/files.rs](../crates/core/src/task/files.rs) |
| The queue: one task per place | [crates/core/src/task/queue.rs](../crates/core/src/task/queue.rs) |
| Marks pinned as commits under `refs/onehand/` | [crates/core/src/task/marks.rs](../crates/core/src/task/marks.rs) |
| The one driver | [crates/app/src/task/driver.rs](../crates/app/src/task/driver.rs) |
| The `Tasks` global: requests, the queue, unfinished tasks | [crates/app/src/task.rs](../crates/app/src/task.rs) |
| Workflows on offer | [crates/app/src/workflow.rs](../crates/app/src/workflow.rs) |
| Launcher, and driving a task once its place is free | [crates/app/src/shell/workflows.rs](../crates/app/src/shell/workflows.rs) |
| Settings ▸ Workflows | [crates/app/src/settings/workflows.rs](../crates/app/src/settings/workflows.rs) |

## Workflows

A workflow is a TOML file:

```toml
schema_version = 1
name = "Work in checkout"
description = "…"
place = "checkout"        # or "worktree"
misses = 3                # misses allowed in a stretch of steps
timeout = "45m"           # working time; waiting on a person does not count

[[steps]]
id = "plan"
label = "Plan"
kind = "agent"
gates = ["answered", "code_unchanged"]
keep_answer = true
prompt = """
{brief}

This step is the plan. …
"""

[[steps]]
id = "approve"
label = "Approve"
kind = "approval"
of = "plan"

[[steps]]
id = "verify"
label = "Verify"
kind = "command"          # `command = "…"`; left out, the project's check command
on_fail = "implement"
```

onehand ships two, read-only: *Work in checkout* (Plan → Approve → Implement → Verify, in the
checkout, left uncommitted) and *Implement on a branch* (Plan → Implement → Verify, on a new
`workflow/<title>` branch in a worktree beside the project's repository, committed; a project that
is a folder inside its repository works in the same folder of the new checkout, as a worktree made
from the project menu does). The person's own are in
`<config_dir>/onehand/workflows/<slug>.toml`, made by duplicating a shipped one or from *New
workflow* in Settings ▸ Workflows. A build from before the rename kept them in `pipelines/`;
they move here at start, behind the one-instance lock, by the rules in
[architecture.md](architecture.md#workflows).

**A file this build cannot read is never written over.** A `schema_version` above
`workflow::SCHEMA_VERSION` was written by a newer onehand; it is listed as unreadable, and a save
aimed at it is refused. Every read-check-write of a workflow goes through one lock.

### Validation

`workflow::validate` is run on every keystroke of the form, before Save, and before a run starts.
It refuses: no name, no steps, an unreadable timeout, an unsupported schema; a step id that is not
lowercase letters, digits, `-` and `_`, or is used twice; an empty label or prompt; a prompt
variable that is not one; `{output.<id>}` naming anything but an earlier agent step that keeps its
answer; `on_fail` naming anything but an earlier agent step; an approval of anything but an earlier
agent step that keeps its answer; a gate that cannot hold where the workflow works (`committed` in
a checkout, `uncommitted` on a worktree); and `code_changed` beside `code_unchanged`.

### Prompts

A variable is `{name}`: `{brief}` (the title and details), `{instructions}`, `{check_output}` (what
the failed command printed), `{revise}` (a person's note on a revision), `{output.<id>}` (what an
earlier step kept). **Any other brace is text**, so a code sample needs no escaping. Because
validation refuses unknown names, filling a prompt in never fails.

onehand adds, after the prompt: the instructions, a failed command's output and a revision note
with the answer it revises, each **only when the prompt does not place it itself** and only when
there is one; then a horizontal rule, where the work is (`Place`), the repository-conventions paragraph, and
one line per gate saying what onehand will check.

## The engine

`workflow::Run` is the only place a transition is decided. The driver reports what happened —
`measured`, `turn_ended`, `command_finished`, `approved`, `revised`, `stopped`, `failed`, `resume` —
and gets back the next `Action`: `Measure`, `Prompt`, `RunCommand`, `AwaitApproval`, `Finish` or
`Idle` (the report did not fit what the run waits for). Each report also appends a `Transition` to
the run's history, capped at 200.

- **An agent step** is measured first, so its work is judged against where it started; then
  prompted. When its turn ends the gates are read in order against the step's mark, and the first
  that fails is the miss: the session is sent a carry-on prompt naming it. In a checkout, a miss on
  `code_unchanged` or `uncommitted` is measured again before the carry-on, and the mark keeps both
  fingerprints, because a person working in the same checkout may have made the change; the agent is
  told to undo only its own.
- **A command step** runs its command, or the project's check command; passing records the head as
  `verified_at`, failing is a miss and goes back to `on_fail` carrying the output.
- **An approval step** waits. *Continue* goes on; *Revise…* goes back to the step it approves, whose
  prompt then carries the note and its last answer. A revision is not a miss.
- **Misses are counted per stretch**: they reset only when the run reaches a step further on than it
  has been, so a command that keeps failing cannot loop with its `on_fail` step forever.
- **`stopped` never judges the turn** that was under way, whatever the reason: a cut-short turn
  must not pass as finished work. **`resume` keeps the step's mark**, so work done before a restart
  still counts in that step.

## The driver

One global, `crate::workflow::Workflows`, holds every run by its session's uid. The driver
subscribes to the session and maps its events onto the engine:

| Event | Report |
|---|---|
| A turn the run sent ends | facts read in the background, then `turn_ended` |
| A turn ends cancelled (`Chat::cancelled`) | `stopped(ByPerson)` |
| A prompt the run did not send (`prompts_sent` above what it sent, or one queued) | `stopped(TakenOver)` |
| The adapter goes | `stopped(LinkLost)` |
| The session is released | `stopped(Closed)` |
| *Stop* on the strip | the turn is cancelled, then `stopped(ByPerson)` |
| The timeout runs out | the turn is cancelled, then `stopped(TimedOut)` |

**Every one of these goes through one `end`, and a step's command is stopped first.** While a
command runs, the run holds the flag that calls it off (`process::output_until`): ending sets it,
the command's whole process group is killed, and the run is said to be stopped only once the
command has exited, so a stopped run never leaves a build or a test writing to the work. The first
reason given is the one the run ends with.

A prompt asked for before the adapter is up waits for the link. The clock is
`unattended::Budget`: it pauses while a card or an approval waits on a person, and its timer looks
again when it fires rather than being re-armed at every pause.

**Every run belongs to a task, and the task's file is written after every action**
(`<config_dir>/onehand/tasks/<id>.json`), through `task::files::Writer`, one thread carrying out
saves in the order sent, so a late save can never land over a newer one. A task's file is kept
whatever the outcome: it is the task's history. The run keeps its outcome (`Run::outcome`), so a
run with none after a restart was cut off. **A run whose agent stopped or whose session went** can
be resumed: neither is the run's own outcome, and app shutdown can look like either. Quitting
waits, briefly, for the writes still queued (`Writer::flush`), so a run's last save is not lost when
the process exits with its last window.

**A run records step visits** (`Run::visits`): each stay at a step, with its times, what it kept
(the answer, or how a failed command's output ended) and how it came out. Going back to a step is a
new visit, and resuming closes the cut-off visit as `interrupted` and opens a new one of the same
step. **Each visit's start and end is pinned as a commit** (`task::marks::pin_blocking`): the work
as it stands, untracked files included and ignored ones left out, committed from a temporary
index so the person's own index is untouched, and kept under
`refs/onehand/tasks/<task>/<run>/<visit>/<start|end>` so `git gc` keeps it. The driver pins before
it carries out a step's first action, so the start mark lands before the prompt or command touches
the work; one commit serves a visit's end and the next one's start. A mark that cannot be pinned is
logged and left out, and never stops a run: gates read `Mark` and `Facts`, not the pinned commits. While a mark is
being pinned, an approval, a revision, a Stop and every other ending wait for it and for the action
it holds back, so nothing moves the run on under that action and no ending races the pin. A driver
taking a run up counts as pinned only the marks up to the last one that landed
(`Run::pinned_count`), so a mark the app quit before pinning is pinned on resume.

**A turn's answer is what the agent said after the run's own prompt** (`Chat::prose_since`,
counted from where the transcript stood when the prompt went), so a turn that said nothing answers
nothing rather than passing an `answered` gate with the turn before it. The work a checkout holds
uncommitted is fingerprinted with FNV-1a, never std's hasher, because the fingerprint is kept in
the run's file and compared after a restart, possibly by a build made with another Rust. An
untracked file counts by its contents, not only its name: the step after a failed check often fixes
the very file the change before it created.

**What waits for approval is shown from the run**, not the transcript (`Run::under_review`):
the strip's *Review…* opens the kept answer, so a run resumed in a new session is not approved
blind.

## Starting, queueing and resuming

The composer's `+` menu and the keymap's `run_workflow` (no default key) open the launcher on the
project on screen. A checkout workflow starts a session there; a worktree workflow first cuts
`workflow/<title>` (or the first free `-N`) off `HEAD` beside the project and adds it as a project.
A workflow with a command step that names no command needs the project's check command, set under
Settings ▸ Workflows and kept in the workspace file (`WorkspaceConfig::checks`).

**Every start goes through the queue** (`task::request`), Resume included. A place is the
checkout git sees: the canonical top level of the repository, or a folder's own canonical path
outside git (`task::queue::place_blocking`), so two projects that are folders of one checkout share
one place. One task works in a place at a time; a start that finds its place taken waits, first
in, first out, says so in a notification and shows as *Queued* on the project page, where *Stop*
calls it off. A task called off before its run took a step drops that empty run and reads as
stopped by a person; a resume called off goes back to how it was. **A place is given up only once
the work has stopped**: a run that ends while its session's turn is still going keeps it until
that turn ends, the agent goes or the session is closed, and a command step ends only after its
process group has exited. The end mark is pinned first, and then the next task waiting starts in
the window it was asked from. A worktree workflow makes its worktree at launch, its own place, so
it never waits.

The launcher keeps the new task before asking for its place, so one waiting survives a restart.
After a restart nothing is running or queued: a task whose last run has no outcome reads as
interrupted. A project page lists, under *Unfinished tasks*, the interrupted tasks started from it
or working in it, with *Dismiss* (kept as history, never offered again) and *Resume*, and the
queued ones with *Stop*. Nothing restarts an agent by itself. *Resume* adds the task's folder back
as a project if it left the workspace, starts a session on the agent the run used, and calls
`Run::resume`, which also starts a run that never took a step. A task that ended exhausted,
failed, timed out, done or stopped is kept on disk and not listed yet.

At boot, behind the instance lock and right after the templates move, every run a build from
before tasks kept in `<config_dir>/onehand/pipeline-runs/` becomes a task of the same id in
`tasks/`, with that one run, cut off (`task::files::migrate_old_dir_blocking`). The move shares its
rules with the templates' (`config::migrate_dir_blocking`): a file that does not read stays and is
reported, a task already in `tasks/` under that name wins (it may have moved on since), each file
is written in full and on disk before its old one goes, and the folder goes once empty.

## Checking it by hand

The workflow mock agent plays an agent's part in a run, so every path below is walked in seconds
without an API key. Add it in Settings ▸ Agents or `onehand.toml`, with the script's absolute
path: the agent starts in the scratch project, where a relative path finds nothing
(`MODULE_NOT_FOUND`).

```toml
[[agents]]
name = "Mock workflow"
command = "node"
args = ["/absolute/path/to/onehand-gpui/crates/core/examples/mock_workflow_agent.js"]
```

It reads what a step wants from the gate rules onehand appends to the prompt, not from the
workflow's wording, so an edited workflow still drives it. Its orders are whole words in the
brief's title: `miss` does nothing every turn, `fail-check` makes the session's first change fail
the check and every later one pass, and `fast` answers at once instead of over about six seconds.
It walks the cases below; it never edits during a plan or commits in a checkout, so the
`code_unchanged` and `uncommitted` carry-ons are reached only by a person changing the work
mid-step.

A run takes the first agent in the list, so put *Mock workflow* first. Set up a scratch repository
with one commit, open it as a project, and set the project's check command (Settings ▸ Workflows) to:

```sh
sleep 5 && grep -q 'check: pass' mock-workflow.txt
```

Then walk each case, with *Work in checkout* unless it says otherwise. Between cases, discard the
change (`git checkout . && git clean -fd`).

| Case | Do | Expect |
|---|---|---|
| Done | Brief `go`. *Continue* at the approval | Plan, Approve, Implement, Verify; *Workflow done*. `mock-workflow.txt` is left uncommitted. On *Implement on a branch*, a new branch holds one commit |
| Stop while a command runs | Brief `go fast`, *Continue*, then *Stop* during Verify's `sleep 5` | The run ends *stopped by hand* only once the command has exited: no `sleep` is left (`pgrep -f 'sleep 5'`) |
| Exhausted | Brief `miss` | The workflow allows three misses, so the plan misses `answered` four times; the fourth ends the run with *too many misses at the Plan step*. The task stays in `tasks/` and is not listed on the project page |
| A failed command goes back | Brief `fail-check`, *Continue* | Verify fails, Implement runs again with the check's output in its prompt, Verify passes; *Workflow done* |
| Approve and Revise | Brief `go`. *Revise…* with a note, then *Continue* | The plan runs again, its prompt carrying the note and the earlier answer; no miss is counted. *Review…* shows the kept answer |
| A restart mid-step, then Resume | Brief `go`, *Continue*, quit while Implement's turn is still answering | At the next start the project page lists the task under *Unfinished tasks*. *Resume* starts a new session at Implement, with its mark kept, and the run carries on to *Workflow done* |
| A restart at an approval, then Resume | Brief `go`, quit while the run waits for approval | *Resume* waits for approval again; *Review…* shows the plan |
| A workflow edited under a run | Duplicate *Work in checkout*, start a run on the copy with brief `go`, then while the run waits for approval delete its Verify step and save | The run still reaches Verify: it uses the snapshot it started with |
| Two tasks in one checkout | Brief `go`, then while it runs start a second with brief `go fast` | A notification says the second is queued; the project page lists it as *Queued*. It starts by itself once the first ends, in a session of its own |
| Stop on a queued task | As above, then *Stop* on the queued row | The row goes; the first run carries on. The task's file keeps no run |
| Dismiss | Quit mid-run, then *Dismiss* on the row at the next start | The row goes; the task's file is kept with `"dismissed": true` |
| Marks | After any run, `git for-each-ref refs/onehand` | A `start` and an `end` ref per visit, under `refs/onehand/tasks/<task>/<run>/<visit>/`; `git show` on one includes `mock-workflow.txt` while it is untracked |
| Runs from an older build | Put a run file from before tasks in `<config_dir>/onehand/pipeline-runs/` and start onehand | It is in `tasks/` under the same name, listed as interrupted, and `pipeline-runs/` is gone |
