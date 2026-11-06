# An application layer: how onehand works, apart from how it is drawn

- Status: proposal, not built. The standard below is a **draft to be tried**, not a rule.
  - Phase 1 builds one complete session slice against it.
  - Phase 2 settles the standard from what the trial showed, and only then moves it into the
    owning documents. Until phase 2, new code follows the existing rules.
- Would supersede, once phase 2 settles it: the rule agreed in #133 ("logic belongs in core when
  two call sites share it, when a second front end could need it, or when it wants a test without
  a GUI"). That rule moved *rules* to core; this one would move *what happens next* there too.
- Contracts it touches: AGENTS.md (*Invariants*, *GPUI model*), docs/rules-and-gotchas.md,
  CONTEXT.md (new terms), and the feature documents whose flows move (tasks.md, unattended.md,
  schedules.md, control.md, hosts.md).

## The problem

onehand already keeps its **model** GUI-free: what a transcript holds, how an ACP event changes
it, when a run's step is over. What it does not keep GUI-free is **how the application behaves**:
what sending a prompt does, what is saved when a turn ends, what a closed stream means, which step
is woken next. That sequencing lives in GPUI entities and their subscribers, so:

- **Replacing GPUI means re-deriving behaviour, not only redrawing it.** A new front end would
  have to reproduce, from reading entity code, the order of send, queue, cancel, save and
  announce.
- **Behaviour is untested where it lives.** An entity's subscriber cannot run without a window.
- **Each entry point carries its own copy.** The composer, the remote bridge, onehand's tools and
  a run each send a prompt their own way (see *Evidence*), and they have drifted.
- **A wrong behaviour has no obvious home.** The same decision is split across a session entity,
  the chat pane's subscriber and core.

The task run already shows the answer. `onehand_core::task::drive::Drive` decides everything a run
does, answering each `Input` with `Effect`s; `crates/app/src/task/driver.rs` only carries them out
and reports back. It has no timer, thread or I/O, and its rules are held by tests without a window.
This proposal makes that shape the standard for **all** application behaviour.

### Evidence, as of `6f92ab49`

- **Sending a prompt has at least three versions.**
  - The composer (`crates/app/src/chat/pane/sessions.rs`, `submit_from`): an offloaded
    project's code first (`chat/pane/offload_send.rs`, `code_first`), then submit, else queue.
  - The remote bridge (`chat/pane/bridge.rs`, `remote_prompt`): `submit_blocker`, then sent,
    queued or refused, with its own "a prompt already waits" refusal. No code-first step.
  - Workers, runs and issue work (`control/workers.rs`, `control/workers/opening.rs`,
    `control/workers/turns.rs`, `task/driver.rs`, `shell/issue_work.rs`): `session.submit` alone.
    The workers' "never queued" is deliberate (`onehand_core::control::turns`).
  - What each origin does about an offloaded project is decided in a different place. The
    composer sends the code first at the moment of sending. Tools, runs, issue work and the
    Telegram bridge are kept off offloaded projects earlier, where projects are listed or picked
    (`ProjectRoot::runs_here`). The current documents say they do not reach an offloaded
    project at all. The remote bridge's path is not covered by those filters (D8).
    Either way, the rule is spread over several places.
- **Saving is split.** The session entity's pump (`chat/session.rs`, `spawn`) owns the quiet save
  after a silence. The chat pane's subscriber (`chat/pane/sessions.rs`) decides that a turn or a
  cycle ending writes the transcript (`archive_detached`), checks an offloaded project's code
  and refreshes the archive list.
- **A stream ending is decided in the pane**: a fork that never opened, a host out of reach, or a
  lost agent (`chat/pane/sessions.rs`, the `ChatEvent::Disconnected` arm).
- **The model carries sequencing too.** `Chat::apply` sends the queued prompt when a turn ends
  (`chat/model/apply.rs` → `flush_queued`). That is "what to do next", not "what is true".
- **The pump decides which signals a batch of events produces**, and in what order: a card is
  announced only after the events that came with it (`chat/session.rs`).

## The layers

