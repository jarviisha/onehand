# R3 step 2a: core `control`, the socket speaks MCP, outside control behind a switch

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An MCP client outside the process (a script, cron) connects to `onehand.sock` and, when
Settings ▸ Connections allows outside control, lists projects, agents, workflows and tasks, stops
a task, opens a file in the Workbench and opens a session (not shown) with a first prompt.

**Architecture:** Core `control` is pure: the catalogue, parsing a `tools/call` into
`control::Call`, and the MCP message handling over a per-connection `Conn`, given the
connection's `Standing`. The socket's tokio side runs `control::handle` per line and sends only a
`Call` across to GPUI with a oneshot for its `Result<String, String>`; the app's `control` module
performs it in the front window. The outside switch is an `AtomicBool` the tokio side reads on
every message, set at boot from `control.outside` and by the switch.

**Tech Stack:** Rust, GPUI, tokio, `serde_json`, `futures` oneshot.

**Spec:** `docs/superpowers/specs/2026-10-23-agent-control-design.md` (Goals 1, 3–5, Decisions 1,
4, Steps row 2). Step 1 plan: `2026-10-23-agent-control-step-1-socket.md`.

## Builder's calls in this step (said in the PR)

- **Step 2 splits in two PRs.** 2a: everything but `start_workflow` / `start_issue`. 2b: those
  two, which need `task::submit` to tell ran from queued (it only toasts today), a task driven off
  screen (the launcher's path shows the session and takes the caret), the worktree cut lifted out
  of the launcher, and an issue read by number.
- **`open_file` takes no `line`**: the editor has no way to go to one (`EditorView::open` says
  so). An outside call names an absolute path inside the project on screen; another project's
  file is refused, because selecting that project would move what the person is looking at.
- **The four-sessions cap and "the calling session's own task is refused"** need a calling
  session, which only step 3 brings; they land there.
- A token in `initialize` is refused as an unknown session until step 3.

## Global Constraints

- One line in, one line out, JSON-RPC 2.0, on the step-1 socket. A connection whose first line is
  `onehand/open` is a second launch, as before; any other first line starts an MCP conversation.
- A malformed line is answered `-32700` and does not close the connection.
- `initialize` without a token is refused while outside control is off. The switch is read on
  every message, so turning it off refuses the next `tools/list` / `tools/call`.
- No tool answers a card, a permission or an approval; none is written.
- A tool steals neither focus nor the caret: a session it opens is not shown; a file it opens does
  not focus the editor.
- Every list is capped at `control::LISTED` = 50 and says how many it left out.
- Core stays GUI-free and runtime-free; nothing in core is `pub` unless the app names it.
- Code describes, never cites a document. `.md` files in English.
- Every visible change follows the `ui` skill; Settings holds no primary button.

## Review Focus

1. A project named by a name two projects share: refused, naming both paths. → Task 1
   `a_name_two_projects_share_is_refused_naming_both`.
2. A client that skips `initialize` and calls `tools/call` straight away: refused, not performed.
   → Task 1 `a_call_before_initialize_is_refused`.
3. The switch turned off while a client is connected: its next call is refused. → Task 1
   `a_call_after_outside_control_is_switched_off_is_refused` (standing is a closure re-read).
4. An argument of the wrong type (`"prompt": 3`, a missing `project`): a tool error the agent can
   read, not a crash or a JSON-RPC error. → Task 1 `every_tool_refuses_malformed_input`.
5. A huge line or a client that never sends a newline: dropped at 64 KiB per line, not buffered
   without end. → Task 3 reads each line through its own `take(LINE_CAP)`.

---

### Task 1: Core `control`

**Files:**
- Create: `crates/core/src/control.rs`, `crates/core/src/control/tests.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod control;`)

**Interfaces (Produces):**
- `pub enum Call { ListProjects, ListAgents, ListWorkflows, ListTasks { project: Option<String> }, StopTask { task: String }, OpenFile { path: PathBuf }, OpenSession { project: String, agent: String, prompt: Option<String> } }` (`Debug, PartialEq`)
- `pub enum Peer { Outside }` — who a connection speaks for, fixed at `initialize`.
- `pub enum Standing { On, Off, Refused(String) }` — `Off` is for step 3's switched-off session.
- `#[derive(Default)] pub struct Conn { peer: Option<Peer> }`
- `pub enum Handled { Reply(String), Perform { id: Value, call: Call }, Quiet }`
- `pub fn handle(conn: &mut Conn, line: &str, standing: impl Fn(&Peer) -> Standing) -> Handled`:
  - not JSON → `Reply` `-32700`; no `id` (a notification) → `Quiet`;
  - `initialize`: `params._meta.onehand.session` present → `-32001 "no such session"`; else
    `Peer::Outside`, and `Refused(why)` → `-32001 why`; otherwise set `conn.peer` and reply
    `{protocolVersion: <the client's, or "2025-06-18">, capabilities: {tools: {listChanged: true}}, serverInfo: {name: "onehand", version}}`;
  - `ping` → `{}`;
  - `tools/list` / `tools/call` before `initialize` → `-32002 "initialize first"`;
  - `tools/list`: `On` → `{tools: catalogue}`, `Off` → `{tools: []}`, `Refused` → `-32001`;
  - `tools/call`: `Refused` → `-32001`; `Off` → tool error "onehand's tools are switched off for
    this session"; unknown name → `-32602`; bad arguments → tool error naming the argument;
    otherwise `Perform`;
  - anything else → `-32601`.
