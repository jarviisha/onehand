# R3 step 1: the socket, and a second launch that forwards its path

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The process holding `instance.lock` binds `onehand.sock` beside it; a second launch sends
`onehand/open` with its path over it and exits 0, and the running process opens that path as a
project in its front window and raises it.

**Architecture:** Core `instance` owns the rule: the socket's path, the `onehand/open` line both
ends write and read, and the blocking client a second launch runs. The app gets a private
`instance` module: the lock-or-forward call `main` makes before GPUI starts, and a listener on a
tokio runtime of its own that hands each forwarded path to GPUI over a `futures` channel.
`Shell::open_root` is cut out of `Shell::add_root` so the picker and the socket open a project
the same way.

**Tech Stack:** Rust, GPUI, tokio (`net`, `io-util` added to the app), `serde_json` (already in core).

**Spec:** `docs/superpowers/specs/2026-10-23-agent-control-design.md` (Goals 1, Decisions 1, 2, 6;
Steps row 1).

## Global Constraints

- Socket path: `instance.lock`'s directory, file `onehand.sock`; too long for a Unix socket
  address (more than 107 bytes on Linux) → `$XDG_RUNTIME_DIR/onehand-<hash of the config dir>.sock`
  (`std::env::temp_dir()` when that variable is unset). The hash is FNV-1a 64 over the dir's bytes,
  16 hex digits, so two builds agree on it.
- Only the lock holder removes a stale socket file and binds. Mode `0600`.
- One line in, one line out, JSON-RPC 2.0. `onehand/open { path }`: answered, then closed.
- A second launch: lock held → forward → exit 0. Socket unreachable → the old message
  (`onehand is already running on <dir>`), exit 1.
- The lock is checked in `main` before GPUI starts, still before the migration it protects.
  `--version` stays first; no flag becomes a folder.
- Core stays GUI-free and runtime-free; tokio I/O never on the UI executor.
- `crates/app/src/shell/boot.rs` is already past 800 lines: this step must not grow it.
- Code describes, never cites a document.

## Review Focus

1. A relative path (`onehand .` from another directory): the second launch must send it absolute,
   or the running process resolves it against its own cwd. → Task 1 test
   `open_request_carries_the_path_absolute`.
2. A path that is not a folder (a typo, a file): refused with a reason the second launch prints,
   exit 1, nothing opened. → Task 1 test `a_path_that_is_not_a_folder_is_refused`.
3. A stale `onehand.sock` from a crashed process: the next holder removes it and binds; a second
   launch against a stale file with no listener gets the old message. → Task 1 test
   `forward_to_no_listener_is_unreachable`.
4. A malformed or foreign line on the socket: answered with a JSON-RPC error, the listener keeps
   serving. → Task 1 test `a_line_that_is_not_open_is_answered_with_an_error`, Task 2 test.
5. The lock could not be taken at all (read-only dir): the app still starts, but must not remove
   or bind a socket another process may own. → Task 2 checks `instance::holding()` before binding.

---

### Task 1: Core: socket path, the `onehand/open` line, the client

**Files:**
- Modify: `crates/core/src/instance.rs` (module header, new items, tests in its `mod tests`)

**Interfaces:**
- Produces:
  - `pub fn holding() -> bool` — this process holds the lock (`LOCK.get().is_some()`).
  - `pub fn socket_path(dir: &Path) -> PathBuf` — reads `XDG_RUNTIME_DIR`, calls
    `fn socket_path_in(dir: &Path, runtime: Option<&Path>) -> PathBuf`.
  - `pub fn open_request(path: Option<&Path>) -> String` — one JSON-RPC line, id 1, method
    `onehand/open`, `params.path` absolute (`std::path::absolute`), or no `path` when `None`.
  - `pub struct Forwarded { pub path: Option<PathBuf> }`
  - `pub fn answer(line: &str) -> (String, Option<Forwarded>)` — the reply line (id echoed) and,
    when it was a good `onehand/open`, what to open. Errors: not JSON → `-32700`; another method →
    `-32601`; a `path` that is not a directory → `-32602` with message `"<path> is not a folder"`.
  - `pub enum Unforwarded { Unreachable, Refused(String) }`
  - `pub fn forward(socket: &Path, path: Option<&Path>) -> Result<(), Unforwarded>` — blocking
    `std::os::unix::net::UnixStream`, 5 s read timeout; a `result` reply → `Ok`, an `error` reply →
    `Refused(message)`, connect or read failure → `Unreachable`.

