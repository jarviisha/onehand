# An application layer: how onehand works, apart from how it is drawn

- Status: phases 1 and 2 built.
  - Phase 1, a trial slice of one session, is #307.
  - Phase 2 settled the standard: it lives in [docs/drives.md](../drives.md), its terms in
    CONTEXT.md (*How onehand is built*), and its short form in AGENTS.md (*Invariants*).
  - What is left here is the plan for phases 3 to 10, the inventory they work from, and the
    record of the trial. Each phase that is built moves its rows out of here, in the same PR.
- Supersedes the rule agreed in #133 ("logic belongs in core when two call sites share it, when a
  second front end could need it, or when it wants a test without a GUI"). That rule moved *rules*
  to core; this one moves *what happens next* there too, with no exceptions.
- Contracts the later phases touch: the feature documents whose flows move (tasks.md,
  unattended.md, schedules.md, control.md, hosts.md).

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

## The standard, and how the trial changed it

The layers, the rules and a new feature's steps are in [docs/drives.md](../drives.md). Against the
draft that phase 1 tried, phase 2 kept, changed or dropped each rule:

- **Kept as drafted:**
  - every change to application state goes through a drive, with no exceptions;
  - reads go to the model;
  - a drive sits beside its model;
  - drives meet in a GUI-free link;
  - every carrier has contract tests.
- **S3 (application and view state):** kept; its example was wrong. A prompt held behind the code
  going up stays in the composer, and whether the composer still holds it is a view fact that
  comes back as an input.
- **S4 (shape):** gained the batch: `apply(event)` answers whether a batch may go on, and
  `end_batch()` closes it; the size cap is the carrier's. `Effect` and `Input` need not be `Clone`.
