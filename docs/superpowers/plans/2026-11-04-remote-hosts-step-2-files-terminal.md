# Remote hosts, step 2: files, editor, git status and terminal on a host Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A host project shows its file tree, opens and saves files in the editor, shows its git status, and runs terminals in the dock, all on the host. A terminal keeps running while the laptop is away, and comes back with its recent output when the project is shown again.

**Architecture:**
- **One seam in core: `Disk`** (new `crates/core/src/disk.rs`). It is `Local` or `Host(HostDisk)`. It has four blocking methods (read a directory, read a file, save a file, read git status). `Local` calls today's functions. `Host` sends the same call over the attach and blocks on the answer through the ACP runtime's tokio handle. The host answers by running those same functions on its own disk. The rules (sorting, caps, the mtime guard, the porcelain parse) stay in one place.
- **The Workbench is handed a `Disk` with its root.** Files and Editor read and write through it. Markdown, Neovim, Issues and Plugins are not offered on a host root.
- **Terminals.** The host owns one PTY per terminal, and keeps that terminal's raw output in a capped buffer. Output, input and resizes travel as new wire lines. On the laptop the dock mounts the same `TerminalView` over a reader and a writer that go through the attach instead of a local PTY.

**Tech Stack:** Rust, GPUI + gpui-component, tokio (ACP runtime and the host service), `portable-pty` 0.9 (already in core), `base64` 0.22 (already in `Cargo.lock`, new direct dependency of core).

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md` (step 2). As built so far: `docs/hosts.md`.

**Two PRs.**
- **2a** is Tasks 1–5: tree, editor, git status and file links. The terminal stays refused, with *Terminals on a host come later*.
- **2b** is Tasks 6–9: the terminal dock.

Each PR goes through review, then CI, then the owner's merge.

## Global Constraints

- Everything in AGENTS.md *Invariants*, *Rules* and *Load-bearing specifics* holds. In particular:
  - Core stays GUI-free.
  - No wildcard arms on our enums. A new `Call`, `Reply` or `FromHost` variant is added to every match.
  - Code describes and never cites a document.
  - Files stay under about 800 lines of code. `client.rs` (716) and `serve.rs` (316) grow, so new code goes in sibling modules as listed.
  - Tests go in `foo/tests.rs`.
  - In `crates/app` only `assets` and `shell` are `pub`. Nothing in core is `pub` unless something outside names it.
- **No tokio I/O on the GPUI executor.** `HostDisk` blocks only on the GPUI *background* executor's threads, through `tokio::runtime::Handle::block_on`. It never blocks on the UI thread or on a tokio worker.
- **Every UI change follows the `ui` skill.** Tasks 4 and 8 answer its `checklist.md` item by item.
- **Words follow CONTEXT.md *Hosts*:** host, host service, attach. Never "server", "remote" or "VPS" on screen.
- **A detach stops nothing on the host.** Quitting, removing a project, a lost connection and a tab that exits because its stream ended never close a host terminal. Only the person closing the tab does.
- Before each PR, run:
  - `cargo test --workspace --locked`
  - `make clippy CLIPPY_EXTRA="-- -D warnings"`
  - `make fmt`
  - `cargo tree -p onehand-core -i gpui-pre` and `cargo tree -p onehand-host -i gpui-pre`. Both must say "did not match any packages".
- Commits are `type(scope): summary` and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Decisions this plan makes (the owner reviews these)

- **Markdown, Neovim, Issues and Plugins are not offered on a host root.**
  - `WorkbenchModeSpec` gains `on_host: bool`, true for Editor only (Files lives inside it).
  - Markdown would need its doc index walked on the host, which is one more call. Neovim is local-only by the spec's non-goals.
- **A host's file ops are answered off the service's stream loop** (`spawn_blocking`), so a slow `git status` holds up no other ask. Only `Watch` keeps answering inline, because its replay order depends on it.
- **The Workbench attaches when a host root is shown,** as the project page does.
  - While the attach runs, it says *Attaching to <host>…*.
  - A failure stands in its place, in the attach's own words.
  - A busy host shows the waiting line, without a wait of its own (the page and the sessions own the wait).
  - Showing the project again, or the window becoming active, tries again: `refresh_worktree` re-resolves the attach for a host root.
- **Git status is read for a host root only while an attach to it is already live** (`hosts::live`). Opening the window never attaches to every host just to label the rail.
- **A tree folder that cannot be read on a host says so.** The Files view gets one failing line in place of the listing, *Could not read <folder> on <host>: …*. Locally, an unreadable folder still lists as empty, as today.
- **Terminals:**
  - **The shell.** The host runs `$SHELL -l` (or `/bin/sh -l`) in the project's folder, with `TERM=xterm-256color`, `COLORTERM=truecolor` and the service's login `PATH`.
  - **The buffer.** Each terminal keeps its last `TERM_KEEP = 1 MiB` of raw output. A watch replays the buffer, then goes live. A buffer that dropped older output starts its replay with the line `\r\n[Earlier output was not kept]\r\n`.
  - **Coming back.** When the dock shows a host project, the laptop lists that folder's terminals on the host and opens a tab on each. With none and the dock open, it starts one, as it does locally.
  - **Closing a tab** ends that shell on the host (`TermClose`), as closing a local tab ends its shell.
  - **A tab whose stream ended** leaves the dock without closing anything on the host. A lost connection is one way this happens.
  - **Running processes.** A host tab reports none to the removal warnings, because it runs on after removal.
- **Bytes on the wire are base64 strings** (`wire::Bytes`). JSON arrays of numbers would roughly triple a build log's size.

## Review Focus

1. **A host that drops while the tree or editor is open.** The next read or save fails with *lost the connection to <host>* in the place that asked (the failing tree line, the editor's save failure toast). It is never an empty tree that looks real. Showing the project again re-attaches. Task 3 tests a `Disk::Host` over a closed connection. Task 4 tests that `refresh_worktree` hands the Workbench a fresh `Disk`.
2. **Saving over a file the agent changed on the host.** The mtime guard runs on the host against the host's clock. A host save returns `Conflict`, as a local one does. Task 2 tests it.
3. **Keystrokes arriving out of order.** Every write to a host terminal goes through the attach's one command queue, in order, with no task per keystroke. Task 6 tests 100 writes arriving in order.
4. **A terminal outliving the laptop.** Detaching, then watching again, replays the output written while away, and marks a buffer that overflowed. Task 7 tests both.
5. **A local and a host project on one path.** Workbench and dock state is keyed by path. Only one of the two can be the active root at a time, and the active root's `Disk` decides. Two such projects already share rail state (a known gap); this change adds no new mixing. Task 4 tests that switching between them hands each its own `Disk`.

---

# PR 2a: tree, editor, git status

### Task 1: The file calls on the wire, and `Disk` in core

**Files:**
- Modify: `crates/core/src/host/wire.rs` (new calls and replies)
- Modify: `crates/core/src/tree.rs`, `crates/core/src/editor.rs`, `crates/core/src/gitstat.rs` (serde derives only)
- Create: `crates/core/src/host/client/files.rs` (the `HostConn` methods, so `client.rs` stays under the line)
- Create: `crates/core/src/disk.rs`, `crates/core/src/disk/tests.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod disk;`)
- Test: `crates/core/src/host/client/tests.rs`, `crates/core/src/host/wire/tests.rs`

**Interfaces:**
- Produces, in `wire.rs`:
  - `Call::ReadDir { path: PathBuf }` answered by `Reply::Listing { entries: Vec<TreeEntry>, cut: usize }`
  - `Call::ReadFile { path: PathBuf }` answered by `Reply::File { text: String, mtime: Option<SystemTime> }`
  - `Call::SaveFile { path: PathBuf, text: String, expected: Option<SystemTime> }` answered by `Reply::Saved(SaveOutcome)`
  - `Call::GitStatus { root: PathBuf }` answered by `Reply::Git(Option<GitStatus>)`
  - The `Answer` slots gain `listing`, `file`, `saved` and `git`.
- Produces: `Serialize`/`Deserialize` on `TreeEntry`, `GitStatus`, `FileChange` and `SaveOutcome`. Enable serde's `rc` feature in core's `Cargo.toml` for `SaveOutcome::Saved`'s `Arc<String>`.
- Produces, in `client/files.rs`:
  - `HostConn::read_dir(&self, dir: &Path) -> Result<(Vec<TreeEntry>, usize), HostError>`
  - `HostConn::read_file(&self, path: &Path) -> Result<(String, Option<SystemTime>), HostError>`
  - `HostConn::save_file(&self, path: &Path, text: String, expected: Option<SystemTime>) -> Result<SaveOutcome, HostError>`
  - `HostConn::git_status(&self, root: &Path) -> Result<Option<GitStatus>, HostError>`
  - `HostConn::host(&self) -> &str`
  - All of these are async.
- Produces, in `disk.rs`:
  ```rust
  #[derive(Clone)]
  pub enum Disk { Local, Host(HostDisk) }
  #[derive(Clone)]
  pub struct HostDisk { conn: HostConn, rt: tokio::runtime::Handle }
  impl HostDisk { pub fn new(conn: HostConn, rt: tokio::runtime::Handle) -> Self }
  impl Disk {
      pub fn read_dir(&self, dir: &Path) -> Result<(Vec<TreeEntry>, usize), String>;
      pub fn read_file(&self, path: &Path) -> Result<(String, Option<SystemTime>), String>;
      pub fn save_file(&self, path: &Path, text: String, expected: Option<SystemTime>) -> SaveOutcome;
      pub fn git_status(&self, root: &Path) -> Option<GitStatus>;
      /// The host's name, or None for this machine.
      pub fn host(&self) -> Option<&str>;
  }
  ```
  - Every method is blocking and has a doc comment saying never call it on a UI loop or a tokio worker.
  - `Local` wraps the existing functions, `read_dir` as `Ok`.
  - `Host` uses `rt.block_on(conn.…)`. Failures read *Could not read <dir> on <host>: <e>* for `read_dir`, *On <host>: <e>* for `read_file`, and `SaveOutcome::Failed("On <host>: <e>")` for `save_file`. `git_status` gives `None`.

- [ ] **Step 1: Write the failing tests**
  - `wire/tests.rs`: `file_calls_round_trip`. Serialize then deserialize one of each new `Call` and `Reply`. A `SaveOutcome::Saved` must keep its `text`, and a `GitStatus` its `entries`.
  - `client/tests.rs` (over the existing pipe harness `HostConn::over`):
    - `read_dir_takes_the_listing`: the fake host answers `Listing` with 2 entries and `cut: 3`. Expect `Ok((entries, 3))`.
    - `a_refused_read_file_is_a_protocol_error`: answer `Err("No such file")`. Expect `Err(HostError::Protocol(_))` whose text contains `No such file`.
    - `an_unexpected_answer_to_save_is_refused`: answer `Done` to a `SaveFile`. Expect `Err(Protocol)`.
  - `disk/tests.rs`:
    - `local_reads_and_saves_like_the_editor`: use a tempdir. `Disk::Local.save_file` then `read_file` round-trips. A second save with the stale mtime after an outside write gives `Conflict`.
    - `a_host_disk_over_a_closed_connection_says_so`: build a `HostConn` over a pipe whose far end is dropped, and call `read_dir` from a plain `std::thread`. Expect `Err` containing `on vps`. `git_status` is `None`, and `save_file` is `Failed`.
- [ ] **Step 2: Run them, and see them fail to compile**
  - Run: `cargo test -p onehand-core host:: disk::`
- [ ] **Step 3: Implement the wire variants, the derives, the `HostConn` methods and `Disk`**
  - `HostConn::over` is `#[cfg(test)]`. The disk test needs it from `disk/tests.rs`, so make it `pub(crate)` under `cfg(test)`.
