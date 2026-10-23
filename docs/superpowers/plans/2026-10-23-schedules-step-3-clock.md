# R4 step 3: the clock, scheduled tasks, and where a task came from

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Schedules in `schedules.toml` fire while onehand runs: each due time starts a task,
off screen, with the schedule's agent, mode, settings, tool set and issue labels; missed and
overlapping times go as each schedule says; a scheduled task's end is a desktop notification; every
task says where it came from.

**Spec:** `docs/superpowers/specs/2026-10-23-schedules-design.md` (Goals 2–3, Decisions 2–6, *A
task says where it came from*). Steps 1–2 merged (#214, #215).

## Global Constraints

- A start goes through what exists: preflight (`Shell::run_preflight`), worktree cut,
  `start_workflow_told`, `task::submit_told`, off-screen drive. A prompt start is a task of a
  one-agent-step `Checkout` workflow whose step prompt is `{brief}` and whose brief body is the
  schedule's prompt (a user's `{…}` would fail the workflow's validation).
- The schedule's agent, mode, settings, tools and labels are copied onto the task; Retry and
  Resume use the copy. A step's own mode or option wins over the schedule's.
- The clock: one look when the first window is up, then every minute. The file is read each look
  (so a hand edit holds), decided on the UI thread, written back off it.
- A schedule whose project no window holds waits. A start that fails says why on the schedule and
  notifies once per new reason; it tries again at the next due time.
- Core stays GUI-free; no file I/O on the UI thread.

## Review Focus

1. A schedule fired while its last task is still waiting on an approval → busy, skipped or queued
   as set (busy includes waiting on a person).
2. A Resume of a scheduled task after onehand restarted → its session gets the copied tool set.
3. A queued time whose last task is then stopped → the queued start goes at the next look.
4. Two windows holding the project → one start (the first window holding it).
5. A schedule naming a workflow id no longer in the library → said on the schedule, notified once.

---

### Task 1: Core

- `task::Origin { Schedule { id: String, name: String }, Tool }` and on `Task`:
  `#[serde(default)] origin: Option<Origin>`, `#[serde(default)] grant: Grant` with
  `pub struct Grant { pub tools: Vec<String>, pub labels: Vec<String> }` (Default).
- `schedule::prompt_template(name: &str) -> Template` — `Template::blank(name)`, one step
  `StepSpec { id: "prompt", label: "Prompt", kind: Agent { prompt: "{brief}", gates: [],
  keep_answer: false, mode: None, options: {} } }`; `validate` finds nothing.
- `schedule::fill(template: Template, mode: Option<&str>, options: &BTreeMap<String, String>) -> Template`
  — each agent step with no mode takes `mode`; each option the step does not set takes the
  schedule's.
- `schedule::brief(s: &Schedule) -> Brief` — prompt: title = name, body = prompt; workflow:
  title, body.
- Tests: `the_prompt_template_validates`, `fill_keeps_a_steps_own_settings`,
  `old_task_files_load_with_no_origin`.

### Task 2: App — starting with an origin and a grant

- `Shell::run_preflight(.., agent: Option<&str>, ..)`: the agent judged is the one named, else the
  first.
- `Shell::start_workflow_told(setup, template, brief, start: Started, window, cx)` with
  `pub(crate) struct Started { origin: Origin, grant: Grant, agent: Option<String> }`: the task
  takes them (agent after `new_task`'s default).
- `control::start` (made `pub(crate)`, taking `Started`) — R3's tools pass
  `Origin::Tool`; the clock passes the schedule's.
- `drive_task`: once the session is up, `control::grant(uid, &task.grant)`.
- `control::grant(uid, &Grant)`: `Sessions.only` (new) and `labels`; `standing` for a session
  not switched on → `Only(set)` when it has one, else `Off`. `forget` clears it.
- `place::let_go`: a task whose origin is a schedule → `notify_scheduled(name, said)` (a
  `pub(crate)` wrapper beside `notify_turn_ended`).
- `listing::rows` name: `Schedule { name }` → `"<name> · <workflow>"`; `Tool` →
  `"by a tool · <workflow>"`.

### Task 3: App — the clock

- `crates/app/src/schedules.rs`: global `Clock { opened_at: u64, last_task: HashMap<String, String>,
  queued: HashSet<String>, told: HashMap<String, String>, _tick: Task<()> }`.
- `boot(cx)` after the first window; `look(cx)`: load (background) → per schedule: the window
  holding `project` (first), `busy = last_task` is working, `decide(s, now, opened_at, busy,
  &Local)`; a queued one not busy → start; `Start` → template (`prompt_template`, or the library
  entry by id, else said), `fill`, setup (agent, mode, check), `control::start` → `Placed`'s task
  id into `last_task`; `Skip(why)` → `said`; `Queue` → `queued`; anything but `Wait` →
  `last_fired = now`; changed → save (background).
- A failure: `said = why`; notify when `told[id]` differs.

### Task 4: Documents, checks, review, PR

- `docs/schedules.md` (new: what a schedule is, the file, the clock, missed/overlap, the task's
  origin and grant, notifications); `docs/tasks.md` (the origin on a row); `docs/control.md` (a
  schedule's session stands with its set); CONTEXT.md **Schedule**; CHANGELOG; CLAUDE.md routes
  `docs/schedules.md`.
- `make fmt`, clippy `-D warnings`, `cargo test --workspace --locked`; `/drive` a schedule a
  minute ahead if the owner asks; Opus review; PR; CI; ask before merging.
