# Proposal: agent settings per step

- Date: 2026-10-21.
- Status: proposal. Nothing here is a requirement until a task asks for it; once built, it moves
  into [workflows.md](../workflows.md) (and [CONTEXT.md](../../CONTEXT.md) for the new term) and
  this file leaves `docs/proposals/` in the same PR.

## Why

A run carries every step on one session, and the only thing a workflow can say about how the
agent works is nothing: the run's mode (`Setup::mode`, from `[unattended] mode`) is set once,
before the first prompt, and holds for every step. A person wants each step to say how the agent
works on it: the plan in a strong model at high effort in a planning mode, the implementation in a
cheaper model with fast on. The agent already publishes all of this (its modes, and its config
options: model, effort, fast, whatever else it names), and the composer already sets it; a
workflow cannot.

Later, a step should be able to name a different agent altogether. That is a separate project (it
means a session per step, and a step losing the conversation before it); this one does not build
it, and nothing here needs reserving for it: `store::parse` refuses a key it does not read, so a
future `agent` key cannot be confused with anything an older file says.

## Goals

- An agent step can name a mode and any of the agent's config options, by the adapter's own ids.
- Each visit to that step runs with them, including a visit after `on_fail` sends the run back, a
  revision, a Resume and a Retry.
- A value the agent does not offer **blocks**: before the run starts when what the agent offers
  is known, and fails the run at that step when it is found only once running. Never a silent run
  on another model.
- What a step visit actually ran with is recorded, so a finished task says which model planned.

## Non-goals

- A different agent per step (the later project).
- Overriding a workflow's step settings when starting a run, or per issue.
- Step settings in the shipped workflows: model ids differ between agents, and the shipped ones
  must run on any agent.

## Anti-goals

- A run that ends looking right while a step ran on a model the workflow did not name.
- An older build writing over a file that holds the new keys (already prevented: it lists the file
  as unreadable and refuses a save aimed at it).

## The file

On an agent step only:

```toml
[[steps]]
id = "plan"
label = "Plan"
kind = "agent"
mode = "plan"                                            # the adapter's mode id
options = { model = "opus", effort = "high", fast = "off" } # config option id = choice value
gates = ["answered", "code_unchanged"]
keep_answer = true
prompt = "…"
```

- Both are optional. `options` keys are `ConfigOption::id`, values are `ConfigChoice::value`,
  exactly as the agent sends them (not the names the composer shows).
- `validate.rs` refuses: either key on a step that is not an agent step; an empty mode, option id
  or value; an option id `mode` (modes are their own field; the parser already drops the `mode`
  config group).
- No `SCHEMA_VERSION` bump (my call): the unknown-key refusal already keeps an older build off
  these files. The builder may bump it if the "written by a newer onehand" message is wanted.

## In a run

The rule lives in core as pure code (the planner below), because the driver, preflight and tests
share it.

- **The run's starting settings**: when the agent first comes up in the run, the run's mode
  (`Setup::mode`) is set as today; that mode (or, when unset, the one the agent came up in) and
  every config option's value as the agent came up are kept on the run, persisted with it, so a
  Resume on a new session finds the same ones.
- **At the start of every visit to an agent step, before its first prompt**, the step's mode and
  each named option are checked against what the session offers now (`chat.modes`,
  `chat.config_options`). One not offered fails the run at that step, like a mode not offered
  today (`found_late`, a configuration failure). Otherwise each value not already in force is
  sent (`Chat::set_mode`, `Chat::set_config_option`) and a note is left in the session, as
  *Mode … set* is today.
- **The prompt waits for the agent's answer to every pick.** `set_mode` and `set_config_option`
  are optimistic in the composer, and an agent can refuse a pick (the mock refuses *Max* effort),
  so the driver holds the step's first prompt until each pick it sent is answered; a refused pick
  fails the run at that step, a configuration failure naming the setting and the agent's reason.
  What a visit records is what was confirmed, never only what was sent.
