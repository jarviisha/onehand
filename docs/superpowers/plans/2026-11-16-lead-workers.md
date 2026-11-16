# Lead workers: kept across a restart, closed by the lead, a model per turn — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A lead's workers come back when its conversation is opened again after a restart; a
lead can close a worker; `send_prompt` can name a model.

**Architecture:** Every rule lives in `onehand_core::control`. `ControlDrive` keeps the
relation, mints stable handles, and emits `Effect::Keep` whenever what is kept for a lead
changes. A new `control::kept` module owns the `workers.json` file beside the lead's conversation.
Coming back runs through the open pipeline the drive already has: resolve the folder off the UI
thread, reserve it, start the session. It skips the picks and the first prompt, and the session
is resumed on its conversation without connecting. The app only carries the effects out.

**Tech Stack:** Rust, GPUI (app only), serde_json, existing `onehand_core::chat::store`.

**Spec:** `docs/superpowers/specs/2026-11-16-lead-workers-design.md`. Read it before any task.

## Global Constraints

- Core stays GUI-free. After every task, `cargo tree -p onehand-core -i gpui-pre` errors with
  "did not match any packages".
- No async runtime in core. Blocking functions are named `*_blocking`, and the app runs them on
  `cx.background_executor()`.
- No `pub` in core unless something outside the crate names it. In `crates/app` new modules stay
  private.
- No comment or string cites a document. Comments give the reason in their own words.
- Match our own enums exhaustively, with no wildcard arms (`crates/app/src/guards.rs` checks).
- Split a file before it passes about 800 lines of code, tests not counted.
  `crates/core/src/control/drive.rs` is already at about 950, so Task 1 splits it first.
- Old handle shape `<run>-<uid>` is refused with today's words:
  `"{text} is from an earlier run of onehand; its worker is gone"`.
- `close_worker` answers `closed`. Its refusals, in this order:
  - the person holds it: today's `held_by_lead` words;
  - a card waits: `"a card waits on a person; only they answer it"`;
  - a turn runs: `"a turn is running; cancel it first"`.
- Kept file name: `workers.json`, inside the lead's conversation directory
  (`chat::conv_dir(store, session_id)`).
- Checks: `make fmt`, `cargo test -p onehand-core`, `cargo test -p onehand`,
  `make clippy CLIPPY_EXTRA="-- -D warnings"`. Commit messages are `type(scope): summary`, ending
  with the `Co-Authored-By` line.

## Review Focus

1. **A worker's folder is deleted** (worktree removed by hand) between quit and resume. It must
   become a closed record with a reason, and nothing is opened. Tested in Task 6.
2. **A kept worker's conversation is open as an ordinary session** in some window when the lead
   comes back. Its folder is held, so it must not open twice. Tested in Task 6.
3. **`workers.json` is unreadable or malformed.** Reading it yields nothing kept and never
   panics, and the next write replaces it. Tested in Task 3.
4. **The app crashes mid-write.** The write goes to a temporary file and is renamed, so the old
   file survives. Tested in Task 3.
5. **The lead uses an old handle after the person closed the lead and reopened its
   conversation.** It answers `link: closed`, not "not a worker you opened". Tested in Task 3.

---

### Task 1: Split the relation queries out of the drive

**Files:**
- Create: `crates/core/src/control/drive/relations.rs`
- Modify: `crates/core/src/control/drive.rs` (move lines about 414–528: `lead_of`, `workers_of`,
  `give_back_offered`, `give_back`, `find`, `last`, `held`, `sent`)

**Interfaces:**
- Produces: the same public methods on `ControlDrive`, now in `impl ControlDrive` inside
  `drive/relations.rs` (a `mod relations;` in `drive.rs`). Signatures unchanged.

- [ ] **Step 1:** Move the methods as they are, with their doc comments. Add a module header
  saying what the relation is, then add `mod relations;`.
- [ ] **Step 2:** Run `cargo test -p onehand-core control::` and expect every test to pass with
  no edit to any test.
- [ ] **Step 3:** Commit `refactor(control): keep a lead's relation with its workers in a module
  of its own`.

