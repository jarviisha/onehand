# Routed Builds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** an agent's build command in a project on this machine runs on the project's picked build host, through a shim on the agent's `PATH` that hands it to the running onehand over its socket.

**Architecture:** `onehand --route <socket> <program> <args…>` is a thin client written into one-line shim scripts placed first on the agent's `PATH`. The running onehand answers `onehand/route` on its instance socket: core decides (project, pick, machine switch, matching routed command, same system, host reachable), builds with the existing engine, and streams the lines back; or answers "run here", and the client execs the real program. A process-wide `RouteDrive` in core holds the machine switch and each host's standing; `ProjectsDrive` owns the project's pick and routed commands.

**Tech Stack:** Rust workspace (core GUI-free, `onehand-io` tokio runtimes, GPUI app), JSON-RPC lines over the Unix socket, POSIX `sh` shims, ssh/git/tar as the build engine already uses.

**Spec:** `docs/superpowers/specs/2026-11-23-routed-builds-design.md`

## Global Constraints

- Core stays GUI-free and runtime-free: `cargo tree -p onehand-core -i gpui-pre` must still fail to match; blocking functions only in core.
- `onehand-host` must not route anything: nothing in `AgentSpec::launch` or the host service changes; the `PATH` is added in `crates/io/src/acp.rs` only.
- No `pub` in core unless named outside the crate; new app modules private.
- Code never cites a document in comments or strings.
- Every file stays under about 800 lines of code (tests not counted); `crates/app/src/chat/pane/project_page.rs` (713) and `crates/core/src/project/drive.rs` (747) get new siblings, not growth.
- Our own enums matched exhaustively, no wildcard arms (`crates/app/src/guards.rs`).
- Never hard-code a colour or size; buttons via `crate::controls::action`, menu rows via `controls::menu_item`.
- Routes go only to a host declared (or taken as) Linux, from a Linux laptop (`std::env::consts::OS == "linux"`). *(spec: same system)*
- Messages, verbatim: `running on <host>`; `<host> did not answer; building here`; `onehand is not reachable; building here`; `waiting for the build already running on <host>`. All on stderr, each prefixed `onehand: `.
- Nested marker env var: `ONEHAND_ROUTED_HERE=1`.
- Checks before every commit: `make fmt`, `cargo test -p <crate touched>`; before the PR: `cargo test`, `make clippy CLIPPY_EXTRA="-- -D warnings"`.

## Deviations from the spec, decided while planning (fold into the spec's owning docs in Task 10)

- **The machine switch is a flag file**, `<config dir>/builds/hosts-off` (present = off), not `state.toml`: `RecentsDrive` holds `AppState` in memory and writes it whole, so a second writer would be clobbered.
- **"Brings artifacts back" is not a separate switch**: a routed command brings back iff it names artifacts.
- **The palette command is one static label**, *Toggle builds on hosts* (command labels are `&'static str`).
- **Reachability is a probe**: `ssh -o BatchMode=yes -o ConnectTimeout=5 <host> true` before the snapshot; exit 255 is "did not answer".
- **Shims are (re)written at every agent launch** from the project files on disk, so a routed command added mid-session reaches sessions opened after it (known gap, Task 10).

## Review Focus

1. A routed command's program word holding `/`, a space, a quote or `..` would write a shim outside its folder or break the script: refused at parse (Task 1 test `program_word_is_a_plain_name`).
2. "Run here" exec finding the shim itself loops forever: the real program is looked for on `PATH` without the shim folder, and none found exits 127 (Task 7 tests `real_program_skips_the_shim_folder`, `missing_real_program_exits_127`).
3. The agent kills `--route` mid-build: the host command is stopped, the lock let go (Task 6 test `dropped_connection_cancels_the_build`).
4. Arguments with spaces or quotes (`cargo test -- "a b"`) reach the host as the same words (Task 3 test `args_keep_their_words`).
5. A folder of no project, a host project, an offloaded project, hosts off, no pick: answered "run here" with no line, no ssh (Task 3 tests `here_without_*`).

---

### Task 0: Spike: a sandboxed agent's shell reaches the socket

Throwaway; nothing committed but a note in the PR body.

