# Lead workers, PR 4: `send_prompt` and `cancel_turn` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a lead's agent sends its worker the next turn, once per turn number and never queued, and cancels a running turn unless a card waits on a person; and the three minors left from PR 1's review are fixed.

**Architecture:** Core gains the two pure rules (`control::turns::send` and `control::turns::cancel`) over a `Chat` and what the relation last sent, and the two tools, added to `SESSION_ONLY`. The app resolves the worker as `session_state` does, applies the rule, submits or cancels, and moves the relation on. The PR 1 minors: a reservation refuses a folder nested in or holding a held one; the process reading skips onehand's own children; a window project's git failing refuses instead of reading as "no checkout".

**Tech Stack:** Rust, GPUI.

**Spec:** `docs/proposals/lead-workers/spec.md` sections 3, 4 and 5; section 1 for the minors.

## Global Constraints

- `send_prompt` is checked in this order: held by the lead; the turn number (`already sent` before anything else, so a retry while the turn runs is answered, not refused as busy); then `Chat::submit_blocker`, in its words. Never queued.
- `cancel_turn` is refused while any permission or question waits, while the person holds the worker, and with no turn running; it answers no card.
- Both are for a session's agent only: added to core's `SESSION_ONLY`.
- Core GUI-free; nothing `pub` without an outside user; comments never cite documents.
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui-pre` erroring.

## Review Focus

- A send retried after the turn it started already ended: still `already sent`, nothing sent. Pinned in Task 1.
- A send after the person typed into the worker: refused as held by the person. Pinned in Task 1.
- A cancel while a subagent's permission waits after the turn: refused as waiting, not as no turn. Pinned in Task 1.
- Two workers on `/repo` and `/repo/sub`: the second refused. Pinned in Task 4.
- `git status` started by onehand in the folder at the moment of the reading: not an occupant. Pinned in Task 4.

---

### Task 1: the rules

**Files:** Create `crates/core/src/control/turns.rs` (`pub mod turns;`), tests `crates/core/src/control/turns/tests.rs`.

**Interfaces:**
- Produces:
  - `pub struct Last { pub turn: Option<u64>, pub text: Option<String>, pub sent: usize }` — what the relation last sent.
  - `pub enum Send { Go, AlreadySent(u64) }` and `pub fn send(chat: &Chat, last: &Last, turn: u64, text: &str) -> Result<Send, String>`:
    1. `chat.prompts_sent > last.sent` → `Err("the person holds this worker: it has typed into it since your last turn")`;
    2. `last.turn == Some(turn)`: same text → `Ok(AlreadySent(turn))`, other text → `Err("turn <n> was already sent with other text")`;
    3. `last.turn > Some(turn)` → `Err("turn <n> is below turn <m>, the last you sent")`;
    4. `chat.submit_blocker(text, &[])` → `Err(hint)`;
    5. `Ok(Go)`.
  - `pub fn cancel(chat: &Chat, last: &Last) -> Result<u64, String>` — person holds → refused; any pending permission or question → `Err("a card waits on a person; only they answer it")`; not busy → `Err("no turn is running")`; else `Ok(last turn or 0)`.

- [ ] **Step 1: Failing tests:** `a_next_turn_goes`; `the_same_turn_and_text_is_already_sent` (also after that turn ended); `the_same_turn_with_other_text_is_refused`; `a_turn_below_the_last_is_refused`; `a_busy_worker_refuses_a_new_turn_without_queueing`; `a_lost_worker_refuses_in_its_words`; `a_worker_the_person_holds_refuses_a_send`; `a_running_turn_can_be_cancelled`; `a_card_waiting_refuses_the_cancel`; `no_turn_running_refuses_the_cancel`; `the_person_holding_refuses_the_cancel`.
- [ ] **Step 2–4:** FAIL, implement, PASS (`cargo test -p onehand-core --locked control::turns`).
- [ ] **Step 5:** Commit `feat(control): when a lead may send its worker a turn or cancel one`.

### Task 2: the tools

**Files:** `crates/core/src/control.rs` (`Call::SendPrompt { session, text, turn: u64 }`, `Call::CancelTurn { session }`, `TOOLS`, `SESSION_ONLY` → 3, catalogue, parse, allow-list), its tests; the app's `perform_now` arms answer `Err("not yet")` until Task 3.

- [ ] **Step 1: Failing tests:** both parse; `turn` required, ≥ 1; an outside program is neither listed nor allowed either; the catalogue test's list. Update `instance.rs`'s socket test: an outside program lists every tool but the three.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5:** Commit `feat(control): send_prompt and cancel_turn, for a session's agent`.

### Task 3: the answers

**Files:** Modify `crates/app/src/control/workers/state.rs` (a shared `fn worker(caller, session, cx) -> Result<(u64, Entity<ChatSession>, Relation), String>` used by all three: closed → `Err("<handle> is closed")` for send and cancel, `Ok` with `link: closed` kept for `session_state`), create `crates/app/src/control/workers/turns.rs`; modify `crates/app/src/control.rs` (the arms).

- `send_prompt`: `turns::send`; `Go` → `session.submit(text)`, then the relation's `turn`, `text`, `sent = prompts_sent`; answer `turn <n> accepted`. `AlreadySent(n)` → `turn <n> already sent`.
- `cancel_turn`: `turns::cancel`; `Ok(n)` → `session.chat.cancel_turn()`, `cx.notify()`; answer `cancelling turn <n>`.

- [ ] **Step 1:** No GPUI harness for the arms: the rules are Task 1's. Ruling to ledger.
- [ ] **Step 2:** Implement. **Step 3:** `cargo test -p onehand --locked` PASS.
- [ ] **Step 4:** Drive: a Claude lead opens a Mock UI worker with turn 1; `send_prompt` turn 2 while turn 1 runs (refused, busy); `cancel_turn` while its card waits (refused); the person answers the cards; `cancel_turn` (cancelling turn 1); `send_prompt` turn 2 (accepted), the same again (already sent), turn 2 with other text (refused), turn 1 (below); the person types; `send_prompt` turn 3 (held by the person).
- [ ] **Step 5:** Commit `feat(control): a lead sends its worker the next turn, or cancels it`.

### Task 4: the minors left from PR 1

**Files:** `crates/app/src/control/workers.rs` (`Reservations::take`), `crates/core/src/worktree/occupants.rs` (`Entry.parent`, `/proc/<pid>/stat`'s fourth field, `lsof -R`), `crates/core/src/worktree/worker.rs` (`repo_of_blocking -> Result<Option<Repo>, String>`: `Ok(None)` when git says the folder is no checkout, `Err` when git could not answer), the callers; tests beside each.

- [ ] **Step 1: Failing tests:** `a_folder_inside_a_reserved_one_is_refused`, `a_folder_holding_a_reserved_one_is_refused`; `a_child_of_onehand_is_not_an_occupant` (entry with `parent` in `skip_parents`), `parse_lsof_reads_the_parent`; `a_plain_folder_is_no_checkout` (`Ok(None)`), `a_missing_folder_is_an_error` (`Err`).
- [ ] **Step 2–4:** FAIL, implement (the app passes `std::process::id()` as the parent to skip; a window project whose git fails refuses the open with git's words), PASS.
- [ ] **Step 5:** Commit `fix(control): nested folders, onehand's own children, and a git that fails`.

### Task 5: the contract

`docs/control.md` (two rows, *A worker*: sending, cancelling, nested folders, children), spec sections 3 and 4 built, README. Commit `docs(control): send_prompt and cancel_turn`.
