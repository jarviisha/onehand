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

The first drives are `onehand_core::chat::drive::SessionDrive`, one session, and
`onehand_core::task::drive::Drive`, one run. `ChatSession` (`crates/app/src/chat/session.rs`)
is the session's carrier, and `crates/app/src/task/driver.rs` the run's.

## Rules

### 1. Every change to application state goes through a drive

There are no exceptions, not even for a change that only forwards to the model and answers no
effects. An exception list is a judgement every contributor makes differently, and a feature
that is pure today and gains I/O tomorrow changes inside its drive while its callers stay.

`SessionDrive::chat_mut` is the one way around this rule. It exists for code that has not moved
yet, and new code adds no call to it. Two kinds of call remain: the app's, which phase 3 gives
methods, and `task::drive::link`'s, which hands the run's drive its session's `Chat` to put the
step's mode and picks in force (rule 10).

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
fn prompt(&mut self, text: &str, staged: &[StagedAttachment], origin: Origin, code: Code)
    -> (Intake, Vec<Effect>);
fn stop(&mut self) -> Vec<Effect>;
// What the world reports back: one `Input` enum.
fn handle(&mut self, input: Input) -> Vec<Effect>;
// A stream, one event at a time, and the end of the batch it came in.
fn apply(&mut self, event: AcpEvent) -> (Vec<Effect>, Next);
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
- **A drive never reads the clock.** A wait is an `Arm` effect answered by an `Elapsed` input.
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
- Closing honours the answers still on their way: a session closed while its turn's save is in
  flight waits for that save. Not built yet; today that answer is lost with the session (#309).

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

Built so far for what phase 1 moved: `Chat`'s `submit`, `queue`, `unqueue`, `cancel_turn`,
`flush`, `unflush`, `stream_ended`, `quiet_spell` and `starts_quiet_save`. Every other mutator
(`apply`, `pick`, `rename`, `answer_ask` and the rest) is still `pub`, reached through `chat_mut`
until its phase moves it.

No guard backs this up yet: a guard is added once the same mistake has been made in several
places, as every guard in `crates/app/src/guards.rs` was.

### 9. A drive sits beside its model

`SessionDrive` is `core/src/chat/drive.rs`; a run's drive is `core/src/task/drive.rs`. The
repository is cut by feature, and there is no `application/` folder to split each feature in
two.

### 10. Drives meet in a link

A drive never holds another. Where one drive's effects become another's commands, and one's
signals another's inputs, is behaviour too. So it is a GUI-free module of core tested with the
real drives on both sides, as `task::drive::link` joins a run and its session. A GPUI subscriber
that routes between drives is a rule in the wrong place.

Not met yet in one place: the run's drive is handed its session's `Chat` through the link and
changes it directly (`Chat::pick`, `Chat::set_mode`), rather than through `SessionDrive`'s
methods. Phase 4 decides whether a drive may change another drive's model.

### 11. Every carrier is tested against its contract

A few tests per carrier, with a fake source of events, and GPUI's test context where the carrier
is GPUI's (`ChatSession`'s are in `crates/app/src/chat/session/tests.rs`). They check that:
- effects run in the order given, each exactly once;
- every failure reaches the drive, and none is swallowed;
- each late answer gets the handling its effect declares;
- after a session closes, nothing of it stays running and no effect runs for it;
- deferred work runs after the update that asked for it, never inside it.

### 12. A model does no I/O

A model changes itself and answers; it sends nothing and writes nothing. Two things in `Chat`
still do I/O, and are moving into effects:
- the requests it sends to its agent (`Chat::submit`, `Chat::cancel_turn`);
- the save its drop writes when a session closes.

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