- [ ] **Step 1:** Start a listener on a Unix socket under the config dir, as onehand's would be: `python3 -c 'import socket,os;p=os.path.expanduser("~/.config/onehand-spike.sock");os.path.exists(p) and os.remove(p);s=socket.socket(socket.AF_UNIX);s.bind(p);s.listen(1);c,_=s.accept();print(c.recv(99));c.send(b"ok\n")'` in the background.
- [ ] **Step 2:** From Codex in its default sandbox: `codex exec --sandbox workspace-write "run: python3 -c 'import socket,os;s=socket.socket(socket.AF_UNIX);s.connect(os.path.expanduser(\"~/.config/onehand-spike.sock\"));s.send(b\"hi\");print(s.recv(9))'"`. Expected: `b'ok\n'`.
- [ ] **Step 3:** The same from Claude Code with `sandbox.enabled: true` in a scratch settings file (`claude -p --settings <file> …`). Expected: `b'ok\n'`.
- [ ] **Step 4:** If either is refused, stop and bring the owner the result before Task 1. Remove the socket.

### Task 1: Routed commands in the project file

**Files:**
- Create: `crates/core/src/workspace/routed.rs`, `crates/core/src/workspace/routed/tests.rs`
- Modify: `crates/core/src/workspace.rs` (module + re-export beside `BuildHost`), `crates/core/src/project.rs:98-125` (`Project` fields)

**Interfaces:**
- Produces:
  - `pub struct RoutedCommand { pub words: String, pub artifacts: Vec<PathBuf> }` (serde; `artifacts` skipped when empty)
  - `impl RoutedCommand { pub fn parse(words: &str, artifacts: &str) -> Result<Self, String>; pub fn program(&self) -> &str; pub(crate) fn matches(&self, program: &str, args: &[String]) -> bool }`
  - `Project { …, #[serde(default, skip_serializing_if = "Vec::is_empty")] pub routes: Vec<RoutedCommand>, #[serde(default, skip_serializing_if = "Option::is_none")] pub builds_on: Option<HostName> }`

- [ ] **Step 1: Write the failing tests** in `routed/tests.rs`:
  - `parse_keeps_words_and_artifacts`: `parse("  cargo   build ", "target/debug/onehand\n\ntarget/release/onehand")` → words `"cargo build"`, two artifacts.
  - `program_word_is_a_plain_name`: `parse(w, "")` is `Err` for `"../cargo build"`, `"/usr/bin/cargo build"`, `"car'go build"`, `""`, `"   "`; `Ok` for `"cargo-nextest run"`, `"dotnet build"`. Allowed program characters: ASCII alphanumerics, `.`, `_`, `-`, not starting with `.` or `-`.
  - `artifacts_are_refused_as_build_hosts_are`: `parse("cargo build", "../x")` is `Err` (via `inside_refusal`).
  - `matches_leading_words_after_options`: `"cargo build"` matches `("cargo", ["build","-p","x"])`, `("cargo", ["+nightly","build"])`, `("cargo", ["-q","build","--release"])`; not `("cargo", ["test"])`, `("cargo", ["builds"])`, `("dotnet", ["build"])`.
  - `matches_a_bare_program`: `"make"` matches `("make", [])` and `("make", ["all"])`.
  - `project_file_round_trips_routes_and_pick`: a `Project` with two routes and `builds_on = Some("100-52")` serialises and reads back equal; a project without them writes neither key.
- [ ] **Step 2:** `cargo test -p onehand-core routed` → fails to compile (no `RoutedCommand`).
- [ ] **Step 3:** Implement. Matching: skip leading args starting with `-` or `+`, then compare the remaining words of `self.words` after the program to the next args, word for word.
- [ ] **Step 4:** `cargo test -p onehand-core routed` and `cargo test -p onehand-core project` → PASS.
- [ ] **Step 5:** Commit `feat(builds): a project names the commands its agents build on a host`.

### Task 2: ProjectsDrive owns the pick and the routed commands

**Files:**
- Create: `crates/core/src/project/drive/routes.rs` (`impl ProjectsDrive` block; `mod routes;` in `drive.rs`)
- Modify: `crates/core/src/project/drive.rs:32-56` (`Thing::Routes`, `Thing::BuildsOn`; `Value::Routes(Vec<RoutedCommand>)`, `Value::BuildsOn(Option<HostName>)`, and wherever `Value` is applied back on a failed write), `crates/core/src/project/drive/tests.rs`