### Task 2: Handles that survive a restart

**Files:**
- Modify: `crates/core/src/control/open.rs` (`handle`, `parse_handle`)
- Modify: `crates/core/src/control/workers.rs` (`Relation` gains `handle: String`)
- Modify: `crates/core/src/control/drive.rs` and `drive/relations.rs` (handle map; mint at
  `Input::Started`)
- Modify: `crates/app/src/control/workers/turns.rs` (`find` resolves through the drive)
- Test: `crates/core/src/control/open/tests.rs`, `crates/core/src/control/drive/tests.rs`

**Interfaces:**
- Produces:
  - `pub fn mint_handle() -> String` in `open.rs`, built from `super::new_token()`. Shape
    `w-<32 hex>`, so it never parses as `<run>-<uid>`.
  - `pub fn parse_handle(text: &str, run: &str) -> Result<Handle, String>`, with
    `pub enum Handle { Stable(String) }`. The old `<run>-<uid>` shape of this run or an earlier
    one is refused with the earlier-run words. Anything else is
    `"{text} is not a session's handle"`.
  - `ControlDrive::uid_of(&self, handle: &str) -> Option<u64>` (live sessions only).
  - `ControlDrive::handle_of(&self, uid: u64) -> Option<String>`.
  - `ControlDrive::find(&self, caller: u64, handle: &str, open: impl Fn(u64) -> bool) ->
    Result<Found, String>`. It now takes the handle and resolves it itself, so
    `crates/app/src/control/workers/turns.rs::find` stops calling `parse_handle` and `this_run`.
  - `open_worker`'s answer `session:` line and its "still opening" line use the stable handle.

- [ ] **Step 1: Write the failing tests.**
  - `a_minted_handle_parses_and_is_not_a_run_handle`: `parse_handle(&mint_handle(), "r1")`
    is `Ok(Handle::Stable(_))`.
  - `a_run_uid_handle_is_from_an_earlier_run`: `parse_handle("r0-7", "r1")` is `Err` containing
    `"earlier run"`.
  - In the drive tests, `the_open_answers_with_a_stable_handle`: after the existing happy-path
    open, the answer's `session:` value starts with `w-`, and `uid_of` maps it to the started
    uid.
- [ ] **Step 2:** Run `cargo test -p onehand-core handle` and expect FAIL.
- [ ] **Step 3:** Implement. The handle is minted when the open's session starts
  (`Input::Started`), stored on `Relation.handle`, and indexed in a
  `handles: HashMap<String, u64>`. `closed_workers` is keyed by handle from now on:
  `HashMap<String, Closed>`, with `pub struct Closed { lead: u64, why: Option<String> }`.
- [ ] **Step 4:** Update every existing test that built or parsed `<run>-<uid>` handles, then run
  `cargo test -p onehand-core control:: && cargo test -p onehand control` and expect PASS.
- [ ] **Step 5:** Commit `feat(control): name a worker by a handle that outlives the run`.

### Task 3: Keep what a lead holds in `workers.json`

**Files:**
- Create: `crates/core/src/control/kept.rs`, `crates/core/src/control/kept/tests.rs`
- Modify: `crates/core/src/control/workers.rs`. `Relation` gains `folder: PathBuf`,
  `agent: String`, `name: Option<String>`, `conversation: Option<String>`, and
  `person: bool`. `person` is the hold carried across a restart; it is true when the worker came
  back held by the person, and `give_back` clears it.
- Modify: `crates/core/src/control/turns.rs` (`Last` gains `person: bool`, and `held_by_lead`
  refuses when it is true) and `state.rs` (`Held` gains `person: bool`, and `held by:` reads it)
- Modify: `crates/core/src/control/drive.rs` (new `Effect::Keep`, new `Input::Conversation`,
  `forget` writes closed records)
