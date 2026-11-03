# Remote hosts, step 1a: the host service and its client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A host service (`onehand-host`) that runs ACP sessions on a machine reached over SSH and
keeps them running while nobody is attached, plus the GUI-free client in core that installs it,
attaches to it, and turns a host session back into the same `Stream<Item = AcpEvent>` a local
session gives. No app change beyond two exhaustive match arms; the app wiring is step 1b.

**Architecture:**
- **The journal is the truth.** The service drives each session through core's own
  `acp::connect`, on the host, so the agent's `fs/*` and `terminal/*` requests are served there.
  It appends everything that shapes the transcript to an append-only journal per session: what the
  agent sent (`AcpEvent`) and what the person asked (`AcpRequest`).
- **Attaching replays the journal, then goes live.** The laptop-side stream maps journal entries
  back to `AcpEvent`s, requests included, as a new `AcpEvent::Echo`. The laptop's `Chat` is
  rebuilt by `Chat::apply` alone, with no new path in the app.
- **One stream per attach.** It is `ssh <host> ~/.onehand-host/<build>/onehand-host attach`, which
  relays stdio to the service's Unix socket and starts the service first when none is running.

**Tech Stack:** Rust 2021, tokio (process, net, io-std), futures, serde/serde_json, the system
`ssh` binary.

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md` (step 1 of *Order of work*;
this plan is its first half).

## Global Constraints

- `onehand-host` never links GPUI. `cargo tree -p onehand-host -i gpui-pre` must fail with "did
  not match any packages". The same holds for `onehand-core` as before.
- Core dictates no async runtime beyond what `acp::connect` already does: async fns and streams
  that need a tokio runtime to be polled on. The caller owns that runtime.
- Nothing on the host listens on the network. The service binds only a Unix socket, mode `0600`,
  under `~/.onehand-host/data/`.
- The laptop's agent `env` is never sent: the laptop sends the `AgentSpec` with `env` cleared, and
  the host resolves it with `AgentSpec::launch` against the host's own PATH.
- The journal is append-only, one JSON line per entry. A torn last line costs that entry only.
- New core items are `pub` only if `onehand-host` or the app names them. `onehand-host` is a
  binary, so its own items stay `pub(crate)` or private.
- Code describes; it never cites: no comment or string names a document.
- Match our own enums exhaustively: no wildcard arms for `AcpEvent`, `AcpRequest`, `Call`, `Reply`,
  `Line` or `Entry`.
- Files stay under about 800 lines of code; tests go in `foo/tests.rs` beside `foo.rs`.
- Commits: `type(scope): summary`, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`,
  `make fmt`.

## Decisions this plan makes (beyond the spec)

- **Wire framing:** serde-tagged NDJSON enums with a correlation id, not JSON-RPC envelopes. Both
  ends are ours and ship together, so the envelope buys nothing. This deviates from the spec's
  "same style as the control socket"; the PR says so.
- **Build id, not version:** the install directory and the handshake use a build id, the FNV-1a
  64-bit hash of the `onehand-host` binary's bytes, in hex. Every crate is `0.1.0`, so a version
  string would let a stale dev build stay installed.
- **Requests replay, but never echo live:** on `Watch` the service sends every journal entry so
  far, `Request` entries included, then live entries **except** `Request`. The single attached
  laptop already applied its own requests locally, so echoing them live would apply them twice.
- **Attachments do not cross yet:** a `Prompt` with attachments goes to the agent without them, and
  the service journals an `Error` event saying so. Laptop paths mean nothing on the host.
- **PATH on the host:** the service starts from a non-interactive SSH command, whose PATH often
  lacks nvm, `~/.local/bin` and the like. At startup it asks the user's login shell for PATH
  (`$SHELL -ilc 'printf %s "$PATH"'`, 10 s timeout) and falls back to its own.
- **What `--local` means:** the smoke example takes `--local` to attach through
  `onehand-host attach` run directly instead of through `ssh`. That is the whole difference.

## Review Focus

Most likely to bite, with the test that pins each:

1. **Detaching mid-turn** (laptop sleeps, Wi-Fi drops): the turn carries on and every event is
   journaled. A later `Watch` replays all of it. *Task 5,* `detach_mid_turn_loses_nothing`.
