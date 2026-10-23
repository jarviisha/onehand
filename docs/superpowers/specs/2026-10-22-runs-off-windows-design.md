# Design: runs off windows (R2)

- Date: 2026-10-22.
- Source: the architecture audit of the same day (local notes), item A1 and bug B31, re-read
  against `main` at `513f4b2`.
- Contracts that still govern: [CONTEXT.md](../../../CONTEXT.md), [docs/workflows.md](../../workflows.md)
  (*the run engine and its one driver*), [docs/tasks.md](../../tasks.md),
  [docs/unattended.md](../../unattended.md) (its seven rules, rule 1 above all). Every visible
  change follows the `ui` skill.

## Intent

A run is owned by a window today. The driver holds the window's handle. The run's session lives
in that window's chat pane, so closing the window ends the run (`Stop::Closed`). Every start
path needs a `&mut Window` and a `Context<Shell>`, and a claim fails when no window holds the
project. On top of that, the driver's sequencing has no tests: holding calls while a mark is
pinned, the budget clock, take-over detection, the settings picks and the status-check wait
(`crates/app/src/task/driver*.rs`, about 1,200 lines).

What comes next needs runs that do not need a window to *start*. Cron needs runs started by
time, reverse control needs runs started by an agent, and Jira and Slack need runs started by a
connector. R2 moves the driver's decisions into core as a tested state machine, gives every start
one door, and splits the unattended claim so a second trigger can reuse it. A run still lives in
the window it was placed in, and closing that window still stops it (the owner's call).

## Goals

1. **The driver decides in core and acts in the app.** A core `task::drive` machine takes
   events (a turn ended with its facts, a command exited, a mark landed, a forge call landed,
   status checks seen, Stop, the agent up, a foreign prompt, a card parked or answered, the
   clock) and returns effects (pin, measure, read facts, prompt, pick, cancel, run a command,
   forge call, watch checks, note, save, finish). The app maps `ChatEvent`s onto it and performs
   the effects. Every rule the driver applies today is tested without a GUI.
2. **One door for every start.** Launcher, Retry, Resume, a check, a claim and a review answer
   all go through one core `Start` and one app `submit`. None needs a `&mut Window`; a window
   that asked is told, as an optional "say it here".
3. **The unattended claim is three steps:** find → claim → prepare. A later trigger (time, a
   connector) reuses claim + prepare. The window between the search and the claim is checked
   again on the UI thread, which fixes B31 (a Retry pressed meanwhile spending a review
   request).
4. **Runs are found by task, not by session.** `live` is keyed by task id, so a run can later
   own more than one session.

## Non-goals

- Keeping the app alive with no window open, or starting runs at boot by themselves. Both
  belong to R4 (cron). The app still quits with its last window, and a run under way then reads
  as interrupted, as today.
- A run with no session at all, parked while its status checks run (audit A1). The session
  stays open for now, as `forge.rs` already marks with `ponytail:`.
- Several sessions per run, multi-agent views (R6), or the command surface and MCP (R3).
- **A run outliving its window.** Closing the window a run's session is in stops the run as
  *Closed* (resumable), as today.
- Changing what any rule decides: the engine (`workflow::Run`), the queue, the cap, the reports
  and the seven unattended rules stay as they are.

## Anti-goals (a failure even if it works)

- A run that answers a card or an approval for a person because no window is watching
  (unattended rule 1).
- A run that keeps going with nothing on screen saying so: a run with no window holding its
  project must still be visible on the Tasks page of every open window, with Stop.
- A behaviour change hidden inside the refactor. Steps 1–3 move logic without changing what a
  person sees; step 4 changes only the B31 race.
- The driver's logic copied rather than moved, leaving two drivers to disagree. That has
  happened once already, and it is what the "one driver" rule in workflows.md is about.

## Constraints (from the repository)

- Core stays GUI-free and runtime-free: the machine is synchronous (input → effects), with no
  timers or tasks of its own.
- A `ChatSession` is an app entity created without a window (`ChatSession::spawn`); a window's
  `ChatPane` holds it in `conversations`. That is the seam a later run-owned session would use;
  R2 does not use it.
- `Tasks` (`crates/app/src/task.rs`) is a GPUI global, and every caller reaches the driver
  through it. `task.rs` (888 lines of code), `unattended.rs` and `core/unattended.rs` are
  already past the 800-line rule, so each step that touches one splits it along a seam.
- Shell work from outside a window goes through `handle.update(…)` with the defer rule; the
  audit counted about a dozen hand-written walks over `Shared.windows`. A single
  `with_shell` / `windows_holding(root)` helper is in scope where R2 touches them. R3 owns the
  rest.

## Decisions

### 1. `onehand_core::task::drive`

A struct `Drive` holding what `Driven` holds today minus the GPUI handles: the task id, the
`Run`, the pinned count, held calls, the sent and answer-from counts, a pending prompt, up and
settled flags, the picking state, the mode out, the step noted, the `Budget`, the time spent
before, the card flag, a command running and a stop waiting. Its API:

```rust
pub enum Input { … }   // one variant per event listed in goal 1, carrying what was read
pub enum Effect { … }  // one variant per effect listed in goal 1
impl Drive {
    pub fn start(task: String, run: Run, first: Action, pinned: usize, now: Instant) -> (Self, Vec<Effect>);
    pub fn on(&mut self, input: Input, now: Instant) -> Vec<Effect>;
}
```

- Inputs carry snapshots, not handles. For example "agent up" carries the modes offered; "picks
  answered" carries the chat's offer, in-force and held settings, and whether picks are still
  waiting. The machine never reads a session.
- **Held calls stop being boxed closures.** They are queued `Input`s, replayed in order once the
  pin lands, with an ending winning over the action, as today. This is the main structural
  change. The rule stays as written in `driver.rs`.
- The clock becomes an `Effect::ArmClock(Duration)` and an `Input::ClockFired`. The app owns the
  timer.
- The status-check loop stays in the app (it is I/O on a timer). It feeds `Input::ChecksSeen`.

### 2. The app adapter

`crates/app/src/task/driver.rs` shrinks to:
- a `ChatEvent` → `Input` mapping;
- an `Effect` → action executor (spawn a pin, submit a prompt, run a command, call the forge,
  write a note, save);
- the `Subscription`s and the session handle.

`drive_check` stops re-building the pin → run → cancel → stopped path, and runs through the
same machine, with no session.

### 3. One start

A core `task::Start { template, brief, setup, origin }` builds the task and its first run for
every start kind. The app's `task::submit(start, said_in: Option<AnyWindowHandle>, cx: &mut App)`
asks the queue and drives when the place is free. `request`'s `&mut Window`/`Context<Shell>` go.
Where a queued task starts is the first window holding its project when its place frees (today
it is the window it was asked in, `Tasks.asked_in`; that window is preferred while it is open),
and `said_in` says where its toasts go.

### 4. find → claim → prepare

`core/unattended/claim.rs::begin_blocking` splits into:
- `find_blocking` (search + preflight, read-only);
- `claim_blocking` (label + comment);
- `prepare_blocking` (worktree cut or review fast-forward; already separate).

The app runs find in the background, returns to the UI thread to confirm the task found is
still free (B31: a Retry pressed meanwhile), then claims and prepares in the background.

## Steps (one PR each, in order)

| Step | What | Behaviour change |
|---|---|---|
| 1 | `task::drive` in core, with tests for every rule the driver applies; the app's driver becomes its adapter | none |
| 2 | `live` keyed by task id; `drive_check` on the same machine; split `task.rs` | none |
| 3 | One `Start` + `submit`; `asked_in` and the window parameters go | none (toasts land where they did) |
| 4 | find → claim → prepare, with the UI-thread recheck between | B31 fixed |

Each step updates the document that owns what it changes: workflows.md (the driver), tasks.md,
unattended.md, and CHANGELOG.md for step 4.

## Testing

- **Step 1:** the machine's tests. Each comment in today's `driver.rs` that states a rule gets
  a test:
  - an ending held while a pin lands wins over the action;
  - a turn ending while a pin is held is not judged;
  - the budget does not run while a card or an approval waits;
  - a foreign prompt is a take-over;
  - a refused mode fails the run on its mode, not a step's;
  - a Stop while a command runs ends the run only once the command exits;
  - the first stop reason is the one kept.

  Plus today's app tests, unchanged.
- **Steps 2–4:** the existing tests plus one each, for the check path on the machine, a `Start`
  per kind, and the recheck refusing a claim of a task that became busy.

## Left to the builder

- The exact `Input` and `Effect` variants, and how the picking state is split between machine
  and adapter.
- Where `windows_holding(root)` lives.

## Answered by the owner

1. **A run does not outlive its window.** Closing the window its session is in stops it as
   *Closed* (resumable), as today. R2 is steps 1–4.