```
┌ UI ── crates/app (drawing), crates/ui, the drawing half of each Workbench mode ─────┐
│ Views, layout, focus, scrolling, animation, widgets. View state only.               │
│ Reads state; sends commands. Decides nothing about behaviour.                       │
└──────────┬───────────────────────────────────────────────────────────────────────────┘
           │ commands · reads · what is on screen
┌──────────▼ Drivers ── one per front end; GPUI's live in crates/app ─────────────────┐
│ Own the drives' place in the front end, pump streams into them, carry out effects,  │
│ report results back as inputs, defer what touches another entity. No behaviour ifs. │
└──────────┬──────────────────────────────────────────────┬────────────────────────────┘
           │ commands / inputs → effects                   │ calls
┌──────────▼ Drives ── crates/core, beside their model ┐  ┌▼ Executors ── GUI-free ─────┐
│ SessionDrive, task::Drive, schedules, unattended,    │  │ ACP runtime, disk writes,   │
│ offload, hosts, remote, control, settings, workspace │  │ git, host connection, forge,│
│ No I/O, no timer, no thread, no runtime.             │  │ remote channel, socket,     │
└──────────┬───────────────────────────────────────────┘  │ clock                       │
           │ reads and changes                             └┬────────────────────────────┘
┌──────────▼ Model and rules ── crates/core ────────────────▼────────────────────────────┐
│ Chat, transcript, Run, Task, Workspace, Config, Issues, GitStatus …                   │
│ Blocking I/O primitives and thin async wrappers: acp::connect, store, worktree, journal│
└───────────────────────────────────────────────────────────────────────────────────────┘
```

| Layer | Knows | Must not know |
|---|---|---|
| Model | itself | anything above it, a runtime, the clock |
| Drive | the model | I/O, threads, timers, a runtime, GPUI. Every outside act is an `Effect` |
| Executor | core's I/O primitives, tokio | GPUI; any behaviour decision |
| Driver | drives, executors, its front end | behaviour. An `if` about behaviour here is a misplaced rule |
| UI | everything below, read-only | the order of anything |

**Swap test.** A new front end rewrites the UI and its drivers. Drives, model, executors and every
test of them stay. The drivers are small and mechanical, because they hold no decisions.

## The standard

A **draft**. Phase 1 tries it on one session slice. Phase 2 keeps, changes or drops each rule,
and only then do the rules bind new code. Rules that depend on an open decision say so.

### S1. Every change to application state goes through a drive. No exceptions.

Application state is what onehand owns about its work. A change to it is made by calling a drive.
It holds even when the drive's method only forwards to the model and answers no effects. An
exception list is a judgement every contributor makes differently; a single rule is checked by the
compiler (S8). A feature that is pure today and gains I/O tomorrow changes inside its drive, and
its callers do not move.

### S2. Reads go to the model directly.

A drive hands out read access to its model (`session.chat() -> &Chat`). It does not re-export
getters. A getter that only forwards buys nothing, and hundreds of them would bury the drive.

### S3. View state stays in the UI. State is classified by what it means, not by whether it is kept.

**Application state** is a fact about the work: what onehand does, what an agent receives, how
something ends, what any entry point reads as true. **View state** is how one viewer looks at that
work: changing it never changes what onehand does or says anywhere else.

The questions, in order:

1. **Does changing it change what onehand does?** A prompt sent, a step woken, a file written, an
   outcome, something told to a person. Yes means application state.
2. **Must two front ends showing the same session at once agree on it?** Yes means application
   state. If each may differ, as two windows may scroll one transcript differently, it is view
   state.
3. **Who decides its value: a rule, or the viewer's act of looking?** A value set by a rule is
   application state. A value set by looking (focus, what is on screen, what is open) is view
   state.

**Persistence is a separate axis.**
- View state can be kept across restarts: scroll positions, open sections, panel sizes. The front
  end keeps it in a store of its own, written through an executor. No drive reads it.
- Application state can be memory only: a turn running, a prompt queued, a card waiting.

**A view fact that behaviour depends on enters as an input.** The UI keeps owning it, and the drive
is told: "this session is on screen" is `Input::Shown(bool)`, because whether a finished turn is
announced depends on it, and the viewer's looking decides it. The drive never stores the view
state itself, only what it was told.

| Application state: through a drive | View state: in the UI, kept or not |
|---|---|
| the queued prompt; a prompt held while an offloaded project's code goes up; a card and its answer | which sections, turns, tools and thoughts are open |
| a setting; a session's name; a run's step | scroll positions; focus handles; panel sizes and zoom |
| a session marked unseen (a telling rule sets it; being shown is the input that clears it) | parsed markdown, decoded images |
| the agent's mode and config picks | the active session in a window (told to drives as `Shown`) |

Borderline cases are settled by question 1, not by where the state lives today.
- A composer's unsent text is view state.
- The same text, once handed to `SessionDrive::prompt` and held behind the code going up, is
  application state. The drive holds it from then on, and the composer only shows it.

