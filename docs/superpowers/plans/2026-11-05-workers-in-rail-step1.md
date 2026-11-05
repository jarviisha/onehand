# Workers in the rail, step 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A lead's workers are listed in the rail under the lead, open in the agent pane as any session, with a band over their composer naming the lead; the lead's strip opens the most urgent worker in the pane.

**Architecture:** The rail model stops dropping workers and links each to its listed lead (`Item::under`); rows emit a lead's workers right after it, so drawing and keys share one order. The arrival stops redirecting a worker to its lead. The band is a pane overlay beside the lead's strip, its words a pure function. The workers' region stays reachable from the header until step 2 deletes it.

**Tech Stack:** Rust, GPUI, gpui-component, onehand's `crates/app`.

**Spec:** `docs/superpowers/specs/2026-11-05-workers-in-rail-design.md`

## Global Constraints

- No hard-coded colour, radius or size: `cx.theme()`, `onehand_ui::tokens`, rems (AGENTS.md).
- Buttons through `crate::controls::action`; follow the `ui` skill and its checklist before finishing.
- Match our own enums exhaustively, no wildcard arms (`guards.rs`).
- No comment or string cites a document.
- Words: *<lead> is coordinating this worker*, *Back to lead*, *You hold this worker*, *Give back to the lead*.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

1. A worker whose lead is in another window: its own ordinary row in this window, band without *Back to lead*.
2. A search matching only a worker: the lead is listed with its whole tree.
3. A folded lead project: its workers vanish with it; the badge still rolls them up (no double count).
4. A lead past the cap: its tree is never split by *N more*.
5. A worker opening (no relation yet, `sent: None`): band reads coordinating, not held.

---

### Task 1: Nest workers in the rail model

**Files:**
- Modify: `crates/app/src/rail/model.rs` (`Item`, `fold_workers`, `emptied`, `visible`, `badge`, `Row::Session`, `capped`, `order`)
- Test: `crates/app/src/rail/tests.rs:593-738` (rewrite the worker tests)

