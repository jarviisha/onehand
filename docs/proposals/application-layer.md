# An application layer: how onehand works, apart from how it is drawn

- Status: phases 1 to 6 built.
  - Phase 1, a trial slice of one session, is #307.
  - Phase 2 settled the standard: it lives in [docs/drives.md](../drives.md), its terms in
    CONTEXT.md (*How onehand is built*), and its short form in AGENTS.md (*Invariants*).
  - Phase 3 moved the rest of the session's behaviour into `SessionDrive` and started the I/O
    crate (`onehand-io`); what it left for later is placed under *Plan*.
  - Phase 4 moved tasks: the run's drive is `RunDrive`, every task and its place are
    `TasksDrive`'s, and starts and retries are decided in core.
  - Phase 5 moved unattended runs into `UnattendedDrive` and the clock into `ScheduleDrive`.
  - Phase 6 moved hosts into `HostDrive`, offloaded projects into `OffloadDrive`, and the attach
    to a host into `onehand-io`.
  - What is left here is the plan for phases 7 to 10, the inventory they work from, and the
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

The task run already shows the answer. `onehand_core::task::drive::RunDrive` decides everything a run
does, answering each `Input` with `Effect`s; `crates/app/src/task/carrier.rs` only carries them out
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
  review after phase 1 found; that moves into inputs in phase 3.


## Inventory: what moves where

Today's homes, by area. "Core has" is what is already GUI-free; "app decides" is what moves. Every
row is re-audited at the start of its phase. Line counts are at `6f92ab49`.

