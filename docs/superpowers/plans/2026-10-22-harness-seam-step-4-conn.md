# Harness seam, step 4: the serve loop as `Conn`, sessions opened with `SessionOpts`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the ACP serve loop's state into one value, `Conn`, so authentication and capability
work (and R3's MCP servers) have one place to land. Open a session with `SessionOpts` instead of
positional arguments. Let a declared agent's "not logged in" line use the agent's own words
(`authMethods`). Cut `acp/client.rs` under the 800-line rule.

**Architecture:** `acp/client.rs` (1,420 lines of code before its tests) splits along three seams
that stand alone:
- `client/transport.rs`: the child process, its stderr tail, its process group;
- `client/forms.rs`: permission and elicitation parsing, and the elicitation answer;
- `client/wire.rs`: JSON-RPC framing and prompt blocks.

`run` keeps the `select!` loop. `handle_message`'s twelve parameters become fields of `struct
Conn`, and its branches become `Conn` methods. `connect(command, args, env, cwd, resume)` becomes
`connect(launch: Launch, opts: SessionOpts)`. `initialize`'s `authMethods` is read into `Conn`, and
`AcpEvent::LoginRequired` carries the agent's advice.

**Tech Stack:** Rust, tokio, serde_json (core only).

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decisions 5 and 6, step 4.

## Global Constraints

- Tasks 1 and 2 change no behaviour: every existing test in `acp/client/tests.rs` passes
  unchanged except for the `connect_over` call shape. The smoke test still runs.
- `SessionOpts` has `cwd` and `resume` only. `mcp_servers` is **not** added (R3 adds it where it is
  used). `"mcpServers": []` stays as written.
- Core stays GUI-free and runtime-free in its API (`connect` still returns a plain `Stream`).
  `#![warn(unreachable_pub)]`: `SessionOpts` is `pub` (the app names it); `Conn` and the new
  submodules' items are private, or `pub(super)` where `client.rs` or its tests need them.
- The login sentences, exactly:
  - a built-in agent: unchanged (`AgentSpec::login_hint`, the table's command under its account
    variable);
  - a declared agent whose agent sent an auth method with a description: `"<name> is not logged
    in. <description>"`, with backticks removed from the description (the line is drawn as plain
    text);
  - otherwise: `"<name> needs you to log in"` (unchanged).
- No file past about 800 lines of code. `client.rs`, `transport.rs`, `forms.rs` and `wire.rs` are
  each checked with `awk '/^#\[cfg\(test\)\]/{exit} {c++} END{print c}'`.
- `make fmt`; `make clippy CLIPPY_EXTRA="-- -D warnings"` must be clean.

## Review Focus

1. **The `biased` select order is unchanged**: deadline, parked responses, stdout, ended, terminal
   stream, UI requests. Moving the branches into `Conn` methods must not reorder them or move an
   `await` across the `select!` (Task 2, by inspection plus the existing
   `the_last_answer_before_an_exit_is_not_lost`).
2. **A `session/load` that falls back to `session/new`** still sends `session/new` with the same
   `cwd` (existing `a_refused_resume_is_said_in_the_agents_words`).
3. **An agent with several auth methods**: the first one that has a `description` is used, and
   one with none is skipped (Task 3 test `the_first_described_auth_method_is_the_advice`).
4. **A built-in agent ignores the agent's advice**, so its account variable is never lost
   (Task 3 test `a_built_in_agent_keeps_its_own_login_words`).
5. **An `authMethods` description with backticks** is drawn without them (Task 3 test,
   same as 3).

---

### Task 1: Cut `client.rs` along three seams (pure move)

**Files:**
- Create: `crates/core/src/acp/client/transport.rs`. It takes `Transport` (with `spawn` and the
  `#[cfg(test)] scripted`), `StderrTail`, `LastWords` (+ impl), `STDERR_TAIL_LINES`,
  `STDERR_LINE_CAP`, `STDERR_GRACE`, `Outgoing`, `explain`, `ADAPTER_GRACE`, `Group` (+ `Drop`).
- Create: `crates/core/src/acp/client/forms.rs`. It takes `parse_permission`, `CUSTOM_SUFFIX`,
  `parse_elicitation`, `field_order`, `str_field`, `parse_elicit_kind`, `parse_choices` and
  `elicit_result`.
- Create: `crates/core/src/acp/client/wire.rs`. It takes `alloc`, `init_params`, `request`,
  `notification`, `response_ok`, `in_words`, `login_required`, `method_not_found`,
  `response_err`, `write_msg`, `parse_modes`, `build_prompt_blocks`, `lines_of`, `file_uri` and
  `base64_encode` (still `pub`, re-exported as today).
- Modify: `crates/core/src/acp/client.rs`. Add `mod transport; mod forms; mod wire;` with
  `use transport::*`-style imports of exactly what `client.rs` and `client/tests.rs` use. Items
  become `pub(super)` as the compiler asks.

**Interfaces:**
- Produces: the same names, reachable from `client.rs` and `client/tests.rs` as today.

- [ ] **Step 1: Count the client tests first:** `cargo test -q -p onehand-core acp::client`. Note
  the passed count.
- [ ] **Step 2: Move the items,** add the `mod`/`use` lines, and fix visibility.
- [ ] **Step 3: Verify:** the same `cargo test` shows the same count. `cargo check -q --workspace
  --examples` is clean. Each of the four files is under 800 lines of code.
- [ ] **Step 4: Commit:** `refactor(acp): the transport, the forms and the wire format in files of their own`.

### Task 2: `Conn` and `SessionOpts`

**Files:**
- Modify: `crates/core/src/acp/client.rs`. Add `pub struct SessionOpts { pub cwd: PathBuf, pub
  resume: Option<String> }`. Change `connect`, `connect_over` and `connect_over_result` to take
  it, and change `run(transport, opts, output)`. Add `struct Conn` and move `handle_message` and
  `handle_terminal` (and `handle_reverse`) onto it.
- Modify: `crates/core/src/acp/mod.rs` (`pub use client::{base64_encode, connect, SessionOpts}`),
  `crates/app/src/acp.rs:198`, `crates/core/examples/acp_smoke.rs`, and the
  `connect_over(transport, …)` calls in `client/tests.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct SessionOpts { pub cwd: PathBuf, pub resume: Option<String> }
  pub fn connect(launch: crate::config::Launch, opts: SessionOpts) -> impl Stream<Item = AcpEvent>
  struct Conn {
      stdin: Outgoing,
      pending: HashMap<u64, Pending>,
      next_id: u64,
      ready: bool,
      session_id: Option<String>,
      images: bool,
      opts: SessionOpts,
      req_tx: ReqTx,
      output: EventTx<AcpEvent>,
      registry: TerminalRegistry,
      resp_tx: mpsc::UnboundedSender<(Value, Value)>,
  }
  impl Conn {
      async fn handle_message(&mut self, msg: Value) -> Result<(), String>;
      async fn handle_terminal(&mut self, method: &str, id: &Value, params: &Value) -> Result<(), String>;
      async fn send(&mut self, method: &str, params: Value, pending: Pending) -> Result<(), String>; // alloc + insert + write
  }
  ```
  `run` builds the `Conn` after writing `initialize` through `conn.send`. Its `select!` arms read
  `conn.ready` and write through `conn.stdin`, and the UI-request arm's body moves into
  `conn.request(req)`.

- [ ] **Step 1: Refactor.** Keep the `select!` arms in the same order (Review Focus 1). Drop
  `#[allow(clippy::too_many_arguments)]`.
- [ ] **Step 2: Verify:** `cargo test -q -p onehand-core acp::` gives the same count as Task 1.
  `cargo check -q --workspace --tests --examples` and `make clippy CLIPPY_EXTRA="-- -D warnings"`
  are clean.
- [ ] **Step 3: Commit:** `refactor(acp): the serve loop's state is one value, and a session opens with its options`.

### Task 3: The agent's own login words

**Files:**
- Modify: `crates/core/src/acp/types.rs`: `AcpEvent::LoginRequired { advice: Option<String> }`.
- Modify: `crates/core/src/acp/client.rs`:
  - `Conn` gains `auth_advice: Option<String>`, read in the `Pending::Init` arm from
    `result.authMethods`: the first entry whose `description` is a non-empty string, with
    backticks removed;
  - both `LoginRequired` sends carry it.
- Modify: `crates/core/src/config/agent.rs`: `login_hint(&self) -> Option<String>`. `Some` for a
  built-in agent (today's words); `None` for a declared or an unknown-preset entry.
- Modify: `crates/core/src/chat/model.rs` and `chat/model/apply.rs`:
  - `login_hint: Option<String>` (the entry's own words, when it has any);
  - `login_pending` becomes `Option<Option<String>>`, or a `login_advice: Option<String>` beside
    the flag; the builder chooses;
  - `take_login_hint` returns, in order, the entry's words, `"<agent> is not logged in.
    <advice>"`, or `"<agent> needs you to log in"`.
- Modify (callers): `crates/app/src/chat/session.rs:315`, `crates/app/src/agent_check.rs:116`,
  `crates/core/examples/acp_smoke.rs:167`, and the `AcpEvent::LoginRequired` uses in
  `chat/model/tests.rs`.
- Modify: `docs/agents.md`, *Not logged in*: the declared-agent bullet now reads "a declared
  agent: the agent's own words when it sends any (*OpenCode is not logged in. Run opencode auth
  login in the terminal*), else *Gemini needs you to log in*". Also `CHANGELOG.md` (*Fixed* or the
  agents line).
- Test: `crates/core/src/acp/client/tests.rs`, `crates/core/src/chat/model/tests.rs`,
  `crates/core/src/config/agent/tests.rs`.

**Interfaces:**
- Consumes: `Conn` from Task 2.
- Produces: `AcpEvent::LoginRequired { advice: Option<String> }`, `AgentSpec::login_hint() ->
  Option<String>`, `Chat.login_hint: Option<String>`.

- [ ] **Step 1: Write the failing tests:**
  ```rust
  // client/tests.rs — extend the existing refused_start helper's handshake so `initialize`
  // answers with authMethods: [{"id":"a","name":"A"}, {"id":"b","name":"B","description":"Run `x login` in the terminal"}]
  #[tokio::test]
  async fn the_first_described_auth_method_is_the_advice() {
      // a session/new refused with code -32000 → events contain
      // LoginRequired { advice: Some("Run x login in the terminal".into()) }
  }
  // chat/model/tests.rs
  #[test]
  fn a_declared_agent_is_told_how_to_log_in_in_its_own_words() {
      // chat.agent = "OpenCode", login_hint None;
      // apply(LoginRequired { advice: Some("Run opencode auth login in the terminal") }), then an Error
      // → the failure item's text == "OpenCode is not logged in. Run opencode auth login in the terminal"
  }
  #[test]
  fn a_built_in_agent_keeps_its_own_login_words() {
      // login_hint Some("Codex is not logged in. Run codex login …"), advice Some("other")
      // → the text is the login_hint
  }
  // config/agent/tests.rs: the existing declared-agent login test becomes
  // `assert_eq!(declared.login_hint(), None)`
  ```
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core login`. Expected: a
  compile error (`LoginRequired` has no field `advice`).
- [ ] **Step 3: Implement** as listed under *Files*.
- [ ] **Step 4: Full checks:**
  - `make fmt`, `cargo test -q --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`;
  - `cargo tree -p onehand-core -i gpui` (must error);
  - `ACP_PRESET=copilot cargo run -q -p onehand-core --example acp_smoke` (Expected: CONNECTED …
    TURN ENDED, as in step 3).
- [ ] **Step 5: Commit:** `feat(acp): a declared agent that is not logged in is told how in the agent's own words`. Open
  the PR against `main`.