**Interfaces:**
- Consumes: Task 1's `RoutedCommand`, `Project.routes`, `Project.builds_on`.
- Produces (all `#[must_use]`, effects as `put_build_host`'s):
  - `pub fn put_route(&mut self, id: &ProjectId, route: RoutedCommand, replacing: Option<usize>) -> Result<Vec<Effect>, String>`
  - `pub fn remove_route(&mut self, id: &ProjectId, route: &RoutedCommand) -> Vec<Effect>`
  - `pub fn pick_build_host(&mut self, id: &ProjectId, host: Option<HostName>) -> Result<Vec<Effect>, String>`

- [ ] **Step 1: Failing tests** in `drive/tests.rs`:
  - `put_route_refuses_a_host_project`: a project with `host` set → `Err("<name> runs its sessions on a host; it builds there already")` (same words as `put_build_host`).
  - `put_route_refuses_the_same_words_twice`: second `cargo build` → `Err("<name> already routes cargo build")`; replacing index 0 with itself is `Ok`.
  - `at_most_twenty_routes`: the 21st → `Err("<name> routes at most 20 commands")`.
  - `pick_must_be_a_build_host`: `pick_build_host(Some("nope"))` → `Err("<name> does not build on nope")`; `Some("100-52")` with that build host → `Ok`, the project's `builds_on` set, one write effect; `None` → clears it.
  - `removing_the_picked_build_host_clears_the_pick`: `remove_build_host` of the picked host also clears `builds_on` in the same write.
- [ ] **Step 2:** `cargo test -p onehand-core project::drive` → FAIL.
- [ ] **Step 3:** Implement through the drive's existing `change` path (shown at once, put back if the write fails).
- [ ] **Step 4:** Tests → PASS.
- [ ] **Step 5:** Commit `feat(builds): the projects' drive keeps a project's pick and routed commands`.

### Task 3: The route decision

**Files:**
- Create: `crates/core/src/host/route.rs`, `crates/core/src/host/route/tests.rs` (`pub mod route;` in `crates/core/src/host.rs`)
- Modify: `crates/core/src/host/build.rs:19` (`mod shell;` → `pub(crate) mod shell;`, for the shims' quoting in Task 7)
- Modify: `crates/core/src/host/build/cli.rs:73-111` — extract the project lookup of `asked_blocking` into `pub(crate) fn project_of_blocking(root: &Path, cwd: &Path) -> Result<Found, String>` with `pub(crate) struct Found { pub project: Project, pub top: PathBuf, pub within: PathBuf }`; `asked_blocking` calls it.

**Interfaces:**
- Consumes: Task 1, `cli::entry_for`, `config::HostSpec`, `build::cli::command_line`.
- Produces:
  - `pub(crate) struct Ask { pub program: String, pub args: Vec<String>, pub cwd: PathBuf }`
  - `pub(crate) enum Decision { Here, Host(Routed) }`
  - `pub(crate) struct Routed { pub host: HostName, pub top: PathBuf, pub within: PathBuf, pub entry: BuildHost /* host + this route's artifacts, into None */, pub command: String }`
  - `pub(crate) fn decide(found: Result<Found, String>, hosts_on: bool, laptop_os: &str, declared: &[HostSpec], ask: &Ask) -> Decision` (pure)

- [ ] **Step 1: Failing tests** (pure, with a built `Found`):
  - `here_without_a_project` (`Err` found), `here_without_a_pick`, `here_when_hosts_are_off`, `here_for_a_host_project`, `here_for_an_offloaded_project`, `here_when_the_pick_is_no_longer_a_build_host`, `here_when_no_route_matches`, `here_for_a_windows_host`, `here_from_a_mac` (`laptop_os = "macos"`).
  - `host_for_a_matching_route`: `cargo build -p x` with route `cargo build` (artifacts `target/debug/x`) → `Host(Routed{ host: "100-52", entry.artifacts == ["target/debug/x"], entry.into == None, command == "cargo build -p x", within as found })`.
  - `args_keep_their_words`: args `["test","--","a b","it's"]` → `command == command_line(&["cargo","test","--","a b","it's"], &HostOs::Linux).unwrap()`.
- [ ] **Step 2:** `cargo test -p onehand-core host::route` → FAIL.
- [ ] **Step 3:** Implement `decide`; "here" never carries a message (the fall-back messages are Task 6's).
- [ ] **Step 4:** Tests → PASS; `cargo test -p onehand-core host::build` still PASS.
- [ ] **Step 5:** Commit `feat(builds): decide whether an agent's command goes to the build host`.

### Task 4: The build engine for routed runs

**Files:**
- Modify: `crates/core/src/host/build.rs` (`Job`, `build_in`, `lock`, `send_back`), `crates/core/src/host/build/shell.rs` (`run`, new `send_fresh`), `crates/core/src/host/build/tests.rs`

**Interfaces:**
- Produces:
  - `Job` gains `pub fresh_only: bool` (bring back only artifacts this run wrote; none written is not a failure) and `pub wait: bool` (wait for the folder's lock instead of refusing, saying `waiting for the build already running on <host>` once through `note`). Existing callers set both `false`.
  - `pub(crate) fn build_cancellable(job: &Job<'_>, line: &mut dyn FnMut(&str), note: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Built, String>` — `note` takes onehand's own lines (the waiting line).
  - `pub(crate) fn reachable_blocking(host: &HostName, os: &HostOs) -> bool` — the probe in Deviations.
  - `Shell::send_fresh(self, dir: &str, paths: &[PathBuf], mark: &str) -> Result<String, String>`; `Cmd` returns `Err("routed commands build on Linux hosts only")`.
- Decided: the run script touches `.git/onehand-route-mark` in the host's folder just before the command when `fresh_only`; `send_fresh` tars only the paths with something newer than the mark (`find "$p" -newer <mark> -print -quit`), and `Built.brought` lists exactly those.

- [ ] **Step 1: Failing tests** (the local-host harness `tests.rs` already uses: a host that is this machine):
  - `fresh_only_brings_back_what_the_run_wrote`: artifacts `a`, `b`; the command writes `a` only → `brought == ["a"]`, exit 0, no error; `b` left untouched here.
  - `fresh_only_with_nothing_written_is_not_a_failure`: `brought` empty, exit 0.
  - `fresh_only_ignores_an_artifact_left_from_an_earlier_run`: run 1 writes `a`; run 2 writes nothing → run 2 `brought` empty.
  - `waiting_job_waits_for_the_lock`: hold the folder's lock on another thread for 300 ms; a `wait: true` build succeeds after it, `note` saw the waiting line once; a `wait: false` build in the same state is refused as now.
  - `cancel_stops_a_running_command`: command `sleep 30`, cancel set after 200 ms → `Built.exit == None` within 5 s.
- [ ] **Step 2:** `cargo test -p onehand-core host::build` → FAIL.
- [ ] **Step 3:** Implement; `build_blocking` becomes `build_cancellable` with a never-set flag and a `note` writing nothing new.
- [ ] **Step 4:** Tests → PASS (all of `host::build`).
- [ ] **Step 5:** Commit `feat(builds): a routed run waits its turn and brings back only what it wrote`.

### Task 5: RouteDrive — the machine switch and each host's standing

**Files:**
- Create: `crates/core/src/host/route/drive.rs`, `crates/core/src/host/route/drive/tests.rs`

**Interfaces:**
- Produces:
  - `pub struct RouteDrive { hosts_on: bool, standing: BTreeMap<HostName, Standing> }`
  - `pub enum Standing { Reached, NotAnswering }`
  - `pub enum RouteEffect { WriteSwitch(bool) }` — carried by `pub fn write_switch_blocking(dir: &Path, on: bool) -> std::io::Result<()>` (creates or removes `builds/hosts-off`).
  - `impl RouteDrive { pub fn load(dir: &Path) -> Self; pub fn hosts_on(&self) -> bool; pub fn standing(&self, host: &HostName) -> Option<Standing>; #[must_use] pub fn set_hosts_on(&mut self, on: bool) -> Vec<RouteEffect>; pub(crate) fn reached(&mut self, host: &HostName, answered: bool) }`
  - `pub fn route_drive() -> std::sync::MutexGuard<'static, RouteDrive>` — one per process, loaded from `config_dir()` on first use (as `ControlDrive` is held).

- [ ] **Step 1: Failing tests:** `on_by_default` (no file); `off_when_the_flag_file_is_there`; `set_hosts_on_answers_a_write_only_on_change`; `write_switch_round_trips` (write false → `load` says off; write true → file gone); `standing_follows_the_last_run` (`reached(h,false)` → `NotAnswering`; then `reached(h,true)` → `Reached`).
- [ ] **Step 2:** `cargo test -p onehand-core route::drive` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Tests → PASS.
- [ ] **Step 5:** Commit `feat(builds): one switch for builds on hosts, and what each host last did`.

### Task 6: The socket's route call

**Files:**
- Create: `crates/core/src/host/route/call.rs`, `crates/core/src/host/route/call/tests.rs`
- Modify: `crates/io/src/socket.rs` (`accept`: route first lines to a new `serve_route`; `Incoming::Routed`), wherever `Incoming` is matched in `crates/app/src` (the app's `instance` carrier: on `Incoming::Routed`, `cx.refresh_windows()`)

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces (core):
  - `pub fn route_request(program: &str, args: &[String], cwd: &Path) -> String` — `{"jsonrpc":"2.0","id":1,"method":"onehand/route","params":{"program","args","cwd"}}`
  - `pub fn route_ask(line: &str) -> Option<Ask>` — `None` when the line is not `onehand/route` (then the socket goes on to `instance::open` and MCP as now)
  - `pub enum Said { Out(String), Note(String) }` and `pub fn said_line(said: &Said) -> String` — `{"jsonrpc":"2.0","method":"onehand/line","params":{"out":…}}` / `{"note":…}`
  - `pub enum RouteEnd { Here, Exit(i32) }` and `pub fn end_line(end: &RouteEnd) -> String` — `{"jsonrpc":"2.0","id":1,"result":{"run":"here"}}` / `{"result":{"exit":n}}`
  - `pub fn serve_blocking(ask: &Ask, emit: &mut dyn FnMut(Said), cancel: &AtomicBool) -> RouteEnd` — finds the project (`project_of_blocking`), reads `route_drive()`, `decide`s; for `Host`: `Note("running on <host>")`, probe (`reachable_blocking`; on false `reached(host,false)`, `Note("<host> did not answer; building here")`, `RouteEnd::Here`), else `build_cancellable` with `fresh_only: true, wait: true`, `reached(host,true)`, the existing hint / brought-back / did-not-finish notes as `--build` prints them, `Exit(code)` (130 stopped, 1 for a step that failed after `Note(why)`).
  - Client: `pub fn client_blocking(socket: &Path, program: &str, args: &[String], cwd: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<RouteEnd, String>` — `Err` when the socket cannot be reached or the answer never comes.
- io: `serve_route(ask, reader, writer, tx)`: `spawn_blocking` `serve_blocking`, lines through a channel to the writer; a task reading `reader` sets `cancel` on EOF; after the end line, `tx.unbounded_send(Incoming::Routed)`.

- [ ] **Step 1: Failing tests** (`serve_blocking`'s decision is Task 3's tests; here the wire and the client):
  - `route_ask_reads_only_route_lines`: `route_ask(route_request(..))` round-trips program/args/cwd; `route_ask(instance::open_request(None))` is `None`.
  - `lines_and_ends_are_json_rpc`: `said_line` / `end_line` parse back with the shapes above.
  - `client_prints_lines_and_returns_the_exit`: a test listener (std `UnixListener` on a temp path, thread) sends two `out` lines, one `note`, `exit 3` → `out` holds both lines, `err` holds `onehand: <note>`, result `Exit(3)`.
  - `client_reports_an_unreachable_socket`: path with no listener → `Err`.
  - `dropped_connection_cancels_the_build` (io, `crates/io/src/socket.rs` tests or a sibling): serve a route whose `serve_blocking` is replaced by a closure that waits on `cancel`; drop the client after 100 ms; the closure sees `cancel` within 2 s. (Make `serve_route` take the serving function as a parameter for this.)
- [ ] **Step 2:** `cargo test -p onehand-core route::call` and `cargo test -p onehand-io socket` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Tests → PASS; `cargo test -p onehand` builds with the new `Incoming` arm.
- [ ] **Step 5:** Commit `feat(builds): the socket builds an agent's routed command on its host`.

### Task 7: `onehand --route`, the shims, and the agent's PATH

**Files:**
- Create: `crates/core/src/host/route/shim.rs`, `crates/core/src/host/route/shim/tests.rs`
- Modify: `crates/app/src/main.rs` (dispatch beside `--build`, before the lock), `crates/core/src/acp/client.rs:65-76` (`SessionOpts.term_env: Vec<(String, String)>`; every constructor: `crates/io/src/acp.rs`, `crates/host/src/hub.rs`, `crates/core/examples/acp_smoke.rs`, `crates/core/src/acp/client/tests.rs` pass `Vec::new()` except io), `crates/core/src/acp/terminal.rs:98-147` (`create` takes `extra_env: &[(String, String)]`, applied before the request's own env), `crates/io/src/acp.rs:275-290`

**Interfaces:**
- Produces:
  - `pub fn route_main(socket: &Path, program: &str, args: &[String]) -> i32` — the whole flag: if `ONEHAND_ROUTED_HERE` is set, run here at once; else `client_blocking`; `Here` → run here; `Err` → `onehand: onehand is not reachable; building here` then run here with `ONEHAND_ROUTED_HERE=1`; `Exit(n)` → `n`. "Run here" = `exec` of `real_program(program, PATH, shim_dir)`, or 127 with `onehand: <program>: command not found`.
  - `pub(crate) fn real_program(program: &str, path: Option<&OsStr>, shim_dir: &Path) -> Option<PathBuf>`
  - `pub fn shim_dir(config: &Path) -> PathBuf` — `<config>/route-shims`
  - `pub fn write_shims_blocking(dir: &Path, exe: &Path, socket: &Path, programs: &[String]) -> std::io::Result<()>` — one `0755` file per program, `#!/bin/sh\nexec '<exe>' --route '<socket>' <program> "$@"\n` (paths quoted with `Shell::Posix.word`); files for programs no longer named are removed.
  - `pub fn agent_path_blocking() -> Option<OsString>` — collects every program of every project file under `config_dir()`; none → `None`; else writes the shims (exe = `current_exe()`, socket = `instance::socket_path(&config_dir())`) and answers `<shim dir>:<inherited PATH>`.
- io: when `agent_path_blocking()` is `Some(path)`, push `("PATH", path)` onto `launch.env` and `opts.term_env`.
- main: `[_, "--route", socket, program, args @ ..]` → `std::process::exit(route_main(...))`.
- Fall-back marker: a fall-back "here" (`did not answer`, `not reachable`, hosts off is *not* a fall-back) sets `ONEHAND_ROUTED_HERE=1` for the exec'd program. `serve_blocking`'s `did not answer` reaches the client as a note followed by `Here`; the client sets the marker when the last note before `Here` was one.

- [ ] **Step 1: Failing tests:**
  - `real_program_skips_the_shim_folder`: PATH `shim:/a:/b` where `shim/cargo` and `/b/cargo` exist → `/b/cargo` (temp dirs).
  - `missing_real_program_exits_127`: `route_main` with `ONEHAND_ROUTED_HERE=1` for a program on no PATH → 127.
  - `shims_are_written_and_pruned`: write `["cargo","dotnet"]` then `["cargo"]` → only `cargo`, executable, its text the line above.
  - `shim_quotes_paths_with_spaces`: exe `/tmp/a b/onehand` → the line holds `'/tmp/a b/onehand'`.
  - `acp_terminal_gets_the_extra_env`: `TerminalRegistry::create` with `extra_env [("PATH","X")]` runs `sh -c 'echo $PATH'` → output `X`; a request env naming `PATH` wins over it.
- [ ] **Step 2:** `cargo test -p onehand-core route::shim acp::terminal` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Tests → PASS; `cargo test -p onehand-host` and `cargo test -p onehand-io` PASS; `cargo tree -p onehand-core -i gpui-pre` still errors.
- [ ] **Step 5:** By hand: `target/debug/onehand --route /nonexistent.sock true` → prints `onehand: onehand is not reachable; building here` and exits 0.
- [ ] **Step 6:** Commit `feat(builds): an agent's PATH leads its build commands to onehand`.

### Task 8: The chip on the project page, and the palette command

Follow the `ui` skill and answer its checklist.

**Files:**
- Create: `crates/app/src/chat/pane/build_chip.rs` (`mod build_chip;` in the pane module)
- Modify: `crates/app/src/chat/pane/project_page.rs:419-440` (the head's end: the chip before *Run check*), `crates/app/src/keymap.rs` / its `commands` (a `toggle_host_builds` command, label *Toggle builds on hosts*, no default key, scope as other window commands), the shell's action handler for it

**Interfaces:**
- Consumes: `ProjectsDrive::pick_build_host` (Task 2), `route_drive()`, `RouteDrive::set_hosts_on`, `write_switch_blocking` (Task 5, carried off the UI thread), `Standing`.
- Produces: `fn build_chip(project: &Project, cx: &mut Context<ChatPane>) -> Option<AnyElement>` — `None` for a project with no build host or one whose sessions run on a host.

Words, verbatim: chip *Builds: This machine* / *Builds: <host>* / *Builds: here (hosts off)* / *Builds: here (<host> not answering)* (warning ink); menu: label *Builds*, rows *This machine* and each same-system build host (the pick checked), a separator, *Build on hosts* checked while on.

- [ ] **Step 1:** Implement; picking a row calls the drive and carries its effects as `put_build_host`'s carrier does; the switch row and the palette command set the switch and carry `WriteSwitch` on the background executor.
- [ ] **Step 2:** `cargo test -p onehand` (guards included) → PASS; `make clippy CLIPPY_EXTRA="-- -D warnings"` clean.
- [ ] **Step 3:** Commit `feat(builds): pick where a project builds from its page`.

### Task 9: Routed commands in Settings ▸ Projects

Follow the `ui` skill.

**Files:**
- Create: `crates/app/src/settings/projects/routes.rs` (`mod routes;` in `settings/projects.rs`)
- Modify: `crates/app/src/settings/projects.rs` (render the section under *Build hosts*)

**Interfaces:**
- Consumes: `RoutedCommand::parse`, `ProjectsDrive::put_route` / `remove_route`.

Words, verbatim: heading *Routed commands*; a row: its words over its artifacts joined by `, `, or *Brings nothing back*; edit mark and *Remove…* asking *Stop routing <words>?*; *No routed commands*; *Add routed command…* opens a dialog: *Command* (the words), *Artifacts* (one path a line), *Cancel*, *Add*/*Save*, refusals said in the form. Under *Command*, when the project folder holds `Cargo.toml`: *Suggested: cargo build, cargo test, cargo clippy*; `*.sln` or `*.csproj` at its top: *Suggested: dotnet build, dotnet test* — each a link filling the field. At most 20 rows (the drive refuses the 21st).

- [ ] **Step 1:** Implement (the folder check runs on the background executor when the page shows, never in render).
- [ ] **Step 2:** `cargo test -p onehand` → PASS; clippy clean.
- [ ] **Step 3:** Commit `feat(builds): declare a project's routed commands in its settings`.

### Task 10: Documents, then the end of the spec and plan

**Files:**
- Modify: `docs/hosts.md` (new section *Routed builds* after *Builds on a host*: the shim, the route call, the decision order, the messages, the switch file, fresh-only artifacts, waiting, same system, nested marker), `docs/control.md` (the `onehand/route` first line beside `onehand/open`), `docs/screens.md` (the chip and its menu on the project page head; the Settings section; the palette command), `CONTEXT.md` (**Routed command**: a command a project names whose run by one of its agents goes to the project's picked build host. _Avoid_: intercepted, proxied, wrapped command), `docs/known-gaps.md` (shims reach sessions opened after a routed command is added; the local build cache rebuilds after a routed build; Linux laptops only), `CHANGELOG.md` if the repo keeps an Unreleased section
- Delete: `docs/superpowers/specs/2026-11-23-routed-builds-design.md`, `docs/superpowers/plans/2026-11-23-routed-builds.md`

- [ ] **Step 1:** Write the sections; every behaviour in Tasks 1–9 has its sentence, with the code it lives in named as the existing docs do.
- [ ] **Step 2:** `cargo test` and `make clippy CLIPPY_EXTRA="-- -D warnings"` → PASS.
- [ ] **Step 3:** Commit `docs(builds): routed builds in the hosts, control and screens documents`, removing the spec and plan in the same commit.
