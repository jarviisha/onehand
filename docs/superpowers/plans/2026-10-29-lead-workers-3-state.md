# Lead workers, PR 3: `session_state` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a lead's agent asks `session_state(session)` about a worker it opened and reads, one fact per line, its link, its turn and how the last ended, what waits on a person, what activity core knows of, who holds it, the lead's turns sent and over, and what it runs with — without ever being told "stopped" on facts that cannot prove it.

**Architecture:** Core gains the rule that writes the state from a `Chat` and the relation's numbers (`control::state`), a parser for the handle (`control::open::parse_handle`), one accessor on `Chat` for the agent working by itself, and the tool `session_state`, listed and allowed only to a session's agent. The app resolves the handle against this run, checks the caller owns the worker, and answers `link: closed` for a worker of this run whose session is gone.

**Tech Stack:** Rust, GPUI.

**Spec:** `docs/proposals/lead-workers/spec.md`, section 2 (`session_state`), *Who gets what*, section 5 (who holds a worker); PR 3 in *Pull requests*.

## Global Constraints

- Built from core's separate facts, never from `SessionSignal`.
- `activity` is `running: <what>`, `none reported` or `unknown`; `unknown` whenever the link is lost or still connecting, since a lost link clears what core held. Nothing in the answer says "stopped"; `link: closed` is the only proof.
- `session_state`, like the later `send_prompt` and `cancel_turn`, is never listed to an outside program, and a call from one is refused.
- A handle from another run is refused as such; another lead's worker, or a session no lead opened, is refused; a handle of this run whose session is gone answers `link: closed`.
- Core GUI-free; nothing `pub` without an outside user; comments never cite documents; files under ~800 lines.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui-pre` erroring.

## Review Focus

- A worker the person prompted after the lead: `held by: person`, and the lead's turn not counted as over by the person's turn ending. Pinned in Task 1.
- A lost link right after a turn ran: `turn` reads how it ended as core says, `activity: unknown`, never `none reported`. Pinned in Task 1.
- A subagent or background item outliving the turn: `turn: ended` with `activity: running: …`. Pinned in Task 1.
- A handle with a uid that never existed in this run, and one whose worker closed and was forgotten: both answer `link: closed`, since a uid is never reused within a run and the caller cannot learn anything else from it. Pinned in Task 3.
- The tool listed to a schedule's session standing with a set that does not name it: not listed. Pinned in Task 2.

---

### Task 1: the state, from core's facts

**Files:** Create `crates/core/src/control/state.rs` (`pub mod state;` in `control.rs`), tests `crates/core/src/control/state/tests.rs`; modify `crates/core/src/chat/model/background.rs` (`pub fn working_by_itself(&self) -> bool`, `own_cycle != OwnCycle::None`); modify `crates/core/src/control/open.rs` (`parse_handle`), its tests.

**Interfaces:**
- Produces:
  - `pub fn parse_handle(text: &str, run: &str) -> Result<u64, String>` — `"<run>-<uid>"`; another run's → `Err("<text> is from an earlier run of onehand; its worker is gone")`; anything else → `Err("<text> is not a session's handle")`.
  - `pub struct Held { pub turn: Option<u64>, pub sent: usize, pub dirs: Vec<PathBuf> }` — what the relation knows.
  - `pub fn state(chat: &Chat, held: &Held) -> String` — the lines, in order:
    - `link: connecting|connected|lost`
    - `turn: none` (no prompt sent) | `running` (`busy`) | `failed: <Chat::failure>` | `cancelled` | `ended`
    - `waiting: none` or `<n> permission[s]`, `<n> question[s]` (unresolved cards, `pending_permissions`, `pending_asks`) — only a person answers these
    - `activity: unknown` (link not connected) | `running: <parts>` (`the turn`, `<n> in background: <names, at most 5, then "and N more">`, `<n> subagents`, `working by itself`) | `none reported`
    - `held by: lead` | `person` (`prompts_sent > held.sent`)
    - `turns: sent <n>, ended <n>` when the lead's turn is over (not running and held by the lead), `turns: sent <n>, not yet ended` otherwise; `turns: none sent` without a turn
    - `mode: <in force or the agent's own>`, `model: <model option's current or the agent's own>`, `dirs: <held.dirs or none>`

