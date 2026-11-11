# An application layer: how onehand works, apart from how it is drawn

- Status: phases 1 to 10 built.
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
  - Phase 7 moved telling into `SessionDrive`, the remote bridge into `RemoteDrive`, onehand's
    tools into `ControlDrive` and the settings and the agent check into `SettingsDrive`, and the
    remote channel and the instance socket into `onehand-io`.
  - Phase 8 moved each window's workspace into `WorkspaceDrive` (the session lifecycle, the
    workspace edits and the workspace file; how a session begins on a host is core's), and the
    shared issues file into `FileDrive`.
  - Phase 9 moved the Workbench mode contract's GUI-free half into core, the Editor and the
    Plugins mode into `EditorDrive` and `PluginsDrive`, and what the Issues views read of the work
    into `WorkDrive`.
  - Phase 10 moved the routing between drives still in the app into core (a start off screen,
    where a driven task lands, what a run tells unattended runs) and tests whole flows across
    drives with no front end (`task::flows`).
  - Phase 11 takes every leftover of phases 4 to 10, in five PRs by area; its plan is under
    *Plan*.
  - What is left here is what each phase left for later, the record of the trial and the
    decisions; *Done when* is not met until phase 11 is built, and the proposal leaves
    `docs/proposals/` with phase 11's last PR.
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
| Workbench modes | editor rules, file tree, markdown | each mode crate (`AnyView`, `Window` in `onehand-workbench`) | per mode, only where a mode has behaviour (phase 9): `EditorDrive`, `PluginsDrive` |

## The I/O crate (D5: started in phase 3)

`onehand-io` holds the runtimes onehand's I/O runs on, held to core's check (`cargo tree -p
onehand-io -i gpui-pre` fails, in CI). Phase 3 moved the agent runtime into it (`onehand_io::acp`);
what an agent is given to reach onehand's tools stays the front end's, through `acp::Pass`. Phase
6 moved the attach to a host and the front end's calls over it (`onehand_io::host`); what follows
from an attach is `HostDrive`'s. An offloaded project's errands call the host from a blocking body
in core, polling the runtime's handle the front end passes them, as a host's disk already did
(`disk::HostDisk`). Phase 7 moved the remote channel's runtime (`onehand_io::remote`) and the
instance socket's, a second launch's line and the MCP conversation alike (`onehand_io::socket`),
which asks the front end each connection's standing on every message. Disk writes and git calls stay blocking functions in core. Core keeps its rule of
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
  as before (D19). The attach and the front end's calls over it are `onehand_io::host` (D5); `HostFailure`
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
- `hosts::attach_waiting` still waits out a busy host and attaches again in the carrier, for the
  terminal dock, the Workbench and the folder list.
- The pane still folds a lost agent's end into a session whose wait for its host failed
  (`host_back_failed`, `host_wait_failed`): the session's lifecycle, phase 8's.
- How a host session begins (`offline_start`, `host_restart`, `crates/app/src/chat/session/host.rs`)
  and *End on <host>* (`shell/host_end.rs`) are the session's lifecycle, phase 8's.
- The pane's subscriber still tells a fork that never opened, a host out of reach and a lost agent
  apart at a stream's end, reading what the drive holds; phase 8 moves it with the lifecycle.
- A host's real attach is tested by hand: it runs on a runtime GPUI's test scheduler does not
  drive.

**Phase 7: remote bridge, onehand's tools, second launch, settings, agent check.** Built:
- Telling is `SessionDrive`'s (D9, D21): which signal is news (a turn or cycle that ended, a card
  that waits, an agent lost rather than a fork that never opened or a host out of reach) is said
  with `Effect::Look`; the pane answers at once with where the person is (`Input::Seen`), and the
  drive keeps the badge (`unseen`) and says what to tell (`Effect::Tell`). `Input::Shown` reads it.
  What a chat is told of the news is `remote::link::announcement`.