**Interfaces:**
- Produces: `Item.under: Option<usize>` (index of its listed lead in `items`); `pub(super) fn nest_workers(items: &mut [Item])` replacing `fold_workers` (keeps every item; sets `under`, the lead's `waiting` and `workers` as today); `pub(super) fn emptied(items: &[Item]) -> Vec<usize>` (projects whose every item is nested); `Row::Session { item, flat, nested: bool }`.

- [ ] **Step 1: Rewrite the worker tests** in `rail/tests.rs` with the existing `with_workers` helper:
  - `workers_are_listed_under_their_lead`: by project, rows are lead then its workers (`nested: true`) in uid order, even when a worker's `root` differs from the lead's.
  - `a_worker_of_a_lead_not_listed_stays` (kept, `under == None`, `nested: false`).
  - `a_project_holding_only_nested_workers_is_emptied`: `emptied` returns the worktree root.
  - `the_cap_counts_trees`: 6 leads, the 6th with 3 workers → no `More` row; 7 leads → `More { hidden: 1 }` after the 6th lead's workers.
  - `a_search_matching_a_worker_lists_its_lead_and_tree`: query = worker title → lead row + all its workers.
  - `the_needs_attention_filter_lists_the_lead_with_its_tree` (kept, now also asserts the worker rows).
  - `a_folded_project_hides_the_tree_and_badges_it_once`: folded lead project → no worker rows; `badge` = the worker's waiting state; the worker's own project contributes nothing.
  - Keep `the_lead_row_keeps_its_own_state…`, `meta_counts_waiting_workers`, `a_lead_more_urgent…`.
- [ ] **Step 2: Run** `cargo test -p onehand rail::` → the new tests fail to compile (`nest_workers`, `nested`).
- [ ] **Step 3: Implement.** `visible` keeps only items with `under == None`, a top item kept when it or any item nested under it matches the query (attention through `rolled()`, as now). `capped` caps top items and pushes each one's workers (`nested: true`, the same `flat`) right after it, by uid. `badge` reads only top items. `order` needs no change beyond the field.
- [ ] **Step 4: Run** `cargo test -p onehand rail::` → PASS.
- [ ] **Step 5: Commit** `feat(rail): list a lead's workers under it`.

### Task 2: Draw nested rows

**Files:**
- Modify: `crates/app/src/rail.rs:175-241, 294-311` (`List::of`, the `Row::Session` arm)
- Modify: `crates/app/src/rail/session.rs:265-420` (`session_row`)

**Interfaces:**
- Consumes: Task 1's `nest_workers`, `emptied(&items)`, `Row::Session { nested }`.
- Produces: `session_row(item, who, flat, nested: bool, mark, cx)`.

- [ ] **Step 1: `List::of`** calls `nest_workers(&mut items)` and `emptied(&items)`; drop `had`. The comment over the transient filter says a worktree holding only workers drawn under their lead.
- [ ] **Step 2: `session_row`**: a nested row indents one more `INDENT` than its lead's row (`flat` lead: one, tree: two), carries a left rule in `cx.theme().border` joining it to the lead, rounds only its right corners, takes no drag or drop, and its `who` is the agent even in a flat list.
- [ ] **Step 3: Run** `cargo check -p onehand` → clean; `cargo test -p onehand rail::` → PASS.
- [ ] **Step 4: Commit** `feat(rail): draw workers nested under their lead`.

### Task 3: Arriving at a worker shows it

**Files:**
- Modify: `crates/app/src/shell/workers.rs` (delete `Arrival`, `arrival`, `turn_to_lead`; module header no longer says workers never show on their own)
- Modify: `crates/app/src/shell/workers/tests.rs` (delete the arrival tests, keep `region_closes`')
- Modify: `crates/app/src/shell/roots.rs:137, 262-266` (no `worker`, no `show_workers` after `show`)
- Modify: `crates/app/src/shell/sessions.rs:319-326` (`mru_order` filters nothing by lead)

- [ ] **Step 1: Delete** the redirect and its tests; keep `region_closes` and its test (the region still follows the lead on screen, and closes on a worker, which has none).
- [ ] **Step 2: Run** `cargo test -p onehand shell::` → PASS; `cargo clippy -p onehand` shows no dead code.
- [ ] **Step 3: Commit** `feat(shell): arriving at a worker shows it`.

### Task 4: The band and the strip

**Files:**
- Create: `crates/app/src/chat/pane/worker_band.rs` + `worker_band/tests.rs`
- Modify: `crates/app/src/chat/pane/lead_strip.rs` (lift its surface into `pub(super) fn over_composer(id, cx) -> Stateful<Div>`; press emits `ShowSession`)
- Modify: `crates/app/src/chat/pane/overlay.rs:39` (band, else strip)
- Modify: `crates/app/src/chat/pane/events.rs:78-80` (`ShowWorker(u64)` → `ShowSession(u64)`), `crates/app/src/shell/boot.rs:169` (→ `shell.show_session`)
- Modify: `crates/app/src/chat/pane.rs` (`mod worker_band;`)

**Interfaces:**
- Produces: `enum BandAct { BackToLead, GiveBack }`; `fn band(lead: Option<&str>, held: bool, offered: bool, lead_here: bool) -> (String, Option<BandAct>)`; `ChatPane::worker_band(&self, cx) -> Option<AnyElement>`.

- [ ] **Step 1: Tests** in `worker_band/tests.rs`:
  - `band(Some("Refactor"), false, false, true)` → `("Refactor is coordinating this worker", Some(BackToLead))`
  - `band(Some("Refactor"), false, false, false)` → same words, `None`
  - `band(Some("Refactor"), true, true, true)` → `("You hold this worker", Some(GiveBack))`
  - `band(Some("Refactor"), true, false, true)` → `("You hold this worker", None)`
  - `band(None, false, false, false)` → `("Its lead is coordinating this worker", None)`
- [ ] **Step 2: Run** `cargo test -p onehand worker_band` → fails to compile.
- [ ] **Step 3: Implement** `band`; `worker_band` returns `None` unless the active session has `control::lead_of`; `held` = its `Worker.sent` (from `workers_of(lead)`) with `chat.prompted_since`; `offered` = `control::give_back_offered`; `lead_here` = the lead's conversation is in this pane; the lead's name by `worker_name(lead)`. Drawn on `over_composer`: the words muted and truncating, then a quiet short `action`. *Back to lead* emits `ShowSession(lead)`; *Give back to the lead* reads `prompts_sent` at the press and calls `control::give_back`, as the detail's menu does.
- [ ] **Step 4: Overlay** draws `worker_band` when it returns some, else `lead_strip`; the strip's press emits `ShowSession(most_urgent)` and its module header says it opens that worker in the pane.
- [ ] **Step 5: Run** `cargo test -p onehand` → PASS (lead_strip tests unchanged).
- [ ] **Step 6: Commit** `feat(chat): a band over a worker's composer names its lead`.

### Task 5: Documents

**Files:** `docs/screens.md:109-119, 196-206, 293`, `docs/control.md:237-240`, `CONTEXT.md:40-43`, the spec (commit it).

- [ ] **Step 1:** screens.md: the rail lists a lead's workers under it (the spec's settled points); the strip opens the most urgent worker in the pane; the band over a worker's composer; a worker asked for anywhere shows itself. The region's section stays for step 2. control.md *What the person sees* and CONTEXT.md *Worker*: listed under its lead in the rail, shown as any session.
- [ ] **Step 2:** `ui` skill checklist answered; full checks (Global Constraints); commit `docs: workers listed under their lead`.