- [ ] **Step 4: Run the tests and see them pass**
  - Run: `cargo test -p onehand-core`
  - Expected: all pass.
- [ ] **Step 5: Commit** `feat(host): read and write a host's files and git status from the laptop`

### Task 2: The host service answers the file calls

**Files:**
- Modify: `crates/host/src/serve.rs` (`answer`)
- Create: `crates/host/src/files.rs` (`fn answer_file(call) -> Result<Reply, String>`, run on `spawn_blocking`)
- Test: `crates/host/src/serve/tests.rs`

**Interfaces:**
- Consumes: Task 1's `Call`/`Reply` variants, and `tree::read_dir_sorted`, `editor::read_blocking`, `editor::save_blocking` and `gitstat::read_blocking`, unchanged.
- Produces: in `answer`, a file call spawns `tokio::task::spawn_blocking(move || files::answer(call))`. Its result is sent as `FromHost::Answer { id, result }` on `out` when done. Every other call stays inline.

- [ ] **Step 1: Write the failing tests** in `serve/tests.rs`, through the existing socket harness:
  - `the_host_lists_a_folder`: a tempdir holding `b.txt` and the folder `a/`. `ReadDir` answers `Listing` with `a` first, then `b.txt`, and `cut: 0`.
  - `the_host_reads_and_saves_a_file`: `ReadFile` then `SaveFile` with the mtime read gives `Saved`. Then write the file from the test and wait 10 ms so the mtime moves. A `SaveFile` with the old mtime gives `Saved(Conflict { .. })`.
  - `a_missing_file_is_refused`: `ReadFile` of a missing path gives `Err(_)`.
  - `git_status_of_a_plain_folder_is_none`: `GitStatus` on a tempdir with no repo gives `Git(None)`.
  - `a_slow_call_holds_up_no_other`: send `GitStatus` and `List` back to back. Both are answered; the order is not asserted.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand-host serve::`
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p onehand-host`
- [ ] **Step 5: Commit** `feat(host): serve the tree, files and git status`