- The remote bridge is `RemoteDrive` (`onehand_core::remote::drive`): the gate, every command and
  its words, the reconcile, away, a channel that ends, and a press checked against the sessions
  open. What a prompt, a stop (taken back first), a press or a look at the pickers does to one
  session is `remote::link`'s. It reaches only sessions whose project runs here (#308, D22).
  The channel's runtime is `onehand_io::remote`; the app's `remote.rs` carries (361 lines, from
  696, and the pane's bridge from 487 to 155).
- onehand's tools are `ControlDrive` (`onehand_core::control::drive`): tokens and standing, the
  switch and grant, labels, the cap, the reservations, requests and relations
  (`control::workers`), and a worker's open as steps, its deadline armed at the call; what the
  world reports comes back as one `Input`. What a call is refused for before anything is done is
  `control::calls`. The drive is held in one process lock by the app's carrier (D23).
- The settings and the agent check are `SettingsDrive` (`onehand_core::config::drive`), on
  `Shared` (D24): the agents, the appearance, outside control's save (its value the tools'
  drive's, put in force through `control::link`), what each agent was seen to offer, and a check
  started once, folded by `fold_probe`, failing after its wait.
- A second launch has no drive of its own (D25): it changes no application state but the folder
  it opens, a workspace edit (phase 8). Its socket is `onehand_io::socket`.
- Carrier contract tests: `crates/app/src/remote/tests.rs`, `crates/app/src/control/tests.rs`,
  `crates/app/src/saves/tests.rs`.

Left for later:
- The pane's answer to a session's look and a worker's open carried through a window are tested by
  hand; so is an agent check's probe, which starts a real agent.
- The keymap's save and the workflows named for unattended runs keep `saves::write_config`; the
  keymap is the front end's, the workflows `UnattendedDrive`'s.
- `perform_now` still routes each call to the window and the other drives it needs; what a call is
  refused for is core's.
- *Test* beside an agent (`check_agent`) only reads the machine and stays the shell's.

**Phase 8: the workspace, then issues.** Two PRs (D29). The first, the workspace:
- Each window's workspace is `WorkspaceDrive` (`onehand_core::workspace::drive`, split along its
  seams: `sessions.rs`, `projects.rs`, `git.rs`), one drive for the lifecycle and the edits, since a
  session lives inside its project (D26), held per window on the `Shell` (D27). It derefs to the
  tree; the tree's mutators are `pub(crate)`. Its carrier is `crates/app/src/shell/carry.rs`.
- The session lifecycle: where a session is added (refused with no agent or no project, beside a
  worker on its lead's project), what goes when it closes (a run's or a worker's project left
  empty, the lead shown after a worker), a run's session and project (added, ended, kept), a tool's
  open that never opened, a finished run's session left while watched, and the selection. What
  closing and restarting ask first is `SessionDrive`'s (`closing`, `restarting`).
- A stream's end carries why it ended (`Signal::Disconnected(Ended)`), so the pane routes a fork
  that never opened and a host out of reach without reading the model, and telling reads the same
  answer. A wait on a host that fails ends the session through the drive (`wait_failed`,
  `back_failed`).
- How a session begins on a host (`host_begin`, `Start`, `host_restart`, `offline_start`, with
  `HostStart`, which `onehand-io` re-exports) is core's `host::session`, and what a window's wait on
  a host comes to when it ends (`back_landed`, `idle_landed`) is `host::drive`'s (D28).
- The workspace edits: a folder opened (the picker, a second launch, a worktree), one kept for work
  about to start, a removal by host and path and what its question is about (`Losses`), the pin,
  the order, the unattended switch, the check command, the name, where an offloaded project's
  sessions run, the storage binding.
- The workspace file: the burst's wait (`Arm`/`Elapsed`), the write in order, each thing put back
  alone when its write fails, the name remembered once a write lands, and the write a closing
  window makes.
- Each project's `git status` is kept in the drive: which projects a sweep reads, and which
  sweep's answer is kept.
- The carrier has contract tests on a real `Shell` (`crates/app/src/shell/carry/tests.rs`); core's
  drive tests hold every rule moved, and characterisation tests written first pin the lifecycle's
  edges and the pane's open, close and restart.

Left for later:
- `HostFlow` keeps each window's share of a host's waits; `hosts::attach_waiting` stays `HostDrive`'s
  carrier, as it serves the terminal dock, the Workbench and the folder list, never a session.
