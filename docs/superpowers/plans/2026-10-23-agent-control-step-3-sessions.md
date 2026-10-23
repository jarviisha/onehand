# R3 step 3: an agent in a switched-on session has onehand's tools

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every session's agent is given one stdio MCP server, onehand itself as
`onehand --mcp-proxy <socket> <token>`; while the session's switch in the composer's `+` menu is
on, that agent can call the nine tools, landing in its own window; switching either way tells the
agent with `notifications/tools/list_changed`.

**Spec:** `docs/superpowers/specs/2026-10-23-agent-control-design.md` (Goals 2–3, Decisions 3, 4
(the calling session's rules), 5, 6; Steps row 3). Earlier plans: steps 1, 2a, 2b.

## Builder's calls (said in the PR)

- **The warm adapter keeps warming.** Its token is minted when it is started, before any session
  exists, and passed to the session that claims it. A token not yet claimed is known and
  switched off, so an agent that starts its MCP servers at `session/new` is connected, offered
  nothing, and told when the switch goes on.
- **The switch is kept by session uid**, so a restart keeps it; it goes when the pane closes the
  session. Not saved: a session reopened later starts off.
- **The indicator** is a chip in the composer's status row beside the mode chip: the `Bot` mark,
  lit in the warning ink while on (the fast chip's rule for a mark of a mode switched on),
  *Controls onehand*; pressing it switches it off.
- **"An unattended run's session"** is one whose task works an issue; the entry is not offered
  there, and the chip never shows.

## Global Constraints

- `main` keeps its rule that no flag becomes a folder: `--mcp-proxy` takes exactly two arguments
  and runs before the lock.
- No tool answers a card, a permission or an approval.
- While the switch is off, `tools/list` is empty and every `tools/call` is refused.
- A session the tools opened, or a run they started, starts off (the default).
- A calling session's call lands in its own window; `open_file` resolves a relative path against
  its project; `stop_task` refuses the caller's own task; a caller has at most four sessions open
  that its tools opened, the fifth refused naming the four.

## Review Focus

1. A forged token in `initialize` from the agent itself: the proxy replaces it. → core test
   `the_proxy_puts_the_token_on_initialize_only` (done).
2. A token whose session closed: refused as no such session. → Task 2 test on the token map.
3. The switch toggled with no connection, or two connections on one token: each connection told,
   a closed one dropped. → Task 2 test.
4. The fifth session: refused naming four. → Task 3 (`open_session` counts the caller's opened
   uids still open).
5. A session whose agent ignores `list_changed`: still refused while off. → core test (done).

---

### Task 1: Core — `Peer::Session`, `with_token`, `proxy`, `tools_changed`, `SessionOpts.mcp_servers`

- Done in `control.rs` (tests pass). Add `SessionOpts { cwd, resume, mcp_servers: Vec<Value> }`,
  read by `session/new` and `session/load`; `pub fn proxy_entry(command: &Path, socket: &Path, token: &str) -> Value`
  (`{name: "onehand", command, args: ["--mcp-proxy", socket, token], env: []}`);
  `pub fn new_token() -> String` (two `RandomState` hashes, 32 hex digits). Tests: the entry's
  shape; two tokens differ; `session/new` carries the entry (client test).

### Task 2: App — tokens, the switch, notifications

**`crates/app/src/control.rs`**:
- `static SESSIONS: Mutex<Sessions>` with `tokens: HashMap<String, Option<u64>>`,
  `on: HashSet<u64>`, `conns: HashMap<String, Vec<UnboundedSender<String>>>`,
  `opened: HashMap<u64, Vec<u64>>`.
- `pub(crate) struct Token(String)` — `Token::mint()` registers it unclaimed; `claim(&self, uid)`;
  `Drop` forgets it and its connections.
- `pub(crate) fn switched_on(uid) -> bool`, `pub(crate) fn switch(uid, on)` (sends
  `tools_changed` to every connection of a token claimed by `uid`), `pub(crate) fn forget(uid)`.
- `standing`: `Peer::Session(t)` → unknown `Refused("no such session")`, unclaimed or off `Off`,
  on `On`.
- `converse`: replies and notifications go through one writer task; a `Peer::Session` connection
  registers its sender after `initialize`.

**`crates/app/src/acp.rs`**: `start` mints a `Token` and puts `proxy_entry(current_exe, socket, token)`
in `SessionOpts`; `Warm` keeps its token; `connect` returns it with the receiver.

**`crates/app/src/chat/session.rs`**: `ChatSession` holds the `Token`, claimed with its uid.
**`crates/app/src/chat/pane/sessions.rs`** `close`: `control::forget(uid)`.

Test (`control.rs`): `a_token_speaks_for_its_session_until_it_goes` — mint, standing `Off`,
claim + switch on → `On`, a registered sender gets `tools_changed`, drop → `Refused`.

### Task 3: App — the caller's rules

- `Incoming::Call(Peer, Call, Answer)`; `perform(peer, call, answer, cx)`: the window holding the
  calling uid (else `front`); `open_file` relative → against the caller's project;
  `stop_task` of the caller's own task → `Err("that is this session's own task")`;
  `open_session` → `opened[caller]` still open ≥ 4 → `Err(naming them)`, else pushed.

### Task 4: App — the composer's switch and its chip

- `Act::Control` in `presentation.rs`: row *Let the agent control onehand*, `checked` from
  `switched_on`, mark `IconName::Bot`; `add_rows(session, cx)` leaves it out for an issue task's
  session; `act_rows`' `offered` arm the same; `apply_pick` flips it and notifies.
- `status_row`: the chip when on, pressing it switches off, tooltip *This session's agent can use
  onehand's tools. Press to stop it*; the row is drawn when only this chip is there.

### Task 5: Documents, checks, review, PR

- `docs/control.md` (the proxy, tokens, the switch, the caller's rules), `docs/screens.md`
  (the composer's status row and `+` menu), `CHANGELOG.md`; the `ui` checklist in the PR.
- `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test --workspace --locked`; a
  smoke run of the proxy against the socket is the core test `the_proxy_carries_lines_both_ways`.
  Opus review; PR; CI; ask before merging.