### Task 3: The Workbench reads through a `Disk`

**Files:**
- Modify: `crates/workbench/src/lib.rs`
  - `WorkbenchMode::set_root(&mut self, root: &Path, disk: &Disk, cx: &mut App)`
  - `WorkbenchModeSpec.on_host: bool`
- Modify: every mode's `set_root` and `SPEC`: `crates/editor`, `crates/files`, `crates/markdown`, `crates/neovim`, `crates/issues`, `crates/claude-plugins`. Editor's spec has `on_host: true`, and every other spec has `false`.
- Modify: `crates/files/src/view.rs`
  - `scan` calls `disk.read_dir`.
  - A failure sets `failed: Option<(PathBuf, String)>`, drawn as one failing line in place of the rows.
  - A row's click takes the entry's own `is_dir` instead of `entry_is_dir` (a stat on the UI thread, and a host path is not on this disk). Delete `entry_is_dir`.
- Modify: `crates/editor/src/view.rs`
  - Keep each root's `Disk` beside its buffers (`disks: HashMap<PathBuf, Disk>`, set in `set_root` and dropped in `forget_root`).
  - Open goes through `disk.read_file` and save through `disk.save_file`, using the `Disk` of the root the buffer belongs to.
- Test: `crates/files/src/view/tests.rs` (create if missing), `crates/editor/src/view/tests.rs`

