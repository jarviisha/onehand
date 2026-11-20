# Workflows

How a workflow is written, checked and run: the file, the engine, and the run's drive and its carrier.

A run takes a brief through the steps of a workflow: an agent step prompts the
session and is judged by gates onehand checks itself, a command step runs a command in the work, an
approval step waits for a person, and the forge steps (push, pull request, status checks) take a
worktree's branch to the forge. The words are defined in [CONTEXT.md](../CONTEXT.md).

## Where the code is

| Part | Where |
|---|---|
| Workflow types and serde | [crates/core/src/workflow/template.rs](../crates/core/src/workflow/template.rs) |
| What stops a workflow being saved or run | [crates/core/src/workflow/validate.rs](../crates/core/src/workflow/validate.rs) |
| A step's agent settings, and the planner that picks them one at a time | [crates/core/src/workflow/settings.rs](../crates/core/src/workflow/settings.rs) |
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
| The run's drive: what it decides | [crates/core/src/task/drive.rs](../crates/core/src/task/drive.rs) |
| What a run holds while a mark is pinned, and what it does for each action | [crates/core/src/task/drive/pin.rs](../crates/core/src/task/drive/pin.rs), [steps.rs](../crates/core/src/task/drive/steps.rs) |
| A run's forge calls and its looks at the status checks | [crates/core/src/task/drive/forge.rs](../crates/core/src/task/drive/forge.rs) |
| The tasks' drive: every task, its place, a run's end | [crates/core/src/task/tasks.rs](../crates/core/src/task/tasks.rs) |
| A new task as the launcher describes it | [crates/core/src/task/launch.rs](../crates/core/src/task/launch.rs) |
| A visit's settings put in force before its first prompt | [crates/core/src/task/drive/settings.rs](../crates/core/src/task/drive/settings.rs) |
| The run's carrier: what the app carries out for it | [crates/app/src/task/carrier.rs](../crates/app/src/task/carrier.rs) |
| Each step's settings judged before a start | [crates/core/src/preflight/settings.rs](../crates/core/src/preflight/settings.rs) |
| The `Tasks` global: the tasks' drive carried out, the runs under way | [crates/app/src/task.rs](../crates/app/src/task.rs), [task/place.rs](../crates/app/src/task/place.rs) |
| Workflows on offer | [crates/app/src/workflow.rs](../crates/app/src/workflow.rs) |
| Launcher, and driving a task once its place is free | [crates/app/src/shell/workflows.rs](../crates/app/src/shell/workflows.rs) |
| The Workflows page and its editor | [crates/app/src/workflows_page.rs](../crates/app/src/workflows_page.rs) |
| An agent step's *Agent settings* in the editor | [crates/app/src/workflows_page/step/settings.rs](../crates/app/src/workflows_page/step/settings.rs) |
| Each project's check command field | [crates/app/src/shell/check_commands.rs](../crates/app/src/shell/check_commands.rs) |

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
mode = "plan"             # agent settings, by the agent's own ids; both optional
options = { model = "strong", effort = "high" }
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
`workflow/<title>` branch in a worktree in the project's worktree folder (beside the project's
repository with none set), recorded in the project, committed; a project that
is a folder inside its repository works in the same folder of the new checkout, as a worktree made
from the project menu does) and *Work an issue* (`builtin:issue`, the same steps on a branch, then
Push → Pull request → Status checks; what a lead's `start_issue` runs by default, see
[control.md](control.md)). The person's own are in
`<config_dir>/onehand/workflows/<slug>.toml`, made by duplicating a shipped one or from *New
workflow* on the Workflows page. A build from before the rename kept them in `pipelines/`;
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

### Agent settings

An agent step may name a `mode` and `options`, a table of config option ids to values. Both are the
agent's own ids, as it sends them (`ConfigOption::id` and `ConfigChoice::value`), never the names
the composer shows. A workflow is not tied to an agent, so nothing is judged when it is saved; a
value a step names that the agent does not offer **blocks**, before the start when what it offers is
known and at the step when it is found only once running, and never runs on something else. **A
setting a step does not name goes back to what the run started with**, never what the step before it
left: a *Plan* step in a planning mode must not leave *Implement* unable to edit, so a step runs the
same whichever step ran before it. None of the shipped workflows names any: model ids differ between
agents, and they must run on any. On the Workflows page an agent step has an *Agent settings*
section offering the choices of the agent runs take by default (the first configured), as it was seen to offer them in this process; a muted line under the heading names it
(*Choices … offers*). *Mode* is a row of choices in one filled track, *Run's mode* first, then each
mode offered; each option offered, in the agent's order, is a row named by its id, *Run's value*
first, then each value (twelve options and twelve values at most, then how many more). A row shows
the value the step sets, picked or typed below; picking sets it, *Run's …* clears it, and either
drops a typed row setting the same id. A value the step names that the agent does not offer (written
for another agent, or by hand in the file) stays on its row, chosen: a mode or a model in the
warning ink, marked *not offered*; another option's value muted, marked *not seen*, since the values
were learned under whatever model the agent last ran and the run checks them when the step starts.
Below, *Other options*: rows typed by id and value, and *Add option*, for what the agent does not
list. While what the agent offers is not known yet, *Mode* and the option rows are typed, and the
line says so beside *Check the agent*, started in the project selected in the rail (else the first
one the window has open, else the home folder); a value picked while the offer was known keeps its
row, unmarked, to keep or leave to the run. When the check comes back the choices are drawn, and a
typed row naming an option offered, with one of its values, becomes that option's pick, unless it is
being typed in, when it stays a typed row. With no agent configured both are typed, and the line
says so. The editor judges nothing: the preflight does. A shipped workflow's choices refuse, saying
it is duplicated to be changed. An option set twice on a step, by two rows or by a pick and a row,
is a problem, and *Save* refuses.

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

