# Remote hosts, step 1b: host projects in the window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A person adds a project that lives on a host. They start, watch, answer and stop its sessions in the chat pane like local ones. They close the laptop, and on opening it again they find every session and its whole transcript as the host kept them.

**Architecture:**
- **The project.** A `ProjectRoot` gains `host: Option<HostName>`.
- **The connection.** One `HostConn` per host lives on `Shared` (new `crates/app/src/hosts.rs`) and runs on the ACP runtime's tokio.
- **The session.** A host session is a chat whose events come from `HostConn::watch` instead of `acp::connect`, through the same `Events` channel, so `ChatSession` and `ChatPane` only branch where the stream starts. Its transcript is not written to the laptop's store; the host's journal is the transcript.
- **Local-only work.** Git, the Workbench, the terminal, worktrees, warm-up, tasks, issues, unattended runs and control tools skip a host root behind one predicate.

**Tech Stack:** Rust, GPUI + gpui-component, tokio (the app's own ACP runtime), `onehand_core::host`.

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md` (step 1, second half). Built on step 1a (#283).

## Global Constraints

- Everything in AGENTS.md *Invariants*, *Rules* and *Load-bearing specifics* holds. In particular:
  - Core stays GUI-free.
  - No wildcard arms on our enums.
  - Code describes and never cites a document.
  - Files stay under about 800 lines of code.
  - Tests go in `foo/tests.rs`.
  - In `crates/app` only `assets` and `shell` are `pub`.
- **Every UI change follows the `ui` skill** (`.claude/skills/ui/`), and Task 7 and Task 8 answer `checklist.md` item by item.
  - Buttons go through `crate::controls::action` with a named variant and size.
  - A refusing button uses `.refuses(why)`.
  - A failure is said once: a toast or an inline line.
  - Lists are capped and say how many they left out.
  - Never hard-code a colour or a size.
- **Words follow CONTEXT.md *Hosts*:** host, host service, attach, journal, build. Never "server", "remote", "cloud" or "VPS" on screen.
- **Agents' env is never sent to a host.** The laptop's `AgentSpec::launch` never runs for a host session, since it checks the laptop's PATH.
- **No tokio I/O on the GPUI executor.** Host work runs on `AcpRuntime`'s runtime, and results cross back through a `futures` channel or a oneshot awaited in `cx.spawn`.
- Before the PR, run:
  - `cargo test --workspace --locked`
  - `make clippy CLIPPY_EXTRA="-- -D warnings"`
  - `make fmt`
  - `cargo tree -p onehand-core -i gpui-pre` and `cargo tree -p onehand-host -i gpui-pre`, which must say "did not match any packages"
- Commits are `type(scope): summary`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Decisions this plan makes (the owner reviews these)

- **Where hosts persist.** A host root is persisted as a parallel `hosts` map in `onehand-workspace.toml` (path → host), following the precedent of `pinned`, `unattended` and `checks`. `roots` stays a list of paths, so older files still read.
- **Root identity is `(host, path)`.** `add_root` dedupes on the pair, and a host path is never canonicalized.
  - The app's per-root maps are keyed by path alone, but none of them is used for a host root. They are git, workbench, terminal and checks, all skipped.
  - A local `/srv/app` beside a host `/srv/app` therefore shares only the rail's fold state and MRU order. This is a known gap, written in `docs/known-gaps.md`.
- **The host name `local`** (exact) means "this machine, without ssh" (`HostAddr.local`). The owner and `/drive` can then try host projects with no SSH host. It is documented in docs/hosts.md.
- **Adding a host project.**
  - The rail gets a second row after *Add project…*: *Add a project on a host…*. It opens a dialog with *Host* and *Folder*.
  - *Add* attaches to the host (installing it if needed), checks the folder is a directory there, then adds the root.
  - Picking from `~/.ssh/config`, browsing folders and cloning stay in step 3.
- **The project page of a host root** lists the host's sessions on that folder (`HostConn::list`, filtered by cwd), newest first, capped at 50. Each row reads `agent · started <age> · running|ended`. Opening a row attaches its session in the chat pane.
- **Restart on a host session.** It re-watches the session if the host still runs it. If the session has ended, Restart opens a new host session with `Opening::Resume(<the agent's session id from the transcript>)`.
- **A lost connection** ends the session's stream with the client's `Disconnected("lost the connection to <host>")`. Restart re-attaches. Retrying by itself, with a backoff, stays in step 3, and docs/known-gaps.md says so.
- **A busy build mismatch** (`HostError::Busy`) opens a `Confirm`. It names the host and its mid-turn sessions, and asks whether to restart the host service onto this build, which stops them. On confirm, a new core call `client::restart` shuts the service down regardless of what is busy, then connects.
- **Replay times.**
  - Each journal line carries `at` (unix seconds). The client yields `AcpEvent::At(secs)` before an entry whose second differs from the last one it said.
  - `Chat::apply(At(t))` makes every wall-clock stamp in `apply` use `t` until the next `At`. Live entries carry `at` too.
  - Durations measured with `Instant` (a thought's or a tool's elapsed time) read short on a replay. That is a known gap.
- **"Agent not found" names the host.** `HostConn::open`'s error is prefixed `On <host>: `.
- **Own types for ids:** `HostSession(String)` and `Build(String)` in `host::wire`, serde-transparent.
- **The bundled binary** is `current_exe()`'s directory joined with `onehand-host`.
  - If it is missing, the failure reads: "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`".
  - `make dev` and `make showcase` build `-p onehand-host` too.

## Review Focus

1. **Laptop asleep, then woken:** the chat pane of a host session shows *lost the connection*. Restart re-attaches and the transcript is whole, with no duplicated items. *Task 5,* `restart_rewatches_a_running_host_session`.
2. **A host project after the app restarts:** the rail shows it with its host, and its page lists the sessions the host kept. Opening one replays it. *Task 3,* `a_host_root_round_trips_through_the_workspace_file`; *Task 8,* `host_sessions_are_listed_for_their_folder_newest_first`.
3. **Nothing local runs on a host root.** No `git status`, no warm agent, no Workbench rescan, no terminal cwd and no `@`-scan of a laptop folder with that path. *Task 6,* `a_host_root_is_never_given_local_work`.
4. **The agent missing on the host:** the session ends with a line that names the host. *Task 2,* `open_errors_name_the_host`.
5. **The bundled `onehand-host` missing** (a `cargo run` without it): adding a host project says how to build it, once, inline in the dialog. *Task 4,* `a_missing_bundled_binary_is_said_plainly`.

---

### Task 1: Times and ids on the wire (core)

**Files:**
- Modify: `crates/core/src/host/{wire.rs, journal.rs, client.rs}`, `crates/host/src/hub.rs`
- Modify: `crates/core/src/acp/types.rs` (`AcpEvent::At`), `crates/core/src/chat/model/apply.rs`
- Modify: every exhaustive `AcpEvent` matcher rustc names
- Test: `host/{wire,journal,client}/tests.rs`, `crates/host/src/hub/tests.rs`, `chat/model/tests.rs`

**Interfaces — Produces:**
- `pub struct HostSession(pub String)` and `pub struct Build(pub String)`. Both `#[serde(transparent)]`, `Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd`, with `Display`. They replace the bare `String` session ids and build ids in `Call`, `Reply`, `FromHost`, `SessionInfo`, `HostError::Busy`, the hub and the client. `build_id(&[u8]) -> Build`.
- `FromHost::Entry { session, seq, at: u64, entry }`.
- A journal line is `{"at":<secs>,"entry":<Entry>}`. Reading falls back to a bare `Entry` line with `at: 0`, so a 1a journal still reads.
- `journal::read` and `read_from` yield `(seq, at, Entry)`. `Journal::append(&mut self, at: u64, entry: &Entry)`.
- `AcpEvent::At(u64)`: "the events after this one happened at this unix second; a host replay says it".
- `Chat::apply(At(t))` sets the chat's clock override, and the wall-clock stamps in `apply` read it: `event_at` (apply.rs:164), the background and subagent `last_activity` (:317, :327), `finish_active_turn`, and `push_user`'s `sent_at` (turn.rs:529). A local session never yields `At`, so it keeps `now_secs()`.
- `HostConn::watch` yields `At(at)` before an entry when `at` differs from the last `At` it yielded on that stream.

- [ ] **Step 1: Write the failing tests:**
  - `a_journal_line_keeps_its_time` and `a_bare_entry_line_still_reads`, in journal tests.
  - `watch_says_the_time_before_entries_whose_second_changed`, in client tests: entries at 100, 100 and 105 yield `At(100)`, e1, e2, `At(105)`, e3.
  - `a_replayed_prompt_and_turn_end_keep_the_journal_s_times`, in chat tests: `At(1000)`, `Echo(Prompt)`, `At(1060)`, `TurnEnded`. The user item has `sent_at == Some(1000)`, its `completed_at` is 1060, and `last_activity` is 1060.
  - A hub test: `recorded_entries_carry_their_time`.
- [ ] **Step 2:** Run `cargo test -p onehand-core host:: chat::model` and `cargo test -p onehand-host`. Expect a compile failure.
- [ ] **Step 3:** Implement. The hub stamps `at` with the unix seconds helper when it records. Add the exhaustive `At` arms elsewhere: `subagent.rs`, `agent_check.rs`, `acp_smoke`, `busy_after` keeps the value it was given, and `host_smoke` prints it.
- [ ] **Step 4:** Run `cargo test --workspace --locked`. Expect PASS.
- [ ] **Step 5:** Commit `feat(host): journal lines keep their time, ids get their own types`.

### Task 2: Probe, restart and named errors (core client + host)

**Files:**
- Modify: `crates/core/src/host/{wire.rs, client.rs}`, `crates/host/src/{hub.rs, serve.rs}`
- Test: client and serve or hub tests

**Interfaces — Produces:**
- `Call::Probe { path: PathBuf }` → `Reply::Probed { is_dir: bool }`. The host answers with `std::fs::metadata(path).is_ok_and(|m| m.is_dir())`.
- `HostConn::is_dir(&self, path: &Path) -> Result<bool, HostError>`.
- `pub async fn restart(addr: &HostAddr, remote: &str, build: &Build) -> Result<HostConn, HostError>`: attach, send `Shutdown` whatever is busy, then attach again, and fail with `Protocol` if the second attach still finds another build.
- `HostConn` remembers its `HostAddr.ssh`, and `open`'s errors read `On <host>: <why>`. This applies to every `Err` from `open`: refused, unreachable or protocol.

- [ ] **Step 1: Write the failing tests:**
  - `probe_answers_whether_a_folder_is_there`: hub or serve against a temp dir and a missing path.
  - `restart_shuts_down_a_busy_host_and_reattaches`: the client over the duplex fake counts one `Shutdown` despite `busy` being non-empty.
  - `open_errors_name_the_host`: the fake host refuses `Open` with "`claude` is not on this machine — …", and the error text starts with "On vps: ".
- [ ] **Step 2:** Run them. Expect FAIL.
- [ ] **Step 3:** Implement. `Probe` is answered in serve's `answer` without the hub.
- [ ] **Step 4:** Run `cargo test --workspace --locked`. Expect PASS.
- [ ] **Step 5:** Commit `feat(host): probe a folder, restart a busy host, name the host in errors`.

### Task 3: A project on a host (core workspace)

**Files:**
- Modify: `crates/core/src/workspace.rs`, `crates/core/src/config.rs` (`WorkspaceConfig`)
- Test: `crates/core/src/workspace/tests.rs`, or the existing workspace tests file

**Interfaces — Produces:**
- `pub struct HostName(pub String)` in `crates/core/src/workspace.rs` (serde transparent, `Display`), with `HostName::addr(&self) -> host::client::HostAddr`, which gives `local: true` for exactly `local`.
- `ProjectRoot.host: Option<HostName>`, plus `ProjectRoot::on_host(&self) -> Option<&HostName>`.
- `WorkspaceConfig.hosts: BTreeMap<PathBuf, HostName>` (`#[serde(default)]`), written by `to_config` and re-matched in `from_config` as `pinned` is.
- `Workspace::add_host_root(&mut self, host: HostName, path: PathBuf) -> usize`: no canonicalize; dedupes on `(host, path)`; selects an existing twin. `add_root` (local) dedupes on `(None, path)`.
- `label_for` takes the last path component, as for a local root.

- [ ] **Step 1: Write the failing tests:**
  - `a_host_root_round_trips_through_the_workspace_file`: `to_config` → TOML → `from_config` keeps the host, the path as typed, the label and the pin.
  - `a_host_root_and_a_local_root_with_one_path_are_two_roots`.
  - `adding_the_same_host_root_twice_selects_it`.
  - `a_workspace_file_without_hosts_still_reads`.
  - `the_host_named_local_is_this_machine`.
- [ ] **Step 2:** Run `cargo test -p onehand-core workspace`. Expect FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p onehand-core`. Expect PASS.
- [ ] **Step 5:** Commit `feat(workspace): a project may live on a host`.

### Task 4: One attach per host (app)

**Files:**
- Create: `crates/app/src/hosts.rs` (private module; add it to `lib.rs`'s list)
- Modify: `crates/app/src/state.rs` (`Shared.hosts`), `crates/app/src/acp.rs` (a `pub(crate)` spawn onto its runtime)
- Modify: `Makefile` (`dev` and `showcase` build `-p onehand-host` too)
- Test: `crates/app/src/hosts/tests.rs`

**Interfaces — Produces:**
- `pub(crate) struct Hosts`, on `Shared` as `hosts: Hosts`. Each `HostName` holds either a ready `HostConn` or the last failure.
- `pub(crate) fn conn(cx: &App, host: &HostName) -> impl Future<Output = Result<HostConn, HostFailure>>`:
  - It reuses a ready connection.
  - Otherwise it runs `install(&addr, &bundled_binary()?)` then `connect(..)` on the ACP runtime.
  - A connection whose pump has ended is dropped and made again on the next call.
  - It is awaited in `cx.spawn`.
- `pub(crate) enum HostFailure { Missing(String), Busy { host: HostName, build: Build, busy: Vec<HostSession> }, Other(String) }`. `Missing` is the bundled-binary text. `Other` is a sentence a person reads, naming the host.
- `pub(crate) fn restart(cx: &App, host: &HostName) -> impl Future<Output = Result<HostConn, HostFailure>>`, through `client::restart`.
- `fn bundled_binary() -> Result<PathBuf, HostFailure>`: `current_exe()`'s directory joined with `onehand-host`.
- In acp.rs, `AcpRuntime::host_events(&self, conn: HostConn, start: HostStart) -> Events`, where `pub(crate) enum HostStart { Open { cwd: PathBuf, spec: AgentSpec, opening: Opening }, Watch(HostSession) }`:
  - It reuses `events()` and `forward()` unchanged.
  - `Open` runs `conn.open` then `conn.watch`. An `Err` becomes one `AcpEvent::Disconnected(text)`.
  - It returns the host session id through a oneshot beside `Events`, as `(Events, oneshot::Receiver<HostSession>)`, so the pane can store it.

- [ ] **Step 1: Write the failing tests:**
  - `a_missing_bundled_binary_is_said_plainly`: `bundled_binary_in(dir)` on an empty dir gives `Missing` with the exact text above.
  - `a_ready_connection_is_reused_and_a_dead_one_made_again`: with a fake connect fn injected through a crate-private seam, call count 1 after two `conn`s, and 2 after the first one's pump ends.
- [ ] **Step 2:** Run `cargo test -p onehand hosts`. Expect FAIL.
- [ ] **Step 3:** Implement. Only the connect step needs the seam; `install`/`connect` are the production fn.
- [ ] **Step 4:** Run `cargo test -p onehand --locked`. Expect PASS.
- [ ] **Step 5:** Commit `feat(app): one attach per host, on the ACP runtime`.

### Task 5: A host session in the chat pane (app)

**Files:**
- Modify: `crates/app/src/chat/session.rs` (`ChatSession::spawn`)
- Modify: `crates/app/src/chat/pane/sessions.rs` (`show`, `connect`/`connect_as`, `restart`) and the `Conversation` type
- Modify: `crates/app/src/shell/roots.rs` (`arrive_at_active_root` passes the root's host)
- Test: the pane or session tests beside them

**Interfaces:**
- Consumes: Task 3 `ProjectRoot::on_host`, Task 4 `Hosts::conn` and `AcpRuntime::host_events`.
- Produces:
  - `Conversation.host: Option<(HostName, Option<HostSession>)>`: set on the first show of a session in a host root, with its session id filled in once `host_events` reports it.
  - `ChatSession::spawn` branches on the host:
    - It awaits `Hosts::conn`, then calls `host_events`.
    - It gives `Chat::new(.., store: None)`, so nothing is written to the laptop's store.
    - It skips `completion::scan_files`.
    - It claims no control token.
  - A `HostFailure` ends the stream with one `Disconnected(text)`. `Busy` additionally emits a pane event, `ChatPaneEvent::HostBusy { host, build, busy }`, for the shell (Task 7).
  - Restart, from `ChatPane::restart`:
    1. If it has a host session id, it re-watches (`HostStart::Watch`) when `conn.list()` shows that session not ended.
    2. Otherwise it uses `HostStart::Open` with `Opening::Resume(<chat's ACP session id>)`.
    3. With neither, it uses `HostStart::Open` with `Opening::Fresh`.
  - Fork is refused on a host session: the header's fork action `.refuses(Some("Not on a host yet"))`.

- [ ] **Step 1: Write the failing tests** at the level the pane allows:
  - `restart_rewatches_a_running_host_session` and `restart_resumes_an_ended_host_session`: the restart choice lives in a pure fn `fn host_restart(info: Option<&SessionInfo>, acp_session: Option<&str>) -> HostStart` in `pane/sessions.rs`, tested directly.
  - `a_host_chat_has_no_store`: `Chat::new` is called with `None` for a host root, through whatever constructor the spawn uses.
- [ ] **Step 2:** Run. Expect FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p onehand --locked`. Expect PASS.
- [ ] **Step 5:** Commit `feat(chat): a host session in the chat pane`.

### Task 6: Nothing local for a host root (app)

**Files:** the call sites below; add one helper in `crates/app/src/shell/roots.rs`, `fn local_root(&self, idx) -> Option<&Path>`, which is `None` for a host root, and route each site through it.
- `Shell::refresh_git` (roots.rs:586) skips host roots.
- `arrive_at_active_root`:
  - `workbench.set_root` and `terminal.set_root` (roots.rs:184/186) are not called. The Workbench shows its empty state and the terminal dock refuses to open a shell, with the reason "Files and terminals on a host come later".
  - `warm_default_agent` (roots.rs:252) is skipped.
- `project_menu` (rail/project.rs:172-291) hides these items for a host root:
  - *New worktree…*
  - *Open terminal*
  - *Refresh Git status*
  - the unattended toggle
  - *Run check* / the check input

  *Copy project path* copies `<host>:<path>`.
- Pages and tools leave host roots out, or refuse them:
  - `page_projects` and `page_roots` (shell/sessions.rs:719/770) leave host roots out of Tasks, Issues and Workflows.
  - `unattended::tick` and `problem` skip them.
  - `control::pick_project` refuses them with "<project> is on a host; onehand's tools do not reach it yet".
  - `open_forwarded` and the CLI seed only ever add local roots, as now.
- The project page's conversation history (`scan_project_history`, `follow_archives`) is not run for a host root; Task 8 replaces it.
- **Test:** `a_host_root_is_never_given_local_work`, in `shell` tests or a pure helper's tests. A workspace with one local and one host root: the roots `refresh_git` would read, the warm target, and `page_roots` all hold the local root only.
- [ ] Steps: test first (fails), implement, `cargo test -p onehand --locked`. Commit `feat(app): a host project gets no local work`.

### Task 7: Adding a host project, and the rail (app UI)

**Files:**
- Create: `crates/app/src/dialogs/host_project.rs`, built from `dialogs/branch.rs`'s shape: draft on `Shell`, mounted in `shell/render.rs`, listed in `dialog_up()`.
- Modify: `crates/app/src/rail.rs` / `rail/model.rs` (`Row::AddHostProject` after `Row::AddProject`), `crates/app/src/rail/project.rs` (meta strip and `project_hint`)
- Modify: `crates/app/src/shell/roots.rs` (`open_host_root`), and a `Confirm` for `HostBusy` (from Task 5, and from the dialog's own attach)

**Interfaces:** consumes Task 2 `is_dir`, Task 3 `add_host_root`, Task 4 `Hosts::conn` / `restart`.

**Behaviour, copy fixed here:**
- **Rail row:** *Add a project on a host…*, the same builder and height as *Add project…*, directly under it.
- **Dialog title:** "Add a project on a host". Its fields:
  - **Host**: about "An alias from your SSH config, or user@machine". It is focused on open.
  - **Folder**: about "The project's folder on that host, from /". Both use `text_input(..).tall()`.
- **Footer:** *Cancel* (quiet) and *Add* (primary, `.tall()`).
  - *Add* `.refuses(..)` with "Name the host", "Name the folder", or "The folder starts at /".
  - It shows `.loading(true)` while it attaches and probes.
- **Errors** are shown once, inline under the fields in the danger ink, as `branch.rs` does:
  - the `HostFailure` text;
  - "There is no folder <path> on <host>" when `is_dir` is false.

  On success the dialog closes and `open_host_root(host, path)` runs: `add_host_root`, select, show, save. There is no git refresh.
- **Busy:** a `HostFailure::Busy` closes nothing. It opens `Confirm`:
  - Title: "Restart the host service on <host>?"
  - Body: "<host> runs another build of the host service, and these sessions are mid-turn: <names, capped at 5, then 'and N more'>. Restarting stops them."
  - Confirm label: *Restart*. On confirm, `Hosts::restart`, then the original action continues.
- **Rail project row:**
  - The meta strip starts with the host name in the meta ink (plain text, no badge), before the branch slot, which a host root never fills.
  - `project_hint`'s last line is `<host>:<path>` in the mono face.
  - The folder icon stays.
- **Test:** pure helpers for the refusal reasons (`fn add_refusal(host: &str, folder: &str) -> Option<&'static str>`) and for the busy body's names cap, tested in `dialogs/host_project/tests.rs`.
- [ ] Steps: tests first, implement, answer `.claude/skills/ui/checklist.md` items 1–17 in the task report (yes / no with file:line / n/a with why), `cargo test -p onehand --locked`. Commit `feat(app): add a project on a host`.

### Task 8: A host project's page (app UI)

**Files:**
- Modify: `crates/app/src/chat/pane/project_page.rs`, or a sibling `project_page/host.rs` if the file would pass ~800 lines.

**Behaviour:**
- **Fetching.** For a host root, the page asks `conn.list()` when shown and on a *Refresh* quiet action in its head.
- **The list:**
  - It keeps the sessions whose `cwd` equals the root's path, newest first by the id's leading seconds, capped at `HOST_SESSIONS_CAP = 50`, ending on "N more on this host" when the cap bites.
  - A row reads `agent · started <age> · running|ended`, with `running` in the running ink when busy.
  - Pressing a row opens it, as an existing conversation opens: a session minted in the root, its `Conversation.host` set to that id, and `HostStart::Watch`.
- **Waiting:** "Attaching to <host>…" with the library's spinner.
- **Failure:** the `HostFailure` text as the page's failed line, with *Try again*.
- **Empty:** "No sessions on <host> for this folder yet".
- **Test:** a pure `fn host_rows(list: &[SessionInfo], cwd: &Path, cap: usize) -> (Vec<&SessionInfo>, usize)` in `host_sessions_are_listed_for_their_folder_newest_first`, which also checks the cap and the left-out count.
- [ ] Steps: test first, implement, answer the ui checklist in the report, `cargo test -p onehand --locked`. Commit `feat(app): a host project's page lists its sessions`.

### Task 9: Documents

**Files:**
- Create: `docs/hosts.md`, the feature doc: what a host is, adding one, sessions, attach and detach, the journal and replay, builds and the restart offer, the `local` host, and a *Not built* section linking the steps still to come.
- Modify:
  - `AGENTS.md`: add `docs/hosts.md` to "The features that span core and app".
  - `docs/screens.md`:
    - *Rail*: the *Add a project on a host…* row, and the host name in a project row's meta.
    - *Pages without a session*: a host project's page.
  - `docs/known-gaps.md`: one bullet each, in its format:
    - files, the editor and the terminal on a host;
    - no automatic re-attach;
    - a local and a host root on one path sharing fold and MRU state;
    - replayed durations reading short;
    - no onehand tools, workflows, tasks, issues, unattended runs or worktrees for host projects;
    - attachments not carried;
    - forking a host session.
  - `CONTEXT.md` *Hosts*: add **Host project** ("a project whose folder lives on a host; its sessions run there") with _Avoid_: remote project.
  - `CHANGELOG.md` *Hosts*: projects on a host can be added and worked in the window.
  - `docs/superpowers/specs/2026-11-03-remote-hosts-design.md`: mark steps 1a/1b built in *Order of work*.
- [ ] Commit `docs(hosts): host projects in the window`.

---

## Not in this step

- **Step 2:** the file tree, editor, git and terminal on a host.
- **Step 3:**
  - picking a host from `~/.ssh/config`, a folder browser and clone;
  - re-attaching with a backoff;
  - reading a not-attached host's sessions from a laptop copy.
- **Step 4:** the permission bypass.
- **Steps 5–6:** offloaded projects.