- The pane still holds what a session starts on until it is shown (`pending_resume`,
  `pending_watch`, `pending_fork`), and which of them goes first (a fork, a watch, a resume) is
  still decided in `ChatPane::show`; how a start opens (`Start::opening`), how it begins on a host
  and the prompts it carries (`host_begin`) are core's.
- The recents list (`Shared.recents`) is touched, named and forgotten by the shell's storage code.
- A second launch picks the front window and waits for the first in the carrier
  (`crates/app/src/instance.rs`); the folder it opens goes through the drive.
- `shell/cleanup.rs` (*Remove worktree…*) is a task's, not the workspace's: its rule is
  `removal::judge`, and a window closing while it runs loses `worktree_removed` (#319).
- The panels' layout is view state the front end keeps in the workspace file (`layout_mut`).
- *End on <host>* (`shell/host_end.rs`) still runs its attach and the host's end in the shell,
  closing the session watching first through the drive; what a failed attach reads as is core's
  (`HostFailure::said`).
- How a host session that cannot be attached begins (which failures read the kept copy, and which
  of those end on why) is still decided in `chat/session/host.rs`, beside `offline_start`.
- Showing a host session already watched rather than opening it twice, and which projects each
  page lists, are still the shell's (`shell/sessions.rs`).

The second, issues:
- One issues file is `FileDrive` (`onehand_core::issues::drive`), one per file for the process:
  the read and the token that drops an older one, a change shown before its write and what comes
  in meanwhile held (`Pending`, moved from the Issues views), a write landing (a sync after it, the
  newest refused read again), when a sync is due and the one that follows a sync asked for while
  one ran, and an issue sent to the forge one at a time. It is told the time. A change is named (`Change`: create, edit, open or close,
  resolve, keep in step) and written through its `Write` effect, and what the write and a publish came to is said as a `Signal`
  (`Wrote`, `PublishFailed`), which only the view that made the change or pressed *Publish* acts
  on. The Issues views' `IssuesFile` is its carrier, and emits those signals. Which issue may be published (`publishable`) and the session working an issue
  (`LocalIssue::working_in`) are core's; which projects a run is offered on is the workspace
  drive's (`offered_for_runs`).
- The carrier has contract tests (`crates/issues/src/view/store/tests.rs`).

Left for later, with phase 9's Workbench modes (moved there, as phase 9 records):
- What the Issues views read of a pull request, the work left and a review, their filters, the
  template's fill and the close question stay in the views; so does which landed write moves a
  view's selection (`IssuesView::heard`: only the newest change, nothing picked since, no form
  open), the selection being the view's.
- `Issues`' methods stay public: a tool creating an issue, an unattended run's notes and the
  issue work's note write the file outside the drive, under core's lock, and the views read them
  again when told the file may have changed.
- When a session's note on an issue it took up is written (once, after the agent names the
  conversation) is still the shell's (`shell/issue_work.rs`).

**Phase 9: Workbench modes.** Only modes with behaviour get a drive; a mode that only shows
something keeps its view (D30). Two PRs (D33). The first, the contract and two modes:
- `onehand-workbench`'s contract splits (D32): what a mode is apart from GPUI (`ModeId`,
  `WorkbenchModeSpec` and where a mode is offered, `Request`, and whether the Workbench attaches
  again to a host, `workbench::again`) is `onehand_core::workbench`; `onehand-workbench` keeps the
  GPUI mount (`WorkbenchMode`, `Ask`) and re-exports the rest. A reading size travels as pixels in
  an `f32`. Every mode, the panel and the Issues page match `Request` whole, with no fall-through
  arm, so a new request is a compile error in each until it is decided there.
- The Editor's open files are `EditorDrive` (`onehand_core::editor::drive`), one per mode, as the
  mode is one per window (D31): every project's tabs, what each buffer was read through, the
  folders that may not be written and what stands to be said about a save, in the model it derefs
  to (`Editors`). It decides a read landing as a tab, a save and the one asked while it runs, a
  conflict and the overwrite it arms, a locked folder, and a close that would lose edits.
  `RootEditors`' fields are `pub(crate)`. The text is the front end's: a write takes the buffer's
  text when it is carried out, and whether the buffer still holds what landed comes back with the
  answer (`Input::Saved { still }`). `EditorView` is its carrier.