2. **A card parked while nobody is attached** stays parked. After a re-attach it is replayed as
   open and can be answered. *Task 5,* `card_parked_while_detached_is_answerable_after`.
3. **Service killed mid-write:** a torn last journal line is skipped and everything before it is
   kept. *Task 4,* `torn_last_line_costs_only_itself`.
4. **A host running another build with a busy session:** the client never shuts it down and
   reports which sessions are busy. *Task 6,* `mismatch_with_busy_sessions_refuses`.
5. **A prompt with attachments:** no laptop path reaches the agent, and the transcript says why.
   *Task 5,* `attachments_are_dropped_and_said`.

---

### Task 1: Serde on the ACP data model

**Files:**
- Modify: `crates/core/src/acp/types.rs` (derives on every type; header comment: "serde-free"
  becomes "serialisable, so a host can carry it")
- Modify: `crates/core/src/acp/client.rs` (`Opening` gets the same derives)
- Test: `crates/core/src/acp/types/tests.rs` (new; add `#[cfg(test)] mod tests;` to `types.rs`)

**Interfaces:**
- Produces:
  - `Serialize + Deserialize` on `AcpEvent`, `AcpRequest`, `Attachment` and every type they
    contain. The `AcpEvent::Connected` variant is `#[serde(skip)]`, since it carries a channel.
  - `Opening: Serialize + Deserialize`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn events_round_trip() {
    for ev in [
        AcpEvent::AgentChunk { text: "hi".into(), message: Some("m1".into()) },
        AcpEvent::Permission(sample_permission()),   // rpc_id json!(7), two options
        AcpEvent::Elicitation(sample_elicitation()), // one enum field, two choices
        AcpEvent::ToolCall(sample_tool_call()),      // kind Execute, status Pending, one Text content
        AcpEvent::TurnEnded { stop_reason: "end_turn".into() },
    ] {
        let line = serde_json::to_string(&ev).unwrap();
        let back: AcpEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(format!("{ev:?}"), format!("{back:?}"));
    }
}

#[test]
fn connected_is_never_written() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let ev = AcpEvent::Connected { tx, resumed: false, forkable: false };
    assert!(serde_json::to_string(&ev).is_err());
}