- [ ] **Step 1: Write the failing tests** in `instance.rs`'s `mod tests`:
  - `socket_path_sits_beside_the_lock`: `socket_path_in("/home/u/.config/onehand", Some("/run/user/1"))`
    == `/home/u/.config/onehand/onehand.sock`.
  - `a_socket_path_too_long_falls_back_to_the_runtime_dir`: a dir of 120 bytes → parent is
    `/run/user/1`, file name starts `onehand-`, ends `.sock`, and two calls agree.
  - `open_request_carries_the_path_absolute`: `open_request(Some("rel"))` parsed has
    `params.path` == `std::env::current_dir()?.join("rel")`; `open_request(None)` has no `path`.
  - `a_folder_is_forwarded`: `answer(&open_request(Some(temp_dir)))` → `Some(Forwarded { path: Some(temp_dir) })`, reply has `result` and `"id":1`.
  - `a_path_that_is_not_a_folder_is_refused`: a path to a file → `None`, reply `error.code == -32602`.
  - `a_line_that_is_not_open_is_answered_with_an_error`: `"nonsense"` → `-32700`;
    `{"jsonrpc":"2.0","id":7,"method":"tools/list"}` → `-32601` with `"id":7`; both `None`.
  - `forward_reaches_a_listener_and_hears_its_refusal`: a std `UnixListener` in a temp dir on a
    thread that answers each line with `answer`; `forward(sock, Some(dir))` → `Ok`,
    `forward(sock, Some(file))` → `Err(Refused(m))` with `m` containing `is not a folder`.
  - `forward_to_no_listener_is_unreachable`: a socket path with no listener (and a stale file at
    it) → `Err(Unreachable)`.
- [ ] **Step 2: Run** `cargo test -p onehand-core instance` — expect compile errors (items missing).
- [ ] **Step 3: Implement** the Interfaces items. `serde_json::json!` for every line written.
- [ ] **Step 4: Run** `cargo test -p onehand-core instance` — PASS.

### Task 2: App: lock or forward in `main`, the listener, `Shell::open_root`

**Files:**
- Create: `crates/app/src/instance.rs`
- Modify: `crates/app/src/lib.rs` (`mod instance;`), `crates/app/src/shell.rs` (re-export),
  `crates/app/src/main.rs`, `crates/app/src/shell/boot.rs` (lock out, listener in),
  `crates/app/src/shell/roots.rs` (`open_root`), `crates/app/Cargo.toml` (tokio `net`, `io-util`)

**Interfaces:**
- Consumes: Task 1's `holding`, `socket_path`, `answer`, `Forwarded`, `forward`, `Unforwarded`.
- Produces:
  - `pub fn hold_or_forward()` in `crate::instance`, re-exported as `shell::hold_or_forward`:
    takes the lock; when held, forwards `std::env::args().nth(1)` and exits (0 forwarded;
    `Refused(m)` → `onehand: m`, 1; `Unreachable` → the old message, 1).
  - `pub(crate) fn listen(cx: &mut App)` — returns at once unless `holding()`; removes a stale
    file, binds with tokio `UnixListener`, `0600`, accepts on a runtime kept in a
    `static OnceLock<tokio::runtime::Runtime>` (thread name `onehand-socket`); each connection
    reads one line, writes `answer`'s reply, sends any `Forwarded` over a
    `futures::channel::mpsc::UnboundedSender`, closes. A bind failure prints a line and the app
    carries on. A `cx.spawn` loop on the receiver calls `open_forwarded`.
  - `fn open_forwarded(forwarded: Forwarded, cx: &mut App)` — the front window (the active one in
    `Shared.windows`, else the first): raise it, and with a path call `Shell::open_root`. No window
    and a path → `open_or_focus(Workspace::seeded(path))`.
  - `pub(crate) fn Shell::open_root(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>)`
    — the body `add_root` runs after its picker; `add_root` calls it.

- [ ] **Step 1: Write the failing test** in `crates/app/src/instance.rs`'s `mod tests`:
  `a_second_launch_reaches_the_listener`: a tokio runtime, the accept loop
  (`async fn serve(listener: tokio::net::UnixListener, tx: UnboundedSender<Forwarded>)`, the
  part of `listen` without GPUI) on a temp socket; `onehand_core::instance::forward(sock, Some(dir))`
  from a std thread → `Ok`; the receiver yields `Forwarded { path: Some(dir) }`; a second
  `forward` after a `"nonsense"` line on another connection still → `Ok`.
- [ ] **Step 2: Run** `cargo test -p onehand a_second_launch` — fails to compile.
- [ ] **Step 3: Implement** the Interfaces items; `main` calls `shell::hold_or_forward()` after
  `--version`; `boot` drops its lock block and calls `crate::instance::listen(cx)` first, so a
  second launch finds the socket as early as possible (the lock is already held; the migration
  follows as before).
- [ ] **Step 4: Run** `cargo test -p onehand a_second_launch` — PASS; `cargo check`;
  `wc -l crates/app/src/shell/boot.rs` ≤ 807.

### Task 3: Documents, checks, PR

**Files:**
- Create: `docs/control.md` (the socket: where, who binds, mode, the line format, `onehand/open`,
  what a second launch does; steps 2 and 3 add the tools and the switch)
- Modify: `CLAUDE.md` (route `docs/control.md` under the features list), `CHANGELOG.md`
  (Unreleased: a second launch opens its project in the running app instead of only saying so;
  the Breaking line about a second instance updated)

- [ ] **Step 1:** Write the docs.
- [ ] **Step 2:** `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test -p onehand-core`,
  `cargo test -p onehand`, `cargo tree -p onehand-core -i gpui` (must error). All pass.
- [ ] **Step 3:** Commit, push, open a PR against `main`; wait for CI; ask before merging.