- The Plugins mode is `PluginsDrive` (`onehand_core::claude_plugins::drive`), one per mode: when
  the list is read (as it is drawn, never while a change runs) and the token that drops a read
  about the project before, one change at a time, a refusal said in its project, what went through
  counted for the restart offer, and the scope the next install takes. It is told the time a
  change lands. Claude Code's command line and a plugin's inventory moved into core beside it
  (`claude_plugins::cli`, `claude_plugins::inventory`). `PluginsView` is its carrier.
- Files, Markdown and Neovim keep their views: a tree's listings and a document's index are reads
  of the disk with what is open being the viewer's, and Neovim runs one program per project as the
  terminal dock runs its shells.
- Characterisation tests written first pin the Editor's saves, the Plugins mode's changes (against
  a `claude` of the test's own on `PATH`) and which issue write moves a view's selection. Core's
  drive tests hold every rule moved; the carriers' contract tests are in
  `crates/editor/src/view/tests.rs` and `crates/claude-plugins/src/view/tests.rs`.

The second, the Issues views' own rules:
- What one Issues view reads of the work on its issues is `WorkDrive`
  (`onehand_core::task::work::drive`), one per view, the tab's or the page's: when the issue on
  screen's pull request is read (never read for it, opened, its work moved, or the window back with
  what is shown older than `READ_AGE`), when the page reads every project's and looks up what a
  capped read missed, when what the work left is read, and every answer kept to what asked for it.
  It is told the time and which issue is shown, and keeps the work on every issue the app tells it.
- A press on the review block goes through it: it is sent where the block read the run waiting
  (`Answered::Sent` with a `Press`, which the view hands to the shell), reloads what the run waits
  on now (`Reloaded`), or is refused; an empty note is not sent. How the block is drawn (the whole
  answer, the acceptance, the note being written) stays the view's.
- A template's fill (`template::fill`), whether closing an issue asks first
  (`IssueWork::closing_asks`) and which landed write moves a view's selection
  (`issues::drive::moves_selection`) are core's; the views call them.
- Core's drive tests hold every rule moved, and the views' carrier has a contract test
  (`crates/issues/src/view/reads/tests.rs`): what the work left read off the UI thread, its answer
  reaching the drive once and kept for its work. Only `heard` was pinned before the move; the other
  rules were pinned by their drive tests in the commits that moved them.

Left for later:
- The carrying of the pull request reads has no contract test: the forge connectors are the ones
  built in, and core's fake one is not reachable from the views' crate.
- `Issues`' methods stay public, for the writers outside `FileDrive` (a tool, an unattended run,
  the issue work's note).
- The Issues views keep a timer that asks the file to sync every few minutes; whether a sync is due
  is `FileDrive`'s.
- A diff opened in the full form is read when opened, as the Files tree reads a folder: what is
  open is the viewer's.
- Which tab is on screen (`RootEditors::active`) is view state kept in core's model, as the
  workspace's panel layout is.