- [ ] **Step 1: Failing tests:** `a_connected_idle_worker_reads_none_reported`; `a_running_turn_is_running_and_not_ended`; `a_card_waiting_is_counted` (one permission parked); `a_failed_turn_says_its_words`; `a_cancelled_turn_says_cancelled`; `background_after_the_turn_is_activity_not_the_turn` (turn ended, one background item → `turn: ended`, `activity: running: 1 in background: npm test`); `a_lost_link_makes_activity_unknown` (turn ran, then `Disconnected` → `link: lost`, `activity: unknown`); `the_person_prompting_holds_the_worker` (prompts_sent 2, held.sent 1 → `held by: person`, `turns: sent 1, not yet ended`); `the_lead_s_turn_over_is_ended`; `parse_handle_reads_this_run`, `parse_handle_refuses_another_run`, `parse_handle_refuses_nonsense`.
- [ ] **Step 2:** `cargo test -p onehand-core --locked control::` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(control): a worker's state from core's own facts`.

### Task 2: the tool, for a session's agent only

**Files:** Modify `crates/core/src/control.rs` (`Call::SessionState { session: String }`, `TOOLS`, the catalogue, the parse, `SESSION_ONLY: [&str; 1]` growing in PR 4, the `allowed` closure refusing those names to `Peer::Outside`); `crates/core/src/control/tests.rs`.

**Interfaces:**
- Produces: `Call::SessionState { session: String }`; description "What a worker you opened is doing: its link, its turn, what waits on a person, its activity, who holds it, your turns sent and over, what it runs with. Never says it stopped; only `link: closed` does".

- [ ] **Step 1: Failing tests:** `session_state_parses_its_handle`; `an_outside_program_is_not_listed_session_state` (outside, on: `tools/list` lacks it); `an_outside_program_calling_session_state_is_refused` (refusal names it as a session's agent's tool); `a_session_on_lists_session_state`; `a_set_without_it_does_not_list_it`; update `the_catalogue_lists_every_tool_with_a_schema` to the new list.
- [ ] **Step 2:** `cargo test -p onehand-core --locked control::` — FAIL.
- [ ] **Step 3:** Implement; the app's `perform_now` gets an arm that answers `Err("not yet")` until Task 3.
- [ ] **Step 4:** PASS; `cargo check -p onehand --locked`.
- [ ] **Step 5:** Commit `feat(control): session_state, for a session's agent`.

### Task 3: the answer, in the app

**Files:** Modify `crates/app/src/control/workers/requests.rs` (`Relation.dirs: Vec<PathBuf>`), `crates/app/src/control/workers/opening.rs` (fill it), create `crates/app/src/control/workers/state.rs` (`mod state;`), tests in `crates/app/src/control/workers/tests.rs`; modify `crates/app/src/control.rs` (the arm calls `workers::state::answer`).

**Interfaces:**
- Consumes: `parse_handle`, `state`, `Held`, `RUN`, `Sessions.workers`.
- Produces:
  - `pub(super) enum Whose { Closed, Mine(Held), NotYours }` and `fn whose(relation: Option<&Relation>, caller: u64, open: bool) -> Whose` — no relation and no session → `Closed`; a relation of another lead, or a session with none → `NotYours` (refused "<handle> is not a worker you opened"); a relation of the caller with its session open → `Mine`; with its session gone → `Closed`.
  - `pub(super) fn answer(caller: Option<(u64, PathBuf)>, session: &str, cx: &mut App) -> Result<String, String>`.

- [ ] **Step 1: Failing tests** on `whose`: `a_worker_of_the_caller_is_mine`; `another_lead_s_worker_is_not_yours`; `a_session_no_lead_opened_is_not_yours`; `a_gone_worker_is_closed`; `a_uid_never_seen_is_closed`.
- [ ] **Step 2:** `cargo test -p onehand --locked control::workers` — FAIL.
- [ ] **Step 3:** Implement; the session is found in any window by uid (`ChatPane::session_of` through each shell).
- [ ] **Step 4:** PASS, then drive: a Claude lead opens a Mock UI worker with a prompt and asks `session_state` while the worker's turn runs, while its permission card waits, after it ended (on its error, every other turn), after the person types into it, and after the person closes it; and a handle with another run's prefix.
- [ ] **Step 5:** Commit `feat(control): answer session_state for a lead's worker`.

### Task 4: the contract

**Files:** `docs/control.md` (the tools table, *A worker*), `docs/proposals/lead-workers/spec.md` (section 2 built), `docs/proposals/lead-workers/README.md`.

- [ ] **Step 1:** Write them. **Step 2:** `git diff --check`; no code cites a document. **Step 3:** Global checks. **Step 4:** Commit `docs(control): session_state`.
