# Workflows

How a workflow is written, checked and run: the file, the engine and the one driver.

A run takes a brief through the steps of a workflow: an agent step prompts the
session and is judged by gates onehand checks itself, a command step runs a command in the work, an
approval step waits for a person, and the forge steps (push, pull request, status checks) take a
worktree's branch to the forge. The words are defined in [CONTEXT.md](../CONTEXT.md).

## Where the code is

| Part | Where |
|---|---|
| Workflow types and serde | [crates/core/src/workflow/template.rs](../crates/core/src/workflow/template.rs) |
| What stops a workflow being saved or run | [crates/core/src/workflow/validate.rs](../crates/core/src/workflow/validate.rs) |
| Prompt variables, onehand's additions, carry-on prompts | [crates/core/src/workflow/prompt.rs](../crates/core/src/workflow/prompt.rs) |
| The shipped workflows | [crates/core/src/workflow/builtin/](../crates/core/src/workflow/builtin/) |
| The person's workflows on disk | [crates/core/src/workflow/store.rs](../crates/core/src/workflow/store.rs) |
| The engine: one run as pure state | [crates/core/src/workflow/run.rs](../crates/core/src/workflow/run.rs) |
| Marks, facts, gates, the command runner | [crates/core/src/workflow/facts.rs](../crates/core/src/workflow/facts.rs) |
| What a pull request's status checks say | [crates/core/src/workflow/status_checks.rs](../crates/core/src/workflow/status_checks.rs) |
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
id = "builtin:checkout"   # set by onehand: a file's own name, `builtin:<name>` when shipped
version = 1               # set by onehand: up by one at each save that changes something
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

# On a worktree only, the forge steps:
[[steps]]
id = "push"
label = "Push"
kind = "push"             # onehand pushes the commit the last command passed on

[[steps]]
id = "pull_request"
label = "Pull request"
kind = "pull_request"     # a draft, or the one already open on the branch