- The drives are told the time in three shapes (`SystemTime` in a save's answer, `Instant` for a
  plugin change, seconds for the Issues views' reads) rather than one `Now`; and the Editor's tab ids
  come from a process-wide counter, so replaying its inputs gives the same effects but for those
  ids.
- `claude_plugins::cli` moved into core as it was, past the size a file is split at; it was already
  past it before the move.
- The panel still decides, from the mode's declaration, which mode shows after a project switch
  and which one answers an open file; both are which view is on screen.

**Phase 10: cross-drive flows.** One PR (D37). The audit found three hops between drives still
routed in the app, and moved them into core before testing the flows (D34):
- A start off screen, a schedule's or a tool's, was decided in `control::start_task`: what blocks
  it, whether a worktree is cut first, the task it is kept as (D35). `Launch::off_screen` builds it
  from `task::Starting` (moved from the clock's drive), refused by every blocking finding said
  together, and `TasksDrive::start_off_screen` cuts (`Effect::Cut`), keeps the project in the
  window without putting it on screen (`Effect::Keep`, answered at once with whether the window is
  open), keeps the task with its origin and grant, and asks for its place (`Effect::Ask`, which
  `unattended::link` asks, held to the cap). The preflight's facts are still read in the window.
- Where a task asked for lands was said by the tasks' carrier after the shell drove it, and what
  its session is given by the shell. The drive is told `Input::Driven` and says `Effect::Told`
  itself, `Input::NotDriven` tells the window closed, and `Begin::Session` carries the grant and
  an issue's opening lines, which the shell carries out as given.
- What a run tells unattended runs (waiting, answered, its agent up, a pause) was four calls the
  run's carrier made, reading the session's name itself. The run's drive says
  `Effect::Unattended(Said)`, its agent up with its conversation's name, and
  `unattended::link::Joined::run_said` hands each to its command; `Said::looks` says which need the
  projects and the world read.

The flows are tested in `onehand_core::task::flows` (D36): a harness holds every drive, joined by
its links, and carries each effect once, its I/O done by core's blocking bodies on a scratch
repository or answered as a front end would; an effect a flow never reaches fails the harness.
- A schedule comes due; its task starts off screen on the checkout, its run prompts its session,
  a look while it works starts nothing, its turn ends and the run is done, and the next look starts
  it again.
- A tick claims a labelled issue held to the cap of one; its run on the claim's worktree opens with
  its lines and prompts its session, a second tick claims nothing, the agent commits and ends its
  turn, the project is dropped, the issue is told once, and the slot freed claims the next issue.

Characterisation tests written first pin a start off screen on a real shell
(`crates/app/src/control/start/tests.rs`): refused by what blocks the launcher with no task kept,
and kept with its origin and the first agent, its project kept but not put on screen, and the
caller told where it landed. Core's drive tests hold every rule moved.

Left for later:
- Cutting a worktree (`cut_worktree_blocking`) is still a blocking function in the app's shell;
  the flows start on the checkout, and the harness fails a cut.
- `Effect::Keep` is carried by the tasks' carrier on the window's `WorkspaceDrive`: the two drives
  have different owners (the process, a window), so no link joins them in one call.
- The flows do not reach a pick by hand, a review answered, a check, a command step, the forge
  steps, a session's own saves or its telling; each has its drive's tests.
- `unattended::link::Joined::settle` and `control::link`'s test still have arms that match the rest
  of an enum (`other =>`, `_ =>`).
- The harness's drive of a task (`Flow::drive`) restates the shell's `drive_task` in its order,
  rather than testing it: the shell needs a window.
- Smells the review noted and left: `task, launch, window` travel together in `Cut` and `Keep`;
  a cut worktree is a bare tuple (`tasks::Cut`); `schedule::drive::Starting` only re-exports
  `task::Starting`; `submit_told` lives on for the app's tests only.
- `Task`'s fields stay public (phase 4): no app code builds a task by its fields any more, every
  start's task being built in core, but the app's tests do, and readers everywhere read them.

**Phase 11: what the phases left.** Re-audited on 2026-11-11 against the code after phase 10
(D38-D41). Every item each phase left for later is moved, tested or fixed, in five PRs by area,
each with its own docs and characterisation tests first; the by-design items move too (D40).
Already done and struck from the lists: the pane folding a host wait's failure (phase 6, now the
drive's `wait_failed`/`back_failed`), the pane telling a stream's ends apart (phase 6, now
`Signal::Disconnected(Ended)` passed on whole), and `claude_plugins::cli`'s size (phase 9: 568
lines of code, under the line).

PR 11a, tasks and unattended runs:
- whether a press still answers what a run waits at (`task/approval.rs`) into core;
- what answering a review is judged on (`read_review`, `unattended/launch/answer.rs`), as a core
  blocking body the drive asks for;
- cutting a worktree (`cut_worktree_blocking`) into core, so the flows' harness cuts too;
- what a retry, a retry with current settings and a review's answer do next (`shell/retry.rs`);
- what a person pressed and is still being done (`acting`) into the tasks' drive;
- *Remove worktree…* (`shell/cleanup.rs`) as the tasks' drive's, so a window closing mid-removal
  loses nothing (#319, its own commit);
- the workflows named for unattended runs saved through a drive, not `saves::write_config`;
- the run's project kept for a start off screen (`Effect::Keep`) joined to the window's workspace
  drive by a link;
- a claim's run brought up through `drive_task`, tested on a real shell; the flows' harness drives
  through what the shell does rather than restating it;
- phase 10's smells: `task, launch, window` as one type, a cut worktree named, the re-export of
  `Starting` dropped, `submit_told` moved into the tests.

PR 11b, sessions and hosts:
- what a session starts on (`pending_resume`, `pending_watch`, `pending_fork`) and which goes first
  (`ChatPane::show`) into the session's drive, the stored transcript read off the UI thread (it is
  read inline today, its own commit);
- how a host session that cannot be attached begins (`chat/session/host.rs`), and *End on
  <host>* (`shell/host_end.rs`) into `host::session`;
- waiting out a busy host and attaching again (`hosts::attach_waiting`) into `HostDrive`;
- each window's share of a host's waits (`HostFlow`) into a drive;
- showing a host session already watched rather than opening it twice (`shell/sessions.rs`);
- a fork that failed (`ForkFailed`, `shell/boot.rs`) and whether a restart is said
  (`restart_told`);
- the pane's answer to a session's look (`ChatPane::look`) tested.

PR 11c, the workspace and the shell:
- the recents list touched, named and forgotten (`shell/storage.rs`) into a drive;
- when a session's note on an issue it took up is written (`shell/issue_work.rs`) into a link of
  the session and the issues;
- which window and drives each tool call goes to (`perform_now`) as core's;
- opening a dropped run's conversation (`open_conversation`), *Run check* on the active project,
  and the Issues page's starts that find nothing (`shell/issues_page.rs`), each said;
- a second launch's choice of window and its wait for the first (`instance.rs`);
- an agent's *Test* (`check_agent`) into the settings' drive;
- a worker's open carried through a window, and an agent check's probe, tested.

PR 11d, issues and the Workbench:
- `Issues`' writers outside `FileDrive` (a tool, an unattended run's notes, the issue work's note)
  made through its `Change`s, and its methods `pub(crate)`;
- the Issues views' sync timer into `FileDrive`'s clock;
- which mode shows after a project switch and which answers an open file into core's
  `workbench`;
- the pull request reads' carrying tested, with core's fake connector reachable under a test
  feature.

PR 11e, across the drives:
- `Task`'s fields `pub(crate)` behind read accessors;
- one `Now` for every drive, and the Editor's tab ids handed in, so a replay gives the same ids;
- no arm in a link or a drive, or their tests, that matches the rest of our own enum;
- the proposal leaves `docs/proposals/`: its standard is `docs/drives.md`'s already, and its
  decisions move there or into the feature documents.

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

Made on 2026-11-10, with phase 7's audit:
- **D21. Telling: the drive asks, the front end answers.** Presence is read when the news comes,
  not pushed into every session at each change of focus.
- **D22. The remote bridge reaches only sessions that run here** (#308): a host or offloaded
  project's session is not listed, followed, prompted, answered or announced.
- **D23. Three drives, one PR**: remote, tools and settings, each beside its model. The tools'
  drive is held in a process lock rather than on `Shared`: a token is minted and let go without a
  window, and the socket's thread reads standing on every message.
- **D24. Outside control's value is the tools' drive's**, which the socket reads; its save and
  put-back are the settings' drive's.
- **D25. No drive for a second launch.** Its runtime half moved to `onehand-io` with the socket the
  tools share.

Made on 2026-11-10, with phase 8's audit:
- **D26. One drive for the workspace**, the session lifecycle and the edits both: a session lives
  inside its project, and two drives would change one model.
- **D27. Held per window on the `Shell`**, as the workspace was.
- **D28. A host session's rules move; `HostFlow` stays** the window's bookkeeping, carrying what
  core decides. `hosts::attach_waiting` serves no session and stays `HostDrive`'s carrier.
- **D29. Two PRs**: the workspace first, then an issues drive for the shared issues file; the
  Issues views' own reads wait for phase 9. The bugs the audit found are fixed in the phase, each a
  commit of its own.

Made on 2026-11-11, with phase 9's audit:
- **D30. Drives for the Editor and the Plugins mode only.** Files, Markdown and Neovim keep their
  views: they show what is on disk or run a program, and decide nothing that outlives the view.
- **D31. Each drive is held by its mode**, one per window's Workbench, as the modes were. The
  restart offer still counts the changes made in its own window.
- **D32. The contract's GUI-free half moves into a core module**, `onehand_core::workbench`, rather
  than a crate of its own; `onehand-workbench` is the GPUI mount and re-exports it. The panel's one
  rule about a host, whether to attach again, goes with it (`workbench::again`).
- **D33. Two PRs**: the contract and the two drives first, then the Issues views' own rules.

Made on 2026-11-11, with phase 10's audit:
- **D34. Phase 10 moves the routing still in the app before testing the flows:** a start off
  screen, where a driven task lands and what its session is given, and what a run tells
  unattended runs. What needs a window (the preflight's facts, a session started, a project kept)
  stays the carriers'.
- **D35. A schedule's or a tool's task is built in core** (`Launch::off_screen`), as an issue's is
  (D16).
- **D36. One harness for the flows**, in `task::flows`, one file per flow, rather than a rig per
  link.
- **D37. One PR** for the moves, the flows and the docs.

Made on 2026-11-11, with phase 11's audit:
- **D38. Phase 11 takes every leftover of phases 4 to 10**: the rules still in the app, the bugs
  found (#319, a transcript read inline), the untested carrying, and the clean-up.
- **D39. Five PRs by area**: tasks and unattended runs, sessions and hosts, the workspace and the
  shell, issues and the Workbench, then what runs across the drives; each with its docs.
- **D40. The items earlier phases called view state or a carrier's own move too**: what a person
  pressed (`acting`), each window's share of a host's waits, an agent's *Test*, a second launch's
  window, the Issues sync timer, the Workbench's mode choice and a start off screen's kept project.
- **D41. The proposal leaves `docs/proposals/` with phase 11's last PR.**

Phase 10 changed nothing on purpose: a start off screen, a drive's answer and a run's word to
unattended runs reach the same places in the same order. A start off screen refused (by its
preflight, a failed cut, or its window gone) now reaches `control::start_task`'s caller as a
landing (`Placed::Refused`) rather than an error; the tool and the clock both read the two alike. The one difference is unreachable before:
a driven task gone from the drive between its begin and its session is no longer refused as gone.

Phase 9 changed, on purpose:
- A save that wrote nothing (the file changed on disk, or is gone) is said under that project's
  files only; before, the line stood over whichever project was on screen until a save or an open
  answered it.
- A file whose read lands after its project left the workspace opens nothing and says nothing;
  before, it brought the project's tabs back, or said its failure.
- A read of the Plugins list made before the project on screen changed is dropped even when the
  person has come back to that project; the next draw reads it again.

Phase 8 changed, on purpose:
- Going back to the header's history (offered only between turns) keeps the session that was live
  running, out of sight, until a past conversation is chosen; before, its agent was dropped the
  moment the list was read. A list read for a session restarted or closed meanwhile puts nothing
  up, where before it took the new session off its conversation.
- A session waiting on a busy host's sessions is no longer told as a lost agent when its stream
  ends (no badge, desktop notice or remote bridge word).
- A project removed takes the worktree and branch forms about it, and a `git status` sweep under
  way no longer puts its status back.
- An order put back after a failed write tells a local and a host project on one path apart.
- A failed note on an issue a session took up is said in its window, once; before, it was only
  logged.

Phase 7 changed, on purpose:
- The remote bridge no longer lists or reaches a host or offloaded project's session (#308): a
  session is reached when its project runs here (`ProjectRoot::runs_here`).
- A session's badge goes with its drive: a restart of its agent starts it unread.
- A worker's 120 seconds run from the call, as control.md says, so they can run out while its
  folder is still read; before, they were looked at only once its session had started.
- The cap of four counts the sessions a lead's tools opened in every window, not only the lead's.
- A session's connections hear that its tools changed once the call that switched, granted or
  closed it is over, not inside it.

Phase 6 changed, on purpose:
- Putting the offload form away no longer stops a send under way: it runs to its end and keeps
  what went for the next offload. The project stays here, as before.
- An errand of an offloaded project that panics answers as a failure, *Something went wrong here; try
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