- **Picks go in order, model first** (mode, then `model`, then the rest by id), and each later
  setting is checked against the offer as it stands after the ones before it were answered: an
  agent may change which effort values exist, or whether fast exists, when the model changes
  (`config_option_update`). So the core rule is a step-by-step planner (given what is in force
  and offered now, the next pick or a refusal or done), driven one round at a time, not one
  check-and-send.
- Resume opens a new visit (`Run::resume`) on a new session, so the same visit-start rule applies:
  the open step's settings are set again before its first prompt there, whatever the reconnect's
  `reapply_prefs` restored, and the new visit records them.
- **A setting a step does not name goes back to the run's starting value**, never what the step
  before it left (decided with the person): a *Plan* step in a planning mode must not leave
  *Implement* unable to edit. So every visit sets every setting, the step's own or the starting
  one, and a step's behaviour does not depend on which step ran before it, `on_fail` and Resume
  included. A starting value the agent no longer offers fails the run at that step, like any
  other setting not offered.
- A person's pick in the composer during a run holds until the next visit starts.
- Settings come from the run's snapshot of the workflow, like everything else a step says, so
  Retry keeps them and *Retry with current settings* takes the workflow's current ones.
- Watch for model aliases: an adapter may offer `opus` while reporting `opus[1m]` in force
  (`Chat::reapply_prefs` skips the model for this reason). Compare against the offered choice
  values only; whether "already in force" should treat an alias as a match is for the builder to
  settle against the Claude adapter.

## Before it starts

- What an agent offers is learned per spec today for modes only (`Shared::modes_seen`, fed by a
  session coming up and by *Check the agent*). It grows to hold the config options too (each id
  with its choice values), learned at the same two moments.
- Preflight checks every agent step's settings against that, for the run's agent. Because one
  option's choices may depend on the model, what was learned is only certain for the mode, the
  model and an option id the agent never offered; for any other option on a step that also sets a
  model, a value missing from what was learned is the non-blocking *not known yet*, and the run's
  own check at that step stays the authority (my call). Otherwise not offered is a
  blocking finding naming the step, the setting, the value and what the agent offers; not known
  yet is the same non-blocking finding with *Check the agent* that a mode gets today. The run's
  own `Setup::mode` check stays as it is. Findings are bounded like every list: one per step and
  setting, capped, with the cap said on screen when it bites.
- The Retry dialog's list of what a run keeps says when a step's setting is no longer offered,
  as it does for the mode.

## What is recorded

A `Visit` gains what was in force when its first prompt went: the mode and every config option's
current value (all of them, not only those the step named). Optional and skipped when empty in the task file, so old task
files read. The task detail shows it on the visit in muted text (exact placement by the `ui`
skill).

## The editor

An agent step's form on the Workflows page gets the two settings: a mode field and a list of
option rows (id, value; add, remove). A workflow is not tied to an agent, so the fields are free
text, with suggestions from what any configured agent has been seen to offer in this process,
each labelled by agent (my call). The editor does not judge a value against an agent; preflight
does. Follows the `ui` skill and its checklist; a read-only shipped workflow shows the fields
disabled.

## Testing

- Core unit tests: validation, the pick/refusal function, preflight findings, `Visit` serde with
  and without the new field.
- `mock_workflow_agent.js` offers modes and config options (model, effort, fast), behind an
  argument so the existing *mode = ""* rows in workflows.md keep working, and answers
  `set_mode`/`set_config_option`; it says the settings in force in its answer so a by-hand row can
  see them.
- New by-hand rows in workflows.md: a step that sets a model; going back via `on_fail` restores
  it; a value not offered blocks before start; one that stops being offered fails at its step.

## Left to the builder

Field and type names, where the pure function lives in `workflow/`, the note's wording, the
preflight cap's size, the editor's layout within the `ui` skill, how the mock is switched.