- `pub fn answered(id: &Value, outcome: Result<String, String>) -> String` — a `tools/call`
  result: `{content: [{type: "text", text}], isError}`.
- `pub fn pick_project<'a>(projects: &'a [(String, PathBuf)], named: &str) -> Result<&'a Path, String>`
  — by path (an absolute `named` equal to a project's path, trailing slash ignored) or by name;
  two names alike → `"two projects are named X: /a, /b; name one by its path"`; none →
  `"no project named X in this window"`.
- `pub const LISTED: usize = 50;` and `pub fn listed(lines: Vec<String>, empty: &str) -> String` —
  joined by newlines, cut at `LISTED` with a last line `"… and N more not listed"`; `empty` when
  there are none.
- Catalogue (private `fn catalogue() -> Value`): the seven tools, each `name`, a one-line
  `description` in CONTEXT.md's words, and an `inputSchema` (`type: object`, `properties`,
  `required`, `additionalProperties: false`).

- [ ] **Step 1: Write the failing tests** in `control/tests.rs`: for each tool a call that parses
  (`Perform` with the expected `Call`) and one refused (`every_tool_refuses_malformed_input`: a
  missing required argument and a wrong type each give an `isError` reply naming the argument);
  `tools_list_is_empty_for_a_session_switched_off` (standing `Off`);
  `a_call_is_refused_while_switched_off`; `initialize_without_a_token_is_refused_while_outside_control_is_off`;
  `initialize_with_a_token_is_refused_until_sessions_have_tokens`;
  `a_malformed_line_is_answered_and_the_connection_goes_on` (then `tools/list` still answers);
  `a_call_before_initialize_is_refused`; `a_call_after_outside_control_is_switched_off_is_refused`;
  `a_notification_is_not_answered`; `an_unknown_tool_is_a_protocol_error`;
  `a_name_two_projects_share_is_refused_naming_both`; `a_project_is_found_by_path_or_name`;
  `a_long_list_says_how_many_it_left_out`; `answered_marks_a_refusal_as_an_error`.
- [ ] **Step 2: Run** `cargo test -p onehand-core control` — fails to compile.
- [ ] **Step 3: Implement** the Interfaces items.
- [ ] **Step 4: Run** `cargo test -p onehand-core control` — PASS.

### Task 2: Config and the Settings switch

**Files:**
- Modify: `crates/core/src/config.rs` (`pub struct ControlConfig { pub outside: bool }`,
  `#[serde(default)]`, `AppConfig.control` after `unattended`, its `Default`; a round-trip test
  `control_outside_is_off_unless_said`)
- Create: `crates/app/src/control.rs` (this task: `static OUTSIDE: AtomicBool`,
  `pub(crate) fn boot(cfg: &ControlConfig)`, `pub(crate) fn outside() -> bool`,
  `pub(crate) fn set_outside(on: bool, window: &mut Window, cx: &mut App)` via
  `crate::saves::write_config("control-outside", "Saving outside control", .., Some(Change { before, after, put }), ..)`
  as `Shell::set_appearance` does, `cx.refresh_windows()` after each put)