`Chat` already keeps some view state (`toggle_tool`, `toggle_ask`, `toggle_thought`,
`toggle_tool_output`). Decision D3 settles whether it moves out or is named as the one allowed
kind.

### S4. The shape of a drive.

```rust
// Commands, from the UI, the remote bridge, onehand's tools or another drive: methods.
fn prompt(&mut self, text: &str, staged: &[StagedAttachment], from: Origin, now: Instant)
    -> (Intake, Effects);
fn answer_ask(&mut self, idx: usize, custom: bool) -> Effects;   // pure: answers no effects

// What the outside world reports: one `Input` enum.
fn handle(&mut self, input: Input, now: Instant) -> Effects;
fn feed(&mut self, batch: impl Iterator<Item = AcpEvent>, now: Instant) -> Effects;
```

- **Commands are methods.** A command reads as ordinary code, returns what the caller needs at
  once (`Intake::Sent | Queued | Refused(why) | CodeFirst`), and returns effects for the rest.
- **Reports are an `Input`.** Effect results, streams, timers and events from other drives come
  back through `handle`. A test can replay a recorded `Input` sequence and get the same effects.
- **`Effects` is ordered.** It is carried out in the order given. "Do A, then B once A answers"
  is A now and B on A's `Input`, never two effects hoping for an order.
- **Time is an input.** `now` is passed in; a wait is `Effect::Arm(Duration)` answered by
  `Input::Elapsed`. No drive reads the clock.

`task::Drive` takes its commands as `Input` today (`Stop`, `Approved`, `Revised`). Decision D2
settles whether it converts to methods.

### S5. A late answer is matched by token, and what it means is declared per effect.

An effect that needs an answer (`Save`, `SendCode`, `Pin`, `RunCommand`) carries a token minted
by the drive: a per-drive counter, or the thing it acts on, such as a turn number or a run id. The
answer `Input` carries the token back.

The token only says that the drive has moved on since the effect was asked for. It does not say
the act did not happen. A save may have written, code may be on the host, and a command may still
be running and still holding its place. So two things are kept apart:

- **The drive's state is not updated as if the answer were current.** Every late answer gets this.
- **What the act left in the world is still handled.** What that takes depends on the effect, so
  each answered effect declares one of:

| Late answer | Meaning | Example |
|---|---|---|
| **Drop** | the act left nothing behind, or reading it again is harmless | `Measure`, `ReadFacts`: a read for a step that has moved on |
| **Record** | the act happened and must be known, though it no longer moves the flow | a `Save` that landed for a session since closed: it is on disk, so nothing writes it again; code that reached the host after its prompt was changed: the host holds that code now |
| **Clean up** | the act left a resource that is no longer anyone's | a temporary file, a started process, an attach nobody waits on |
| **Await** | the act is still going, and something stays held until it ends | a command called off: its place is freed only on `CommandExited`, as `task::Drive` does today (`stopping`) |

Calling an act off is its own effect (`CallOffCommand`), and it never stands in for its answer: a
drive that asked for a stop still waits for the answer that says the act is over.

`task::Drive` handles these cases one by one today (`held`, `waiting`, `stopping`). The standard
asks each answered effect to declare its row, and to have a test of its late answer (S11).

### S6. A failed effect is an input, never a panic and never a silent drop.

Every answered effect reports `Result<T, String>`, or a typed failure where the drive branches on
the kind. What the person is told is decided by the drive, as an effect (`Effect::Tell`), so a
failed save reads the same from every front end.

### S7. One driver shape.

A driver does five things and nothing else:

1. Owns the drive, or a handle to it (D4).
2. Pumps outside streams into `feed` or `handle`.
3. Carries out effects in order on executors.
4. Reports each result back through `handle`, with its token.
5. Defers what would update another entity, or an entity already being updated.

Effects every drive shares (`Tell`, `Save`, `Arm`, `Changed`) are carried out by one shared
function per front end, not one copy per driver.

Holding no behaviour does not make a driver correct. It can still:
- run effects out of order, or run one twice;
- lose a failure instead of reporting it;
- drop a token;
- keep an adapter, a timer or a task alive after its session closed.

S11 tests those.

### S8. Enforced by visibility first, by guards second.

- Model methods that change application state become `pub(crate)` in core, so only drives in the
  same crate call them. Core exports drives and the model's read side. A UI call to `chat.submit`
  then does not compile, and `dead_code` finds a mutator no drive uses. This is the existing rule
  "nothing in core is `pub` unless something outside the crate names it", applied to writes.