#[test]
fn requests_round_trip() { /* Prompt with one Attachment, PermissionResponse, ElicitationResponse(Decline), SetMode */ }
```

Equality goes through `Debug`, because several types do not derive `PartialEq`.

- [ ] **Step 2:** Run `cargo test -p onehand-core acp::types`. Expect a compile failure: the
  trait is not implemented.
- [ ] **Step 3:** Add `serde::{Serialize, Deserialize}` derives throughout `types.rs` and on
  `Opening`. If any field's type has no serde impl, stop and report it rather than wrapping it.
- [ ] **Step 4:** Run `cargo test -p onehand-core`. Expect PASS, with every existing test still
  green.
- [ ] **Step 5: Commit:** `feat(acp): carry the ACP data model as JSON`

### Task 2: `AcpEvent::Echo`, a request replayed into a transcript

**Files:**
- Modify: `crates/core/src/acp/types.rs` (new variant)
- Modify: `crates/core/src/chat/model/apply.rs` (handle it)
- Modify: `crates/core/src/chat/model/turn.rs` (the by-rpc-id helpers, beside `answer_permission`)
- Modify: `crates/app/src/acp.rs`, `crates/app/src/agent_check.rs` (exhaustive arms only)
- Modify: `crates/core/src/acp/client.rs`, `crates/core/src/acp/client/*.rs`,
  `crates/core/src/chat/model/subagent.rs`, wherever `rustc` reports a non-exhaustive match
- Test: `crates/core/src/chat/model/tests.rs`

**Interfaces:**
- Produces:
  - `AcpEvent::Echo(AcpRequest)`: "a request this session's person made earlier, replayed from a
    host's journal. Applied to the transcript as the request would have been; never sent."
  - Applying it never sends anything on `Chat::tx`.

- [ ] **Step 1: Write the failing tests** (in `chat/model/tests.rs`, with its existing helpers
  `permission(..)` and `ask_item()`):

```rust
#[test]
fn echoed_prompt_is_a_user_message_and_a_busy_turn() {
    let (mut chat, mut rx) = connected_chat();
    chat.apply(AcpEvent::Echo(AcpRequest::Prompt { text: "build it".into(), attachments: vec![] }));
    assert!(matches!(chat.items.last(), Some(ChatItem::User(u)) if u.text == "build it"));
    assert!(chat.busy);
    assert!(rx.try_recv().is_err(), "an echo is never sent");
}

#[test]
fn echoed_permission_answer_resolves_the_card_by_rpc_id() {
    // Permission with rpc_id json!(7) and options [("allow","Allow"),("deny","Deny")]
    // then Echo(PermissionResponse { rpc_id: json!(7), option_id: Some("allow") })
    // → that card's resolved == Some("Allow"); pending_permissions() empty; nothing on rx.
}

#[test]
fn echoed_question_answer_settles_the_card() {
    // Elicitation (ask_item) then Echo(ElicitationResponse { same rpc_id, Decline })
    // → resolved == Some("Skipped"), accepted == false.
    // Accept(..) instead → resolved == Some("Answered"), accepted == true.
}

#[test]
fn echoed_cancel_settles_parked_cards_without_sending() {
    // a parked permission, then Echo(Cancel) → cancelled == true, card resolved, nothing on rx.
}

#[test]
fn echoed_picks_and_stops_change_nothing() {
    // Echo(SetMode), Echo(SetConfigOption), Echo(StopBackground) → items unchanged.
    // The agent's own PickConfirmed / Background events carry those.
}
```

- [ ] **Step 2:** Run `cargo test -p onehand-core echoed_`. Expect a compile failure: no such
  variant.
- [ ] **Step 3:** Add the variant and its handling in `Chat::apply`, matched exhaustively on the
  `AcpRequest`:
  - `Prompt`: `push_user(text, vec![])` and `busy = true`.
  - `PermissionResponse`: find the open permission card with that `rpc_id` and resolve it with the
    option's name, or with the cancelled wording for `None`.
  - `ElicitationResponse`: settle the open ask with that `rpc_id`.
  - `Cancel`: what `cancel_turn` does, without the send.
  - The rest: nothing.

  Put the by-rpc-id lookups beside `answer_permission`/`answer_ask` in `turn.rs`, sharing their
  resolve code. Add the arms in the app: `acp.rs` and `agent_check.rs` treat `Echo` like any
  event that only the chat reads.
- [ ] **Step 4:** Run `cargo test --workspace --locked`. Expect PASS.
- [ ] **Step 5: Commit:** `feat(chat): replay a person's request into a transcript`

### Task 3: The host wire

**Files:**
- Create: `crates/core/src/host.rs` (module header plus `pub mod wire; pub mod journal; pub mod client;`)
- Create: `crates/core/src/host/wire.rs`
- Test: `crates/core/src/host/wire/tests.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod host;`, in the module list with a one-line description)

**Interfaces:**
- Produces, in `onehand_core::host::wire`:

```rust
pub const PROTOCOL: u32 = 1;

/// What the laptop sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ask { pub id: u64, pub call: Call }

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    Hello { build: String, protocol: u32 },
    Shutdown,
    List,
    Open { cwd: PathBuf, agent: AgentSpec, opening: Opening },
    Watch { session: String },
    Request { session: String, request: AcpRequest },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Welcome { build: String, protocol: u32, busy: Vec<String> }, // busy: session ids mid-turn
    Sessions(Vec<SessionInfo>),
    Opened { session: String },
    Done,
}

/// What the host sends, one per line.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Answer { id: u64, result: Result<Reply, String> },
    Entry { session: String, seq: u64, entry: Entry },
    /// Another attach took this host; this stream is about to close.
    Kicked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// The agent came up: `AcpEvent::Connected` without its channel.
    Ready { resumed: bool, forkable: bool },
    Event(AcpEvent),
    Request(AcpRequest),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo { pub session: String, pub cwd: PathBuf, pub agent: String, pub busy: bool, pub ended: bool }

/// Whether a session is mid-turn after `entry`, given whether it was before.
pub fn busy_after(busy: bool, entry: &Entry) -> bool;

/// The build id of a host binary: FNV-1a 64 over its bytes, as 16 hex digits.
pub fn build_id(bytes: &[u8]) -> String;
```

- [ ] **Step 1: Write the failing tests**
  - `busy_after`:
    - `Request(Prompt)` gives `true`.
    - `Event(TurnEnded)` gives `false`.
    - `Event(Disconnected(_))` gives `false`.
    - `Event(AgentChunk)` keeps the value it was given.
  - `build_id(b"")` is `"cbf29ce484222325"` and `build_id(b"a")` is `"af63dc4c8601ec8c"`, the
    FNV-1a 64 test vectors.
  - `Ask`/`Line` round trip through `serde_json` for one `Open` and one `Entry`.
- [ ] **Step 2:** Run `cargo test -p onehand-core host::wire`. Expect FAIL.
- [ ] **Step 3:** Implement. `busy_after` matches `Entry` and the `AcpEvent`/`AcpRequest` inside
  it exhaustively.
- [ ] **Step 4:** Run the same tests. Expect PASS.
- [ ] **Step 5: Commit:** `feat(host): the wire between a laptop and a host`

### Task 4: The journal

**Files:**
- Create: `crates/core/src/host/journal.rs`
- Test: `crates/core/src/host/journal/tests.rs`

**Interfaces:**
- Produces, in `onehand_core::host::journal`, all blocking std I/O:

```rust
pub struct Journal { /* file, next seq */ }
impl Journal {
    /// Open `dir/journal.jsonl` for appending, creating `dir`; `next` continues after what is there.
    pub fn open(dir: &Path) -> io::Result<Journal>;
    /// Append one entry as one line, flushed; returns its seq (0-based line number).
    pub fn append(&mut self, entry: &Entry) -> io::Result<u64>;
}
/// Every readable entry in `dir/journal.jsonl` with its seq, in order. A line that does not parse is skipped.
pub fn read(dir: &Path) -> io::Result<Vec<(u64, Entry)>>;
```

- [ ] **Step 1: Write the failing tests**
  - `appends_and_reads_back_in_order`: three entries give seqs 0, 1, 2, and `read` returns them
    in order.
  - `reopening_continues_the_count`: reopen, then append, gives seq 3.
  - `torn_last_line_costs_only_itself`: write two good lines and then
    `{"event":{"agent_chunk":` with no newline. `read` returns the two entries. The next `append`
    starts on a fresh line and gets seq 3, because a torn line still counts as a line, so seqs
    never shift.
- [ ] **Step 2:** Run `cargo test -p onehand-core host::journal`. Expect FAIL.
- [ ] **Step 3:** Implement. Seq is the line index. `open` counts the lines, and if the file does
  not end in `\n` it writes one, so a torn tail never fuses with the next entry.
- [ ] **Step 4:** Run. Expect PASS.
- [ ] **Step 5: Commit:** `feat(host): an append-only journal per host session`

### Task 5: The `onehand-host` binary and its hub

**Files:**
- Create: `crates/host/Cargo.toml` (`name = "onehand-host"`, edition 2021; deps:
  - `onehand-core` by path
  - `tokio` with `rt-multi-thread`, `macros`, `net`, `io-std`, `io-util`, `process`, `sync`, `time`
  - `futures`, `serde_json`, `dirs`)
- Modify: root `Cargo.toml` (add `crates/host` to `members`)
- Create: `crates/host/src/main.rs`: `onehand-host serve | attach | --build-id`
- Create: `crates/host/src/hub.rs`: sessions, the journal, watchers. The tested core of the
  service.
- Create: `crates/host/src/serve.rs`: lock, socket, connection loop, one attach at a time, login
  PATH.
- Create: `crates/host/src/attach.rs`: the stdio↔socket relay, starting `serve` when needed.
- Test: `crates/host/src/hub/tests.rs`

**Interfaces:**
- Consumes: Tasks 1–4; `onehand_core::acp::{connect, SessionOpts, Opening}`;
  `onehand_core::config::AgentSpec::launch`; `onehand_core::instance::{hold_lock, socket_path}`.
- Produces (crate-private):

```rust
/// How a session's agent is reached: `acp::connect` in production, a fake in tests.
pub(crate) type Connector =
    Arc<dyn Fn(Launch, SessionOpts) -> BoxStream<'static, AcpEvent> + Send + Sync>;

pub(crate) struct Hub { /* dir, path: Option<OsString>, connector, sessions */ }
impl Hub {
    pub(crate) fn new(dir: PathBuf, path: Option<OsString>, connector: Connector) -> Hub;
    /// Start a session: journal under `dir/sessions/<id>/`, agent driven on a spawned task.
    pub(crate) fn open(hub: &Arc<Mutex<Hub>>, cwd: PathBuf, agent: AgentSpec, opening: Opening) -> Result<String, String>;
    /// Send every entry so far to `sink`, then live entries except `Request`.
    pub(crate) fn watch(&mut self, session: &str, sink: UnboundedSender<Line>) -> Result<(), String>;
    /// Journal `request`, then hand it to the agent.
    pub(crate) fn request(&mut self, session: &str, request: AcpRequest) -> Result<(), String>;
    pub(crate) fn list(&self) -> Vec<SessionInfo>;
    /// Drop every watcher (the attach went away or was replaced). Sessions run on.
    pub(crate) fn unwatch_all(&mut self);
}
```

- Behaviour fixed here:
  - **Session ids** are `<unix secs>-<counter>`.
  - **Agent events:** each one is journaled. `Connected` becomes `Ready` and its `tx` is kept for
    `request`. The stream ending journals `Event(Disconnected(..))` if it did not already say
    one, and marks the session `ended`.
  - **`request` before `Ready`** gives `Err("the agent is still starting")`. On an ended session it
    gives `Err("this session has ended")`.
  - **A `Prompt` with attachments** is sent without them, after journaling
    `Event(Error { text: "Attachments are not sent to a host yet; the prompt went without them", detail: None })`.
  - **`SessionOpts`** have `mcp_servers: vec![]` and `dirs: vec![]`.
  - **`agent.env`** is ignored even if non-empty, since the laptop clears it. Launch uses
    `agent.launch(path.as_deref())`, and its `Err` is returned as is.
  - **Serve:**
    - The data dir is `$ONEHAND_HOST_DIR` or `~/.onehand-host/data`.
    - The service takes `hold_lock(dir)`; if the lock is held it exits 0.
    - It removes a stale socket, binds `socket_path(dir)` and sets mode 0600.
    - It logs to `dir/host.log`.
  - **One attach at a time:** a new connection makes the old one get `Line::Kicked` and closes it,
    then calls `unwatch_all`.
  - **`Hello`** answers `Welcome { build: build_id(own exe bytes), protocol: PROTOCOL, busy }`.
    **`Shutdown`** answers `Done`, then exits.
  - **Attach** connects to the socket. If that fails it spawns
    `current_exe() serve` with `process_group(0)`, null stdio and `ONEHAND_HOST_DIR` passed on,
    then retries every 100 ms for 5 s. It copies stdin to the socket and the socket to stdout until
    either side closes.
  - **Login PATH:** `serve` runs `$SHELL -ilc 'printf %s "$PATH"'` with a 10 s timeout, falling
    back to its own `PATH`.

- [ ] **Step 1: Write the failing hub tests** with a fake `Connector`. It hands each opened
  session a `futures::channel::mpsc` the test pushes `AcpEvent`s into, and records requests from
  the `tx` it put in `Connected`. Runtime: `#[tokio::test]`. `dir`: a fresh temp dir per test,
  following the existing core tests' pattern for temp dirs.
  - `opened_session_streams_ready_then_events`: push `Connected`, `SessionId("s")` and
    `AgentChunk`, then `watch`. The sink gets `Entry` seq 0 `Ready`, 1 `Event(SessionId)`,
    2 `Event(AgentChunk)`.
  - `detach_mid_turn_loses_nothing`: watch, `request(Prompt)`, then drop the sink receiver. Push
    three `AgentChunk`s and `TurnEnded`, then watch with a new sink. It receives all six entries,
    the `Request(Prompt)` included, and `list()` shows `busy == false`.
  - `live_requests_are_not_echoed`: watch, then `request(Prompt)`. The fake agent received the
    `Prompt`. The live sink got no `Request` entry, and a fresh `watch` gets one.
  - `card_parked_while_detached_is_answerable_after`: with no watcher, push
    `Permission(rpc_id 7)`. `watch` replays it. `request(PermissionResponse { rpc_id: 7, .. })`
    reaches the fake agent.
  - `attachments_are_dropped_and_said`: `request(Prompt { attachments: [one] })` reaches the agent
    with `attachments` empty, and the journal holds the `Error` event before the `Request`.
  - `request_before_ready_is_refused` and `ended_session_is_refused_and_listed_ended` (the fake
    stream closes).
  - `unknown_preset_fails_open`: an `AgentSpec` whose preset is unknown makes `open` return
    `AgentSpec::launch`'s error text.
- [ ] **Step 2:** Run `cargo test -p onehand-host`. Expect FAIL (does not compile).
- [ ] **Step 3:** Implement `hub.rs`, then `serve.rs`, `attach.rs` and `main.rs` around it. The
  connection loop reads `Ask` lines, dispatches on `Call` exhaustively, and writes `Line`s. A line
  that does not parse is answered with `Answer { id: 0, result: Err("unreadable request") }` and
  the connection stays up.
- [ ] **Step 4:** Run `cargo test -p onehand-host`. Expect PASS. Then run
  `cargo tree -p onehand-host -i gpui-pre`. Expect "did not match any packages".
- [ ] **Step 5:** Release packaging: in `.github/workflows/release.yml` build `-p onehand-host`
  too, and copy `target/release/onehand-host` into the tarball beside `onehand`. Add the
  `crates/host` row to AGENTS.md's *Repo layout* table: "`onehand-host`: the service that runs
  sessions on a host reached over SSH; built from core alone, never links GPUI".
- [ ] **Step 6: Commit:** `feat(host): the service that keeps sessions on a host`

### Task 6: The laptop-side client

**Files:**
- Create: `crates/core/src/host/client.rs`
- Test: `crates/core/src/host/client/tests.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces, in `onehand_core::host::client`:

```rust
#[derive(Debug)]
pub enum HostError {
    /// ssh could not run, or the host refused us.
    Unreachable(String),
    /// The host is not one this build can run on (`uname -sm` said this).
    Unsupported(String),
    /// The host runs another build, with these sessions mid-turn; it was left alone.
    Busy { build: String, busy: Vec<String> },
    /// The stream broke or the host answered something unexpected.
    Protocol(String),
}

/// How to reach a host: the `ssh` destination, and `local` for running the relay directly (tests, smoke).
pub struct HostAddr { pub ssh: String, pub local: bool }

/// Copy `binary` to the host unless that build is already there; return its remote path
/// (`~/.onehand-host/<build>/onehand-host`). Refuses a host whose `uname -sm` is not `Linux x86_64`
/// (the only build shipped).
pub async fn install(addr: &HostAddr, binary: &Path) -> Result<String, HostError>;

/// Attach, say hello, and settle the build: same build → connected; other build with nothing busy
/// → `Shutdown` it and attach again once; other build busy → `Busy`.
pub async fn connect(addr: &HostAddr, remote: &str, build: &str) -> Result<HostConn, HostError>;

#[derive(Clone)]
pub struct HostConn { /* sender to the connection task */ }
impl HostConn {
    pub async fn list(&self) -> Result<Vec<SessionInfo>, HostError>;
    pub async fn open(&self, cwd: PathBuf, agent: AgentSpec, opening: Opening) -> Result<String, HostError>;
    /// The session as a local one reads: `Ready` → `Connected` (with a `tx` whose requests go to the
    /// host), `Event(e)` → `e`, `Request(r)` → `Echo(r)`. Ends with `Disconnected` when the stream
    /// closes ("lost the connection to <host>") or the host was taken ("<host> was attached from
    /// another machine").
    pub fn watch(&self, session: &str) -> impl Stream<Item = AcpEvent> + Send + 'static;
}
```

- Behaviour fixed here:
  - **ssh command:** `ssh -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 <ssh> <remote> attach`.
    `BatchMode` so a password prompt fails fast instead of hanging. With `local`, the command is
    `<remote> attach`.
  - **Install** creates `~/.onehand-host/<build>/` and pipes the bytes over stdin into
    `onehand-host.tmp`, then runs `chmod 755` and `mv` into place, so a broken copy is never
    picked up.
  - **`open` clears `agent.env`** before sending.
  - The connection task is spawned with `tokio::spawn`, so the caller's tokio runtime drives it,
    the same requirement as `acp::connect`.

- [ ] **Step 1: Write the failing tests** over a `tokio::io::duplex` pair. A `#[cfg(test)]`
  constructor `HostConn::over(read, write)` skips ssh, and a test-side fake host reads `Ask`s and
  writes `Line`s.
  - `watch_maps_entries_to_events`: the host sends `Ready`, `Event(AgentChunk)` and
    `Request(Prompt)` for the session. The stream yields `Connected { resumed: false, .. }`,
    `AgentChunk` and `Echo(Prompt)`, in that order.
  - `requests_on_tx_go_to_the_host`: sending `AcpRequest::Cancel` on the `Connected` tx makes the
    fake host read `Call::Request { session, request: Cancel }`.
  - `closing_the_stream_ends_watch_with_disconnected`: the stream yields
    `Disconnected(text containing "lost the connection")`.
  - `kicked_ends_watch_saying_so`: the stream yields `Disconnected("… attached from another machine")`.
  - `open_clears_env`: `Call::Open`'s `agent.env` is empty.
  - `mismatch_with_busy_sessions_refuses` and `mismatch_idle_shuts_down_and_reattaches`. These
    drive the build-settling logic, factored as
    `async fn settle(conn, build) -> Result<Settled, HostError>` with
    `enum Settled { Same, Restarted }`, where the test counts `Shutdown` calls.
- [ ] **Step 2:** Run `cargo test -p onehand-core host::client`. Expect FAIL.
- [ ] **Step 3:** Implement. One task owns the stream. It routes `Answer`s to waiting callers by
  id (oneshot per `Ask`) and `Entry`s to the watch sender for that session.
- [ ] **Step 4:** Run `cargo test --workspace --locked`. Expect PASS.
- [ ] **Step 5: Commit:** `feat(host): attach to a host from core`

### Task 7: End-to-end smoke example

**Files:**
- Create: `crates/core/examples/host_smoke.rs`

**Interfaces:**
- Consumes: Task 6's `install`, `connect` and `HostConn`.

The usage is `host_smoke <ssh-dest|--local> <cwd> [go|watch <session>]`. The agent comes from
`ACP_CMD`, the same convention as `acp_smoke`.
- `go`:
  1. installs `target/debug/onehand-host`;
  2. connects;
  3. opens a session with that agent;
  4. sends one prompt through the `Connected` tx;
  5. prints events for 3 s;
  6. exits *mid-turn* and prints the session id.
- `watch <session>`: reconnects, prints every event until `TurnEnded`, then exits.

- [ ] **Step 1:** Write the example. A smoke is not unit-tested; it is the manual check.
- [ ] **Step 2: Run it locally:**

```bash
cargo build -p onehand-host
ACP_CMD="node crates/core/examples/mock_ui_agent.js" cargo run -p onehand-core --example host_smoke -- --local /tmp go
cargo run -p onehand-core --example host_smoke -- --local /tmp watch <session printed above>
```

  Expected: the second run prints the prompt as an `Echo`, every chunk the first run printed, and
  the chunks it missed, then `TurnEnded`. `pgrep -f 'onehand-host serve'` still lists the service
  after both have exited.
- [ ] **Step 3:** If an SSH host is at hand, run the same two commands with its alias in place of
  `--local`. Otherwise say in the PR that only `--local` was exercised.
- [ ] **Step 4: Commit:** `feat(host): a smoke run that detaches mid-turn`

---

## Not in this step (step 1b and later)

- **1b:** host projects in the workspace and the rail. A host session's chat pane is fed by
  `HostConn::watch`. The laptop store does not persist host transcripts yet, because a replay
  into a fresh `Chat` would append them twice. Also in 1b: the bundled `onehand-host` path beside
  `current_exe()`, and the busy-mismatch restart offer in the UI.
- **Step 2:** the tree, editor, git and terminal on a host. **Step 3:** picking a host, and the
  laptop's own copy of transcripts for reading while detached. **Step 4:** bypass. **Steps 5–6:**
  offloaded projects.
- **Session survival across a restart of the service** (resume from the journal) is not built.
  Sessions of a service that died show `ended`.
- ponytail: the journal grows without bound and every `Watch` replays it whole. Add a replay
  cursor once a long session makes attaching slow.
