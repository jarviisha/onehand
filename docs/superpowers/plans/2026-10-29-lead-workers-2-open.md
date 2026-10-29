# Lead workers, PR 2: the handle and the open — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `open_session`, from a session's agent, answers only once the worker is up: connected, in the unattended mode, on the model named, its first prompt accepted as turn `turn`, within 120 seconds; it answers with a handle naming the run of the app, dedupes on `request`, leaves nothing behind when it fails, and abandons the open when its lead goes.

**Architecture:** Core gains the pure rules: the worker's mode (`config::worker_mode`), the next step of an open against a live `Chat` (`control::open::step`), the handle's text, and `request`/`turn`/`model` on `Call::OpenSession`. The app keeps two tables in `Sessions` — the requests per lead and the lead–worker relation — and drives one open as an async loop on `cx.spawn` that wakes on the worker session's changes and on the deadline, re-checking the lead each time, then answers. A failed open closes only its own session (`Shell::drop_opened`) and drops a root only if the open added it and nothing else stands on it.

**Tech Stack:** Rust, GPUI, futures.

**Spec:** `docs/proposals/lead-workers/spec.md`, section 1 (*The mode and the model*, *Waiting, and the answer*, *The request id*), *Terms*, *Who gets what*; PR 2 in *Pull requests*.

## Global Constraints

