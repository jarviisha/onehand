# Remote hosts step 3: the host UI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a person pick a host from `~/.ssh/config`, browse its folders, clone a repository
there, and keep reading a host's sessions while it is not connected, re-attaching by itself.

**Architecture:** Two PRs. **3a** (`feat/remote-hosts-add`) grows the existing *Add a project on
a host* form: host suggestions parsed in core from `~/.ssh/config`, a folder list read over the
attach (`ReadDir` plus a new `Home` call), and a *Clone a repository* way that asks the host to
`git clone` (new `Clone` call). **3b** (`feat/remote-hosts-offline`) keeps a raw copy of each
host journal on the laptop (`host::copy` in core), watches from the copy's last seq, reads the
copy while the host is away, and re-attaches lost hosts with a backoff, restarting lost sessions
by themselves.

**Tech Stack:** Rust, GPUI + gpui-component, tokio (ACP runtime only), serde NDJSON wire.

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md` (*Order of work* step 3,
*Transcripts on the laptop*, *Adding a host project*, *Detached is a state*). As built so far:
`docs/hosts.md`. Read both, and `AGENTS.md`, before any task.

## Owner decisions (2026-11-04)

- **The laptop's copy is the journal's own lines**, `{seq, at, entry}`, at
  `<config_dir>/hosts/<host>/<session>.jsonl`, never `items.jsonl`. It is read back through the
  same entry→event rule the watch uses, so a replay never appends twice. The spec's "same store
  format" line is amended to say this (Task 10).
- **A lost connection (not taken by another machine) re-attaches by itself** with a backoff, while
  something of that host is shown; sessions lost with it re-watch by themselves, from where their
  copy ends. A host taken by another machine is still never taken back by itself.
- **One form, two ways:** *Open a folder* and *Clone a repository*, in the existing form.
  *Host* offers the aliases from `~/.ssh/config` and the hosts already in the workspace; the
  folder is picked from a list of the host's folders starting at its home.
- **Two PRs**, as above.

## Global Constraints

- `onehand-core` stays GUI-free: `cargo tree -p onehand-core -i gpui-pre` must error with "did
  not match any packages". `crates/host` never links GPUI.
- Core dictates no async runtime; the client is polled on the ACP runtime; tokio I/O never on the
  UI executor (`hosts::on_runtime`).
- Nothing in core is `pub` unless named outside the crate (`#![warn(unreachable_pub)]`).
- Match our own enums exhaustively, no wildcard arms (`crates/app/src/guards.rs`). New `Call`
  variants must be added to every exhaustive match (`crates/host/src/files.rs`,
  `crates/host/src/serve.rs`, `client.rs`'s `Answer`).
- Every UI change follows the `ui` skill (`.claude/skills/ui/`) and answers its checklist; no
  hard-coded colour, radius or size; buttons through `crate::controls::action`, menu rows through
  `controls::menu_item`/`menu_row`, menus through `onehand_ui::menu_below`.
- Every list is bounded by a named cap, and says on screen when the cap bites.
- Code describes, never cites a document. Every `.md` in English. Files under ~800 lines of code
  (split along a seam in the same PR).
- Checks before each PR: `cargo test --workspace --locked`,
  `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`. Never bare `cargo fmt`.
- Build output: `target/` is a symlink to `/data`; any extra worktree sets
  `CARGO_TARGET_DIR=/data/<name>-target`.
- Words on screen (exact):
  - Host suggestions menu trigger tooltip: *Pick a host*.
  - Folder list head: *Folders in <path> on <host>*; up row: *..*; cap line:
    *<n> more folders not shown*; empty: *No folders here*.
  - Way choice: *Open a folder* / *Clone a repository*.
  - Clone fields: *Repository* (*A git URL the host can clone with its own credentials*) and
    *Into* (*The folder on that host to clone into, from /*).
  - Clone refusals: *Name the repository*, *Name the folder to clone into*,
    *Cannot tell the repository's name from this URL*.
  - Clone busy label: *Cloning…*; failure: *Could not clone on <host>: <git's words>*;
    target exists: *There is already <path> on <host>*.
  - Not connected (rail tooltip, page line): *Not connected to <host>; trying again*; the
    session's notice: *Not connected to <host>. This is the copy kept on this machine, up to
    <rel time>; it carries on once the host is back*.

## Review Focus

1. **An ssh config with `Include`, `Match`, wildcards and negations** — only plain aliases are
   offered; a missing or unreadable file offers none, never an error. (Task 1 tests.)
2. **A laptop copy longer than the host's journal** (host data wiped, session id reused) — the
   host replays from the start and the laptop drops its copy; nothing is drawn twice. (Task 7.)
3. **A clone that asks for credentials** — it must fail, never hang the host or the form:
   `GIT_TERMINAL_PROMPT=0`, stdin null, a deadline. (Task 2.)
4. **A host taken by another machine** must not enter the reconnect loop. (Task 9.)
5. **A torn last line in the laptop's copy** (laptop crashed mid-append) — the copy reads up to
   the last whole line and the watch continues from that seq. (Task 6.)

---

# PR 3a — `feat/remote-hosts-add`

The branch already exists (renamed from `feat/remote-hosts-ui`, on `origin/main` 6725964b).

### Task 1: Aliases from `~/.ssh/config` (core)

**Files:**
- Create: `crates/core/src/host/sshconf.rs` (+ `sshconf/tests.rs`)
- Modify: `crates/core/src/host.rs` (add `pub mod sshconf;`)

**Interfaces:**
- Produces: `pub fn aliases(ssh_dir: &Path) -> Vec<String>` — reads `ssh_dir/config`, follows
  `Include` (a relative path is under `ssh_dir`; `~/` is the home; a `*` in the file name globs
  that one directory, sorted), depth-capped at 8 (`INCLUDE_DEPTH`), and returns every `Host`
  pattern that has none of `*`, `?`, `!`, in file order, deduplicated. Keywords are
  case-insensitive, `Host=a` and `Host a b` both parse, `#` comments and `Match` blocks are
  skipped (a `Match` block's lines are not `Host` lines). Unreadable files are skipped.
- Produces: `pub fn ssh_dir() -> Option<PathBuf>` — `dirs::home_dir()/.ssh`.

- [ ] **Step 1: Write the failing tests** in `sshconf/tests.rs`, each on a `tempfile` dir:
  `plain_aliases_in_order` (`Host vps\nHost box nas` → `["vps","box","nas"]`),
  `wildcards_and_negations_are_left_out` (`Host * !x dev-? ok` → `["ok"]`),
  `include_relative_and_glob` (`Include conf.d/*` with `conf.d/a` `Host a`, `conf.d/b`
  `Host b` → `["a","b"]`), `include_loop_stops` (a file including itself returns its aliases
  once), `missing_config_is_empty`, `equals_and_case` (`host=vps` → `["vps"]`),
  `duplicates_once`.
- [ ] **Step 2:** `cargo test -p onehand-core sshconf` → fails to compile.
- [ ] **Step 3:** Implement `aliases` / `ssh_dir`.
- [ ] **Step 4:** `cargo test -p onehand-core sshconf` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): read host aliases from the ssh config`.

### Task 2: `Home` and `Clone` on the wire (core, host, client)

**Files:**
- Modify: `crates/core/src/host/wire.rs` (+ `wire/tests.rs`), `crates/core/src/host/client.rs`
  (`Answer`), `crates/core/src/host/client/files.rs`, `crates/host/src/files.rs`,
  `crates/host/src/serve.rs` (dispatch off the loop, like the file calls)
- Test: `crates/core/src/host/client/tests.rs`, a test module beside `crates/host/src/files.rs`

**Interfaces:**
- Produces (wire): `Call::Home` → `Reply::Home(PathBuf)` (the host user's home, from
  `dirs::home_dir()`, error `"the host user has no home folder"`); `Call::Clone { url: String,
  into: PathBuf }` → `Reply::Done`.
- Produces (core): `pub fn clone_name(url: &str) -> Option<String>` in `wire.rs` — the last
  segment after `/` or `:`, trailing `/` and `.git` stripped; `None` when empty, `.` or `..`.
- Produces (client): `HostConn::home(&self) -> Result<PathBuf, HostError>`;
  `HostConn::clone_repo(&self, url: &str, into: &Path) -> Result<(), HostError>`, which waits
  `CLONE_WAIT = 15 min` instead of `ASK_WAIT` (add `ask_within(call, wait)`; `ask` calls it with
  `ASK_WAIT`). Errors from the host are wrapped with `on_host`.
- Host side: `Clone` refuses `into` that exists (`"there is already {into}"`) or a relative
  `into`; runs `git clone -- <url> <into>` with stdin null, `GIT_TERMINAL_PROMPT=0`,
  `GIT_ASKPASS=` (empty) and `SSH_ASKPASS_REQUIRE=never`; on failure the error is git's last
  non-empty stderr line (or `"git exited with <status>"`), and a partial `into` git left is
  removed only if this call created it.

- [ ] **Step 1: Failing tests.** `clone_name`: `https://github.com/o/repo.git` → `repo`,
  `git@github.com:o/repo.git` → `repo`, `/srv/git/repo/` → `repo`, `https://x/` → `None`,
  `..` → `None`. Host (`files.rs` tests, tempdir): `clone_makes_the_folder` (make a repo with
  `git init` + one commit in a tempdir, clone its path into `tmp/out`, assert `out/.git` exists),
  `clone_refuses_an_existing_folder`, `clone_failure_says_gits_line` (url `/nonexistent`, error
  non-empty and the target absent). Client (pipe harness in `client/tests.rs`):
  `home_reads_the_reply`, `clone_waits_past_ask_wait` (paused tokio clock: answer after 60 s →
  `Ok`).
- [ ] **Step 2:** `cargo test -p onehand-core host:: && cargo test -p onehand-host` → fail.
- [ ] **Step 3:** Implement. `Home` and `Clone` join the file calls answered off the stream loop.
- [ ] **Step 4:** Same commands → PASS. `cargo tree -p onehand-core -i gpui-pre` still errors.
- [ ] **Step 5:** Commit `feat(host): a host's home folder and clone over the attach`.

### Task 3: Host suggestions and the folder list in the form (app)

**Files:**
- Modify: `crates/app/src/dialogs/host_project.rs` (+ its `tests.rs`),
  `crates/app/src/shell/host_project.rs`, `crates/app/src/hosts.rs` (thin `on_runtime` wrappers)
- Create if `dialogs/host_project.rs` passes ~800 lines: `dialogs/host_project/browse.rs`

**Interfaces:**
- Consumes: `sshconf::aliases`, `sshconf::ssh_dir`, `HostConn::home`, `Disk::read_dir` (blocking: run it on the background executor) via
  `hosts::disk_over` (or `HostConn` listing directly), `Workspace` host roots.
- Produces: `pub(crate) fn host_choices(aliases: Vec<String>, used: Vec<HostName>) ->
  Vec<String>` (used hosts first, then aliases, deduplicated, capped at `HOST_CHOICES_CAP = 50`)
  in `dialogs/host_project.rs`; `HostProjectDraft` gains `browse: Option<Browse>` where
  `pub(crate) struct Browse { host: HostName, at: PathBuf, dirs: Vec<PathBuf>, cut: usize,
  error: Option<String>, loading: bool }`.
- Behaviour:
  - The aliases are read once when the form opens, on the background executor (never inline).
  - Beside *Host*, a quiet icon button (`IconName::ChevronDown`, tooltip *Hosts from your SSH
    config*) opens `menu_below` with `host_choices`; picking one fills *Host*. With no choices
    the button is not drawn.
  - Beside *Folder*, *Browse* (refused with *Name the host* while *Host* is empty) attaches
    (`hosts::attach_waiting`, showing the busy line as *Add* does), starts at *Folder* when it
    is an absolute folder the host lists, else at `home()`, and shows the folder list under the
    field: head *Folders in <path> on <host>*, an *..* row unless at `/`, one row per directory
    (`is_dir` entries only, at most `BROWSE_CAP = 200`, then *<n> more folders not shown*
    counting both the cap and the host's own `cut`), *No folders here* when none. Pressing a row
    sets *Folder* to that path and lists it. A failure is the list's own line, in danger ink.
  - Typing in *Host* drops the list (it was another host's).
  - Only the newest listing lands (an attempt counter, as `HostFlow::attempt` does).

- [ ] **Step 1: Failing tests** in `dialogs/host_project/tests.rs`:
  `host_choices_puts_used_first_and_dedups`, `host_choices_caps`, and a pure
  `browse_rows(at: &Path, entries: &[TreeEntry], cut: usize) -> (Vec<PathBuf>, usize)` test:
  files left out, sorted as given, `cut` added to the hidden count.
- [ ] **Step 2:** `cargo test -p onehand host_choices browse_rows` → fail.
- [ ] **Step 3:** Implement; follow the `ui` skill and answer its checklist in the commit body.
- [ ] **Step 4:** `cargo test -p onehand` → PASS; `cargo check`.
- [ ] **Step 5:** Commit `feat(hosts): pick a host and browse its folders in the form`.

### Task 4: *Clone a repository* (app)

**Files:**
- Modify: `crates/app/src/dialogs/host_project.rs` (+ tests), `crates/app/src/shell/host_project.rs`,
  `crates/app/src/hosts.rs`

**Interfaces:**
- Consumes: `HostConn::clone_repo`, `wire::clone_name`, Task 3's `Browse` (the list fills
  *Into* in this way).
- Produces: `pub(crate) enum Way { Folder, Clone }` on `HostProjectDraft` (`way: Way`, plus
  `repo: Entity<InputState>`), `pub(crate) fn clone_refusal(host: &str, repo: &str, into: &str)
  -> Option<&'static str>`, and `pub(crate) fn clone_target(repo: &str, into: &Path) ->
  Option<PathBuf>` (`into.join(clone_name(repo)?)`).
- Behaviour: a `onehand_ui::choice` row at the top, *Open a folder* / *Clone a repository*.
  In *Clone*: *Host*, *Repository*, *Into* (the *Folder* input, re-labelled; *Browse* fills it,
  starting at home). The primary button reads *Clone*, refuses per `clone_refusal` (and
  *The folder starts at /*), shows *Cloning…* while running, then probes nothing: a clone that
  returns `Ok` adds `clone_target` with `open_host_root` and closes the form. A failure stands
  inline, *Could not clone on <host>: …*, or *There is already <path> on <host>*. A busy host
  waits as *Add* does (`Want::Add`).

- [ ] **Step 1: Failing tests:** `clone_refusal` (each of the three words, `None` when all
  set and `into` absolute), `clone_target_joins_the_name`, `clone_target_none_for_a_bare_host`.
- [ ] **Step 2:** `cargo test -p onehand clone_` → fail.
- [ ] **Step 3:** Implement; `ui` skill checklist.
- [ ] **Step 4:** `cargo test -p onehand` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): clone a repository on a host`.

### Task 5: Docs for 3a

**Files:** `docs/hosts.md` (*Adding a host project*, *Not built*), `docs/screens.md` (the form,
if it is described there), `docs/known-gaps.md` (clone credentials are the host's; a clone
running past 15 min reads as failed), the spec's *Order of work* step 3 status line.

- [ ] **Step 1:** Update the documents to what was built; no code cites them.
- [ ] **Step 2:** Full checks (Global Constraints), then commit
  `docs(hosts): the form's host list, folder list and clone`.
- [ ] **Step 3:** Whole-branch review (opus, `origin/main..HEAD`), fix what it confirms, PR,
  CI, ask the owner before merging.

---

# PR 3b — `feat/remote-hosts-offline`

Branch from `origin/main` after 3a is merged.

### Task 6: The laptop's copy of a journal (core)

**Files:**
- Create: `crates/core/src/host/copy.rs` (+ `copy/tests.rs`)

**Interfaces:**
- Produces:
  - `pub struct Copy` — one session's copy at `<root>/<host>/<session>.jsonl`, the host part
    sanitized with `chat::store`'s filename rule (`sanitize`, made `pub(crate)` if needed).
  - `pub fn copies_dir() -> PathBuf` = `config_dir().join("hosts")`.
  - `impl Copy { pub fn open(root: &Path, host: &HostName, session: &HostSession) -> Copy;
    pub fn read(&self) -> journal::Entries; pub fn last(&self) -> Option<u64>;
    pub fn append(&mut self, seq: u64, at: u64, entry: &Entry) -> io::Result<()>;
    pub fn clear(&mut self) -> io::Result<()> }` — lines `{"seq":…,"at":…,"entry":…}`; `read`
    keeps whole lines only and skips ones that do not parse; `last` is the last whole line's seq
    (cached after the first read); `append` refuses a seq not above `last` (returns `Ok`,
    writes nothing). `open` does no I/O.
  - `pub fn events(entries: &journal::Entries) -> Vec<AcpEvent>` — the offline read: an
    `At(at)` before an entry whose second differs from the last said (0 says none), `Ready`
    dropped, `Event(e)` → `e`, `Request(r)` → `Echo(r)`. This is the same rule as the pump's;
    the pump's replay branch calls a shared `fn as_event(entry) -> Option<AcpEvent>` from here
    for the non-`Ready` cases.
  - `pub fn save_list(root: &Path, host: &HostName, list: &[SessionInfo]) -> io::Result<()>`
    and `pub fn load_list(root: &Path, host: &HostName) -> Vec<SessionInfo>` —
    `<root>/<host>/sessions.json`, written whole (temp file + rename).

- [ ] **Step 1: Failing tests:** `append_then_read_round_trips`, `append_below_last_is_ignored`,
  `torn_last_line_is_left_out` (write a partial line by hand; `read` and `last` ignore it, the
  next `append` starts on a fresh line), `clear_empties`, `events_say_time_once_per_second`,
  `events_drop_ready_and_echo_requests`, `list_round_trips`, `missing_list_is_empty`.
- [ ] **Step 2:** `cargo test -p onehand-core host::copy` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(hosts): keep a copy of a host session's journal on the laptop`.

### Task 7: Watch from where the copy ends (wire, host, client)

**Files:**
- Modify: `crates/core/src/host/wire.rs`, `crates/host/src/hub.rs` (+ tests),
  `crates/core/src/host/client.rs` (+ `client/tests.rs`), callers of `HostConn::watch`
  (`crates/app/src/acp.rs` host events)

**Interfaces:**
- Produces (wire): `Call::Watch { session, #[serde(default)] after: Option<u64> }`;
  `FromHost::Replay { session, #[serde(default)] after: Option<u64> }` — the host echoes
  `after` when its journal has a line with that seq (it then replays only seqs above it), and
  says `None` (replaying from the start) otherwise.
- Produces (client): `HostConn::watch(&self, session: &HostSession, copy: Option<Copy>) ->
  impl Stream<Item = AcpEvent>`. With a copy: sends `after: copy.last()`; on `Replay { after:
  Some(_) }` it first yields the copy's entries (through the same watcher path as replayed ones,
  so `Ready` is held and `At` said), then the host's tail; on `Replay { after: None }` it
  clears the copy and takes the host's whole replay. Every entry the watcher takes is appended
  to the copy (`// ponytail:` a small blocking append on the runtime thread; move to
  `spawn_blocking` if a copy lives on a slow disk).

- [ ] **Step 1: Failing tests.** Hub: `watch_after_replays_only_later_seqs`,
  `watch_after_past_the_end_replays_all_and_says_none`. Client (pipe harness):
  `watch_with_copy_yields_copy_then_tail` (copy holds seqs 0..2, host replays 3..4 with
  `after: Some(2)`: events in order, once each; the copy then holds 0..4),
  `watch_with_longer_copy_drops_it` (host says `after: None`: the copy is cleared and refilled
  from the host's replay, nothing yielded twice), `watch_without_copy_is_unchanged` (existing
  tests pass with `None`).
- [ ] **Step 2:** `cargo test -p onehand-core host:: && cargo test -p onehand-host` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS, and the 1a/1b replay tests still pass.
- [ ] **Step 5:** Commit `feat(hosts): watch a host session from where the laptop's copy ends`.

### Task 8: Reading a host session while it is not connected (app)

**Files:**
- Modify: `crates/app/src/acp.rs` (host events take a `Copy`), `crates/app/src/chat/session/host.rs`,
  `crates/app/src/chat/pane/project_page/host.rs`, `crates/core/src/chat` (a notice kind if one
  is needed, as `say_host_wait`)

**Interfaces:**
- Consumes: `Copy`, `copy::events`, `copy::save_list`/`load_list`, `copies_dir()`.
- Behaviour:
  - Every watch of a host session passes `Copy::open(&copies_dir(), host, id)`.
  - When the attach fails with `HostFailure::Other` for a `Watch`/`Rewatch` start whose copy is
    not empty, the session applies `copy::events(&copy.read())` to its `Chat` (no channel, so
    nothing reaches anything), adds the notice *Not connected to <host>. This is the copy kept
    on this machine, up to <rel time>; it carries on once the host is back* (the last entry's
    `at`, via `onehand_core::rel_time`), and joins the reconnect of Task 9 instead of ending.
    With an empty copy it ends as today.
  - The project page saves each list it reads (`save_list`); when its attach fails it shows
    `load_list` (filtered by folder, same cap and order) under the line *Not connected to
    <host>; trying again*, rows pressable (they open the offline read above).
- Tests: a pure `fn offline_start(copy_len: usize, failure: &HostFailure) -> OfflineStart`
  (`Read` / `End` / `Wait`) in `chat/session/host.rs`, tested for the three cases.

- [ ] **Step 1:** Failing test for `offline_start`.
- [ ] **Step 2:** `cargo test -p onehand offline_start` → fail.
- [ ] **Step 3:** Implement; `ui` skill checklist for the notice and the page line.
- [ ] **Step 4:** `cargo test -p onehand` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): read a host session from the laptop's copy while away`.

### Task 9: Re-attach a lost host by itself (app)

**Files:**
- Modify: `crates/app/src/hosts.rs` (+ `hosts/tests.rs`), `crates/core/src/host/client.rs`
  (`HostConn::closed`), `crates/app/src/shell/host_project.rs` (`HostFlow`), `crates/app/src/rail/project.rs`,
  `crates/app/src/rail/session.rs` + `rail/model.rs` (`SessionSignal`), `crates/app/src/chat/session.rs`,
  `crates/app/src/workbench/panel.rs` (joins instead of its own activation retry where simpler)

**Interfaces:**
- Produces (core): `HostConn::closed(&self) -> impl Future<Output = ()> + Send` (tokio
  `UnboundedSender::closed`).
- Produces (app):
  - `pub(crate) enum HostState { Unknown, Attaching, Live, Lost(String), Taken }` and
    `pub(crate) fn state(cx: &App, host: &HostName) -> HostState`.
  - `pub(crate) fn back(cx: &App, host: &HostName) -> Option<impl Future<Output = Result<(),
    String>>>` — like `idle`: listener-counted, one loop per host, `None` unless the host is
    `Lost`. The loop attaches with a backoff starting at `RECONNECT_FIRST = 2 s`, doubling to
    `RECONNECT_MAX = 60 s`; misses never end it; it ends `Ok` when an attach lands, `Err(why)`
    when the host turns out taken or busy-on-another-build (then the busy wait takes over, as
    today), and stops when its last listener goes.
  - `pub(crate) fn next_delay(prev: Option<Duration>) -> Duration` (pure, tested).
  - On every landed attach, a task on the runtime awaits `conn.closed()` and crosses back:
    not `taken()` → `Lost("lost the connection to <host>")`; taken → `Taken`. Each change calls
    `cx.refresh_windows()`.
  - A host session whose stream ended while its conn is closed and not taken (checked on the
    conn, never by matching the text) does not end: it keeps its transcript, says the notice of
    Task 8, gets `SessionSignal::HostLost` (rail draws it calm, *Not connected to <host>;
    trying again*, never as needing attention), and joins `HostFlow` as
    `Want::Back(uid)`; when `back` lands it is restarted (`ChatPane::restart`), whose re-watch
    now starts from the copy.
  - A host project's rail row shows the not-connected mark (a muted icon with the tooltip
    *Not connected to <host>; trying again*) while `state` is `Lost`, and *<host> was attached
    from another machine* while `Taken`. `Unknown` (never attached this run) shows nothing.
- Tests in `hosts/tests.rs`: `next_delay_doubles_to_the_cap`, `taken_never_reconnects`
  (state machine fed `closed` with `taken = true` → `Taken`, `back` is `None`),
  `lost_is_listened_to_until_the_last_listener_goes` (same harness as the busy waits' tests).

- [ ] **Step 1:** Failing tests above.
- [ ] **Step 2:** `cargo test -p onehand hosts::` → fail.
- [ ] **Step 3:** Implement; `ui` skill checklist for the rail mark and the session signal.
- [ ] **Step 4:** `cargo test --workspace --locked` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): attach a lost host again by itself`.

### Task 10: Docs for 3b

**Files:** `docs/hosts.md` (new *The laptop's copy* section; *Attach and detach*; *Not built*),
`docs/known-gaps.md` (drop the "not attached again by itself" and "blank for a moment" gaps;
add: copies are never pruned; terminals still come back only when shown), `docs/screens.md`
(rail mark, page line), `CONTEXT.md` (*copy* of a host session if it becomes a term), the spec
(*Transcripts on the laptop*: the copy is the journal's lines, not the store format; step 3
built).

- [ ] **Step 1:** Update; full checks; commit `docs(hosts): the laptop's copy and re-attaching`.
- [ ] **Step 2:** Whole-branch review (opus), fix, PR, CI, ask the owner before merging.
