# Drives and carriers

How onehand keeps what it does apart from how it is drawn. The model says what is true; a
**drive** decides what happens next; a **carrier** carries that out in one front end; the UI
draws. A new front end rewrites its UI and its carriers, and keeps every drive, the model and
their tests.

This is the standard new code follows. Code written before it is being moved to it phase by
phase; the order, and what each phase moves, is in
[proposals/application-layer.md](proposals/application-layer.md).

## The layers

| Layer | Where | Knows | Must not know |
|---|---|---|---|
| Model | `crates/core` | itself | anything above it, the clock, I/O |
| Drive | `crates/core`, beside its model | the model | I/O, threads, timers, a runtime, GPUI |
| Carrier | each front end (GPUI's are in `crates/app`) | drives, the front end | what happens next |
| UI | `crates/app`, `crates/ui`, the Workbench modes' views | everything below, to read | the order of anything |

The drives so far:

| Drive | What it decides | Its carrier |
|---|---|---|
| `onehand_core::chat::drive::SessionDrive` | one session | `ChatSession` (`crates/app/src/chat/session.rs`) |
| `onehand_core::task::drive::RunDrive` | one run | `crates/app/src/task/carrier.rs` |
| `onehand_core::task::tasks::TasksDrive` | every task and the place each works in | the `Tasks` global (`crates/app/src/task.rs`, `task/place.rs`) |
| `onehand_core::unattended::drive::UnattendedDrive` | unattended runs: the tick, the claim, the cap, a run's reports | `crates/app/src/unattended.rs`, its drive held in the `Tasks` global |
| `onehand_core::schedule::drive::ScheduleDrive` | the clock schedules run by | the clock in `crates/app/src/schedules.rs` |
| `onehand_core::host::drive::HostDrive` | one attach per host, what is known of each, the waits on a busy or lost host | `crates/app/src/hosts.rs`, its drive held on `Shared`, the attach run by `onehand_io::host` |
| `onehand_core::host::offload::drive::OffloadDrive` | every offloaded folder: who holds its code, its moves, the poll, a branch's updates | `crates/app/src/offload.rs`, its drive held in the `Offloads` global |
| `onehand_core::remote::drive::RemoteDrive` | the remote bridge: a line or a press from a chat, what it asks of a session (`remote::link`), what is said back, whether the person is away | `crates/app/src/remote.rs`, its drive held on `Shared`, the channel run by `onehand_io::remote` |
| `onehand_core::control::drive::ControlDrive` | onehand's tools: tokens and standing, each session's switch and grant, the cap, a worker's open | `crates/app/src/control.rs`, `control/workers.rs`, its drive held in one process lock, the socket served by `onehand_io::socket` |
| `onehand_core::config::drive::SettingsDrive` | the agents, the appearance, outside control's save, what each agent offers and its check | `crates/app/src/saves.rs`, its drive held on `Shared` |

## Rules

### 1. Every change to application state goes through a drive

There are no exceptions, not even for a change that only forwards to the model and answers no
effects. An exception list is a judgement every contributor makes differently, and a feature
that is pure today and gains I/O tomorrow changes inside its drive while its callers stay.

Telling is a session's too: which of its signals is news, the badge on its row and where the news
is said are `SessionDrive`'s, which asks where the person is (`Effect::Look`) and is answered at
once (`Input::Seen`).

A front end has no way around this rule for a session: `SessionDrive` hands out no mutable
`Chat`, and the app changes a session only through the drive's methods. Inside core no drive
changes another drive's model either (rule 10).

### 2. Reads go to the model

A drive derefs to its model (`SessionDrive` to `Chat`) and re-exports no getters. A getter
that only forwards buys nothing, and hundreds of them would bury the drive.

### 3. Application state and view state are told apart by what they mean

**Application state** is a fact about the work: what onehand does, what an agent receives, how
something ends, what any entry point reads as true. **View state** is how one viewer looks at
that work. Ask, in order:

1. Does changing it change what onehand does: a prompt sent, a step woken, a file written, an
   outcome, something told to a person? Then it is application state.
2. Must two front ends showing the same session at once agree on it? Then it is application
   state. Two windows may scroll one transcript differently, so a scroll position is view state.
3. Who sets it, a rule or the viewer's act of looking? A rule's value is application state;
   focus, what is on screen and what is open are view state.

**Persistence is a separate axis.**
- View state may be kept across restarts, in the front end's own store, which no drive reads.
- Application state may live in memory only, as a turn running or a card waiting does.

**A view fact that behaviour depends on enters as an input.** The UI keeps owning it, and the
drive is told it. For example, whether the composer still holds the words a prompt was sent
with comes back with the code send's answer (`Input::CodeSent { still }`). The drive never keeps
view state of its own.

| Application state | View state |
|---|---|
| a queued prompt, a card and its answer, a setting, a session's name, a run's step, the agent's picks | which sections, turns, tools and thoughts are open; scroll positions; focus; parsed markdown and decoded images |

### 4. The shape of a drive

```rust
// Commands, from the UI, the remote bridge, onehand's tools or another drive's link: methods.
fn prompt(&mut self, text: &str, staged: &[StagedAttachment], origin: Origin, code: Code, now: Now)
    -> (Intake, Vec<Effect>);
fn stop(&mut self) -> Vec<Effect>;
// What the world reports back: one `Input` enum.
fn handle(&mut self, input: Input, now: Now) -> Vec<Effect>;
// A stream, one event at a time, and the end of the batch it came in.
fn apply(&mut self, event: AcpEvent, now: Now) -> (Vec<Effect>, Next);
fn end_batch(&mut self) -> Vec<Effect>;
```

- **Commands are methods.** A command returns what its caller needs at once (an `Intake`) and
  effects for the rest. Who sent it is an argument (`Origin`), and what differs by sender is
  one table inside the drive, not a copy at each caller.
- **Reports are an `Input`.** The answers to effects come back through `handle`, and so do
  timers and a stream's end. Replaying a recorded sequence of inputs gives the same effects.
- **A stream is folded one event at a time.** Where a batch may end is behaviour, so `apply`
  answers it (`Next`); how many events a batch takes at most is the carrier's cap.
- **Effects are ordered.** "Do A, then B once A answers" is A now, and B on A's input.
- **A drive never reads the clock.** A wait is an `Arm` effect answered by an `Elapsed` input,
  and what may stamp or time something is told the time (`Now`), which the carrier reads.
  Its model reads no clock either (rule 12).
- `Effect` and `Input` need not be `Clone` or `PartialEq`; tests compare what they need.

### 5. A late answer is handled as its effect declares

An answer can arrive after the drive has moved on, and the act it answers may still have
happened: a save may have written, code may be on its host, a command may still be running.
Each effect that is answered declares, in its own doc, which of these its late answer gets:

| Late answer | When | Example |
|---|---|---|
| **Drop** | the act left nothing behind | the quiet wait, superseded by a later one (`Arm`/`Elapsed`) |
| **Record** | the act happened and must be known | a save that failed is handed back to the next; code that went up is up |
| **Clean up** | the act left a resource nobody holds | a temporary file, a started process |
| **Await** | something stays held until the act ends | a command called off frees its place only when it exits |

- Only a *drop* answer needs a token to tell it apart: a *record* answer carries what it is
  about.
- Calling an act off is its own effect, and never stands in for the answer that says it is over.
- Closing honours the answers still on their way. A session closed while a save is in flight
  cannot wait for it, since its drive goes with it; so it writes the whole transcript instead,
  and the store drops the lines of any save built before that write lands. Restarting a session
  with a save in flight does the same: the snapshot handed on says so, and the session that
  carries on writes the whole transcript at its first save.

### 6. A failure is an input

Every effect that can fail is answered with a `Result`. What a person is told about a failure is a signal the
drive gives (`Signal::SaveFailed`, `Signal::CodeFailed`), said once at the edge of a run of
failures; the words and where they appear are the front end's.

### 7. A carrier carries out, and decides nothing

A carrier does these things, and nothing else:
- holds its drive;
- pumps a stream into it;
- carries out its effects in order;
- reports each result back;
- defers what would update another entity, or one already being updated.

An `if` about what happens next in a carrier is a rule in the wrong place.

A drive's effects may need more than one part of a front end. `SendCode` needs the composer and
the window, which the session entity does not have. So `ChatSession::perform` carries out what
it can and hands the rest back to the pane (`ChatPane::carry`). Whichever part carries an
effect, it is carried exactly once.

### 8. The compiler holds the rule

A model's methods that change application state are `pub(crate)`, so only drives in core can
call them, and core exports drives and the model's read side. A call from the app to
`Chat::submit` does not compile, and `dead_code` finds a mutator no drive uses.

Built for `Chat`, `Task`, the task queue and the workflow engine's `Run`: every method that
changes them is `pub(crate)`. `Task`'s fields are still public, for the app to build a task with,
though an issue's task is built in core (`Task::of_issue`). Unattended runs, the clock, hosts
and offloaded projects keep their state in their drives' private fields; an offloaded folder's
entry is read through its drive, never handed out to change. The remote bridge's chats, the tools'
state and the settings are kept in their drives too: `Chats`' mutators are `pub(crate)`, the tools'
state is private to `ControlDrive`, and `SettingsDrive` derefs to its settings without `DerefMut`.
A schedule is the file's data, and
what the clock or a switch writes onto it is decided by core's functions the carrier runs in its
writer (`schedule::merge_looked`, `schedule::switch`). The other models are not there yet; each
phase that moves one makes its mutators `pub(crate)`.

No guard backs this up yet: a guard is added once the same mistake has been made in several
places, as every guard in `crates/app/src/guards.rs` was.

### 9. A drive sits beside its model

`SessionDrive` is `core/src/chat/drive.rs`; a run's drive is `core/src/task/drive.rs`, the
tasks' `core/src/task/tasks.rs`, unattended runs' `core/src/unattended/drive.rs`, the clock's
`core/src/schedule/drive.rs`, hosts' `core/src/host/drive.rs` and offloaded projects'
`core/src/host/offload/drive.rs`, the remote bridge's `core/src/remote/drive.rs`, the tools'
`core/src/control/drive.rs` and the settings' `core/src/config/drive.rs`. The
repository is cut by feature, and there is no `application/` folder to split each feature in
two.

### 10. Drives meet in a link

A drive never holds another. Where one drive's effects become another's commands, and one's
signals another's inputs, is behaviour too. So it is a GUI-free module of core tested with the
real drives on both sides, as `task::drive::link` joins a run and its session,
`task::tasks::link` a run and its task, and `unattended::link` unattended runs and the tasks (every
start held to the cap, a claim kept as a task, a run's end telling its issue). A drive that reads
the tasks reads them with the runs under way through `task::tasks::link::Seen`, which says what
each task is doing and who holds a slot. Which of a session's signals checks an offloaded
project's code is `OffloadDrive::session_said`'s, and how a host stands, the offload poll is
told as it comes round (`HostSeen`). Outside control's value is the tools' drive's and its save
the settings', joined in `control::link`. A GPUI subscriber
that routes between drives is a rule in the wrong place.

**A drive reads another drive's model, and never changes it.** The run's drive reads its
session's `Chat` and decides the mode and the settings picks to put in force; it keeps them, and
the link makes each through `SessionDrive::pick` before anything else reaches the run.

### 11. Every carrier is tested against its contract

A few tests per carrier, with a fake source of events, and GPUI's test context where the carrier
is GPUI's (`ChatSession`'s are in `crates/app/src/chat/session/tests.rs`). They check that:
- effects run in the order given, each exactly once;
- every failure reaches the drive, and none is swallowed;
- each late answer gets the handling its effect declares;
- after a session closes, nothing of it stays running and no effect runs for it;
- deferred work runs after the update that asked for it, never inside it.

Not built yet for every carrier or every check. `ChatSession`'s tests hold order and exactly once,
requests reaching the agent in order, a failed save reaching the drive, the quiet wait and its
cancel at the stream's end, a close with a save in flight and a wait armed, and a watch ended by
its host's lost connection said as out of reach, one taken by another machine ended. The run's and
the tasks' carriers' tests (`crates/app/src/task/tests.rs`) hold a run's prompt going once, its
failed writes reaching its drive and said once, a session closed under a run leaving nothing
listening, a finished run hearing nothing more, the place held while a cancelled turn drains, a
Stop before a command starts, and a dismiss put back when its write fails. Hosts' carrier
(`crates/app/src/hosts/tests.rs`) holds a wait's end reaching every listener once and the last
listener going ending the wait and its clock; offloaded projects'
(`crates/app/src/offload/tests.rs`) an errand's answer landing once, the poll's clock coming round
and armed again, an answer reaching its asker once, a late answer for a folder let go landing on
nothing, and keeping a kept folder again telling nobody, so the window's observer settles.
Unattended runs' carrier (`crates/app/src/unattended/tests.rs`) holds its clock coming round and
armed again, a start over the cap refused where it was asked, a pause, a config that cannot work,
and a report sent once and dropped once it lands; the clock's (`crates/app/src/schedules/tests.rs`)
its first look after its wait, a file it cannot read said, a look that moves nothing writing
nothing, and a failed switch put back. The remote bridge's carrier (`crates/app/src/remote/tests.rs`) holds a line answered once, a
session no window holds said gone, a scan landing once off the UI thread and nothing told after the
channel ends; the tools' (`crates/app/src/control/tests.rs`) a switch told to each connection
once, after the call, and an open's deadline coming round on the clock; the settings'
(`crates/app/src/saves/tests.rs`) a write landing once, a failed appearance put back and the agent
list kept. Still untested: the pane's answer to a session's look (`ChatPane::look`), a worker's
open carried through a window (its start, watch and first prompt), an agent check's probe, which
starts a real agent, `SendCode`'s late answer and the pane's half
of the session (`ChatPane::carry`), `SaveTitle` (written on the ordered writer's own thread, which
GPUI's test scheduler does not wait for), deferred work, the forge calls (the connectors are the
ones built in), a command called off while it runs (the test scheduler runs it to its end before a
Stop can reach it), and what needs a window's shell: a claim's run brought up, a schedule's start,
a run's project kept or dropped; and a host's real attach, which runs on a runtime GPUI's test
scheduler does not drive.

### 12. A model does no I/O and reads no clock

A model changes itself and answers; it sends nothing, writes nothing, and is told the time rather
than reading it. `Chat` queues what is to go to its agent, which its drive hands on as
`Effect::Request`, and stamps and times by the `Now` its drive was told. One thing in it still
does I/O: the save its drop writes when a session closes, synchronously, on the thread that
drops it.

## A new feature

Before any GPUI code is written:

1. Name the application state the feature adds, and check it against rule 3.
2. Pick the drive it belongs to, or add one beside its model.
3. Write its commands, inputs and effects in core, and their tests. Declare each answered
   effect's late answer (rule 5).
4. Carry out its effects in a carrier, add no rule there, and give the carrier its contract
   tests (rule 11).
5. Draw it.

A feature whose drive method only forwards to the model still does steps 2 and 3.