- **S5 (late answers):** only a *drop* answer needs a token. Closing must honour the answers still
  on their way (#309).
- **S6 (failures):** `Effect::Tell` dropped. What a person is told is a signal, and the words are
  the front end's.
- **S7 (one driver shape):** "one driver per drive" dropped. A drive's effects may be carried by
  more than one part of a front end, each exactly once. "Driver" became **carrier**.
- **S8 (enforcement):** visibility only. No guard is added until a mistake recurs, the bar every
  guard in `guards.rs` meets.
- **New, rule 12:** a model does no I/O and reads no clock. `Chat`'s requests to its agent and its
  drop save are moving into effects. `Chat` also reads the wall clock and `Instant::now`, which the
  review after phase 1 found; no phase moves that yet.


## Inventory: what moves where

Today's homes, by area. "Core has" is what is already GUI-free; "app decides" is what moves. Every
row is re-audited at the start of its phase. Line counts are at `6f92ab49`.

| Area | Core has | App decides today | Target drive |
|---|---|---|---|
| Session: events and saving, what phase 1 left | `SessionDrive::apply`, `handle`, `save_meta` | `chat/session.rs` pump (`host::went_away`, checked before an event reaches the drive); `chat/pane/sessions.rs` subscriber (offload check, archive refresh); the rename's own save (`chat/pane/titles.rs`) | `SessionDrive::apply` / `handle` |
| Session: telling | `attention` (`telling`, `Away`), `remote::Chats::announcement` | `chat/pane/bridge.rs` (`turn_ended_detached`, `awaiting_user_detached`, `link_lost_detached`), desktop notifications | `SessionDrive` signals, with `Input::Shown` |
| Session: cards, picks, stop | `answer_permission`, `answer_ask`, `pick`, `set_mode`, `cancel_turn`, `stop_background` | called straight from the transcript, header and remote paths | `SessionDrive` methods |
| Session: lifecycle | `Chat` construction, `resume_from`, `adopt_fork`, `rename`, the workspace tree | `chat/session.rs` (`spawn`, `adopt`), `chat/pane/sessions.rs` (phases, `close`), `shell/sessions.rs` (946 lines), `shell/fork.rs`, `chat/session/host.rs` | `SessionDrive` + a workspace drive |
| Task runs | `task::Drive`, `task::queue`, `workflow::Run` | `task/driver.rs` (already a carrier), `task/approval.rs`, `task/place.rs` | `task::Drive` (D2) |
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

## The I/O crate (D5: early, in phase 3)

The ACP runtime (`acp.rs`), the remote channel's runtime (`remote.rs`), the attach to a host
(`hosts.rs`), disk writes and git calls are GUI-free in what they do, but they live in the app and
reach `Shared`. Phase 3 moves them into one GUI-free crate, with the same check core has
(`cargo tree -p <crate> -i gpui-pre` fails). Core keeps its rule of dictating no runtime; this
crate is where the runtime is chosen. Its name is chosen in phase 3. It is not *executor*, which
GPUI already uses (`cx.background_executor()`).

## Where the drives live (D4: R)

The success criterion is that **application flows run and are tested without GPUI**, across
drives too: a schedule starts a task, the task's run prompts its session, the session's turn ends,
the run moves on. That needs the routing between drives to be GUI-free; it does not need one
owner for every drive.
- **Ownership:** each drive is held by the GPUI entity that draws it, as `ChatSession` holds a
  `SessionDrive`.
- **Routing:** a link module in core per pair of drives that meet, tested with the real drives,
  as `task::drive::link` joins a run and its session.

The trial found nothing that argued for one hub owning every drive. That question is opened again
only on a concrete need.

## Plan

Each phase starts by re-auditing its rows, then writes **characterisation tests** of today's
behaviour (in core where the logic already is, in the app's `#[cfg(test)]` modules where it is
not), then moves. A phase is behaviour-neutral unless it says otherwise. A phase is done when its
decisions are deleted from the app, its mutators are `pub(crate)` in core, and its tests run in
`cargo test -p onehand-core`.

**Phase 1: a trial slice of one session.** Built in #307; what it showed is below.

**Phase 2: settle the standard.** Built: [docs/drives.md](../drives.md), the glossary's *How onehand
is built*, and AGENTS.md's *Invariants*. The decisions are below.

**Phase 3: the rest of the session, and the I/O crate.**
- Host sessions, forks, telling (`Input::Shown`, a signal out), cards, picks and background work.
- The open and closed state in `Chat` (`toggle_*`) moves to the UI (D3).
- The lifecycle in `shell/sessions.rs`. Closing becomes a command that saves what is left and
  waits for the saves in flight (#309).
- `Chat`'s requests to its agent become an effect (`Effect::Request`), carried out on the
  request channel.
- The GUI-free I/O moves into its own crate (D5).
- Then every `Chat` mutator is `pub(crate)`, and `SessionDrive::chat_mut` is gone.

**Phase 4: tasks.**
- Start, retry and the queue's app half move into drives.
- `task::Drive` becomes `RunDrive`, takes its commands as methods rather than as `Input`
  (`Stop`, `Approved`, `Revised`), and declares each answered effect's late answer (D2).
- `crates/app/src/task/driver.rs` becomes the run's carrier, by name too.

**Phase 5: schedules and unattended runs.** The clock and the tick become `Arm`/`Elapsed`; the
window choice becomes an input the carrier supplies.

**Phase 6: offloaded projects and hosts.** Splits `offload.rs` along the way, which the line rule
already demands.

**Phase 7: remote bridge, onehand's tools, second launch, settings, agent check.** Mostly callers
of drives built earlier, so mostly deletion.

**Phase 8: workspace edits and issues.**

**Phase 9: Workbench modes.** Only modes with behaviour get a drive; a mode that only shows
something keeps its view. `onehand-workbench`'s contract splits into a GUI-free request and answer
part, and the GPUI mount.

**Phase 10: cross-drive flows.** Link tests with the real drives and fake I/O:
- schedule → task → run → session;
- unattended tick → claim → run → report.

## What the trial showed

Phase 1 was built on `refactor/session-drive`:
- `onehand_core::chat::drive` holds `SessionDrive`;
- `onehand_core::task::drive::link` is the router (option R);
- `ChatSession` is the session's carrier.

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

**Behaviour changed:** none intended. These differ from before; the review after phase 1 found
the last four:
- Issue work's first prompt is decided on the event that connects the agent, not on the first
  session event after it: the same moment in practice. To a session already up it goes at once,
  where before it waited for the session's next event.
- A session that onehand's tools open for an agent sends its first prompt through the same
  "once up" path as issue work. It goes inside the event that connects the agent, not after the
  rest of that batch; the handshake says `Connected` last, so this is the same moment in
  practice.
- The held first prompt is dropped only when an event the drive folds leaves the link lost. A link
  cut outside the drive (`host::went_away`, the pane's `Disconnected` for a host gone) leaves it
  held, to go if the same session comes up again; before, it was dropped at the first change
  that left the agent not connecting. One prompt is held at a time, and a second replaces it.
- Resending a failed turn's prompt goes in as a person's prompt. While a turn still runs (an
  error said mid-turn, before the turn's end), it now queues, replacing a prompt already waiting;
  before, it was refused.
- The save-failure latch is the session's, not the conversation's, so a restart of the agent arms
  it again: a store still failing is said once more after a restart.
- A host's lost connection (`host::went_away`) no longer marks the batch changed, so a batch of
  only that event does not resync the parsed markdown.

Each is settled in phase 3: kept and moved into the owning document, or put back.

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
- **Carrier contract tests** (S11): order, exactly once, no lost failure, tokens round-tripped,
  nothing left alive after close, deferred work outside the update. A few per carrier, not one per
  effect.
- By-hand checks per phase, run in the app on request (`/drive`), cover what only a window shows:
  focus on a card, a repaint during streaming.

## Risks

| Risk | Mitigation |
|---|---|
| The standard is fixed before it is proven, and new code is bent to it | Phase 1 is a trial; nothing binds until phase 2 settles it |
| A linear async flow becomes an explicit state machine, harder to read top to bottom | A drive's module header narrates its flows; waiting states are kept few and named |
| Boilerplate: every effect needs a carrier arm and an answer input | No step can be forgotten, which is the point. Shared effects are carried out once (S7). Phase 1 measures the cost |
| A late answer treated as if its act never happened | S5's table: each answered effect declares drop, record, clean up or await |
| A carrier that decides nothing is still wrong (order, twice, lost failure, leak) | S11 contract tests |
| Re-entrancy: GPUI panics on nested updates | Carriers defer by rule (S7); S11 tests it |
| Repaint cost if every change notifies everything | Per-session notification, whichever D4 option is chosen |
| View state dragged into core "for consistency", or because it is kept across restarts | S3's three questions; persistence classifies nothing |
| Behaviour without tests changes unnoticed during the move | Characterisation tests first, every phase; by-hand checks for what only a window shows |
| Feature work in the same files conflicts with a phase | Decision D7 |
| Engine swap still costs: the UI is most of the code | Expected. This keeps behaviour and its tests, not views |

## Decisions

Made on 2026-11-06:
- **D1. Names.** *Drive*, *carrier* and *effect*, with *application state* and *view state*, in
  the glossary. `SessionDrive` stays; `task::Drive` becomes `RunDrive` in phase 4. *Executor* is
  not a term: GPUI uses the word.
- **D2. Commands are methods.** `task::Drive` converts in phase 4.
- **D3. The open and closed state in `Chat` is view state,** and moves to the UI in phase 3.
- **D4. R:** each drive is owned by its entity, and drives meet in a link in core.
- **D5. The GUI-free I/O gets its own crate early,** in phase 3.
- **D6. PRs:** one per phase, each with its own docs.
- **D7. No feature work in an open phase's files.**
- **D8. Offloaded projects and the send paths:** the remote bridge reaches an offloaded session
  without its code (#308). Every other path is kept off offloaded projects where projects are
  listed or picked.
- **Requests to the agent become an effect** (phase 3).
- **Closing is a command** that saves what is left and waits for the saves in flight (phase 3,
  #309).

## Done when

- No module in `crates/app` decides what happens next. Carriers carry out, the UI draws.
- Every change to application state goes through a drive; the model's mutators are `pub(crate)`.
- `cargo test -p onehand-core` covers every application flow, across drives.
- A new front end needs only views and carriers, plus their contract tests.