- Modify: `crates/app/src/control.rs` (`carry` handles `Effect::Keep`)
- Modify: `crates/app/src/chat/pane/sessions.rs` (`close` reads the closing session's
  conversation id before it is removed, for a lead's last `Keep`)
- Test: `kept/tests.rs`, `drive/tests.rs`, `turns/tests.rs`

**Interfaces:**
- Produces, in `kept.rs`:
  ```rust
  #[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
  pub struct Kept { pub workers: Vec<KeptWorker>, pub closed: Vec<KeptClosed> }
  pub struct KeptWorker { pub handle: String, pub conversation: String, pub agent: String,
      pub folder: PathBuf, pub dirs: Vec<PathBuf>, pub name: Option<String>,
      pub turn: Option<u64>, pub text: Option<String>, pub person: bool }
  pub struct KeptClosed { pub handle: String, pub why: Option<String> }
  pub fn read_blocking(conv_dir: &Path) -> Kept
  pub fn write_blocking(conv_dir: &Path, kept: &Kept) -> std::io::Result<()>
  ```
  (Same derives on the two inner structs.) `read_blocking` returns `Kept::default()` when the
  file is missing, unreadable or malformed, logging the last two with `log::warn!`.
  `write_blocking` writes `workers.json.tmp`, then renames it. An empty `Kept` removes the file.
- Produces, in the drive:
  - `Effect::Keep { lead: u64, kept: Kept }`. The `workers` list is in open order, and only
    relations with a known `conversation` are included.
  - `Input::Conversation { uid: u64, session_id: String }`, reported whenever a worker's chat
    gets or changes its `session_id`.
  - `Keep` is emitted after: `Input::Begun` writes a relation; `sent` moves it; `give_back`;
    `Input::Conversation`; `forget(worker)`, which moves the worker to `closed` with `why: None`;
    and `forget(lead)`. On a lead's forget, its relations become closed records with
    `why: Some("its lead was closed")` and `workers` goes empty. This is the owner's rule: the
    person closing the lead lets its workers go for good.
- The app carries `Effect::Keep` by finding the lead's conversation directory from its
  `Chat.session_id` and `chat::conversations_dir()`, then calling `write_blocking` on the
  background executor. A lead with no session id yet writes nothing. When a lead's session id
  changes, the app writes the current `Kept` to the new directory and removes the old file.
  Since `pane.close` runs `forget` after removing the conversation, it hands the drive the lead's
  directory first: `ControlDrive::forget` stays as it is, and the app reads the id before
  `conversations.remove(&uid)`.

- [ ] **Step 1: Write the failing tests.**
  - `kept/tests.rs`:
    - `a_kept_file_reads_back_as_written` (round trip through a temp dir);
    - `a_missing_file_keeps_nothing`;
    - `a_malformed_file_keeps_nothing` (write `"{not json"`, then read returns default);
    - `writing_nothing_removes_the_file`;
    - `a_write_leaves_no_temporary_file`.
  - `drive/tests.rs`:
    - `a_begun_worker_is_kept_once_its_conversation_is_known`: after `Input::Begun` and then
      `Input::Conversation`, the effects contain `Keep` with one worker carrying that
      conversation.
    - `closing_a_worker_keeps_a_closed_record`: `forget(worker_uid)` emits `Keep` with an empty
      `workers` and one `closed` holding its handle and `why: None`.
    - `closing_the_lead_closes_its_workers_for_good`: `forget(lead)` emits `Keep` whose `closed`
      lists every worker with `why == Some("its lead was closed".into())`.
    - `an_old_handle_of_a_closed_lead_reads_closed` (Review Focus 5): after `forget(lead)`, and
      after seeding the drive from that `Kept` (Task 6's `back`), `find` on the handle is
      `Ok(Found::Closed)`.
  - `turns/tests.rs`: `a_worker_held_across_a_restart_refuses_the_lead` checks that
    `send(chat, &Last { person: true, .. })` is `Err` with the held words, even with
    `chat.prompts_sent == 0`.
- [ ] **Step 2:** Run `cargo test -p onehand-core kept` and `cargo test -p onehand-core control::`
  and expect FAIL.
- [ ] **Step 3:** Implement `kept.rs` and the drive changes above. Add `pub mod kept;` to
  `control.rs`.
- [ ] **Step 4:** In the app, carry `Effect::Keep`. Report `Input::Conversation` from the watch
  the app already holds on each worker, when `chat.session_id` differs from the last one reported
  for that uid. Since `crates/app/src/control/workers.rs` drops its watch after the open answers,
  keep a light watch per worker for this.
- [ ] **Step 5:** Run `cargo test -p onehand-core && cargo test -p onehand control` and expect
  PASS.
- [ ] **Step 6:** Commit `feat(control): keep a lead's workers beside its conversation`.

### Task 4: `close_worker`

**Files:**
- Modify: `crates/core/src/control.rs` (`Call::CloseWorker { session: String }`, parsed from
  `"close_worker"` with `required("session")`, and its `None` arm in the project match)
- Modify: `crates/core/src/control/catalogue.rs` (the entry, `for_sessions_only: true`)
- Modify: `crates/core/src/control/turns.rs`
  (`pub fn close(chat: &Chat, last: &Last) -> Result<(), String>`)
- Modify: `crates/app/src/control.rs` (dispatch arm) and
  `crates/app/src/control/workers/turns.rs` (`pub(in crate::control) fn close(...)`)
- Test: `crates/core/src/control/turns/tests.rs`, `crates/core/src/control/tests.rs`,
  `crates/app/src/control/workers/tests.rs`

**Interfaces:**
- Consumes: `Last.person` (Task 3), and `ControlDrive::find` with handles (Task 2).
- The app's `close` finds the worker, checks `turns::close`, then defers
  `Shell::end_session(uid)` in the worker's window. That runs `pane.close`, then `forget`, then
  the closed record (Task 3); the project goes by the existing `workspace.end` rule. It answers
  `Ok("closed")` once the close is handed over. A closed worker's handle answers `"closed"` again,
  so a retry is harmless.
- Catalogue text: "Close a worker you opened: its session ends and its agent with it; its
  conversation and its folder stay. Refused while the person holds it, while a permission or a
  question waits on a person, and while a turn runs: cancel it first, and close once
  session_state says the turn is over".

- [ ] **Step 1: Write the failing tests.**
  - `turns/tests.rs`:
    - `close_refuses_a_held_worker`;
    - `close_refuses_while_a_card_waits`;
    - `close_refuses_while_a_turn_runs` (expects `"a turn is running; cancel it first"`);
    - `close_takes_an_idle_worker` (`Ok(())`).
  - `control/tests.rs`:
    - `close_worker_parses`;
    - `close_worker_is_never_listed_to_an_outside_program`, following the existing `cancel_turn`
      listing test.
- [ ] **Step 2:** Run `cargo test -p onehand-core close` and expect FAIL.
- [ ] **Step 3:** Implement the core parts, then the app's dispatch and `close`.
- [ ] **Step 4:** Run `cargo test -p onehand-core && cargo test -p onehand control` and expect
  PASS.
- [ ] **Step 5:** Commit `feat(control): let a lead close a worker it holds`.

### Task 5: `send_prompt` names a model

**Files:**
- Modify: `crates/core/src/control.rs` (`Call::SendPrompt` gains `model: Option<String>`,
  parsed as optional)
- Modify: `crates/core/src/control/catalogue.rs` (the `model` property, described as "A value
  of the worker's model option; picked before the prompt and kept for its next turns")
- Modify: `crates/core/src/control/drive.rs` (a pending turn per worker)
- Modify: `crates/app/src/control/workers/turns.rs` (`send` takes `model` and an `Answer`)
- Modify: `crates/app/src/control.rs` (dispatch passes the `Answer` through for this call)
- Test: `crates/core/src/control/drive/tests.rs`, `crates/core/src/control/tests.rs`

**Interfaces:**
- Consumes: `open::step(chat, None, Some(model), picked)`, unchanged. It already answers `Wait`,
  `Pick`, `Refused` (naming what the agent offers) and `Ready`.
- Produces:
  - `ControlDrive::turn_with_model(&mut self, uid: u64, turn: u64, text: String,
    model: String) -> Vec<Effect>`, which yields `Watch(uid)` and `Arm` with `OPEN_DEADLINE`.
    It is called only after `turns::send` answered `Send::Go`, so a retried turn picks nothing.
  - On `Input::Woke` for a uid with a pending turn, the drive runs `step`:
    - `Pick` gives `Effect::Pick`;
    - `Refused(why)` gives `Effect::TurnAnswer { uid, outcome: Err(why) }` and `Unwatch`;
    - `Ready` gives `Effect::Send { uid, turn, text }`.
  - The app carries `Effect::Send` by sending the prompt the way `send` does today, then calls
    `drive().sent(...)`. Its result goes out as the `TurnAnswer`.
  - `Elapsed` for the turn's token answers
    `Err("the agent did not answer the model pick in time")`.
  - `Effect::TurnAnswer { uid: u64, outcome: Result<String, String> }` routes to the `Answer`
    the app stored for that uid.
  - Without `model`, `send` behaves exactly as today and answers synchronously.

- [ ] **Step 1: Write the failing tests** in `drive/tests.rs`, using the chat fixtures the open
  tests use:
  - `a_turn_with_a_model_picks_before_sending`: after `turn_with_model` and a `Woke` on a chat
    offering the model, `Pick` comes before any `Send`.
  - `a_turn_with_a_model_sends_once_in_force`: a `Woke` with the model in force gives
    `Effect::Send`.
  - `a_model_not_offered_refuses_the_turn`: gives `TurnAnswer` `Err` naming what is offered,
    and no `Send`.
  - `a_model_pick_that_never_answers_runs_out`: `Elapsed` gives `TurnAnswer` `Err`.
  - In `control/tests.rs`: `send_prompt_takes_an_optional_model`.
- [ ] **Step 2:** Run `cargo test -p onehand-core model` and expect FAIL.
- [ ] **Step 3:** Implement the core pending turn (`turning: HashMap<u64, Turning>`), then the
  app's carrying.
- [ ] **Step 4:** Run `cargo test -p onehand-core && cargo test -p onehand control` and expect
  PASS.
- [ ] **Step 5:** Commit `feat(control): let a lead's turn name the worker's model`.

### Task 6: Workers come back with their lead

**Files:**
- Modify: `crates/core/src/control/drive.rs`. `Open` gains `back: Option<KeptWorker>`.
  Coming back runs the existing phases:
  - Resolving: `Target` is the kept folder (`Target::Worktree` when absolute and no project);
  - Starting: `Effect::Start` gains `resume: Option<String>`, the conversation's session id;
  - Started: with `back`, no picks and no first prompt. The relation is written from the kept
    entry, with its handle re-bound to the new uid. There is no `Answer`, since no call waits.
- Modify: `crates/core/src/control/drive/relations.rs`, adding
  `pub fn back(&mut self, lead: u64, kept: Kept, projects: &[(String, PathBuf)]) ->
  Vec<Begun>`: one `Begun::Resolve` per kept worker. The closed records are seeded at once, so
  their handles read `Found::Closed`.
- Modify: `crates/app/src/chat/pane/sessions.rs::resume`. Once the archive is read and the
  session is starting on it, it reads `kept::read_blocking(&archive)` off the UI thread, then
  calls `drive().back(lead_uid, kept, &projects)` and carries each `Begun` the way `open_worker`
  carries its own.
- Modify: `crates/app/src/control/workers.rs`. `Effect::Start` with `resume` adds the worker's
  session on that conversation without connecting. It resolves the conversation directory with
  `chat::find_conversation(store, &id)`, and a missing one fails the coming back with
  `"its conversation is gone"`. Sessions already connect on first show.
- Modify: `crates/app/src/control/workers/turns.rs::find`. A found worker whose session has not
  connected is connected now, and the call goes on as today, so `session_state` reads
  `link: connecting` and `send_prompt` is refused in the composer's connecting words.
- Test: `crates/core/src/control/drive/tests.rs`, `crates/app/src/control/workers/tests.rs`

**Interfaces:**
- Consumes: `Kept`, `KeptWorker` and `read_blocking` (Task 3); handles (Task 2).
- Every failure of a coming back (`fail` with `back`) turns the entry into a closed record:
  `KeptClosed { handle, why: Some(<the refusal's words>) }`. It emits `Keep` and closes only its
  own session, if one started. An agent no longer in the list refuses with
  `"its agent <name> is no longer in the agent list"`.
- `session_state` on a handle whose closed record has a `why` answers
  `"link: closed\nwhy: <why>"`.

- [ ] **Step 1: Write the failing tests** in `drive/tests.rs`:
  - `a_kept_worker_comes_back_under_its_lead`: `back` gives a `Resolve`. After `Resolved` and
    `Started`, `lead_of(new_uid) == Some(lead)`, `uid_of(handle) == Some(new_uid)`, and the
    `Start` effect carries `resume == Some(conversation)`. No `Pick`, `Begin` or `Answer` follow.
  - `a_kept_worker_whose_folder_is_gone_is_closed_with_why` (Review Focus 1): a `Resolved` with
    `Err("…: No such file or directory")` gives a `Keep` whose closed record has that `why`, and
    no `Start`.
  - `a_kept_worker_on_a_held_folder_does_not_come_back` (Review Focus 2): a `World` with a
    session on the folder gives a closed record and no `Start`.
  - `a_held_kept_worker_comes_back_held`: `person: true` round-trips, so `last(uid).person` is
    true.
  - `closed_records_read_closed_after_back`.
- [ ] **Step 2:** Run `cargo test -p onehand-core back` and expect FAIL.
- [ ] **Step 3:** Implement the core, then the app's resume hook, lazy start and connect-on-find.
- [ ] **Step 4:** Run `cargo test -p onehand-core && cargo test -p onehand` and expect PASS.
- [ ] **Step 5:** Commit `feat(control): bring a lead's workers back with its conversation`.

### Task 7: One lead conversation in one session

**Files:**
- Modify: `crates/app/src/chat/pane/sessions.rs` (the *Resume in this session…* path), and
  wherever the history opens a conversation (`crates/app/src/chat/pane/history.rs`,
  `project_page.rs`)
- Modify: `crates/core/src/control/drive/relations.rs`, adding
  `pub fn leads_workers(&self, uid: u64) -> bool`
- Test: `crates/core/src/control/drive/tests.rs`

**Interfaces:**
- *Resume in this session…* is refused while `leads_workers(uid)`, with a toast:
  `"This session leads workers; close them or the session first"`.
- Opening a conversation that a session of any window already has open (by `Chat.session_id`,
  read across windows through `crate::windows`) is refused in the words the history already uses
  for an open conversation. Find that string in `history.rs` and reuse it.

- [ ] **Step 1:** Write the failing test `a_lead_with_workers_leads_workers`, plus its negative
  after `forget` of each worker.
- [ ] **Step 2:** Run `cargo test -p onehand-core leads_workers` and expect FAIL.
- [ ] **Step 3:** Implement both guards.
- [ ] **Step 4:** Run `cargo test -p onehand` and expect PASS.
- [ ] **Step 5:** Commit `fix(chat): open a lead's conversation in one session only`.

### Task 8: Documents

**Files:**
- Modify: `docs/control.md` (*A worker*: the handle's shape, a new *Coming back* paragraph,
  `close_worker` in the tool table and its paragraph, `send_prompt`'s `model`)
- Modify: `docs/known-gaps.md` (remove *A lead's workers do not outlive the run of the app* and
  *A lead cannot close a worker, nor pick a model per turn*; the cap line drops "a finished worker
  holds its place … until the person closes it")
- Modify: `CONTEXT.md` (**Worker**: "…*Give back to the lead* returns it; the lead may close it.")
- Delete: `docs/superpowers/specs/2026-11-16-lead-workers-design.md` and this plan, in the PR's
  last commit, since the work is built.

- [ ] **Step 1:** Write the changes, describing what is built, not the plan.
- [ ] **Step 2:** Run `make lint`, `cargo test`, and
  `cargo tree -p onehand-core -i gpui-pre` (expect the "did not match" error).
- [ ] **Step 3:** Commit `docs(control): say a lead keeps, closes and picks a model for its
  workers`.
