# Pipelines

A pipeline run takes a brief through the steps of a pipeline template: an agent step prompts the
session and is judged by gates onehand checks itself, a command step runs a command in the work, and
an approval step waits for a person. The words are defined in [CONTEXT.md](../CONTEXT.md).

## Where the code is

| Part | Where |
|---|---|
| Template types and serde | [crates/core/src/pipeline/template.rs](../crates/core/src/pipeline/template.rs) |
| What stops a template being saved or run | [crates/core/src/pipeline/validate.rs](../crates/core/src/pipeline/validate.rs) |
| Prompt variables, onehand's additions, carry-on prompts | [crates/core/src/pipeline/prompt.rs](../crates/core/src/pipeline/prompt.rs) |
| The two shipped templates | [crates/core/src/pipeline/builtin/](../crates/core/src/pipeline/builtin/) |
| The person's templates on disk | [crates/core/src/pipeline/store.rs](../crates/core/src/pipeline/store.rs) |
| The engine: one run as pure state | [crates/core/src/pipeline/run.rs](../crates/core/src/pipeline/run.rs) |
| Marks, facts, gates, the command runner | [crates/core/src/pipeline/facts.rs](../crates/core/src/pipeline/facts.rs) |
| Run files and their ordered writer | [crates/core/src/pipeline/files.rs](../crates/core/src/pipeline/files.rs) |
| The one driver | [crates/app/src/pipeline/driver.rs](../crates/app/src/pipeline/driver.rs) |
| Templates on offer, unfinished runs | [crates/app/src/pipeline.rs](../crates/app/src/pipeline.rs) |
| Launcher and resume | [crates/app/src/shell/pipelines.rs](../crates/app/src/shell/pipelines.rs) |
| Settings ▸ Pipelines | [crates/app/src/settings/pipelines.rs](../crates/app/src/settings/pipelines.rs) |

## Templates

A template is a TOML file:

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
`pipeline/<title>` branch in a worktree beside the project, committed). The person's own are in
`<config_dir>/onehand/pipelines/<slug>.toml`, made by duplicating a shipped one or from *New
template* in Settings ▸ Pipelines.

**A file this build cannot read is never written over.** A `schema_version` above
`pipeline::SCHEMA_VERSION` was written by a newer onehand; it is listed as unreadable, and a save
aimed at it is refused. Every read-check-write of a template goes through one lock.

### Validation

`pipeline::validate` is run on every keystroke of the form, before Save, and before a run starts.
It refuses: no name, no steps, an unreadable timeout, an unsupported schema; a step id that is not
lowercase letters, digits, `-` and `_`, or is used twice; an empty label or prompt; a prompt
variable that is not one; `{output.<id>}` naming anything but an earlier agent step that keeps its
answer; `on_fail` naming anything but an earlier agent step; an approval of anything but an earlier
agent step that keeps its answer; a gate that cannot hold where the template works (`committed` in
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

`PipelineRun` is the only place a transition is decided. The driver reports what happened —
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

One global, `crate::pipeline::Pipelines`, holds every run by its session's uid. The driver
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

A prompt asked for before the adapter is up waits for the link. The clock is
`unattended::Budget`: it pauses while a card or an approval waits on a person, and its timer looks
again when it fires rather than being re-armed at every pause.

**The run's file is written after every action**, through `pipeline::files::Writer`, one thread
carrying out saves and removals in the order sent, so a late save can never bring back a run whose
file was removed. A run that ends on its own outcome — done, exhausted, failed, stopped by a person,
taken over, timed out — removes its file. **A run whose agent stopped or whose session went keeps
it** and is put on the unfinished list: neither is the run's own outcome, and app shutdown can look
like either. Quitting waits, briefly, for the writes still queued (`Writer::flush`), so a run's
last save or its file's removal is not lost when the process exits with its last window.

**A turn's answer is what the agent said after the run's own prompt** (`Chat::prose_since`,
counted from where the transcript stood when the prompt went), so a turn that said nothing answers
nothing rather than passing an `answered` gate with the turn before it. The work a checkout holds
uncommitted is fingerprinted with FNV-1a, never std's hasher, because the fingerprint is kept in
the run's file and compared after a restart, possibly by a build made with another Rust.

## Starting and resuming

The composer's `+` menu and the keymap's `run_pipeline` (no default key) open the launcher on the
project on screen. A checkout template starts a session there; a worktree template first cuts
`pipeline/<title>` (or the first free `-N`) off `HEAD` beside the project and adds it as a project.
A template with a command step that names no command needs the project's check command, set under
Settings ▸ Pipelines and kept in the workspace file (`WorkspaceConfig::checks`).

At boot every `<config_dir>/onehand/pipeline-runs/*.json` is read into the unfinished list. A
project page shows the unfinished runs started from it or working in it, with *Resume* and
*Discard*. Nothing restarts an agent by itself. *Resume* adds the run's folder back as a project if
it left the workspace, starts a session on the agent the run used, and calls `resume`.