- What rustc cannot catch goes into `crates/app/src/guards.rs`:
  - a drive's file names `std::fs`, `std::thread`, `std::time::Instant::now`, `tokio` or
    `smol`;
  - a driver matches an `Effect` or `Input` with a wildcard arm.

  Guards scan for source patterns. They do not prove ordering, exactly-once execution or
  cleanup; S11 does that.

### S9. A drive sits beside its model. There is no `application/` folder.

`task::Drive` is `core/src/task/drive.rs`; a session's drive is `core/src/chat/drive.rs`. The repo
is cut by feature, and a layer folder would split every feature in two. What makes the layer real
is the term, the shape (S4) and the enforcement (S8).

### S10. Drives talk through effects.

A drive never holds another drive. "The run sends its session a prompt" is `task::Drive` answering
`Effect::Submit`, carried out as a command on `SessionDrive` with `Origin::Run`. The session's
"turn ended" reaches the run as an `Input`. That routing is itself behaviour, so it is a GUI-free
function tested against real drives (D4). It is not a match arm in a GPUI subscriber.

### S11. Every driver keeps a contract, and is tested against it.

These tests use a fake executor, and GPUI's test context where the driver is GPUI's. Each driver
has a few, aimed at its execution contract:

- effects run in the order given, each exactly once;
- every failure reaches the drive as an `Input`, and none is swallowed;
- every answer carries the token of the effect it answers;
- a late answer gets the handling its effect declares (S5): drop, record, clean up or await;
- after a session closes, no adapter, timer or task of it stays alive, and no effect runs for it;
- deferred work runs after the update that asked for it, never inside it.

## Terms

Proposed for CONTEXT.md once phase 2 settles them, in a new section on how onehand is built:

- **Drive**: the GUI-free unit that decides what happens next for one thing, a session, a run or a
  schedule, and answers with effects for its driver to carry out. _Avoid_: controller, service,
  manager, orchestrator, use case.
- **Driver**: what carries out a drive's effects in one front end and reports back. _Avoid_:
  adapter (an agent's), glue, bridge (the remote bridge's).