- Modify: `crates/app/src/lib.rs` (`mod control;`), `crates/app/src/shell/boot.rs` (`crate::control::boot(&cfg.control)` beside `remote::boot`; boot.rs must not grow past 803 lines: take the clone in the existing line block),
  `crates/app/src/settings/pages.rs` (`connections_page`: a second `section(Some("Outside control"), Some(about), cx)` under the connectors with one `Switch::new("control-outside").checked(outside()).small().label("Let programs on this machine control onehand")` in the same `flex_none().cursor_pointer()` holder the unattended switches use; the about text: what a program run as you may do over the socket, the socket's path in mono as the config path is drawn, and that it never answers a card, a permission or an approval)

- [ ] **Step 1:** Write `control_outside_is_off_unless_said` (parse `""` → `false`; parse
  `[control]\noutside = true` → `true`); run `cargo test -p onehand-core control_outside` — fails.
- [ ] **Step 2:** Implement; run it — PASS; `cargo check -p onehand`.

### Task 3: The socket speaks MCP; the app performs calls

**Files:**
- Modify: `crates/core/src/instance.rs`: `answer` becomes
  `pub fn open(line: &str) -> Option<(String, Option<Forwarded>)>` — `None` for anything but an
  `onehand/open` (non-JSON included, which MCP handling answers); its tests follow.
- Modify: `crates/app/src/instance.rs`: the channel carries
  `enum Incoming { Open(Forwarded), Call(Call, oneshot::Sender<Result<String, String>>) }`;
  `serve` reads the first line (5 s, 64 KiB); `open` → as before; `None` →
  `crate::control::converse(first, reader, writer, tx)`. The GPUI loop waits for the first window
  as now, then `Open` → `open_forwarded`, `Call` → `crate::control::perform`. `open_forwarded`'s
  front-window walk moves to `crate::state::front`.
- Modify: `crates/app/src/state.rs`: `pub(crate) fn front(cx: &App) -> Option<(AnyWindowHandle, Entity<Shell>)>`
  (active window, else the first) and `pub(crate) fn with_shell<R>(window: AnyWindowHandle, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) -> R) -> Option<R>`,
  lifted from `task::place::in_window`, which now calls it.
- Modify: `crates/app/src/control.rs`:
  - `pub(crate) async fn converse(first: String, reader: BufReader<OwnedReadHalf>, writer: OwnedWriteHalf, tx: UnboundedSender<Incoming>)`
    — a `Conn`; each line (the first, then each read through its own `take(LINE_CAP)`, no
    timeout) through `control::handle` with `|Peer::Outside| if outside() { On } else { Refused("outside control is off: switch it on in Settings ▸ Connections") }`;
    `Reply` written, `Perform` sent and its answer written with `control::answered`
    (`Err("onehand is closing")` if the oneshot drops), `Quiet` nothing. Ends at EOF.
  - `pub(crate) fn perform(call: Call, cx: &mut App) -> Result<String, String>` on
    `state::front`, with `Err("no window is open")` without one:
    - `ListProjects`: `Shell::remote_roots()` with `check_of` → `"<name> — <path> — check: yes|no"`.
    - `ListAgents`: `Shared.agents` names.
    - `ListWorkflows`: `crate::workflow::templates(cx)`, readable ones `"<id> — <name>"`, an
      unreadable one `"<file> — cannot be read: why"`.
    - `ListTasks`: `task::rows(&roots, cx)` (one project when named, via `pick_project`) →
      `"<id> — <title> — <group> — <at> — next: <acts>"`, the acts from `task_acts` (moved with
      `TaskAct` from `chat/pane/tasks_page.rs` to `task/listing.rs`, `pub(crate)`).
    - `StopTask`: the row among the window's projects, else `Err("no task <id> in this window's projects")`;
      not `stoppable` → `Err("<title> has nothing to stop")`; else `task::stop_task` →
      `Ok("Stopping <title>")`.
    - `OpenFile`: relative → `Err("give an absolute path")`; not a file → `Err`; not under the
      active project → `Err("<path> is not in <active project>, the project on screen")`; else
      `Shell::open_file(path, window, cx)` (the `ChatPaneEvent::OpenFile` arm in `boot.rs`
      lifted into `shell/roots.rs` beside `open_root`; the arm calls it) → `Ok("Opened <path>")`.
    - `OpenSession`: `pick_project`; `agent_or_first(agents, Some(agent))` else
      `Err("no agent named X; list_agents names them")`; `Shell::run_unattended(dir, spec, cx)`
      else `Err("the session did not start")`; a `prompt` waits for `Link::Connected` via
      `cx.observe(&session, ..)`, submitted once (`ChatSession::submit`) and the observer dropped;
      `Link::Lost` drops it too → `Ok("Opened a session with <agent> on <project>; it is in the rail, not shown")`.

- [ ] **Step 1: Write the failing test** in `crates/app/src/instance.rs`'s tests:
  `an_outside_client_is_answered_over_the_socket`: `serve` on a temp socket, a fake GPUI side
  answering each `Incoming::Call` with `Ok("done")`; with `OUTSIDE` false, `initialize` → error;
  with it true, `initialize` → result, `tools/list` → 7 tools, `tools/call list_agents` → text
  `done`; and `onehand/open` still forwards on a new connection.
- [ ] **Step 2: Run** `cargo test -p onehand an_outside_client` — fails to compile.
- [ ] **Step 3: Implement** the Files items.
- [ ] **Step 4: Run** `cargo test -p onehand instance` and `cargo test -p onehand-core` — PASS.

### Task 4: Documents, checks, review, PR

- [ ] `docs/control.md`: the MCP conversation, the seven tools and their answers, outside
  control and its switch, what is left for 2b and step 3. `docs/screens.md`'s Settings line:
  Connections holds the connectors, then *Outside control*. `CHANGELOG.md` (Unreleased).
- [ ] `ui` checklist answered (`.claude/skills/ui/checklist.md`), in the PR.
- [ ] `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test --workspace --locked`,
  `cargo tree -p onehand-core -i gpui` (no match). Opus review of `origin/main..HEAD`; fix what
  it confirms. Commit, push, PR, CI, ask before merging.