The Workflows page has *Export…* in the ⋯ of every readable card and of the editor, shipped ones
included: a native save
dialog, then `store::export_blocking`. *Import…* beside *New workflow* picks a `.toml` file and
reads it with `store::read_blocking`; a file that does not read is refused in a notification, and
one that does opens in the form as a new workflow. Nothing is written until Save, which runs
validation, the name check and `save_blocking`, so an import always gets a new id.

An agent does the same through onehand's tools (see [control.md](control.md)): `read_workflow` gives
a workflow's TOML, and `save_workflow` takes it back through `store::save_text_blocking`, the same
parse, validation and name check, going over the person's own file that has its id, else into a
new one.

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
`on_fail` that is no earlier agent step, and a `wait` that is not a duration; on an agent step, an
empty mode, an option with no id or no value, and an option `mode`, which is set as the step's mode.
`mode` or `options` on any other kind of step is a key onehand does not read there.

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

`workflow::Run` is the only place a transition is decided. The run's drive reports what happened —
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
  (`Brief::pull_request_text`: the brief's title, and a body saying onehand opened it); one closed
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

## The drive and its carrier

The run's drive decides in core, and its carrier acts in the app. `onehand_core::task::drive::RunDrive` is given a
person's commands as methods (`approve`, `revise`, `stop`) and told what happened (an `Input`),
each with the run's session as it stands, and answers with what to do (`Effect`s): pin, measure,
read the work, run a command, call the forge, arm a clock, save, finish. It has no timer or I/O of its own, so every rule below is tested in core. Each effect that
is answered says what its late answer gets: a pin, a command, a push, opening a pull request and
taking a draft out of draft are awaited, so a Stop waits for them; a read, a look and a clock are
dropped; a save is recorded, and a failed one is said once in the run's window until a write lands
(`Signal::SaveFailed`). The carrier (`crate::task::carrier`) maps the session's events onto inputs,
carries out the effects and brings their results back. The drive decides the mode and the settings
picks against its session's `Chat`, which it only reads; the link makes each through the session's
own drive (`SessionDrive::pick`) before anything else reaches the run, and the next is planned once
the agent has answered. The run's notes in its transcript, its prompt and its cancel are asked of
the session the same way, after the picks and in order, so none is an effect a carrier sees: every
command to a run goes through `task::drive::link::Joined`, which holds the run with its session for
the call, tells the run at once of a prompt its session did not take, and answers the session's
effects and then the run's.

The `Tasks` global (`crate::task::Tasks`) holds every run under way by its task id (a check's run
has no session), beside the tasks' drive, which a run's end is handed to;
`crate::workflow::Workflows` keeps only the workflows on offer. The carrier subscribes to the
session and maps its events onto the engine:

| Event | Report |
|---|---|
| A turn the run sent ends | facts read in the background, then `turn_ended`; with the agent still at work on the step (`Chat::running_since` its prompt: background work started since, a subagent), once it no longer is, its own answer to that work included |
| A turn ends cancelled (`Chat::cancelled`) | `stopped(ByPerson)` |
| A turn the run sent ends failed (`Chat::failed`: an error, a refusal, a limit the agent reached) | `failed`, in the failure's words |
| A prompt the run did not send (`prompts_sent` above what it sent, or one queued) | `stopped(TakenOver)` |
| The adapter goes | `stopped(LinkLost)` |
| The session is released | `stopped(Closed)` |
| *Stop* on the strip | the turn is cancelled, then `stopped(ByPerson)` |
| The timeout runs out | the turn is cancelled, then `stopped(TimedOut)` |
| A push or a pull request is done, or fails | `forge_done` |
| A look at the pull request's status checks, on a clock the drive arms every minute, says something other than pending | `status_checks_seen` |

**The forge is called one call at a time**, each decided by the drive: the push; then the pull
request on the branch read, taken up when one is open and opened otherwise, unless Stop was pressed
meanwhile; then, while the status checks run, a look each time the clock the drive armed runs out,
with the logs of at most three failing checks read for a repair, and a draft taken out of draft
once they all pass. The carrier only makes each call on the run's connector and reports it back; a
run whose forge cannot be called at all (no branch of its own, no connector of its name) is told
so, and fails there.

**Every one of these goes through one `end`, and a step's command is stopped first.** While a
command runs, the run holds the flag that calls it off (`process::output_until`): ending sets it,
the command's whole process group is killed, and the run is said to be stopped only once the
command has exited, so a stopped run never leaves a build or a test writing to the work. A
command called off before it started is never started. The first reason given is the one the run
ends with.

A prompt asked for before the adapter is up waits for the link. When the agent first comes up in a
session, a run whose setup names a mode (`Setup::mode`) has its agent put in that mode before the first
prompt; an agent that does not offer it, or refuses it, fails the run on its mode, not on a step's
settings, and
the first prompt waits for the agent's answer. Then the mode and every config option value it is
running, among the values it offers, are kept as the run's starting settings (`Run::started_with`),
once: they are saved with the run, so a Resume on a new session finds the same ones; a Retry, being
a new run, takes them afresh. They are what the agent publishes as it comes up: an option it
publishes only later is not among them, so a step that does not name it leaves it as the agent has
it. The clock is the run drive's `Budget`: it pauses while a card or an approval waits on a person, or
the pull request's status checks run, and its timer looks again when it fires rather than being
re-armed at every pause.

**Every visit to an agent step starts with its settings in force** (`workflow::Settings::for_step`:
the step's own over the run's starting ones). Before the visit's first prompt, the planner
(`workflow::next`) picks the next setting not yet in force, in order: the mode, then `model`, then
the rest by id. Each pick is sent and the prompt held until the agent answers it, and the next one
is planned against what the agent offers after that answer, because an agent may change which values
an option has, or whether it has one at all, when the model changes. **A setting the step names that
the agent does not offer**, or any pick it refuses, fails the run at that step, a configuration
failure naming the setting and the agent's reason. One the step does not name, taken from the run's
starting settings, is still picked when the agent offers it, and left as the agent has it when the
agent no longer does: a step that changes only the model is not failed on an effort the new model
lacks (`workflow::Settings::named_by` says which are the step's own). What an answer carries of the
agent's new offer (`configOptions` in a pick's reply) is taken before the answer itself, so the next
pick is planned on it; an adapter that publishes its new offer only later turns a value not offered
into a pick refused, and it fails at the step all the same. When anything was sent, the session gets a
note, *Settings: mode … · model … · …*, and the visit records what the agent reported when its
prompt went (`Visit::ran_with`, from `Chat::held`), all of it, not only what the step named, and an
alias as the agent reported it. A Resume opens a new visit, so the open step's settings are set
again on the new session, whatever reconnecting restored. A person's own pick in the composer during
a run holds until the next visit starts; one the agent refuses is the person's, not the run's.
Settings come from the run's snapshot, so a Retry keeps them and *Retry with current settings* takes
the workflow's current ones. **An alias is never taken for the value named**: an adapter may report
a value its choices never list (`opus[1m]` against the offered `opus`), so such a value counts as
nothing in force when planning: a step naming `opus` sends it, and an option the run started on an
alias is not among its starting settings, so a step that names nothing leaves it as it is.

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
(the answer, or how a failed command's output ended), its command's result on a command step, the
settings it ran with on an agent step, and how it came out. **A failed run keeps what it failed
on** (`Run::failure`, read as `Run::failed_on`), beside its outcome rather than in it:
*configuration* when what the preflight would have blocked is found only once the run runs
(`preflight::found_late`: the agent no longer configured, a mode the agent does not offer, a
step's setting it does not offer or refuses, no check command to run, a workflow that no longer
validates), *forge* when a forge step or the forge failed, and *other* for the rest. Going back to
a step is a new visit, and resuming closes the cut-off visit as `interrupted` and opens a new one of the same
step. **Each visit's start and end is pinned as a commit** (`task::marks::pin_blocking`): the work
as it stands, untracked files included and ignored ones left out, committed from a temporary
index so the person's own index is untouched, and kept under
`refs/onehand/tasks/<task>/<run>/<visit>/<start|end>` so `git gc` keeps it. The drive pins before
it carries out a step's first action, so the start mark lands before the prompt or command touches
the work; one commit serves a visit's end and the next one's start. A mark that cannot be pinned is
logged and left out, and never stops a run: gates read `Mark` and `Facts`, not the pinned commits. While a mark is
being pinned, an approval or a revision waits for it and for the action it holds back, so nothing
moves the run on under that action; replayed, it still carries the visit it was drawn from, so a
second press held behind the first finds that visit closed. The drive routes a press by the
task, not the session, so a window other than the session's can make it, and the run's clock goes
on only once the engine took the press. A Stop, the timeout or any other ending waits for the pin too,
and then runs instead of that action, with its cancel of the turn: a prompt is never sent to a run
already said to be over, and no ending races the pin. A drive
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
A workflow with a command step that names no command needs the project's check command, set on
the project's page beside *Run check* and kept in its record in the workspace file (`ProjectConfig::check`).

**Workflows are written on a page of their own**, opened from the rail's *Workflows* row: the
workflows on offer as cards, the person's own first, and a search over their names and
descriptions, which the cap applies after. Pressing a card opens its workflow in the editor, a
page of its own; a shipped one opens read-only, changed by *Duplicate to edit*. The editor sets the
workflow's fields (*About*, *How it runs*) in a column beside *Steps*, and the steps run down a
rail, a numbered dot each, one open at a time in the column beside them, each
saying where its failure sends the run back (*on failure ↑ 2*) and its first problem without
being opened; its ⋯ holds *Move up* and *Move down*, refused at the ends, and *Remove*. A name
another workflow has is said in the bar along the editor's foot before *Save* is pressed (*Another workflow is already
called …*), and *Save* refuses until it is changed; one write lands at a time, and the saved
workflow stays open. *Cancel* and *Workflows* (back) ask, as Esc does, when the form holds
changes (*Keep*, *Discard*), and go back to the cards. The draft and the pick are the page's entity's
(`WorkflowsPage`), held per window, so going to another page and back finds them as they were;
going back to the cards, or closing the window, with changes unsaved asks first in a modal. The
pick is kept by key (a file, or a shipped workflow's id), never by place, as the list is read again
after every save. *Run…* on a card asks which project at once, a card holding only what is saved.
*Run…* in the editor's head refuses while the workflow holds changes or was never
saved, as a run starts from what is saved; otherwise it asks which project, the one selected in the
rail first, and opens the launcher on it with the workflow picked; the composer's `+` menu and the
keymap still open the launcher on the project on screen.

Under the workflow picker, a collapsed *Preview* opens on what the run would start with: a line
with where it works, its timeout, its misses and its version; a line per step
(`StepSpec::summary`); and the first prompt as the agent would receive it
(`workflow::first_prompt`), filled with the brief as it is typed, in a scrolling box.

The launcher reads, top to bottom: the workflow menu, what it does and where, the collapsed
*Preview*, *Title*, *Details* and *Instructions*, then what the preflight found (blocks in the
danger ink) above *Run*, spent while a block remains; it scrolls so its footer stays.

**A start is preflighted before anything of its own** (`onehand_core::preflight`): one pure
function, given the kind of start and the facts the app holds, returns findings, each blocking
the start or only saying something, with where it is changed. It never cuts or starts; whoever
carries the start out still does, after it. It judges the configuration that will actually run,
and the app gathers the facts without reaching the disk or the network in a render: the forge
serving a project is found when the form opens.

A new run from the launcher (kind *new run*) is judged on its workflow, the agent, the project's
check command, a worktree workflow on a folder outside git (blocked), the forge's account when
the workflow has forge steps (blocked when `gh` is missing or signed out, as last seen), and that
its branch is cut off `HEAD`. Who each connector acts as is asked off the UI loop once at launch
and again on *Check again* in Settings ▸ Connections (`connector::accounts::AccountsDrive`,
carried by `crate::accounts`); until it answers, the account blocks nothing. Whether the folder is in git and which forge serves
it are read off the UI loop when the launcher opens; until they land, neither blocks. The launcher lists what it found under its fields, and *Run* is spent
while a block remains. A Resume and a Retry are judged by the run's own setup, and a *Retry with
current settings* as a new start is, on what Settings say now
([tasks.md](tasks.md#retry-and-resume)).

| Check | Blocks when | Says |
|---|---|---|
| Workflow | it is not there, or `workflow::validate` finds problems (each listed, first) | |
| Agent | none is configured, or the one named is no longer | |
| Mode | the agent's current offer, learned in this process from the spec as it is now, does not hold it | the mode is not known yet, with *Check the agent* |
| Step settings | an agent step's mode, `model` or option id is not in the agent's offer as learned (eight at most, then how many more) | another option's value not in it: not known yet under the model the step runs on, checked when the step starts; nothing of the agent's offer known yet, once for the whole workflow, with *Check the agent* (not said when the mode's own *not known yet* is) |
| Check command | the workflow runs the project's check command and the project has none | the workflow runs no command: nothing verifies the work |
| Place | a worktree workflow on a folder outside git | for a new run of a checkout workflow, a person's own session that has been prompted in the same checkout, by its name: both edit the same files |
| Base | | that the branch is cut off `HEAD` |
| Forge | `gh` missing or signed out, as last seen, and the workflow has forge steps | the workflow has forge steps and no forge serves the project: they pass at once, and the branch is the result |
| Limits | | every kind: the timeout and the misses allowed, from the snapshot that will run (the run's own for Resume and Retry) |

**What an agent offers is learned whenever it comes up**, a person's session included: its modes,
and each config option with its values. It is kept per agent spec (its command and arguments,
compared whole) for the life of the process (`Shared::offers_seen`). A spec edited since is another
spec, so what was learned of it stops counting; a restart forgets, so an upgraded adapter is never
judged by an old list. Only a list that is current blocks; otherwise the mode is *not known yet*,
and the drive's check when the agent comes up stays the authority. An option's values are only
certain under the model the agent was running when they were learned, the step's or one a person
picked in the composer, so a step is judged on its other options' values only where an id is
missing, and the run's own check at the step decides the rest.

**Check the agent**, beside a mode or step settings not known yet, starts that agent in the project with no
session, reads the modes and config options its `session/new` answers with, and closes it, off
the UI thread and prompting nothing (`agent_check`, through `AcpRuntime::probe`, which never takes the adapter
parked for the next session). It is the one check that costs an agent start, so only its press
runs it. What it learns goes into `Shared::offers_seen` like any other sighting: the launcher, judged
every frame, turns *not known yet* into a block or into nothing; a dialog judged once when it
opened says beside the button what the agent offers. A failure to come up, or no answer
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
  earlier step is the only way back, besides a revision.
- **A step cannot use a different agent from the rest.** One session carries every step; a step
  sets only that agent's mode and options.

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
With `--settings` after the script's path in `args` it offers modes `code` and `plan`, and the
options `model` (`strong`, `cheap`), `effort` (`low`, `high`, `max`) and `fast` (`off`, `on`),
starting on `code`, `strong`, `low` and `off`; under `cheap` it offers no `high` effort, and it
always refuses `max`. Each answer then begins with what is in force,
`[mode code · model strong · effort low · fast off]`. Without it, it offers nothing.
It walks the cases below; it never edits during a plan or commits in a checkout, so the
`code_unchanged` and `uncommitted` carry-ons are reached only by a person changing the work
mid-step.

A run takes the first agent in the list, so put *Mock workflow* first. Set up a scratch repository
with one commit, open it as a project, and set the project's check command (on its page, beside *Run check*) to:

```sh
sleep 5 && grep -q 'check: pass' mock-workflow.txt
```

Then walk each case, with *Work in checkout* unless it says otherwise. Between cases, discard the
change (`git checkout . && git clean -fd`).

| Case | Do | Expect |
|---|---|---|
| Done | Brief `go`. *Continue* at the approval | Plan, Approve, Implement, Verify; *Workflow done*. `mock-workflow.txt` is left uncommitted. On *Implement on a branch*, a new branch holds one commit |
| Stop while a command runs | Brief `go fast`, *Continue*, then *Stop* during Verify's `sleep 5` | The run ends *Workflow stopped* only once the command has exited: no `sleep` is left (`pgrep -f 'sleep 5'`) |
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
| Stop on a running row | *Stop* on a running task's row on the Tasks page | The run ends *Workflow stopped* and moves to *Finished* |
| Marks | After any run, `git for-each-ref refs/onehand` | A `start` and an `end` ref per visit, under `refs/onehand/tasks/<task>/<run>/<visit>/`; `git show` on one includes `mock-workflow.txt` while it is untracked |
| Task detail | After *Exhausted*, press the row's title | The detail shows Run 1: one Plan visit, ending *too many misses at the Plan step*; the misses are in the run's history. Open the visit: its output, then *No files changed.* (or the files, with `+N −N`). Press a file: its diff. *All tasks* goes back |
| Retry from an earlier step | Brief `go`, *Continue*, then *Stop* during Implement; open the task under *Finished* and press *Retry* | The dialog's *From …* menu lists Plan, Approve and Implement, at Implement; pick Plan and the description says it starts at Plan carrying no answers; *Retry* runs Plan again |
| Retry a finished task | After *Done*, open the task and press *Retry* | The menu defaults to Plan; the new run goes through every step, and the detail lists Run 1 under *Earlier runs* |
| Retry a dismissed task | *Dismiss* an ended task, open it under *Finished*, *Retry* | It leaves *Finished*; if the run ends on something nobody chose it is under *Needs attention* again |
| Runs from an older build | Put a run file from before tasks in `<config_dir>/onehand/pipeline-runs/` and start onehand | It is in `tasks/` under the same name, listed as interrupted, and `pipeline-runs/` is gone |
| An unknown key | Copy a workflow file into `<config_dir>/onehand/workflows/` with `gate = "x"` added to a step, then *Import…* it too | The row reads *Cannot be read: it has a key `steps[N].gate` that onehand does not read*; the import is refused with the same reason |
| Export, then import | *Export…* *Work in checkout*, then *Import…* that file | The form opens on it as a new workflow; *Save* refuses, and the bar along the page's foot says *Another workflow is already called Work in checkout*; once renamed it is saved under a new id at version 1 |
| Retry after a rename | Duplicate *Work in checkout* and save it, start a run with brief `miss`, then rename the workflow and save | *Retry* keeps *version 1*, saying Settings now say *version 2*; *Retry with current settings…* lists *Workflow version: version 1 → version 2* and runs the renamed workflow |
| What Retry keeps | Let a run time out, then change the workflow's timeout (or the project's check command) and *Retry* | The dialog lists the agent, mode, check command, timeout and workflow version it keeps; the changed one, in the warning ink, names what Settings say now and *Retry with current settings* |
| Retry with current settings | On a task whose run failed at its push, change the project's check command and press *Retry with current settings…* | *Check command: old → new*, *It starts at the Verify step, the first step whose command changes.*; *Retry with current settings* runs Verify on the new command before anything is pushed |
| Retry with current settings, nothing changed | The same, with nothing changed | *Nothing differs from the last run's configuration.*, and it starts where *Retry* would |
| A workflow gone | Delete the workflow a failed task ran, then *Retry with current settings…* | Blocked, saying the workflow is no longer on offer; *Retry* still runs the run's own snapshot |
| Preview | Open the launcher, expand *Preview*, type a title | The steps are listed, and the first prompt shows the title as it is typed |
| A worktree run outside git | Open the launcher on a folder that is not a git repository and pick a worktree workflow | Under the fields, in the danger ink: no worktree can be cut; *Run* is spent. A checkout workflow there is offered |
| Check the agent | Edit the Mock UI agent's arguments (a new spec), then open the launcher on a workflow whose step sets a `mode` | *Mode … is not known yet* with *Check the agent*; pressed, the button waits with a spinner, then the line goes, or turns into a block in the danger ink naming the modes offered; no session appears, no prompt is sent, and no adapter process is left (`pgrep -f mock_ui_agent`) |
| An agent that does not come up | The same with the agent's command set to one that does not exist | Beside the button: *… could not be checked: …* |
| Limits | The launcher or a Retry dialog | *Before it starts* ends on *Limits: 45m of work, …; 3 failed turns …*, from the workflow that will run |
| A shared checkout | Prompt a session in a project, then open the launcher on a checkout workflow there | *Your session “…” works in this checkout too …*, muted; *Run* is offered |
| A signed-out forge | `gh auth logout`, *Check again* in Settings ▸ Connections, then the launcher on a GitHub project with a workflow with a push step | *GitHub cannot be used: `gh` is not signed in …*, blocking, *Changed in Settings ▸ Connections.*; *Run* is spent |
| No forge for forge steps | The launcher on a project with no `origin`, a workflow with a push step | *No forge serves the project: the forge steps pass at once, and the branch is the result.*, muted |
| Remove a merged worktree | On a GitHub project, a *Work an issue* task done with its pull request squash-merged on GitHub and the remote branch deleted; the run's project closed | Its task's ⋯ on the Tasks page offers *Remove worktree…*; the modal names the folder and branch and says *The forge merged #N at …*; *Remove* removes both, and `git worktree list` and `git branch` no longer show them |
| Refused while used | The same, with a terminal open on the worktree's project in another window; then, instead, a terminal of another project that `cd`s into the worktree | The modal lists *Project …, open in another window, with a terminal, uses the folder.* in the danger ink, and *Remove* is spent; close it and reopen the modal, and it goes |
| Refused with work past the merge | Commit in the worktree after the merge; then, separately, leave a new untracked file | *The branch has 1 commit past the head the forge merged.*; then *Uncommitted or untracked files: ?? …*; nothing removed |
| Another branch checked out | In a merged task's worktree, `git switch -c experiment`, then *Remove worktree…* | *The worktree has `experiment` checked out, not the task's branch …*; nothing removed, no branch deleted |
| A project in a folder | A merged task of a project that is a folder of its repository (`repo/app`) | The modal names the worktree's top, not its `app` folder, and *Remove* removes it |
| Changed while asked | Open the modal on a clean worktree, then create a file in it, then press *Remove* | Nothing removed: *Not removed, as things stand now: Uncommitted or untracked files …* |
| The Workflows page | Press *Workflows* in the rail | The header reads *Workflows*, both docks go, the row is highlighted; Settings has no Workflows page |
| Cards on arrival | Press *Workflows* in the rail | The cards under *Yours* and *Built in*, four to a row on a wide pane, fewer as it narrows; *New* is the one primary |
| A draft kept across pages | *New*, type a name, go to *Tasks* and back to *Workflows* | The editor is there with the name typed |
| Dropping a draft is asked | With that draft, press *Workflows* (back); then *Cancel*; then close the window | *Discard the workflow being edited?* in a modal, *Keep* keeps the editor open; *Cancel* asks the same; closing asks *Discard the workflow being written?*, *Keep* keeps the window open |
| Saved stays open | *New*, name it, *Save*, then *Workflows* (back) | The editor stays on it after *Save*; back on the cards, its card is under *Yours* |
| Keys step the steps | Open a workflow, open its first step, press ↓ and ↑ with no field holding the caret; then click into the prompt and press ↑; then click the step's head and press Esc twice | Each press opens the step after or before in the column beside the steps, held at the ends; in the prompt the caret moves; the first Esc closes the step, the second goes back to the cards |
| Narrow editor | Open a workflow and narrow the pane, then open a step | First the workflow's fields stack over the steps, the step column beside; narrower, the steps alone, and an open step takes their place under *Steps* (back), which closes it |
| A shipped workflow | Open *Work in checkout* | Its fields are disabled, no *Save*; *Duplicate to edit* opens *Work in checkout copy* as a new workflow |
| An unreadable file | Put a `.toml` that does not parse in the workflows folder | Its card names the file and says *Cannot be read: …* in the warning ink; pressed, it opens its reason and *Delete…*, which asks and removes the file |
| Run from the page | Two projects, the second selected in the rail; *Run…* on the *Work in checkout* card; then open a workflow, change it, *Run…* in its head | The menu lists the selected project first; picking it opens the launcher on it with *Work in checkout* picked, the card not opened; on a workflow with unsaved changes *Run…* refuses, saying to save first |
| A prompt written larger | Open an agent step, *Write larger…*, type a line, *Done*; again, type, *Cancel* | The dialog holds the prompt and what it may use; *Done* puts the line in the step's prompt and the form holds a change; *Cancel* leaves it as it was. On a shipped workflow it reads *Read larger…*, the field disabled, *Close* alone |
| A repair loop on the rows | Open a workflow whose Verify goes back to Implement | Verify's row reads *on failure ↑ N* (Implement's number) without being opened |
| Search kept across pages | Type `pull` in the search, go to *Tasks* and back | The search still reads `pull`, the cards narrowed to those matching, *Showing N of M workflows* under them; `zzz` says *No workflows match* |
| A card's own actions | *Export…* and *Delete…* from a card's ⋯ on a workflow of your own | Neither opens the card; *Delete…* asks, then the card goes and the cards stay on screen |
| Enter opens a card | Tab to a card, press Enter; then click into the search and press Enter | The card's workflow opens in the editor; in the search nothing opens |
| Hover never fills | Point at a card, then at its *Run…* | The card's edge turns the muted ink with no fill; over *Run…* the button takes its own hover |
| The check command on the project page | Open a project with no session, type a command in *Check command* | *Run check* appears beside *New session*; the command is kept after a restart |
| A template | *New issue* in the tab, press *Bug* in the *Template* row | The body holds *Problem*, *Scope*, *Acceptance* and *How to check*, each with its hint; the labels field gains `bug`; the row goes once anything is typed in the body. Save with only *Problem* filled: the issue's facts line says *No scope, acceptance or how to check written*, muted |
| A project's own templates | In a project with `.github/ISSUE_TEMPLATE/story.md` (front matter `name: Story`, `labels: story`, body `## Why` and `## Done when`), *New issue* | The *Template* row offers *Story* alone; pressed, the body is the file's body and the labels field gains `story`. Save with only *Why* filled: the facts line says *No done when written* |
| No advice | An issue whose body is a sentence, or written under other headings | No *No … written* line in the detail |
| A narrow dock, the largest zoom | Drag the Workbench to its narrowest and zoom in to the largest step | The primary action and *Step · step N of M* are drawn whole; the words before the step give way |
| The Issues page | With twenty issues across two projects, pick *Issues* in the rail | Both docks go away and the page heads itself *Issues* beside *N open*, *Refresh* and *New issue* |
| Coming back | Pick an issue, drag the panel's edge, filter by a project, scroll the list, then open a session and pick *Issues* in the rail | The same issue open at the same width, filter, search and scroll |
| Narrow | Narrow the window until the panel and a strip of list no longer fit, and pick an issue | The issue covers the list; `×` or Esc finds the list with its filters, scroll and the issue just read selected. At the largest zoom step a row still shows its reference, title and age |
| Open in Issues | In the tab, ⋯ ▸ *Open in Issues* | The page opens on that issue; with the page filtered to another project and *Closed*, it is pinned on top, *Outside current filters*, the filters unchanged |
| The task detail's approval | Open a waiting task's detail on the Tasks page | *Awaiting approval* draws the answer, *Revise…* and *Continue* each beside what it starts; *Continue* moves the run on, as the strip's does |
| One file for two views | With the tab and the page both on one issue, edit it in one | The other shows the edit at once; the footer says one sync, not two |
| A draft kept | Start a new issue in the tab, type a title, open a session and come back; then pick another issue | The draft is still there; picking another issue asks *Drop this draft?* in a modal; an untouched form goes without asking |
| Queued behind a task | Two checkout tasks on one project, the second started while the first runs | Before it starts, the launcher says, as information, that the place is taken and names the first task; a Retry of a checkout task says the same. Started, the notification names the first task: *… is queued behind <its title>, working in <folder>* |
| Push and pull request | On a GitHub project with CI, a *Work an issue* task through Verify | `git ls-remote origin <branch>` shows the commit Verify passed on, a draft pull request is open on the branch, and once its status checks pass it is out of draft |
| A failing check is repaired | As above, with a check that fails on the change | The run goes back to Implement with the check's name and log in its prompt, pushes again, and waits again |
| No checks at all | As above, on a repository with no CI | The run waits ten minutes, then the pull request leaves draft |
| No forge | A *Work an issue* task on a project with no GitHub `origin` | Push, Pull request and Status checks each pass at once; the branch is the result |

The rows below need the mock with `--settings`, first in the list, and a duplicate of *Work in
checkout* whose agent steps are set in its *Agent settings*; brief `go fast`, *Continue* at the
approval.

| Case | Do | Expect |
|---|---|---|
| A step that sets a model | Implement: under `model`, pick `cheap` | Plan's answer begins `[mode code · model strong · …]`, with no *Settings* note; Implement's session note reads *Settings: mode code · model cheap · effort low · fast off*, and its answer begins `[mode code · model cheap · effort low · fast off]` |
| Going back restores it | The same, brief `fail-check fast`; while Verify's `sleep 5` runs, pick *strong* in the composer | Verify fails and Implement's second visit notes *Settings: … model cheap …* again, its answer beginning `[… model cheap …]` |
| A step naming nothing goes back to the start | Plan: *Mode* `plan`, `model` `cheap`; Implement names nothing | Plan notes *Settings: mode plan · model cheap · effort low · fast off*; Implement notes *Settings: mode code · model strong · effort low · fast off* |
| A value not offered blocks | With a session of the mock opened since onehand started, Implement: *Add option* `model` = `opus` (its row above shows `opus · not offered`, chosen, in the warning ink); *Run…* on its card, then the project | In the danger ink: *Step 3 (Implement): the agent offers no `opus` for `model` (it offers: strong, cheap).*, with where it is changed; *Run* is spent |
| A value gone under another model | Implement: `model` `cheap`, `effort` `high` | Nothing blocks the start; the run fails at Implement, a configuration failure: *the agent offers no `high` for `effort` (it offers: low, max)*, and no prompt went to Implement |
| A pick the agent refuses | Implement: `effort` `max` | The run fails at Implement: *the agent refused `max` for `effort`: Max effort is not available on this plan* |
| Check the agent learns the options | A duplicate of *Implement on a branch* whose Implement names `model` = `opus`; add `"again"` to the mock's `args` (a new spec), then open the launcher on the duplicate | *The agent's settings for this workflow's steps are not known yet …*, muted, with *Check the agent*; pressed, it turns into the block naming `opus` and what `model` offers; no session appears |
| The editor before the agent is known | Restart onehand, open the duplicate's Implement | *What Mock workflow offers is not known yet* with *Check the agent*, *Mode* and the option rows typed; pressed, the button spins, then *Choices Mock workflow offers*, *Mode* `Run's mode · code · plan`, rows `model`, `effort`, `fast`, the typed `model` row gone into the `model` row's choice |
| No agent configured | Remove every agent in Settings ▸ Agents, open an agent step | *No agent is configured, so there is nothing to pick from*; *Mode* and options typed as before |
| A shipped workflow | Open *Work in checkout*'s Plan with the agent known | The choices draw, each refusing with *Built in: duplicate it to change it*; no *Add option* |
| What a visit ran with | After *A step that sets a model*, open the task on the Tasks page | Under each visit, muted: Plan *mode code · model strong · effort low · fast off*, Implement *mode code · model cheap · effort low · fast off*; Approve and Verify say nothing |
| An option set twice | On an agent step, *Add option* twice, both `model` | The step's problem reads *the option model is set twice*, on its row too; *Save* refuses |