- **Effect**: one thing a drive asks for: a write, a send, a wait, something said.
- **Executor**: GUI-free code that performs an effect's I/O. _Avoid_: runtime (tokio's), worker
  (a session's).

*Drive* is the code's own word already (`task::drive`: "decided here, carried out elsewhere").
*Session* cannot name the session's drive: `onehand_core::agent::Session` exists, and the glossary
gives the word to the conversation itself. Hence `SessionDrive` (D1).

## Inventory: what moves where

Today's homes, by area. "Core has" is what is already GUI-free; "app decides" is what moves. Every
row is re-audited at the start of its phase. Line counts are at `6f92ab49`.

| Area | Core has | App decides today | Target drive |
|---|---|---|---|
| Session: intake | `Chat::submit`, `queue`, `submit_blocker` | `chat/pane/sessions.rs` (`submit_from`), `chat/pane/bridge.rs` (`remote_prompt`), `chat/pane/offload_send.rs`, the `submit` calls in `control/workers*`, `task/driver.rs`, `shell/issue_work.rs` | `SessionDrive::prompt` |
| Session: events and saving | `Chat::apply`, `flush`, `flush_meta`, `starts_quiet_save`, `own_cycle_wait` | `chat/session.rs` pump (batch signals, quiet save, `saw_offer`, host gone, stream end); `chat/pane/sessions.rs` subscriber (save on turn and cycle end, offload check, archive refresh) | `SessionDrive::feed` / `handle` |
| Session: telling | `attention` (`telling`, `Away`), `remote::Chats::announcement` | `chat/pane/bridge.rs` (`turn_ended_detached`, `awaiting_user_detached`, `link_lost_detached`), desktop notifications | `SessionDrive` effects `Tell`, with `Input::Shown` |
| Session: cards, picks, stop | `answer_permission`, `answer_ask`, `pick`, `set_mode`, `cancel_turn`, `stop_background` | called straight from the transcript, header and remote paths | `SessionDrive` methods |
| Session: lifecycle | `Chat` construction, `resume_from`, `adopt_fork`, `rename`, the workspace tree | `chat/session.rs` (`spawn`, `adopt`), `chat/pane/sessions.rs` (phases, `close`), `shell/sessions.rs` (946 lines), `shell/fork.rs`, `chat/session/host.rs` | `SessionDrive` + a workspace drive |
| Task runs | `task::Drive`, `task::queue`, `workflow::Run` | `task/driver.rs` (already a driver), `task/approval.rs`, `task/place.rs` | `task::Drive` (D2) |
| Task starts and retries | preflight, `retry_plan`, worktree composition (#171) | `shell/workflows.rs` (launcher: preflight → cut → request), `shell/retry.rs` | a task-start drive, or `task::Drive` |
| Schedules | `schedule::decide`, `cron` | `schedules.rs` (the clock, rereading `schedules.toml`, choosing the window) | schedule drive |
| Unattended runs | `unattended` claim, verdict, report (#170) | `unattended.rs` (770), `unattended/launch*`, `unattended/report.rs` (tick, cap, after a run's task ends) | unattended drive |
| Offloaded projects | `host::copy`, offload state | `offload.rs` (934, past the split line), `offload/branch.rs`, `shell/offload.rs` | offload drive |
| Hosts | `host::client`, `host::wire`, `journal` | `hosts.rs` (one attach per host), `hosts/waits.rs`, `shell/host_end.rs`, `shell/host_project*` | host drive |
| Remote bridge | `remote::command`, `remote::Chats`, `press` | `remote.rs` (`answer`, `send_prompt`, `stop_session`), `shell/remote_runs.rs` | remote drive, calling `SessionDrive` |
| onehand's tools | `control` (what a tool may do), `control::turns` | `control.rs` (`perform`), `control/start.rs`, `control/workers/*` | control drive |
| Settings | `AppConfig::update_in_place`, `saving` | `saves.rs`, `shell/settings_page.rs` (`persist_agents`) | settings drive |
| Second launch | `instance` | `instance.rs` | instance drive (small) |
| Agent check | `Shared::saw_offer` input | `agent_check.rs` | settings drive, or its own |
| Workspace edits | the workspace tree | `shell/roots.rs`, `shell/drafts.rs` (rename, worktree, branch), `shell/storage.rs`, `shell/cleanup.rs`, `shell/git_sweep.rs` | workspace drive |
| Issues | `issues`, `connector` | `shell/issues_page.rs`, `shell/issue_work.rs`, the Issues mode | issues drive |
| Workbench modes | editor rules, file tree, markdown | each mode crate (`AnyView`, `Window` in `onehand-workbench`) | per mode, only where a mode has behaviour (phase 7) |

## Executors

The ACP runtime (`acp.rs`), the remote channel's runtime (`remote.rs`), the attach to a host
(`hosts.rs`), disk writes and git calls are GUI-free in what they do, but they live in the app and
reach `Shared`. In the end state they live in one GUI-free crate, `crates/runtime`
(`onehand-runtime`), with the same check as core: `cargo tree -p onehand-runtime -i gpui-pre`
fails. Core keeps its invariant of dictating no runtime; the executor crate is where the runtime
is chosen. When this happens is decision D5.


## Where the drives live: D4, decided after the trial

The success criterion is that **application flows run and are tested without GPUI**, across
drives too: a schedule starts a task, the task's run prompts its session, the session's turn ends,
the run moves on. That needs the **routing** between drives to be GUI-free. It does not need one
owner for every drive. Ownership and routing are separate questions:

- **Ownership:** who holds a drive. Options: a GPUI entity per drive, as today's sessions are; or
  a GUI-free container.
- **Routing:** what one drive's effects and signals become for another. Options: match arms in
  GPUI subscribers, as today; or a GUI-free function.

| Option | Ownership | Routing | Cross-drive tests without GPUI |
|---|---|---|---|
| **A** | entity per drive | in GPUI subscribers | no; routing stays untested |
| **R** | entity per drive | a GUI-free router in core: given an effect or signal and its origin, the command or input for the drive it is for | yes: the router is tested with real drives and no GPUI |
| **H** | one GUI-free hub owning every drive | inside the hub | yes; repaints then need `Effect::Changed(uid)` per session, because several agents stream at once |

R gives the cross-drive tests without moving ownership; H additionally moves ownership. Phase 1
tries R on one link (a run prompting its session, the session's turn ending the run's wait).
Phase 2 decides between A, R and H from that. Whatever the choice, drives are written
owner-agnostic: no handle to their owner, routing only through effects.

## Plan

Each phase starts by re-auditing its rows, then writes **characterisation tests** of today's
behaviour (in core where the logic already is, in the app's `#[cfg(test)]` modules where it is
not), then moves. A phase is behaviour-neutral unless it says otherwise. A phase is done when its
decisions are deleted from the app, its mutators are `pub(crate)` in core, and its tests run in
`cargo test -p onehand-core`.

**Phase 1: a trial slice: one session, end to end.** Nothing is promoted to a rule yet.
- The slice is the flow open → prompt → events → turn end and save → stop → close, for a
  session on this machine. It covers:
  - `SessionDrive::prompt` for every origin, with today's per-origin rules in one table (workers
    never queue, the remote's "already waiting" refusal, the composer's code first);
  - `feed`, with the pump's batch decisions and the pane's turn-end save as effects, and
    `Chat::flush_queued` moved out of `Chat::apply`;
  - `Input::StreamEnded`;
  - stop and close, with S5's late-answer handling for the save that is still writing at close.
- Moved `Chat` mutators become `pub(crate)`, to try S8 on real callers.
- One cross-drive link through a GUI-free router (D4, option R): `task::Drive`'s `Submit`
  reaching `SessionDrive::prompt` with `Origin::Run`, and the session's turn end reaching the
  run as an `Input`.
- The session's driver gets S11's contract tests.
- Out of the slice: host sessions, forks, telling, cards and picks, view state in `Chat`.
- D8 is answered here: every send path is traced to whether it can reach an offloaded project,
  before anything about code-first changes.
- The trial answers, in writing, in this document:
  - **Shape (S4):** do commands as methods and reports as `Input` read well, and does `Intake`
    carry what each caller needs?
  - **Effects:** which effects are shared, which belong to one drive, and is the S5 table complete?
  - **Ownership and routing (D4):** did the router hold up, and does anything argue for a hub?
  - **Enforcement (S8):** did `pub(crate)` catch real callers, and what did it break?
  - **Cost:** lines added in core against lines deleted in the app. Is it a forwarding layer
    anywhere?

**Phase 2: settle the standard.**
- Each of S1 to S11 kept, changed or dropped on the trial's evidence; D1 to D7 decided.
- What is kept moves into its owning documents:
  - the short form into AGENTS.md (*Invariants*);
  - the reasons into docs/rules-and-gotchas.md;
  - the terms into CONTEXT.md;
  - the guards into `guards.rs`.
- From here, new features follow it (*New features*, below).

**Phase 3: the rest of the session.** Host sessions, forks, telling (`Input::Shown`,
`Effect::Tell`), cards, picks, background work, view state in `Chat` (D3), the lifecycle in
`shell/sessions.rs`. Then every `Chat` mutator is `pub(crate)`.

**Phase 4: tasks.** Start, retry and the queue's app half; `task::Drive` aligned with the settled
S4 to S6 (D2).

**Phase 5: schedules and unattended runs.** The clock and the tick become `Arm`/`Elapsed`; the
window choice becomes an input the driver supplies.

**Phase 6: offloaded projects and hosts.** Splits `offload.rs` along the way, which the line rule
already demands.

**Phase 7: remote bridge, onehand's tools, second launch, settings, agent check.** Mostly callers
of drives built earlier, so mostly deletion.

**Phase 8: workspace edits and issues.**

**Phase 9: Workbench modes.** Only modes with behaviour get a drive; a mode that only shows
something keeps its view. `onehand-workbench`'s contract splits into a GUI-free request and answer
part, and the GPUI mount.

**Phase 10: cross-drive flows and the executors' crate** (D4 as settled, D5). Tests with fake
executors:
- schedule → task → run → session;
- unattended tick → claim → run → report.

## What the trial showed

Phase 1 was built on `refactor/session-drive`:
- `onehand_core::chat::drive` holds `SessionDrive`;
- `onehand_core::task::drive::link` is the router (option R);
- `ChatSession` is the session's driver.

Every core and app test passes, and clippy passes with warnings denied.

**Shape (S4): held, with one addition.**
- Commands as methods read as ordinary code at every caller. One `Intake` covered all five
  origins. Each caller still words a refusal its own way (the remote bridge's sentences stay in
  the bridge), which is right: the words are the front end's.
- What S4 did not foresee is the **batch**. Where a batch of events may end is behaviour (a card
  is said after the events that came with it; a batch ends after an event that tells a watcher
  something), but the batch's size cap is the front end's. `apply(event) -> (Effects, Next)` plus
  `end_batch()` split the two cleanly. A drive fed a stream needs this; one fed single inputs
  does not.
- `Effect` and `Input` cannot derive `PartialEq` or `Clone` while they carry a `PendingWrite`, and
  clippy asked for it to be boxed. Tests compare effect names instead.

**Effects: no `Tell`; and one drive, two places that carry it out.**
- What to tell a person became a `Signal` (`SaveFailed`, `CodeFailed`). How it is said is the
  front end's (a toast, here). A separate `Tell` effect would only have been forwarded.
- S7's "one driver per drive" did not hold. `SendCode` and `PromptAgain` need the composer and
  the window, which the session entity does not have. `ChatSession::perform` carries out what it
  can and hands the rest back to the pane (`ChatPane::carry`). A drive's effects may be carried by
  more than one part of a front end; the rule to keep is that every effect is carried exactly
  once, which `perform`'s return makes explicit.

**Late answers (S5): the table held, and needs two notes.**
- `Save` is *record*: a failed write is handed back whatever the drive did since. A recorded
  answer needs no token, since it carries what it is about. Only *drop* answers need one
  (`Arm`/`Elapsed`).
- `SendCode` is *record* too: the line the session said about the send is taken back even when the
  answer is stale.
- *Clean up* and *await* did not occur in this slice. They live in `task::Drive` (a command's
  place held until it exits).
- **Found:** a record answer cannot be honoured once the drive is gone. A session closed while its
  turn's save is in flight drops the answer. Its `Chat` writes on drop only what comes after the
  mark that save moved, so a save that then fails loses that turn. This was so before the trial.
  The fix is the *await* row: close waits for the save in flight, or rewrites from its mark.

**The model still acts.**
- `Chat::submit` and `Chat::cancel_turn` send on the agent's request channel. The send never
  blocks, but it is I/O inside the model.
- `Chat`'s `Drop` writes the transcript synchronously, on the UI thread, when a session closes.
- Phase 2 decides whether requests to the agent become effects, and whether closing becomes an
  effect (`Save`, then *await*) rather than a drop.

**Ownership and routing (D4): R held.**
- `link` joins a real run and a real session with no GPUI. Its tests play a run's prompt, the
  agent's turn and the run moving on. The app's task driver now asks `link` which signal moves a
  run and how a run's prompt and cancel reach its session. Nothing argued for a hub.
- `task::Drive` still changes the `Chat` directly (its picks). Inside core, through `link`,
  that is allowed. Whether a drive may change another drive's model or must go through its
  methods is for phase 4.

**Enforcement (S8): it worked, and showed the size of what is left.**
- `submit`, `queue`, `unqueue`, `cancel_turn`, `flush`, `unflush`, `stream_ended`,
  `quiet_spell` and `starts_quiet_save` became `pub(crate)`. The compiler listed every caller,
  and it found an app test that relied on `flush`.
- `SessionDrive` derefs to `Chat` without `DerefMut`, so every change the slice does not cover
  failed to compile and now goes through `SessionDrive::chat_mut`. There are 61 such sites in 19
  files of the app, which is phase 3's backlog, countable with one search.
- `flush_meta` stayed `pub` for the rename path (out of the slice).
- A guard (`no_flag_is_only_ever_assigned_false`) did not see a flag raised through
  `std::mem::replace`; the drive writes the assignment out.

**S3's example was wrong.** A person's prompt held while the code goes up stays in the composer;
the drive holds only the send's token. Whether the composer still holds the same words is a view
fact, and it comes back with the answer (`Input::CodeSent { still }`). S3's own rule (a view fact
that behaviour depends on enters as an input) covered it; its example did not.

**Cost.**
- Core gained about 660 lines (drive and link, more than a third of it comments) and about
  700 lines of tests.
- The app's non-test code shrank by about 65 lines (+500, −565).
- Decisions moved out of the app:
  - the pump's batch rules;
  - the turn-end and quiet saves, and the save-failure latch;
  - the intake rules of five origins, and the code-first decision;
  - two copies of "send once the agent is up";
  - the run-to-session routing.
- What stayed is carrying out: timers, writes, the code send, notifications.
- It is not a forwarding layer for what it covers. It does look like one at the 61 `chat_mut`
  sites, until phase 3 gives them methods.

**Behaviour changed:** none intended.
- Issue work's first prompt is decided on the event that connects the agent, not on the first
  session event after it: the same moment in practice.
- A session that onehand's tools open for an agent sends its first prompt through the same
  "once up" path as issue work.

**Left in the app, inside the slice, for later phases:**
- the host's lost connection (`host::went_away`, checked before an event reaches the drive);
- the offloaded project's check after a turn (`offload::check_soon`);
- telling (badge, desktop, remote);
- the rename's own save path.

## How it is tested

- **Drive tests** (core): commands and inputs in, effects and state out. Each rule moved gets one,
  and each `Origin` of a shared command gets one.
- **Replay tests** (core): a recorded `Input` sequence, such as a mock agent's transcript or a host
  journal, produces a fixed effect sequence. The Mock UI agent's script is the first fixture.
- **Late-answer tests** (S5): every answered effect has a test of its late answer, for the row it
  declares: dropped, recorded, cleaned up or awaited.
- **Router tests** (D4): cross-drive links with real drives and no GPUI, from phase 1's one link
  to phase 10's whole flows.
- **Driver contract tests** (S11): order, exactly once, no lost failure, tokens round-tripped,
  nothing left alive after close, deferred work outside the update. A few per driver, not one per
  effect.
- By-hand checks per phase, run in the app on request (`/drive`), cover what only a window shows:
  focus on a card, a repaint during streaming.

## Risks

| Risk | Mitigation |
|---|---|
| The standard is fixed before it is proven, and new code is bent to it | Phase 1 is a trial; nothing binds until phase 2 settles it |
| A linear async flow becomes an explicit state machine, harder to read top to bottom | A drive's module header narrates its flows; waiting states are kept few and named |
| Boilerplate: every effect needs an executor arm and an answer input | No step can be forgotten, which is the point. Shared effects are carried out once (S7). Phase 1 measures the cost |
| A late answer treated as if its act never happened | S5's table: each answered effect declares drop, record, clean up or await |
| A driver that decides nothing is still wrong (order, twice, lost failure, leak) | S11 contract tests |
| Re-entrancy: GPUI panics on nested updates | Drivers defer by rule (S7); S11 tests it |
| Repaint cost if every change notifies everything | Per-session notification, whichever D4 option is chosen |
| View state dragged into core "for consistency", or because it is kept across restarts | S3's three questions; persistence classifies nothing |
| Behaviour without tests changes unnoticed during the move | Characterisation tests first, every phase; by-hand checks for what only a window shows |
| Feature work in the same files conflicts with a phase | Decision D7 |
| Engine swap still costs: the UI is most of the code | Expected. This keeps behaviour and its tests, not views |

## Decisions for the owner

Decided on 2026-11-06:
- **D6. PRs.** The trial (phase 1) is its own PR, then one PR per phase, each carrying its own
  docs. One PR for the whole refactor would not be reviewable.
- **D7. Feature work during a phase.** No feature work in a phase's files while that phase is
  open.

After the trial, in phase 2:
- **D1. Names.** *Drive* / *Driver* / *Effect* / *Executor*, and `SessionDrive`. The term must
  not collide with `agent::Session`, *worker* or *runtime*.
- **D2. Commands as methods (S4), and whether `task::Drive` converts** from `Input::Stop` /
  `Approved` / `Revised` to methods in phase 4.
- **D3. View state already in `Chat`** (`toggle_*`): move it to the UI in phase 3, or name
  "per-item open/closed" as the one view state the model may keep. Leaning: move. By S3, opening a
  tool's output changes nothing onehand does, two windows may differ on it, and the viewer decides
  it. If it is to be kept across restarts, it goes into the front end's own store.
- **D4. Ownership and routing:** A, R or H, from the trial's router.
- **D5. The executors' crate:** early, or in phase 10. Until it exists, the executors stay in the
  app and import no GPUI.

Answered by the trial's tracing:
- **D8. Offloaded projects and the send paths.** Tools, runs and issue work are kept off
  offloaded projects where projects are listed or picked (`ProjectRoot::runs_here`). The remote
  bridge is not.
  - `ChatPane::remote_sessions` (`chat/pane/bridge.rs`) lists every open session, a host's and
    an offloaded project's included. So `/use` and a prompt from Telegram reach an offloaded
    session, and the prompt goes without its code.
  - The host then refuses the prompt and the transcript says to send it again. Sending it again
    from Telegram skips the code again.
  - That contradicts docs/hosts.md (*The Telegram bridge does not offer it*, and the offloaded
    projects' list of what does not reach them).
  - The trial kept today's behaviour and wrote it into the drive's origin table (`Origin::Remote`
    sends as it is). The fix is separate, with its own decision: filter the bridge's list, or let
    a remote prompt send the code first.

## New features, from phase 2

Until phase 2 settles the standard, new features follow the existing rules. From phase 2, before
any GPUI code is written:

1. Name the application state the feature adds and check it against S3.
2. Pick the drive it belongs to, or add one beside its model (S9).
3. Write its commands, inputs and effects, and their tests, in core. Declare each answered
   effect's late-answer row (S5).
4. Carry out its effects in the driver, add no behaviour there, and give the driver its contract
   tests (S11).
5. Draw it.

## Done when

- No module in `crates/app` decides what happens next. Drivers carry out, the UI draws.
- Every change to application state goes through a drive; the model's mutators are `pub(crate)`.
- `cargo test -p onehand-core` covers every application flow, across drives.
- A new front end needs only views and drivers, plus their contract tests.