| Area | Core has | App decides today | Target drive |
|---|---|---|---|
| Session: what phase 6 left | `SessionDrive` (every change to a session, a host's lost connection, what it waits on its host for) | `chat/pane/sessions.rs` subscriber (the archive refresh after a turn; the `Disconnected` arm telling a fork that never opened, a host out of reach and a lost agent apart) | the session lifecycle (phase 8) |
| Session: telling (phase 7) | `attention` (`telling`, `Away`), `remote::Chats::announcement` | `chat/pane/bridge.rs` (`turn_ended_detached`, `awaiting_user_detached`, `link_lost_detached`), desktop notifications | `SessionDrive` signals, with `Input::Shown` |
| Session: lifecycle (phase 8) | `SessionDrive::resume`, `fork`, `take_snapshot`, the workspace tree | `chat/session.rs` (`spawn`), `chat/pane/sessions.rs` (phases, `close`), `shell/sessions.rs` (946 lines), `shell/fork.rs` | a workspace drive |
| Remote bridge | `remote::command`, `remote::Chats`, `press` | `remote.rs` (`answer`, `send_prompt`, `stop_session`), `shell/remote_runs.rs` | remote drive, calling `SessionDrive` |
| onehand's tools | `control` (what a tool may do), `control::turns` | `control.rs` (`perform`), `control/start.rs`, `control/workers/*` | control drive |
| Settings | `AppConfig::update_in_place`, `saving` | `saves.rs`, `shell/settings_page.rs` (`persist_agents`) | settings drive |
| Second launch | `instance` | `instance.rs` | instance drive (small) |
| Agent check | `Shared::saw_offer` input | `agent_check.rs` | settings drive, or its own |
| Workspace edits | the workspace tree | `shell/roots.rs`, `shell/drafts.rs` (rename, worktree, branch), `shell/storage.rs`, `shell/cleanup.rs`, `shell/git_sweep.rs` | workspace drive |
| Issues | `issues`, `connector` | `shell/issues_page.rs`, `shell/issue_work.rs`, the Issues mode | issues drive |
| Workbench modes | editor rules, file tree, markdown | each mode crate (`AnyView`, `Window` in `onehand-workbench`) | per mode, only where a mode has behaviour (phase 7) |

## The I/O crate (D5: started in phase 3)

`onehand-io` holds the runtimes onehand's I/O runs on, held to core's check (`cargo tree -p
onehand-io -i gpui-pre` fails, in CI). Phase 3 moved the agent runtime into it (`onehand_io::acp`);
what an agent is given to reach onehand's tools stays the front end's, through `acp::Pass`. Phase
6 moved the attach to a host and every call over it (`onehand_io::host`); what follows from an
attach is `HostDrive`'s. The remote channel's runtime (`remote.rs`) is tangled with `Shared` and
GPUI contexts, so its runtime half moves with phase 7. Disk writes and git calls stay blocking functions in core. Core keeps its rule of
dictating no runtime; this crate is where the runtime is chosen.

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

**Phase 3: the rest of the session, and the I/O crate.** Built:
- What the reader has open (the item folds, the fold counter, a question card's tab and keyboard
  row as `AskPlace`, the `@`-mention file list) is the front end's (D3).
- Cards, picks, modes and background work, names (with their ordered, put-back save), host lines
  and lost links, resume, fork, snapshot and notes all go through `SessionDrive`.
- `Chat`'s requests to its agent are `Effect::Request`, carried out on the request channel.
- `Chat` reads no clock: the carrier tells the drive the time (`Now`).
- A session closed with a save in flight writes the whole transcript, and the store drops a late
  save's lines (#309).
- The agent runtime is in `onehand-io` (D5).
- Every `Chat` mutator is `pub(crate)`; `SessionDrive::chat_mut` is crate-private, kept for the
  run's link (phase 4 decides it).

What it left was placed with phase 4's audit (D9):
- telling (`Input::Shown`, a signal out): badges, desktop notices, the remote bridge's words, in
  phase 7;
- the lifecycle in `shell/sessions.rs` and `shell/fork.rs`, and the pane's phases and close, in
  phase 8;
- restarting a session with a save in flight, whose snapshot carries the moved mark, as a session
  follow-up beside phase 6;
- seeing a host's lost connection, which needs the connection, in phase 6.

**Phase 4: tasks.** Built:
- The run's drive is `RunDrive`, split along its seams (`pin.rs`, `steps.rs`, `forge.rs`). A
  person's Approve, Revise and Stop are its methods; what the world reports stays one `Input`
  (D2).
- Every answered effect declares its late answer: a pin, a command, a push, opening a pull
  request and taking a draft out of draft are awaited; a read, a look and a clock are dropped;
  a save is recorded, and answered (`Input::Saved`), its failure said once (`Signal::SaveFailed`).
  The drive prints nothing: a mark not pinned is a signal too.
- The forge calls are one at a time and decided in the drive: a pull request open on the branch
  taken up, nothing opened after a Stop, the status checks looked at on a clock the drive arms
  (`Arm`/`Elapsed`), at most three failing logs read for a repair, a draft taken out of draft.
- No drive changes another drive's model (D10): the run's drive reads its session and keeps the
  mode and picks it decides, which the link makes through `SessionDrive::pick`.
  `SessionDrive::chat_mut` is gone.
- Every task and its place are `TasksDrive`'s (`onehand_core::task::tasks`), a drive of its own
  beside the runs' (D11): requests, the queue, a run's end and its drain, the last mark, the next
  task, Stop on a queued task, dismiss, retry and the history cap. Its link to `RunDrive`
  (`task::tasks::link`) makes a run's save and end the task's.
- How a task holding its place begins (`TasksDrive::begin`), what the launcher refuses
  (`task::Launch`), a check's retry at once, a retry on another branch and a Resume's preflight
  refusal are core's. The run's link names its session for the task.
- The unattended hooks stay effects the carriers hand to `unattended` (D12).
- `crates/app/src/task/driver.rs` is the run's carrier, `task/carrier.rs`; the `Tasks` global
  carries the tasks' drive (`task/place.rs`). `Task`'s, the queue's and `Run`'s mutators are
  `pub(crate)`. Both carriers have contract tests (`crates/app/src/task/tests.rs`).

Left for later:
- `Task`'s fields are still public: the app builds tasks with them, though it no longer changes a
  kept one (phase 5 built an issue's task in core).
- What a person pressed and is still being done (`acting`) stays in the `Tasks` global.
- `task/approval.rs` still asks which answer a run waits at before a press, to show the new one
  instead; the engine refuses a stale press either way.

**Phase 5: schedules and unattended runs.** Built:
- Unattended runs are `UnattendedDrive` (`onehand_core::unattended::drive`, split along its seams:
  `claiming.rs`, `settings.rs`, `reports.rs`, `blocking.rs`), one drive beside the clock's rather
  than one for both (D13). The tick is its clock (`Arm`/`Elapsed`); the look and the search, the
  recheck before a claim, the claim and the task it is kept as, a pick refused before its claim, a
  search asked for now, the cap and the runs still starting, waiting and answered runs, the pause,
  a run's end and its reports sent once each are its (D15). What its effects do off the UI loop is
  core's too (`drive::blocking`).
- It is held in the `Tasks` global beside the tasks' drive and the runs (D14); `Shared.unattended`
  is gone. `unattended::link` joins it to `TasksDrive`: an issue task asking for its place, its end
  and what it owes its issue become unattended commands, and a claim kept as a task, a review
  answered and a report landed become the tasks'. Every start is held to the cap through
  `Joined::request`. `task::tasks::link::Seen` reads the tasks with the runs under way, so what a
  task is doing and the slot rule (`holds_slot`) are core's.
- The run's unattended effects (`Waiting`, `Answered`, `CameUp`, `Pause`) are carried as the drive's
  commands; the tasks' `Report` is gone (below).
- An issue's task is built in core (`Task::of_issue`); the app no longer sets a task's fields for
  one (D16).
- The clock schedules run by is `ScheduleDrive` (`onehand_core::schedule::drive`): its first look
  and every later one as `Arm`/`Elapsed`, the window holding each project, the tasks still working,
  the workflows on offer, the time and the time zone told it. A look, its starts one at a time, its
  write onto the file as it is (`schedule::merge_looked`) and a switch put back when its write fails
  (`schedule::switch`) are decided there.
- Both carriers have contract tests (`crates/app/src/unattended/tests.rs`,
  `crates/app/src/schedules/tests.rs`). Core's drive tests hold every rule moved, among them a tick
  that searches a real repository's issues, claims one and keeps its task asking for its place.

Left for later:
- `read_review` (`crates/app/src/unattended/launch/answer.rs`) still gathers what answering a review
  is judged on in the app; it only reads, and what it starts goes through the drive.
- What needs a window's shell (a claim's run brought up, a schedule's start, a run's project kept
  or dropped) is tested by hand; the carriers' tests run without a shell.

**Phase 6: offloaded projects and hosts.** Built:
- Offloaded projects are `OffloadDrive` (`onehand_core::host::offload::drive`, split along its
  seams: `blocking.rs`, the errands' bodies; `branch.rs`, a project on a branch). Who holds a folder's
  code, the claim before a prompt, every copy's landing, *Check again*, *Take the host's code*, the
  form's offload (`rsync` checked first), a branch's fetch and update, and the poll's pass, one
  folder at a time with each unreached host tried once, are its. Each errand's body runs off the UI
  loop in core over the attach the carrier hands it (`Reach`), and answers as a failure if it
  panics. The app's `offload.rs` is its carrier (324 lines, from 940); `offload/branch.rs` is gone.
- Hosts are `HostDrive` (`onehand_core::host::drive`): which ask joins which attempt, a person's
  ask waiting on one made by itself and attaching again should it fail, which landing is heard,
  what is known of each host, and the busy and back waits with their clocks and backoff. Its
  carrier (`crates/app/src/hosts.rs`) holds one shared future per attempt, so callers keep awaiting
  as before (D19). The attach and every call over it are `onehand_io::host` (D5); `HostFailure`
  and the waiting line are core's. The shell's rules on a restart and a back wait
  (`back_failure`, `needs_restart`, `said_by_wait`, `restart_failure`) are core's too.
- A host's lost connection is decided in the session's drive (D9): the pump tells it how the
  watch's connection stands (`HostLink`), and `SessionDrive::apply_watched` says the copy's line and
  keeps the wait. What a session waits on its host for (its busy sessions, or the host itself) is
  the drive's, no longer the entity's.
- Which of a session's signals checks an offloaded project's code is
  `OffloadDrive::session_said`'s; the pane's subscriber only hands the signal on.
- Restarting a session with a save in flight honours it: the snapshot says a save was on its way,
  and the session that carries on writes the whole transcript at its first save (D20).
- Both carriers have contract tests (`crates/app/src/hosts/tests.rs`,
  `crates/app/src/offload/tests.rs`), and the session's carrier holds a lost connection and a
  taken one.

Left for later:
- Each window's share of a host's waits (`HostFlow`, `crates/app/src/shell/host_project.rs`) keeps
  its bookkeeping in the shell; the rules it follows are core's.
- How a host session begins (`offline_start`, `host_restart`, `crates/app/src/chat/session/host.rs`)
  and *End on <host>* (`shell/host_end.rs`) are the session's lifecycle, phase 8's.
- The pane's subscriber still tells a fork that never opened, a host out of reach and a lost agent
  apart at a stream's end, reading what the drive holds; phase 8 moves it with the lifecycle.
- The remote bridge still reaches an offloaded session without its code (#308, D8): phase 7.
- A host's real attach is tested by hand: it runs on a runtime GPUI's test scheduler does not
  drive.

**Phase 7: remote bridge, onehand's tools, second launch, settings, agent check.** Mostly callers
of drives built earlier, so mostly deletion. Telling moves here.

**Phase 8: workspace edits and issues.** The session lifecycle moves here.

**Phase 9: Workbench modes.** Only modes with behaviour get a drive; a mode that only shows
something keeps its view. `onehand-workbench`'s contract splits into a GUI-free request and answer
part, and the GPUI mount.

**Phase 10: cross-drive flows.** Link tests with the real drives and fake I/O:
- schedule → task → run → session;
- unattended tick → claim → run → report (phase 5's drive tests reach the task asking for its
  place).

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

Settled on 2026-11-06, at the start of phase 3:
- Kept: the two first-prompt moments, and the latch being the session's. No feature document
  says either moment: `SessionDrive::prompt_when_up` and the save latch say them in their own
  comments, and control.md's "sent once the agent is up" holds as written.
- Put back in phase 3: a resend while a turn runs is refused again (`SessionDrive::resend`, which
  never queues), and the two link cuts outside the drive go through it
  (`SessionDrive::host_lost`, and the pane's `Disconnected` folded as a batch of its own), which
  drops the held prompt and marks the batch changed again.

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
- **D3. The open and closed state in `Chat` is view state,** and moves to the UI in phase 3. So
  do a question card's tab and keyboard row (`AskPlace`, its rules kept in core).
- **D4. R:** each drive is owned by its entity, and drives meet in a link in core.
- **D5. The GUI-free I/O gets its own crate early,** in phase 3: `onehand-io`, starting with the
  agent runtime; the remote and host runtimes follow in phases 6 and 7.
- **D6. PRs:** one per phase, each with its own docs.
- **D7. No feature work in an open phase's files.**
- **D8. Offloaded projects and the send paths:** the remote bridge reaches an offloaded session
  without its code (#308). Every other path is kept off offloaded projects where projects are
  listed or picked.
- **Requests to the agent become an effect** (phase 3). Whether one may go is decided on the link
  being up, not on the send.
- **The time is an input:** the model stops reading the clock, and the carrier says the time
  (phase 3).
- **Closing honours the saves in flight** (phase 3, #309). Built as a whole-file write at close
  rather than a wait: the drive goes with its session, so it cannot wait, and the store drops the
  lines of a save built before that write lands.

Made on 2026-11-09, with phase 4's audit:
- **D9. What phase 3 left** is placed: telling in phase 7, the session lifecycle in phase 8,
  restarting with a save in flight beside phase 6, a host's lost connection in phase 6.
- **D10. A drive reads another drive's model and never changes it.** What one drive decides for
  another's model goes through that drive's methods, called by their link.
- **D11. Tasks have a drive of their own**, beside the runs', joined to `RunDrive` by a link, rather
  than `RunDrive` taking start, retry and the queue.
- **D12. Unattended stays effects in phase 4.** A run's end keeps its issue's report through an
  effect, as the run's other unattended hooks do; unattended's own decisions are phase 5's.

Made on 2026-11-09, with phase 5's audit:
- **D13. Two drives**, one for unattended runs and one for the clock, each beside its model: they
  share no state and meet only through the tasks.
- **D14. Unattended runs' drive is held beside the tasks' drive**, in the `Tasks` global: the slot
  rule reads the tasks and the runs under way at once, and the link needs both drives in one call.
- **D15. All of unattended runs moves in phase 5**: the tick, the search and the claims, the pick,
  the search asked for now and a review answered, as well as the cap and a run's end.
- **D16. An issue's task is built in core.** The other builders (the launcher, a check, a tool, a
  schedule) keep `Task`'s public fields for now.

Made on 2026-11-09, with phase 6's audit:
- **D17. Two drives and a link**, one for hosts and one for offloaded projects, each beside its
  model: they share no state, and an offloaded project only reads how its host stands and asks for
  an attach.
- **D18. The owners stay where they were:** hosts' drive on `Shared`, one per process;
  offloaded projects' in the `Offloads` global.
- **D19. The attach's runtime half moves into `onehand-io`, and callers keep awaiting.** The drive
  decides; the carrier keeps one shared future per attempt and resolves every caller from it.
- **D20. Restarting with a save in flight is fixed in phase 6's PR**, a commit of its own.

Phase 6 changed, on purpose:
- Putting the offload form away no longer stops a send under way: it runs to its end and keeps
  what went for the next offload. The project stays here, as before.
- A errand of an offloaded project that panics answers as a failure, *Something went wrong here; try
  again*, rather than leaving its folder claimed.
- The poll goes through the folders in their path's order, not the map's.
- An attempt joined by several asks is heard once, as a person's when a person's ask joined it,
  where each ask heard it before; a poll's miss counts once per attempt.

Phase 5 changed, on purpose:
- A run's end reached its issue's task through an effect carried after the tasks' drive had queued
  the run's own write, so that write held the run without its report until the last mark was
  pinned. The tasks' drive now keeps the report on its own model (`IssueSource::keep_report`) in
  the write that keeps the run, and `Effect::Report` is gone.

Phase 4 changed, on purpose:
- A run's failed write was only logged; it is now said in the run's window, once until a write
  lands.
- A task's failed write is logged once until a write lands, rather than at every write.
- A Stop while a draft is being taken out of draft waits for that call to land, as a push and an
  open already did.

## Done when

- No module in `crates/app` decides what happens next. Carriers carry out, the UI draws.
- Every change to application state goes through a drive; the model's mutators are `pub(crate)`.
- `cargo test -p onehand-core` covers every application flow, across drives.
- A new front end needs only views and carriers, plus their contract tests.