**Interfaces:**
- Consumes: Task 1's `Disk`.
- Produces:
  - `WorkbenchModeSpec { id, label, key_context, rem_zoom, on_host }`.
  - `set_root(root, &Disk, cx)` on every mode. A mode that touches no files ignores the disk.

- [ ] **Step 1: Write the failing tests** (GPUI `TestAppContext`, as the crates' existing view tests do):
  - Files, `a_folder_the_disk_cannot_read_says_so`: give a fake that always fails. `Disk` is an enum, so use a `Disk::Host` over a dropped pipe, as in Task 1. The view's `failed` is set, and the rows are empty.
  - Files, `a_row_opens_by_its_listed_kind`: a listing entry with `is_dir: true` whose path does not exist on this machine toggles open as a folder. It does not raise `OpenFile`.
  - Editor, `a_buffer_saves_through_its_roots_disk`: two roots, one `Disk::Local` over a tempdir and one over a dropped pipe. Saving a buffer of the second root fails, with words containing `On vps`. The tempdir file is untouched.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand-files -p onehand-editor`
- [ ] **Step 3: Implement**
  - Every other mode compiles with the new `set_root` signature, ignoring the disk.
- [ ] **Step 4: Run all tests**
  - Run: `cargo test --workspace --locked`
  - Expected: all pass. The app does not use `on_host` yet, so it hands `&Disk::Local` everywhere.
- [ ] **Step 5: Commit** `refactor(workbench): modes read and write through a disk`

### Task 4: The app shows a host project's tree, editor and git status

**Files:**
- Modify: `crates/app/src/hosts.rs`
  - `pub(crate) fn live(cx: &App, host: &HostName) -> Option<HostConn>`: a ready slot whose conn is not closed. It never attaches.
  - `pub(crate) fn disk(cx: &App, host: &HostName) -> impl Future<Output = Result<Disk, HostFailure>>`: `attach_with(.., false, false)`, then `Disk::Host(HostDisk::new(conn, acp.handle()))`.
- Modify: `crates/app/src/acp.rs`: `pub(crate) fn handle(&self) -> tokio::runtime::Handle`.
- Modify: `crates/app/src/workbench/panel.rs`
  - `set_root(PathBuf, Disk)`, keeping `on_host` from `disk.host().is_some()`.
  - A host root offers only modes whose spec is `on_host`, and an active mode that is not one switches to the first that is.
  - `refuse` takes `Option<SharedString>`, so a failure or *Attaching to <host>…* can be said.
- Modify: `crates/app/src/shell/roots.rs`
  - `arrival` / `arrive_at_active_root`: a local root goes as today with `Disk::Local`.
  - A host root first refuses the Workbench with *Attaching to <host>…*, then spawns `hosts::disk`.
    - On `Ok`, if that root is still the active one, it calls `workbench.set_root(path, disk)`.
    - On `Busy`, it refuses with `hosts::waiting_line`.
    - On `Other(why)`, it refuses with `why`.
  - The terminal stays refused, now with *Terminals on a host come later* (rename `ON_HOST_LATER` to `TERMINALS_ON_HOST_LATER`).
  - `refresh_worktree`: for an active host root, re-resolve the disk as in arrival, so a dropped attach recovers on window activation and at turn end.
  - `refresh_git` / `git_roots`:
    - Local roots as today.
    - A host root with `hosts::live` reads `Disk::Host(..).git_status(path)` on the background executor, in the same pass.
    - `window.git` stays keyed by path.
  - `Shell::open_file` for a host root opens in the editor (as a local one does) instead of the info toast.
- Modify: `crates/app/src/rail/project.rs`: *Refresh Git status* is shown for a host root too. *Open terminal* stays local-only until PR 2b.
- Test: `crates/app/src/shell/tests.rs`, `crates/app/src/workbench/panel/tests.rs` (create if missing)

**Interfaces:**
- Consumes: Task 1's `Disk`/`HostDisk`, and Task 3's `set_root(&Path, &Disk, cx)` and `WorkbenchModeSpec.on_host`.

- [ ] **Step 1: Write the failing tests**
  - Panel, `a_host_root_offers_only_modes_that_run_there`: modes Editor (`on_host`) and Markdown (not). `set_root(p, host_disk)` with Markdown active switches to Editor, and the mode list holds Editor alone. `set_root(p, Disk::Local)` offers both again.
  - Shell, `a_host_root_on_the_host_local_shows_its_tree`: a root on the host `local` (the 1b tests already attach to it) with a tempdir holding `x.txt`. Arriving at it ends with the Workbench not refused and Files listing `x.txt`. Use the existing host-test helpers in `shell/tests.rs`; if none attach for real, gate this one as the 1b smoke tests are gated.
  - Shell, `switching_between_a_local_and_a_host_root_on_one_path_hands_each_its_disk`: after arriving at the host root then the local root on the same path, the panel's disk is `Local`.
  - Shell, `a_file_link_in_a_host_session_opens_the_editor`: `open_file` on a host root no longer pushes the info toast.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand host_root`
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run all tests and the lint**
  - Run: `cargo test --workspace --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`
- [ ] **Step 5: Answer the `ui` skill's checklist** for the failing tree line, *Attaching to <host>…* and the mode list on a host. Write the answers into the commit body's notes for the reviewer, not into a file.
- [ ] **Step 6: Commit** `feat(hosts): a host project's tree, editor and git status`

### Task 5: Docs for 2a, then the PR

**Files:**
- Modify: `docs/hosts.md`
  - The intro and *What a host project does not do* lose the tree, editor and git status.
  - A new *Files and git status* section covers the `Disk` seam, which modes are offered, attaching when shown, git status only while attached, and the failing line.
  - Neovim, Markdown, Issues and Plugins are listed as not offered.
- Modify: `docs/known-gaps.md`: the "no files…" gap shrinks to the terminal only. Add *Markdown, Neovim, Issues and Plugins are not offered on a host project*, with why.
- Modify: `docs/screens.md`, only if it names the Workbench's refusal text for a host.
- Modify: the spec's *Order of work*: step 2 is "built in two parts: 2a (files) …".

- [ ] **Step 1: Write the docs.** English, no section codes cited from code.
- [ ] **Step 2: Run the full pre-PR checks** from *Global Constraints*.
- [ ] **Step 3: Commit** `docs(hosts): files, editor and git status on a host`
- [ ] **Step 4: Run a whole-branch review, then open the PR**
  - Branch: `feat/remote-hosts-files`
  - Title: `feat(hosts): files, editor and git status on a host`
  - Body: plain text.
  - Then wait for CI and ask the owner before merging.

---

# PR 2b: terminals on a host

Branch `feat/remote-hosts-terminal` from `origin/main` once 2a is merged.

### Task 6: Terminal lines on the wire, and the client's end

**Files:**
- Modify: `crates/core/Cargo.toml` (`base64 = "0.22"`)
- Modify: `crates/core/src/host/wire.rs`
  - `HostTerm(pub u64)` (transparent), and `Bytes(pub Vec<u8>)`, which serializes as a base64 string
  - Calls:
    - `Call::TermOpen { cwd, cols: u16, rows: u16 }` answered by `Reply::TermOpened { term: HostTerm }`
    - `Call::TermList` answered by `Reply::Terms(Vec<TermInfo>)`, with `TermInfo { term: HostTerm, cwd: PathBuf }`
    - `Call::TermWatch { term }`, which replays, then answers `Done`
    - `Call::TermWrite { term, data: Bytes }`
    - `Call::TermResize { term, cols, rows }`
    - `Call::TermClose { term }`
  - What the host sends:
    - `FromHost::TermOutput { term, data: Bytes }`
    - `FromHost::TermExit { term }`
- Create: `crates/core/src/host/client/terms.rs`
- Modify: `crates/core/src/host/client.rs`
  - The pump gains `terms: HashMap<HostTerm, std::sync::mpsc::Sender<Vec<u8>>>`.
  - `TermOutput` is sent to that term's sender.
  - `TermExit`, the stream ending, `Kicked` and a newer watch of the same term all drop the sender, which reads as EOF.
  - `Cmd::Fire(Call)` sends a call whose answer is dropped, keeping the queue's order.
- Test: `crates/core/src/host/client/tests.rs`, `crates/core/src/host/wire/tests.rs`

**Interfaces:**
- Produces, in `client/terms.rs`:
  - `HostConn::term_open(&self, cwd: PathBuf, cols: u16, rows: u16) -> Result<HostTerm, HostError>` (async)
  - `HostConn::term_list(&self) -> Result<Vec<TermInfo>, HostError>` (async)
  - `HostConn::term_watch(&self, term: HostTerm) -> TermReader`
  - `HostConn::term_writer(&self, term: HostTerm) -> TermWriter`
  - `HostConn::term_resize(&self, term: HostTerm, cols: u16, rows: u16)`: sync and fire-and-forget, safe off the runtime
  - `HostConn::term_close(&self, term: HostTerm)`: sync and fire-and-forget
  - `pub struct TermReader`, which implements `std::io::Read` (blocking recv on the mpsc; returns 0 once the sender is gone)
  - `pub struct TermWriter`, which implements `std::io::Write` (each `write` is one `Cmd::Fire(TermWrite)` sent synchronously on the unbounded queue; `flush` is a no-op)

- [ ] **Step 1: Write the failing tests**
  - Wire, `bytes_travel_as_base64`: `Bytes(vec![0, 255, b'\n'])` serializes to `"AP8K"` and back.
  - Client, `term_output_reads_in_order_and_ends_at_exit`: the fake host sends `TermOutput` "ab", then "cd", then `TermExit`. Reading the `TermReader` to the end on a std thread gives `abcd`.
  - Client, `writes_reach_the_host_in_order`: 100 `TermWriter::write` calls of `i.to_string()` from a std thread. The fake host sees 100 `TermWrite` lines in order.
  - Client, `a_lost_connection_ends_every_terminal_reader`: drop the far end. `read` returns `Ok(0)`.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand-core host::`
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p onehand-core`
- [ ] **Step 5: Commit** `feat(host): carry terminals between a host and the laptop`

### Task 7: The host service runs terminals

**Files:**
- Modify: `crates/host/Cargo.toml` (`portable-pty = "0.9"`)
- Create: `crates/host/src/terms.rs`, `crates/host/src/terms/tests.rs`
  - `Terms { next: u64, map: HashMap<HostTerm, Term> }`, behind its own `Arc<Mutex<_>>` (not the hub's lock)
  - `Term { cwd, writer, master, child, kept: VecDeque<u8>, dropped: bool, watcher: Option<UnboundedSender<FromHost>> }`
  - A std reader thread per terminal appends to `kept` (trimmed to `TERM_KEEP = 1024 * 1024`, setting `dropped`) and sends `TermOutput` to the watcher. Both happen under the lock, so a watch's replay and the live output neither overlap nor gap. At EOF it sends `TermExit`, reaps the child and removes the terminal.
- Modify: `crates/host/src/serve.rs`
  - `answer` handles the six term calls.
  - `TermWatch` is answered inline, after its replay, like `Watch`.
  - Unwatching on a kick or a lost attach clears the terminals' watchers too.
  - The login `PATH` is passed to `Terms` for the shell's environment.

**Interfaces:**
- Consumes: Task 6's wire types.
- Produces: `TERM_KEEP`, and `const NOT_KEPT: &str = "\r\n[Earlier output was not kept]\r\n"`, sent first in a replay when `dropped`.

- [ ] **Step 1: Write the failing tests** in `terms/tests.rs`, using `sh` via `SHELL=/bin/sh`:
  - `a_terminal_echoes_what_is_typed`: open, watch, then write `echo hi-$((1+1))\n`. Within 5 s the output contains `hi-2`.
  - `a_watch_after_a_detach_replays_what_came_meanwhile`: open, write `echo away\n` with no watcher, wait for it, then watch. The replay contains `away`.
  - `a_buffer_past_its_cap_says_it_dropped_output`: push more than `TERM_KEEP` bytes through the append function directly, since it is a pure helper. The replay starts with `NOT_KEPT`, and its length is at most `TERM_KEEP + NOT_KEPT.len()`.
  - `closing_a_terminal_ends_its_shell_and_lists_it_no_more`: open, close, then wait for `TermExit`. `list()` is empty.
  - `a_terminal_runs_in_its_folder`: write `pwd\n`. The output contains the tempdir's path.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand-host terms::`
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run them and see them pass**
  - Run: `cargo test -p onehand-host`
- [ ] **Step 5: Commit** `feat(host): terminals that run on while the laptop is away`

### Task 8: The dock shows a host project's terminals

**Files:**
- Modify: `crates/terminal-ui/src/lib.rs`
  - `PtyTab.pty` becomes `Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>`. `process_id`, `finished` and `busy` answer `None`/`false` without one. `Drop` reaps only a child it has.
  - New `pub fn stream_tab(label: SharedString, reader: impl Read + Send + 'static, writer: impl Write + Send + 'static, on_resize: impl Fn(usize, usize) + Send + Sync + 'static, font_size: Pixels, surface: Hsla, cx: &mut App, on_exit: impl Fn(&mut Window, &mut App) + 'static) -> PtyTab`. It builds the same `TerminalView` and config as `spawn_pty`; share a helper rather than copy it.
- Modify: `crates/app/src/terminal.rs`
  - A host root's dock keeps `host_terms: HashMap<EntityId, (HostConn, HostTerm)>` for its tabs (keyed by the view's entity id).
  - **Showing the dock on a host root.** It resolves the attach (`hosts::disk` gives the conn; add `hosts::conn` if cleaner). Then it calls `term_list`, filtered by `cwd == root`, and opens a `stream_tab` for each terminal not already a tab. With none and the dock open, it calls `term_open` with the grid's size, then opens a tab.
    - While this runs, the dock says *Attaching to <host>…*.
    - A failure or a busy host is said as in Task 4.
  - **Resizing** calls `term_resize`.
  - **The tab's exit callback** reaps the tab only. It never calls `term_close`.
  - **The tab's close action** calls `term_close` for a host tab, then removes it.
  - `processes()` leaves host tabs out.
- Modify: `crates/app/src/shell/roots.rs`: `arrive_at_active_root` hands a host root to the dock as well (no refusal). Delete `TERMINALS_ON_HOST_LATER`.
- Modify: `crates/app/src/rail/project.rs`: *Open terminal* is shown for a host root.
- Test: `crates/terminal-ui/src/tests.rs` (or beside it), `crates/app/src/terminal/tests.rs`

**Interfaces:**
- Consumes: Task 6's `term_*`, `TermReader` and `TermWriter`; Task 4's `hosts::disk`/`live`.

- [ ] **Step 1: Write the failing tests**
  - terminal-ui, `a_stream_tab_shows_what_its_reader_gives_and_exits_at_eof`: the reader is a `Cursor` of `b"hello"`. The grid's text contains `hello`, and `on_exit` ran once. `process_id()` is `None`.
  - App dock, `a_host_tab_that_exits_closes_nothing_on_the_host`: a fake conn records fired calls. Ending the reader removes the tab, and no `TermClose` was fired.
  - App dock, `closing_a_host_tab_closes_its_terminal`: the close action fires one `TermClose` for that term.
  - App dock, `host_tabs_are_not_running_processes`: `processes()` is empty with one host tab.
  - The fake conn: if `HostConn` cannot be faked in the app, test against the host `local` with the real service, gated as in Task 4.
- [ ] **Step 2: Run them and see them fail**
  - Run: `cargo test -p onehand-terminal-ui -p onehand terminal`
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run all tests and the lint**
  - Run: `cargo test --workspace --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`
- [ ] **Step 5: Answer the `ui` skill's checklist** for the dock on a host: attaching, a failure, the dropped-output line, and close against exit.
- [ ] **Step 6: Commit** `feat(hosts): the terminal dock on a host project`

### Task 9: Docs for 2b, then the PR

**Files:**
- Modify: `docs/hosts.md`: a *Terminals* section covering the shell, the 1 MiB buffer and its marker, coming back, close against exit, and no removal warning. *What a host project does not do* loses the terminal. *Not built* loses step 2.
- Modify: `docs/known-gaps.md`: drop the terminal gap. If it applies, add *A host terminal's tab gone with a lost connection comes back only when the project is shown again*.
- Modify: the spec's *Order of work*: step 2 is built.
- Modify: `CONTEXT.md`, only if a new term was used on screen (none is planned).

- [ ] **Step 1: Write the docs**
- [ ] **Step 2: Run the full pre-PR checks**
- [ ] **Step 3: Commit** `docs(hosts): terminals on a host`
- [ ] **Step 4: Run a whole-branch review, then open the PR**
  - Title: `feat(hosts): terminals on a host`
  - Then wait for CI and ask the owner before merging.