- Core stays GUI-free and runtime-free; nothing is `pub` unless the app names it.
- Comments never cite a document; files stay under about 800 lines of code (`crates/app/src/control/workers.rs` is about 380: the open's loop goes in a new `control/workers/opening.rs`).
- No blocking work in a handler or a render; the loop runs on `cx.spawn` with `background_executor().timer`.
- An outside program's `open_session` is unchanged: naming `request`, `turn` or `model` is refused, naming them.
- `OPEN_DEADLINE` is 120 seconds from the call, for the whole open.
- The answer, one fact per line: `session: <handle>`, `name: <rail name>`, `project: <root>`, `mode: <id or the agent's own>`, `model: <value or the agent's own>`, `dirs: <folders or none>`, `turn: <n> accepted` (the last only with a prompt).
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui-pre` erroring.

## Review Focus

- An agent that connects but never answers a pick: the open fails at the deadline, its session closed, its root dropped if added, its cap place, reservation and request freed; a late `PickConfirmed` finds the session gone. Pinned in Task 2 (the step stays `Wait`) and checked in the drive.
- A pick the agent refuses (`PickRefused`): the chat puts the old value back; the step must report refused, not ready on the old model. Pinned in Task 2.
- The same `request` retried while the first open still runs, after it finished, after it failed, and with other arguments. Pinned in Task 4.
- The lead closed or switched off mid-open: no prompt sent, no relation written, the worker closed. Checked in Task 5's loop and in the drive.
- A link lost before connecting (agent missing, login required): the open fails with the agent's own words. Pinned in Task 2.

---

### Task 1: the worker's mode

**Files:** Modify `crates/core/src/config.rs` (beside `run_mode`); tests in its existing tests module.

**Interfaces:**
- Produces: `pub fn worker_mode(configured: Option<&str>, runs_agent: Option<&str>, agents: &[AgentSpec], worker_agent: &str) -> Option<String>` — `configured` holds only when `worker_agent` is `agent_or_first(agents, runs_agent)`'s name; else the worker's preset's unattended mode; `mode_set` applied (blank is none).

- [ ] **Step 1: Failing tests:** `worker_mode_takes_the_configured_mode_for_the_runs_agent` (Claude preset, runs agent Claude, configured `plan` → `Some("plan")`); `worker_mode_ignores_it_for_another_agent` (configured `plan`, worker Codex → `None`); `worker_mode_falls_back_to_the_preset` (nothing configured, worker Claude → `Some("acceptEdits")`); `a_blank_configured_mode_is_none`.
- [ ] **Step 2:** `cargo test -p onehand-core --locked worker_mode` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(config): the mode a lead's worker starts in`.

### Task 2: the next step of an open

**Files:** Create `crates/core/src/control/open.rs` (`pub mod open;` in `crates/core/src/control.rs`), tests in `crates/core/src/control/open/tests.rs`.

**Interfaces:**
- Consumes: `workflow::settings::{Settings, Pick, next, Next}`, `acp::roles::Role`, `Chat::{link, failure, offer, in_force, picks_waiting, config_options}`.
- Produces:
  - `pub enum Step { Wait, Pick(Pick), Refused(String), Ready }`
  - `pub fn step(chat: &Chat, mode: Option<&str>, model: Option<&str>, picked: &[String]) -> Step`:
    - `Link::Connecting` → `Wait`; `Link::Lost` → `Refused("the agent did not start: <Chat::failure or 'its link was lost'>")`;
    - `picks_waiting()` → `Wait`;
    - the model's option is the config option whose `role()` is `Role::Model`; a `model` with none → `Refused("the agent offers no models")`;
    - `next(want, named = every wanted group, offer, in_force, picked)`: `Pick` → `Step::Pick`, `Refused(why)` → `Step::Refused(why)`, `Done` → every wanted value in `in_force()`? `Ready` : `Refused("the agent did not take <mode `x` | `v` for `id`>")`.
  - `pub fn handle(run: &str, uid: u64) -> String` → `"<run>-<uid>"`.

- [ ] **Step 1: Failing tests** (chats built as `task/drive/tests.rs`'s `connected()` and `with_models()` do): `waits_while_connecting`; `a_lost_link_is_refused_with_its_words`; `picks_the_mode_then_the_model` (mode `acceptEdits` offered, model `strong` offered → first `Pick(Mode)`, after it is in force `Pick(Option{model})`); `waits_while_a_pick_is_out`; `a_model_not_offered_is_refused_naming_the_offer` (contains "weak, strong"); `no_model_option_is_refused`; `a_refused_pick_is_not_ready` (pick sent, `PickRefused` applied, picked = ["model"] → `Refused` containing "did not take"); `nothing_wanted_is_ready_at_once`; `the_handle_names_the_run`.
- [ ] **Step 2:** `cargo test -p onehand-core --locked control::open` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(control): the next step of opening a worker`.

### Task 3: `request`, `turn` and `model` on `open_session`

**Files:** Modify `crates/core/src/control.rs` (`Call::OpenSession`, the allow-list, the schema, the parse); `crates/core/src/control/tests.rs`.

**Interfaces:**
- Produces: `Call::OpenSession { project, agent, prompt, dirs, request: Option<String>, turn: Option<u64>, model: Option<String> }`; `turn` an integer ≥ 1, refused otherwise ("`turn` must be a turn number, a whole number from 1"). `open_session`'s description says it answers once the session is up, for a session's agent.

- [ ] **Step 1: Failing tests:** extend `every_tool_parses_its_input` with all three; `every_tool_refuses_malformed_input` with `turn: 0`, `turn: "1"`, `model: 3`.
- [ ] **Step 2:** `cargo test -p onehand-core --locked control::` — FAIL.
- [ ] **Step 3:** Implement; the app's arm binds the new fields as `_` until Task 5.
- [ ] **Step 4:** PASS; `cargo check -p onehand --locked`.
- [ ] **Step 5:** Commit `feat(control): open_session takes a request, a turn and a model`.

### Task 4: the requests and the relation

**Files:** Create `crates/app/src/control/workers/requests.rs` (declared in `workers.rs`), tests in `crates/app/src/control/workers/tests.rs`; modify `crates/app/src/control.rs` (`Sessions` gains `requests: Requests` and `workers: HashMap<u64, Relation>`; `forget(uid)` drops `uid`'s requests and every relation where `uid` is the worker or the lead).

**Interfaces:**
- Produces:
  - `#[derive(Clone, PartialEq, Eq, Hash)] pub(super) struct Asked { project, agent, prompt, dirs, turn, model }` — the arguments a request is held to.
  - `pub(super) enum Seen { New, Opening, Answered(String) }`; `pub(super) struct Requests` with `fn see(&mut self, lead: u64, request: &str, asked: &Asked) -> Result<Seen, String>` (other arguments → `Err("request <r> was made with other arguments")`; first sight records `Opening` and returns `New`), `fn answered(&mut self, lead, request, answer: String)`, `fn failed(&mut self, lead, request)` (forgets it), `fn forget_lead(&mut self, lead)`.
  - `pub(super) struct Relation { pub(super) lead: u64, pub(super) turn: Option<u64>, pub(super) text: Option<String>, pub(super) sent: usize }` — written only after the first prompt was accepted (or, with none, once ready).

- [ ] **Step 1: Failing tests:** `a_first_request_is_new`; `the_same_request_while_opening_says_so`; `a_finished_request_answers_again`; `a_failed_request_opens_afresh`; `the_same_request_with_other_arguments_is_refused`; `requests_are_per_lead`; `forgetting_a_lead_forgets_its_requests`.
- [ ] **Step 2:** `cargo test -p onehand --locked control::workers` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(control): the requests a lead made and the workers it holds`.

### Task 5: the open, waited for

**Files:** Create `crates/app/src/control/workers/opening.rs`; modify `crates/app/src/control/workers.rs` (`Open` gains `request`, `turn`, `model`; `finish` hands the started session to the loop instead of answering), `crates/app/src/control.rs` (a `RUN` id minted once, `LazyLock<String>` from `control::new_token()`'s first 8 characters), `crates/app/src/shell/remote_runs.rs` (`pub fn drop_opened(&mut self, uid: u64, dir: &Path, added: bool, cx)` — `drop_session(uid)`, then `forget_root` when `added`, the root is transient and holds no session; `pub fn label_of(&self, uid, cx) -> String` exposing `session_label`).

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces: `pub(super) async fn wait_open(...) -> Result<String, String>` driving one open.

The flow, for a session's agent (outside programs keep PR 1's path, and are refused `request`, `turn`, `model`):
1. Handler: `request` is required; `turn` is required with `prompt` and refused without; a blank `prompt` is refused. `requests.see(lead, request, asked)`: `Opening` → `Ok("<handle> is still opening; call again with the same request")` once the uid is known, else "still opening"; `Answered(a)` → `Ok(a)`; `New` → go on. The deadline starts here.
2. PR 1's checks and `run_unattended`; note whether the root was added (no window root had its canonical path).
3. The loop, each wake: the lead still has a session in some window and stands `On` or with `open_session` in its set, else abandon; past the deadline, fail "the agent was not ready within 120 seconds"; `control::open::step(chat, mode, model, picked)`: `Wait` → wait for the session's next change or the deadline; `Pick(p)` → `chat.pick(&p)`, `cx.notify()`, push its group; `Refused(why)` → fail with `why`; `Ready` → go on.
4. The prompt, if any: `session.submit(prompt)`; false → fail with `submit_blocker`'s hint. Then the relation `{ lead, turn, text, sent: prompts_sent }`, `requests.answered`, and the answer.
- **Fail or abandon** = `Shell::drop_opened(uid, root, added)` (which runs `control::forget`, freeing the reservation and the cap place) and `requests.failed`. An abandoned open answers `Err("the lead went before its worker was ready")`, which nobody reads.
- The wait: `cx.observe(&session, …)` sending on a `futures` unbounded channel, raced against `cx.background_executor().timer(remaining)` with `futures::future::select`; the subscription dropped when the loop ends.

- [ ] **Step 1: Failing test:** none at this level runs without a GPUI window; the rules are Task 2's and Task 4's. Ruling to ledger. The loop is checked in the drive (Step 4).
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** `cargo test -p onehand --locked` (guards) and `cargo check --workspace --locked` — PASS.
- [ ] **Step 4:** Drive with Mock Work and Mock UI as workers and a real Claude lead: a model the agent does not offer (the mocks offer none → "the agent offers no models", nothing left in the rail); a plain open with a prompt (answer lines, `turn: 1 accepted`, the worker's transcript shows the prompt); the same request again (same answer, one worker); the same request with another prompt (refused).
- [ ] **Step 5:** Commit `feat(control): open a worker only once it is up, on its model`.

### Task 6: the contract

**Files:** `docs/control.md` (the `open_session` row: `request`, `turn`, `model`; *A worker*: the waiting, the deadline, the mode, the model, the answer, the request id, cleanup, the lead going), `docs/proposals/lead-workers/spec.md` (section 1 marked built through PR 2), `docs/proposals/lead-workers/README.md`.

- [ ] **Step 1:** Write them. **Step 2:** `git diff --check`; no code cites a document. **Step 3:** Global checks. **Step 4:** Commit `docs(control): open_session waits for its worker and answers its handle`.