[[steps]]
id = "status_checks"
label = "Status checks"
kind = "status_checks"
on_fail = "implement"     # a failing check or a conflict goes back here
wait = "1h"               # how long the checks may take; the default
```

onehand ships three, read-only: *Work in checkout* (Plan → Approve → Implement → Verify, in the
checkout, left uncommitted), *Implement on a branch* (Plan → Implement → Verify, on a new
`workflow/<title>` branch in a worktree beside the project's repository, committed; a project that
is a folder inside its repository works in the same folder of the new checkout, as a worktree made
from the project menu does) and *Work an issue* (`builtin:issue`, the same steps on the branch an
unattended run cuts for its issue, then Push → Pull request → Status checks, and what
`[unattended] workflow` names by default; see [unattended.md](unattended.md)). The person's own are in
`<config_dir>/onehand/workflows/<slug>.toml`, made by duplicating a shipped one or from *New
workflow* in Settings ▸ Workflows. A build from before the rename kept them in `pipelines/`;
they move here at start, behind the one-instance lock (`workflow::store::migrate_old_dir_blocking`), and the
move can be cut short at any point and run again: a file this build cannot read stays where it was
and is reported; a name already in `workflows/` keeps that copy, and the old one is removed only when
it is the same text (otherwise it stays and is reported, since an older build may have saved an edit
there); any other is written whole (temp file and rename), and its directory waited on to reach the
disk, before the old one goes. A `pipelines/` that is there but cannot be listed is reported, not
taken for empty. Anything that is not a template is left alone, and `pipelines/` goes once empty.

**A file this build cannot read is never written over.** A `schema_version` above
`workflow::SCHEMA_VERSION` was written by a newer onehand; it is listed as unreadable, and a save
aimed at it is refused. Every read-check-write of a workflow goes through one lock.
`store::parse` also refuses **a key onehand does not read** (`steps[2].gate`), most likely a typo,
which a save would otherwise drop without a word; such a file is listed as unreadable too.

### Id and version

`store::save_blocking` alone sets them, whatever the template it is handed carries. A new file
(a new workflow, a duplicate, an import) takes its file name as `id` at version 1; an existing file
keeps its `id`, and its `version` goes up by one when what the template says changed (the version
itself left out of the comparison). A file written before ids takes its file name when read. No
old version is kept: a run's snapshot already holds the one it ran. `Template::same_workflow` is
the rule *Retry with current settings* finds a task's workflow again by: the same `id`, so a rename
still finds it, at whatever version is on offer now. A snapshot from before ids falls back to the
same name. A file copied by hand keeps its id, so two may share one, and the first is taken.

### Import and export

Settings ▸ Workflows has *Export…* on every readable row, shipped ones included: a native save
dialog, then `store::export_blocking`. *Import…* beside *New workflow* picks a `.toml` file and
reads it with `store::read_blocking`; a file that does not read is refused in a notification, and
one that does opens in the form as a new workflow. Nothing is written until Save, which runs
validation, the name check and `save_blocking`, so an import always gets a new id.

### Validation

`workflow::validate` is run on every keystroke of the form, before Save, and before a run starts.
It refuses: no name, no steps, an unreadable timeout, an unsupported schema; a step id that is not
lowercase letters, digits, `-` and `_`, or is used twice; an empty label or prompt; a prompt
variable that is not one; `{output.<id>}` naming anything but an earlier agent step that keeps its
answer; `on_fail` naming anything but an earlier agent step; an approval of anything but an earlier
agent step that keeps its answer; a gate that cannot hold where the workflow works (`committed` in
a checkout, `uncommitted` on a worktree); `code_changed` beside `code_unchanged`; a label used by
an earlier step; `{check_output}` in a step no later command step sends back to, or `{revise}` in
one no later approval sends back, since either would always be empty (a status checks step sends
`{check_output}` back too); a worktree workflow with no agent step gated `committed`, whose branch
could end with nothing on it; a forge step in a checkout workflow; a push with no command step before
it, a pull request with no push before it, status checks with no pull request before them or an
`on_fail` that is no earlier agent step, and a `wait` that is not a duration.

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
`measured`, `turn_ended`, `command_finished`, `approved`, `revised`, `forge_done`, `status_checks_seen`,
`stopped`, `failed`, `resume` — and gets back the next `Action`: `Measure`, `Prompt`, `RunCommand`,
`AwaitApproval`, `Push`, `OpenPullRequest`, `AwaitStatusChecks`, `Finish` or `Idle` (the report did not
fit what the run waits for). Each report also appends a `Transition` to
the run's history, capped at 200.

- **An agent step** is measured first, so its work is judged against where it started; then
  prompted. When its turn ends the gates are read in order against the step's mark, and the first
  that fails is the miss: the session is sent a carry-on prompt naming it. In a checkout, a miss on
  `code_unchanged` or `uncommitted` is measured again before the carry-on, and the mark keeps both
  fingerprints, because a person working in the same checkout may have made the change; the agent is
  told to undo only its own.
- **A command step** runs its command, or the project's check command. It passes on its exit status
  alone and records the head as `verified_at` when the work has one; failing is a miss and goes
  back to `on_fail` carrying the output. **Its visit keeps how it came out**
  (`Visit::command`, a `CommandResult`), a pass as well as a failure: passed or not, the exit
  code, the last 200 lines it printed, and the commit and the fingerprint of the uncommitted work,
  untracked files included, read once it ran. The commit and the fingerprint together are what a
  pass covers: the work is judged against both, so an edit after the check, committed or not, is
  never taken for checked work.
- **The forge steps** go to the connector the run's setup names (`Setup::forge`, the one that
  serves the project when a worktree task starts). With none, each passes at once: the branch is the
  result. **A push carries the commit the check passed on**: `Push` is `marks.verified_at`, never
  the head, so what lands is what was checked, and a push with nothing verified fails. A retry keeps
  `verified_at`, and the pull request the last run opened, being on the same branch. *Pull request* takes the one open on the branch, or opens a draft
  (`unattended::pull_request_text`: it closes the issue only where the forge knows it); one closed
  without being merged is refused, never opened again beside. The run keeps the pull request it
  opened or took up (`Run::pull_request`, its number and address), read back once it is open; one
  not read back is still found by its branch. Either failing ends the run as
  failed, on the forge; a retry starts at that step again. *Status checks* is judged by `workflow::judge` from the
  forge's pull request, **on the commit that was pushed**: checks on any other head are waited
  past. A failing check or a conflict wins over one still running and goes back to `on_fail` as a
  miss, carrying what failed and up to three logs as `{check_output}`; all passing takes the pull
  request out of draft and goes on; none at all after a ten-minute grace (or `wait`, when shorter)
  counts as passing. A failing Actions job's log is read from the job itself, so a job that failed
  fast is repaired while slower ones still run, and every log is fenced longer than any fence it
  prints. A push the forge turns down is not tried again over HTTPS. A forge
  that cannot be read, or a draft that cannot be taken out of draft, is waited on like a check still
  running, and nothing is waited on past `wait`: the run fails. Merged ends it done; closed, or no
  pull request at all, fails it. Repairs are bounded by `misses`, like a failing command.
- **An approval step** waits. *Continue* goes on; *Revise…* goes back to the step it approves, whose
  prompt then carries the note and its last answer. A revision is not a miss. **Each press names
  what it approves** (`ApprovalAt`): the run and the approval step's open visit it was drawn from.
  The engine takes it only while that run waits for approval at that visit, and answers `Idle`
  otherwise. The run is named as well as the visit, because visit ids count from 1 in every run.
  So a window still showing a plan another window had revised, a double press, a press after the
  run was cut off and a press from a view of a run a Retry replaced all approve nothing.
- **Misses are counted per stretch**: they reset only when the run reaches a step further on than it
  has been, so a command that keeps failing cannot loop with its `on_fail` step forever.
- **`stopped` never judges the turn** that was under way, whatever the reason: a cut-short turn
  must not pass as finished work. **`resume` keeps the step's mark**, so work done before a restart
  still counts in that step.

## The driver

The `Tasks` global (`crate::task::Tasks`) holds every run under way by its session's uid;
`crate::workflow::Workflows` keeps only the workflows on offer. The driver subscribes to the
session and maps its events onto the engine:

| Event | Report |
|---|---|
| A turn the run sent ends | facts read in the background, then `turn_ended` |
| A turn ends cancelled (`Chat::cancelled`) | `stopped(ByPerson)` |
| A prompt the run did not send (`prompts_sent` above what it sent, or one queued) | `stopped(TakenOver)` |
| The adapter goes | `stopped(LinkLost)` |
| The session is released | `stopped(Closed)` |
| *Stop* on the strip | the turn is cancelled, then `stopped(ByPerson)` |
| The timeout runs out | the turn is cancelled, then `stopped(TimedOut)` |
| A push or a pull request is done, or fails | `forge_done` |
| A look at the pull request's status checks, every minute, says something other than pending | `status_checks_seen` |

**Every one of these goes through one `end`, and a step's command is stopped first.** While a
command runs, the run holds the flag that calls it off (`process::output_until`): ending sets it,
the command's whole process group is killed, and the run is said to be stopped only once the
command has exited, so a stopped run never leaves a build or a test writing to the work. A
command called off before it started is never started. The first reason given is the one the run
ends with.

A prompt asked for before the adapter is up waits for the link. When the agent first comes up in a
session, a run whose setup names a mode (`Setup::mode`, an unattended run's) has its agent put in
that mode before the first prompt; an agent that does not offer it fails the run. The clock is
`unattended::Budget`: it pauses while a card or an approval waits on a person, or the pull
request's status checks run, and its timer looks
again when it fires rather than being re-armed at every pause.

**Every run belongs to a task, and the task's file is written after every action**
(`<config_dir>/onehand/tasks/<id>.json`), through `task::files::Writer`, one thread carrying out
saves in the order sent, so a late save can never land over a newer one. A task's file is kept
whatever the outcome: it is the task's history. The run keeps its outcome (`Run::outcome`), so a
run with none after a restart was cut off. **A run whose agent stopped or whose session went** can
be resumed: neither is the run's own outcome, and app shutdown can look like either. Quitting
calls off every command still running, a step's or a check's, and waits, briefly, for each to
exit, so none goes on writing to the work beside the next task there; then it waits for the
writes still queued (`Writer::flush`), so a run's last save is not lost when the process exits
with its last window.

**A run records step visits** (`Run::visits`): each stay at a step, with its times, what it kept
(the answer, or how a failed command's output ended), its command's result on a command step, and
how it came out. **A failed run keeps what it failed on** (`Run::failure`, read as
`Run::failed_on`), beside its outcome rather than in it: *configuration* when what the preflight
would have blocked is found only once the run runs (`preflight::found_late`: the agent no longer
configured, a mode the agent does not offer, no check command to run, a workflow that no longer
validates), *forge* when a forge step or the forge failed, and *other* for the rest. Going back to a step is a
new visit, and resuming closes the cut-off visit as `interrupted` and opens a new one of the same
step. **Each visit's start and end is pinned as a commit** (`task::marks::pin_blocking`): the work
as it stands, untracked files included and ignored ones left out, committed from a temporary
index so the person's own index is untouched, and kept under
`refs/onehand/tasks/<task>/<run>/<visit>/<start|end>` so `git gc` keeps it. The driver pins before
it carries out a step's first action, so the start mark lands before the prompt or command touches
the work; one commit serves a visit's end and the next one's start. A mark that cannot be pinned is
logged and left out, and never stops a run: gates read `Mark` and `Facts`, not the pinned commits. While a mark is
being pinned, an approval or a revision waits for it and for the action it holds back, so nothing
moves the run on under that action; replayed, it still carries the visit it was drawn from, so a
second press held behind the first finds that visit closed. The driver routes a press by the
task, not the session, so a window other than the session's can make it, and the run's clock goes
on only once the engine took the press. A Stop, the timeout or any other ending waits for the pin too,
and then runs instead of that action, with its cancel of the turn: a prompt is never sent to a run
already said to be over, and no ending races the pin. A driver
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
blind. A press the run no longer waits for is not an error: *Continue* puts up what the run waits
on now, saying *The answer changed since you opened it.*, and *Revise…* refuses to send in place,
keeping the note.

## Starting, queueing and resuming

The composer's `+` menu and the keymap's `run_workflow` (no default key) open the launcher on the
project on screen. A checkout workflow starts a session there; a worktree workflow first cuts
`workflow/<title>` (or the first free `-N`) off `HEAD` beside the project and adds it as a project.
A workflow with a command step that names no command needs the project's check command, set under
Settings ▸ Workflows and kept in the workspace file (`WorkspaceConfig::checks`).

Under the workflow picker, a collapsed *Preview* opens on what the run would start with: a line
with where it works, its timeout, its misses and its version; a line per step
(`StepSpec::summary`); and the first prompt as the agent would receive it
(`workflow::first_prompt`), filled with the brief as it is typed, in a scrolling box.

**A start is preflighted before anything of its own** (`onehand_core::preflight`): one pure
function, given the kind of start and the facts the app holds, returns findings, each blocking
the start or only saying something, with where it is changed. It never claims, cuts or starts;
whoever carries the start out still does, after it. It judges the configuration that will
actually run, and the app gathers the facts without reaching the disk or the network in a
render: `gh`'s sign-in is the state last seen, the forge serving a project is found when the form
opens. What it checks, for a new issue run:

A new run from the launcher is judged the same way (kind *new run*): its workflow, the agent,
the project's check command, a worktree workflow on a folder outside git (blocked), the forge's
account when the workflow has forge steps (blocked when `gh` is missing or signed out, as last
seen), and that its branch is cut off `HEAD`. Whether the folder is in git and which forge serves
it are read off the UI loop when the launcher opens; until they land, neither blocks. The launcher lists what it found under its fields, and *Run* is spent
while a block remains. A Resume and a Retry are judged by the run's own setup, and a *Retry with
current settings* as a new start is, on what Settings say now
([tasks.md](tasks.md#retry-and-resume)).

| Check | Blocks when | Says |
|---|---|---|
| Workflow | it is not there, works in the checkout, or `workflow::validate` finds problems (each listed, first) | |
| Agent | none is configured, or the one named is no longer | |
| Mode | the agent's current offer, learned in this process from the spec as it is now, does not hold it | the mode is not known yet, with *Check the agent* |
| Check command | the workflow runs the project's check command and there is none | the workflow runs no command: nothing verifies the work |
| Place | `HEAD` is detached and no forge serves the project | for a new run of a checkout workflow, a person's own session that has been prompted in the same checkout, by its name: both edit the same files |
| Base | | what the branch is cut off: the default branch on `origin`, fetched first, or the branch checked out |
| Forge | `gh` missing or signed out, as last seen | the workflow has forge steps and no forge serves the project: they pass at once, and the branch is the result (not said for answering a review, which that refuses) |
| Issue | another run works on it; for *Answer a pull request review*, the pull request is not open (closed unmerged, merged, none, or unread), the workflow has no status checks step to repair from, no forge serves the project, the branch on the forge went its own way, or the issue is closed or could not be read | |
| Earlier task | | the issue's last task needs attention: starting makes a second task, and *Retry…* opens that task's Retry dialog instead |
| Slot | `at_once` is reached, or `waiting` runs wait on a person; said with the slots line naming who holds each | |
| Limits | | every kind: the timeout and the misses allowed, from the snapshot that will run (the run's own for Resume, Retry and answering a review) |

**What an agent offers is learned whenever it comes up**, a person's session included, and kept
per agent spec (its command and arguments, compared whole) for the life of the process
(`Shared::modes_seen`). A spec edited since is another spec, so what was learned of it stops
counting; a restart forgets, so an upgraded adapter is never judged by an old list. Only a list
that is current blocks; otherwise the mode is *not known yet*, and the driver's check when the
agent comes up stays the authority.

**Check the agent**, beside a mode not known yet, starts that agent in the project with no
session, reads the modes its `session/new` answers with, and closes it, off the UI thread and
prompting nothing (`agent_check`, through `AcpRuntime::probe`, which never takes the adapter
parked for the next session). It is the one check that costs an agent start, so only its press
runs it. What it learns goes into `Shared::modes_seen` like any other sighting: the issue's start
form, judged every frame, turns *not known yet* into a block or into nothing; a dialog judged once
when it opened says beside the button what the agent offers. A failure to come up, or no answer
within 90 seconds, is said there.

**Every start goes through the queue** (`task::request`), Resume included. A place is the
checkout git sees: the canonical top level of the repository, or a folder's own canonical path
outside git (`task::queue::place_blocking`), so two projects that are folders of one checkout share
one place. One task works in a place at a time; a start that finds its place taken waits, first
in, first out, says so in a notification and shows under *Queued* on the Tasks page, where *Stop*
calls it off. A task called off before its run took a step drops that empty run and reads as
stopped by a person; a resume called off goes back to how it was. **A place is given up only once
the work has stopped**: a run that ends while its session's turn is still going keeps it until
that turn ends, the agent goes or the session is closed, and a command step ends only after its
process group has exited. The end mark is pinned first, and then the next task waiting starts in
the window it was asked from. A worktree workflow makes its worktree at launch, its own place, so
it never waits.

The launcher keeps the new task before asking for its place, so one waiting survives a restart.
After a restart nothing is running or queued: a task whose last run has no outcome reads as
interrupted. The Tasks page lists it under *Needs attention*, with *Resume*, *Retry* and *Dismiss*
(kept as history, never offered again), beside the tasks that ended exhausted, failed or timed
out; a project page links there with one line saying what its tasks need. Nothing restarts an
agent by itself. *Resume* adds the task's folder back as a project if it left the workspace,
starts a session on the agent the run used, and calls `Run::resume`, which also starts a run that
never took a step. *Retry* starts a new run instead; [tasks.md](tasks.md#retry-and-resume) holds
what it carries over.

At boot, behind the instance lock and right after the templates move, every run a build from
before tasks kept in `<config_dir>/onehand/pipeline-runs/` becomes a task of the same id in
`tasks/`, with that one run, cut off (`task::files::migrate_old_dir_blocking`). The move shares its
rules with the templates' (`config::migrate_dir_blocking`): a file that does not read stays and is
reported, a task already in `tasks/` under that name wins (it may have moved on since), each file
is written in full and on disk before its old one goes, and the folder goes once empty.

## Not built

- **A run takes its steps in a straight line.** There is no branching on a step's answer, no
  step that runs only sometimes, and no two steps at once; a command's failure going back to an
  earlier step is the only way back, besides a revision. One session carries every step, so a step
  cannot use a different agent from the rest.
- **A start does not say beforehand that its place is taken.** The preflight has the row, but
  finding the place means asking git where the checkout's top is, which no render may do; a start
  that queues says so in a notification once it has asked. An issue's new run is cut a worktree of
  its own and never queues.

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
| Exhausted | Brief `miss` | The workflow allows three misses, so the plan misses `answered` four times; the fourth ends the run with *too many misses at the Plan step*. The task is listed under *Needs attention* on the Tasks page, and the rail's *Tasks* row counts it. *Retry* says it starts at Plan; *Retry* runs it again in a new run |
| A failed command goes back | Brief `fail-check`, *Continue* | Verify fails, Implement runs again with the check's output in its prompt, Verify passes; *Workflow done* |
| Approve and Revise | Brief `go`. *Revise…* with a note, then *Continue* | The plan runs again, its prompt carrying the note and the earlier answer; no miss is counted. *Review…* shows the kept answer |
| A restart mid-step, then Resume | Brief `go`, *Continue*, quit while Implement's turn is still answering | At the next start the Tasks page lists the task under *Needs attention*, cut off at Implement. *Resume* starts a new session at Implement, with its mark kept, and the run carries on to *Workflow done* |
| A restart at an approval, then Resume | Brief `go`, quit while the run waits for approval | *Resume* waits for approval again; *Review…* shows the plan |
| A workflow edited under a run | Duplicate *Work in checkout*, start a run on the copy with brief `go`, then while the run waits for approval delete its Verify step and save | The run still reaches Verify: it uses the snapshot it started with |
| Two tasks in one checkout | Brief `go`, then while it runs start a second with brief `go fast` | A notification says the second is queued; the Tasks page lists it under *Queued*. It starts by itself once the first ends, in a session of its own |
| Stop on a queued task | As above, then *Stop* on the queued row | The row moves to *Finished*; the first run carries on. The task's file keeps no run |
| Dismiss | Quit mid-run, then *Dismiss* on the row at the next start | The row moves to *Finished*; the task's file is kept with `"dismissed": true` |
| Run check | Set a check command, then *Run check* on the project page, once with a command that passes and once with `false` | A notification says *Check passed in <project>*, or *Check failed in <project>*; the task is under *Finished*, or under *Needs attention* with *Retry* |
| Stop on a running row | *Stop* on a running task's row on the Tasks page | The run ends *stopped by hand* and moves to *Finished* |
| Marks | After any run, `git for-each-ref refs/onehand` | A `start` and an `end` ref per visit, under `refs/onehand/tasks/<task>/<run>/<visit>/`; `git show` on one includes `mock-workflow.txt` while it is untracked |
| Task detail | After *Exhausted*, press the row's title | The detail shows Run 1: one Plan visit, ending *too many misses at the Plan step*; the misses are in the run's history. Open the visit: its output, then *No files changed.* (or the files, with `+N −N`). Press a file: its diff. *All tasks* goes back |
| Retry from an earlier step | Brief `go`, *Continue*, then *Stop* during Implement; open the task under *Finished* and press *Retry* | The dialog's *From …* menu lists Plan, Approve and Implement, at Implement; pick Plan and the description says it starts at Plan carrying no answers; *Retry* runs Plan again |
| Retry a finished task | After *Done*, open the task and press *Retry* | The menu defaults to Plan; the new run goes through every step, and the detail lists Run 1 under *Earlier runs* |
| Retry a dismissed task | *Dismiss* an ended task, open it under *Finished*, *Retry* | It leaves *Finished*; if the run ends on something nobody chose it is under *Needs attention* again |
| Runs from an older build | Put a run file from before tasks in `<config_dir>/onehand/pipeline-runs/` and start onehand | It is in `tasks/` under the same name, listed as interrupted, and `pipeline-runs/` is gone |
| An unknown key | Copy a workflow file into `<config_dir>/onehand/workflows/` with `gate = "x"` added to a step, then *Import…* it too | The row reads *Cannot be read: it has a key `steps[N].gate` that onehand does not read*; the import is refused with the same reason |
| Export, then import | *Export…* *Work in checkout*, then *Import…* that file | The form opens on it as a new workflow; Save asks for another name, and once renamed it is saved under a new id at version 1 |
| Retry after a rename | Duplicate *Work in checkout* and save it, start a run with brief `miss`, then rename the workflow and save | *Retry* keeps *version 1*, saying Settings now say *version 2*; *Retry with current settings…* lists *Workflow version: version 1 → version 2* and runs the renamed workflow |
| What Retry keeps | Let a run time out, then change `[unattended] timeout` (or the project's check command) and *Retry* | The dialog lists the agent, mode, check command, timeout and workflow version it keeps; the changed one, in the warning ink, names what Settings say now and *Retry with current settings* |
| Retry with current settings | On a task whose run failed at its push, change the project's check command and press *Retry with current settings…* | *Check command: old → new*, *It starts at the Verify step, the first step whose command changes.*; *Retry with current settings* runs Verify on the new command before anything is pushed |
| Retry with current settings, nothing changed | The same, with nothing changed | *Nothing differs from the last run's configuration.*, and it starts where *Retry* would |
| A workflow gone | Delete the workflow a failed task ran, then *Retry with current settings…* | Blocked, saying the workflow is no longer on offer; *Retry* still runs the run's own snapshot |
| A timed out issue | An issue whose run timed out, with `[unattended] timeout` changed since | *Timed out at … after …, against its timeout of …*; *Retry…* first, *Retry with current settings…* beside it |
| Preview | Open the launcher, expand *Preview*, type a title | The steps are listed, and the first prompt shows the title as it is typed |
| An issue found by its label | Switch the scratch project on for unattended runs, keep an issue in its Issues tab labelled `auto`, set `[unattended] agent = "Mock workflow"` and `mode = ""` (the mock offers no modes), then *Look for an issue now* in Settings ▸ Workspace | A task *#… · Work an issue* is under *Running*; a worktree on `onehand/local-<n>-<title>` is a project of its own and no session moves on screen. When it ends the project goes from the rail, the task is under *Finished*, and the issue has a note: *onehand left 1 commit on …* |
| An issue picked by hand | *Work an issue…* from the project's menu, choose an issue, *Run* | The chosen row is marked and the start form opens below the list; after *Run* the dialog closes, nothing moves on screen, and its project stays when it ends |
| A workflow picked | *Work an issue…*, choose an issue, choose *Implement on a branch* in the *Workflow* menu, *Run* | The task on the Tasks page names *Implement on a branch*, not *Work an issue* |
| A workflow label | Settings ▸ Workspace ▸ Unattended runs: add `bug` → *Implement on a branch*; label an issue `auto` and `bug`, then *Look for an issue now* | Its task runs *Implement on a branch*; `onehand.toml` has `[unattended.workflows] bug = "builtin:branch"`; the issue keeps `bug` |
| The trigger label refused | Add `auto` as a workflow label | Refused under the row, nothing written |
| A checkout workflow refused | Set `workflow = "builtin:checkout"` under `[unattended]` and restart | Settings says nothing will be picked up, naming the checkout; the Workflow menus do not offer it |
| From the Issues tab | Select an open issue, *Run workflow…* | The start form opens on that issue with no row to pick: *Workflow*, *Where it works* naming the branch and agent, *Instructions for this run*, the limits, *Preview*; *Run* starts the run, the dialog closes and the issue stays on screen, saying the run is starting with *Open session* |
| A mode not offered | Open a session on an agent that offers modes, set `[unattended] mode` to one it does not offer, then *Run workflow…* on an issue | *Before it starts* says, in the danger ink, that the agent offers no such mode and what it offers; *Run* is spent and says one thing blocks; the issue keeps its labels. Edit the agent's spec in Settings ▸ Agents and open the form again: the mode reads *not known yet*, muted, and *Run* is offered |
| A worktree run outside git | Open the launcher on a folder that is not a git repository and pick a worktree workflow | Under the fields, in the danger ink: no worktree can be cut; *Run* is spent. A checkout workflow there is offered |
| A Retry whose mode is gone | Run an issue task with `mode` set to one the agent offers, let it end exhausted, then make the agent offer other modes (or change the spec's mode list) and open a session on it; *Retry* the task | The dialog lists, in the danger ink, that the agent offers no such mode, that the run keeps its own setup and that *Retry with current settings* runs with what Settings say now; *Retry* is spent. Changing `[unattended] mode` does not clear it, and *Retry with current settings…* then shows *Mode: old → new* and runs with the new one |
| A configuration failure | An issue task whose run failed because its mode was not offered | The issue says why, with *Retry with current settings…* first and *Retry…* beside it |
| Answer the pull request review | On a GitHub project, an issue whose task is done with its draft pull request open, a review left on it | The issue offers *Answer the pull request review* beside *Open pull request*; pressed, a new run starts at the repair step with the review as its note, and the issue is told it answers the review, as putting the label back does |
| A review that cannot be answered | The same, after closing the pull request unmerged; then with a commit pushed to the branch on GitHub that the worktree does not have, the pull request open again; then on a workflow with no status checks step | The dialog lists *Its pull request … was closed without being merged …*, then *The branch on the forge went its own way …*, then *… has no status checks step …*, in the danger ink, and *Answer the review* is spent; nothing is claimed, and the issue gets no comment |
| An older run in the review | A run kept by a build from before command results were kept, waiting at an approval after its check | The block's *Check* says *not recorded for this run*, or *cannot tell whether the check covers the work now* on a dirty worktree |
| Check the agent | Edit the Mock UI agent's arguments (a new spec), then *Run workflow…* on an issue with `mode` set | *Mode … is not known yet* with *Check the agent*; pressed, *Checking…*, then the line goes, or turns into a block in the danger ink naming the modes offered; no session appears, no prompt is sent, and no adapter process is left (`pgrep -f mock_ui_agent`) |
| An agent that does not come up | The same with the agent's command set to one that does not exist | Beside the button: *… could not be checked: …* |
| Limits | Any start form or Retry dialog | *Before it starts* ends on *Limits: 45m of work, …; 3 failed turns …*, from the workflow that will run |
| A shared checkout | Prompt a session in a project, then open the launcher on a checkout workflow there | *Your session “…” works in this checkout too …*, muted; *Run* is offered |
| No forge for forge steps | The launcher on a project with no `origin`, a workflow with a push step | *No forge serves the project: the forge steps pass at once, and the branch is the result.*, muted |
| Remove a merged worktree | On a GitHub project, an issue task done with its pull request squash-merged on GitHub and the remote branch deleted; the run's project closed | The issue offers *Remove worktree…*; the modal names the folder and branch and says *The forge merged #N at …*; *Remove* removes both, and `git worktree list` and `git branch` no longer show them |
| Refused while used | The same, with a terminal open on the worktree's project in another window | The modal lists *Project …, open in another window, with a terminal, uses the folder.* in the danger ink, and *Remove* is spent; close it and reopen the modal, and it goes |
| Refused with work past the merge | Commit in the worktree after the merge; then, separately, leave a new untracked file | *The branch has 1 commit past the head the forge merged.*; then *Uncommitted or untracked files: ?? …*; nothing removed |
| Changed while asked | Open the modal on a clean worktree, then create a file in it, then press *Remove* | Nothing removed: *Not removed, as things stand now: Uncommitted or untracked files …* |
| A full slot | With `at_once = 1` and an issue task running, *Run workflow…* on another issue | *Before it starts* names the issue being worked; *Run* is spent |
| An earlier task needing attention | On an issue whose last task ended exhausted, *Run workflow…* | Muted: the last task ended and a new start makes a second task, with *Retry…*, which closes the form and opens that task's Retry dialog |
| A template | *New issue* in the tab, press *Bug* in the *Template* row | The body holds *Problem*, *Scope*, *Acceptance* and *How to check*, each with its hint; the labels field gains `bug`; the row goes once anything is typed in the body. Save with only *Problem* filled: the issue's facts line says *No scope, acceptance or how to check written*, muted |
| A project's own templates | In a project with `.github/ISSUE_TEMPLATE/story.md` (front matter `name: Story`, `labels: story`, body `## Why` and `## Done when`), *New issue* | The *Template* row offers *Story* alone; pressed, the body is the file's body and the labels field gains `story`. Save with only *Why* filled: the facts line says *No done when written*; *Run workflow…* on it says the same |
| No advice | An issue whose body is a sentence, or written under other headings | No *No … written* line, in the detail or the start form |
| Advice on a run | Run a workflow on the templated issue above, by hand and by its label | The start form says the same muted line and *Run* is offered; each run's first report ends on *The issue has no … written.* |
| Instructions for this run | In that form type `Keep the old flag.` under *Instructions for this run*, open *Preview*; *Run*; later *Retry* the task | The first prompt in the preview ends its instructions with the line as it is typed; the run's first prompt carries it, and so does the retry's; the issue's body is unchanged |
| Where the work stands | While that run works, look at the issue | Above the body: *Running · Plan · step 1 of N*, *Working on Plan, started …*, no primary action, *Open session* and *Stop*. Below it: *What the work left* names the branch |
| A long body waiting for approval | Give an issue a body several screens long, run on it, with the mock workflow agent, a duplicate of *Implement on a branch* given an approval step after its first, until it waits there | Without scrolling: *Waiting for approval · <the approval step> · step 2 of N*, *Approving starts …* and *Review…*, which opens the issue on the Issues page with its review block open |
| A step ends while reading | Scroll into the body of an issue whose run is working, and wait for a step to end | Nothing above the body moves or changes height; only the words of the three work lines change |
| A narrow dock, the largest zoom | Drag the Workbench to its narrowest and zoom in to the largest step | The primary action and *Step · step N of M* are drawn whole; the words before the step give way |
| The network pulled | On a GitHub project with an issue whose run opened a pull request, turn networking off and press *Refresh* | The pull request line says *could not be read* in the warning ink, keeping the last state marked stale beside the failure; the branch line stays |
| Switching issues mid-read | Open an issue with a pull request and switch to another kept issue with one at once | The second never shows the first's pull request; *read …* belongs to the one on screen |
| A closed issue with a live run | Close an issue in the tab while its run waits on a card | The confirm says *A run is still working on it; closing does not stop it*; afterwards *Open session to answer* and *Stop* stay, with *Issue is closed; this run is still active*. When the run ends, the line mutes to *Reopen issue* and *Show task* |
| Before | Retry a task on the issue, or start a second one after the first ended | *Before* counts the earlier runs and names the earlier task, in the warning ink if it needs attention; each *Show task* opens it |
| The Issues page | With twenty issues across two projects, some waiting for approval, some running, some never run, pick *Issues* in the rail | Both docks go away and the header reads *Issues*; without opening any, the waiting ones say *Waiting for approval* in the warning ink, the running ones *Running · <step>*, and the ones never run draw no line of work; *Needs attention* lists only the waiting and ended ones |
| An approved issue stays | Under *Needs attention*, pick a waiting issue and approve it in its review block (or its session), then pick *Issues* again | It is still in the list, saying *now Running · …*, at the place it was; picking another issue lets it go |
| Coming back | Pick an issue, filter by a project, scroll the list, then open the run's session (or the task detail, or a diff) and pick *Issues* in the rail | The same issue, filter, search and scroll |
| Narrow | Narrow the window below two columns and pick an issue | The issue alone under *Back*; *Back* finds the list with its filters, scroll and the issue just read selected. At the largest zoom step a row still shows its title and its line of work |
| Open in Issues | In the tab, ⋯ ▸ *Open in Issues*; also press a project row's *auto · #N* pill | The page opens on that issue; with the page filtered to another project and *Closed*, it is pinned on top, *Outside current filters*, the filters unchanged |
| An older task needing attention | An issue whose newest task is done and an older task exhausted | Under *Needs attention*, its row saying *earlier task needs attention* |
| Pull requests not read | On a GitHub project with more open pull requests than one read takes (or `gh` signed out), filter *Pull request open* | It says how many issues were not read, *the list may be incomplete*, with *Read them*; a project whose read failed is named |
| What the work left, in full | Pick an issue whose run has passed Verify | Under *What the work left*: *Check* says it passed on the work as it is, *N commits before the work now*, or *the work changed since the check passed*, with *Printed* its last line; *This run* and *Branch* count the files and open into them, a file opens its diff in place, and *Commits* counts those past where the task started |
| A check against the work now | After a run passed Verify, edit a tracked file in its worktree without committing; then undo it and add an untracked file instead | *Check* says *the work changed since the check passed* both times; a task from before this build on a dirty worktree says *cannot tell whether the check covers the work now* |
| A check in the review | A workflow with *Implement → Verify → Approve (of Implement)*, its check failing once then passing; *Review…* at the approval | *Check* says it passed against the work now, with the last lines the command printed; editing a file in the worktree and pressing *Review…* again says *the work changed since the check passed* |
| Review in place | On the page, an issue waiting for approval: press *Review…* | The block opens below where the work stands, pushing the body down: *Review: Plan*, the plan, *Acceptance* collapsed (when the issue has one), *Plan runs again with your note* beside *Revise…* and *Continue starts Implement: …* beside *Continue*; a plan that changed no file draws no files and no check |
| Continue from the block | Scroll the block's answer half way, press *Continue* | *Approved; sent to the run.* at once, the scroll where it was, then *The run moved on to Implement.*; the block stays until *Close* or another issue is picked; a run reaching its next approval does not open it |
| An answer changed under the reader | Two windows on one workspace, both on the page with the block open on the same waiting issue; in one, *Revise…* with a note; wait for the plan to come back; in the other, press *Continue* | Nothing is approved: the block reloads to the new plan, saying *The answer changed since you opened it.*; a second *Continue* approves it |
| A long answer | A plan longer than 60 lines waiting for approval | The block draws its last 60 lines with *Show all N lines*, and *Showing the last 60 of N lines.* above *Continue*; *Show all* draws the whole of it in place |
| Revise from the block | *Revise…* in the block, *Send back* empty, then with a note | Empty says *Say what to change.*; with a note, *Sent back; sent to the run.*, and the plan runs again with the note |
| Review from the tab | In the Issues tab, an issue waiting for approval: press *Review…* | The Issues page opens on the issue with its review block open |
| The task detail's approval | Open a waiting task's detail on the Tasks page | *Awaiting approval* draws the answer, *Revise…* and *Continue* each beside what it starts; *Continue* moves the run on, as the strip's does |
| One file for two views | With the tab and the page both on one issue, edit it in one | The other shows the edit at once; the footer says one sync, not two |
| A draft kept | Start a new issue in the tab, type a title, open a session and come back; then pick another issue | The draft is still there; picking another issue asks *Drop this draft?* in a modal; an untouched form goes without asking |
| Two at once | With `at_once = 1`, *Work an issue…* while an issue task runs | Refused, naming the issue being worked |
| Who holds the slots | With `at_once = 1` and an issue task running at Implement, open Settings ▸ Workspace | *Slots: 1 of 1* with the holder *#N · Work an issue (Implement)* as a link; pressed, Settings closes and the task opens; *Run workflow…* on another issue says the same line in *Before it starts* |
| No slot held | With nothing running | *Slots: 0 of 1* and *0 waiting on a person, no cap* |
| Waiting capped | `waiting = 1`, an issue task waiting at an approval, `at_once = 2`; *Look for an issue now*, then *Run workflow…* on another issue | Both refused, naming `unattended.waiting`; answer the approval and the next look takes the issue |
| Queued behind a task | Two checkout tasks on one project, the second started while the first runs | The notification names the first task: *… is queued behind <its title>, working in <folder>* |
| Push and pull request | On a GitHub project with CI, an issue task through Verify | `git ls-remote origin <branch>` shows the commit Verify passed on, a draft pull request is open on the branch closing the issue, and once its status checks pass it is out of draft; the issue says *onehand opened … It is ready for review.* |
| A failing check is repaired | As above, with a check that fails on the change | The run goes back to Implement with the check's name and log in its prompt, pushes again, and waits again |
| No checks at all | As above, on a repository with no CI | The run waits ten minutes, then the pull request leaves draft |
| No forge | An issue task on a project with no GitHub `origin` | Push, Pull request and Status checks each pass at once; the branch is the result |
| A review answered | Put the trigger label back on an issue whose pull request is open | The issue's task gets a new run from Implement, its prompt saying how to read the review; it pushes to the same pull request |
| A closed pull request | Close the pull request unmerged, then put the label back | No run; the issue says the pull request was closed and onehand will not open another |
| The label put back mid-run | Put the label back while the issue's task waits on its status checks; also *Work an issue…* on it | The tick passes it over; the pick is refused, *A run is already working on issue #…* |
| A report that could not be sent | On a project served by GitHub, sign `gh` out (`gh auth logout`) before an issue task ends, then sign in again | The task's file keeps the report under `unsent`; the next tick, or the next start, comments it on the issue and empties `unsent` |
