# The lead's own state Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A lead's rail row shows its own state, mark and age, and says how many of its workers wait on the person; the attention filter, *N need attention* and a folded project's badge still see those workers.

**Architecture:** `rail::model::Item` keeps `signal` as the session's own and gains `rolled` (the most urgent of itself and its workers, what the filter and the project badge read) and `waiting` (its workers waiting on the person). `fold_workers` fills those two instead of overwriting the lead's state. The row's meta line counts the waiting workers after the state, in the warning ink.

**Tech Stack:** Rust, GPUI.

**Spec:** `docs/superpowers/specs/2026-10-30-workers-list-design.md` (step 2, *The rail*; builder's calls *The rail's other signals*, *Disconnected and connecting*).

## Global Constraints

- Waiting means `needs_attention`: needs input, failed or disconnected.
- No hard-coded colour: the warning ink from `crate::theme::status_ink`.
- `docs/screens.md`'s rail paragraph on workers changes in the same PR; `CHANGELOG.md` gets a line.
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

- A lead whose only worker failed while the lead idles: row reads *Idle*, the line counts *1 worker needs you*, the attention filter still lists the lead.
- A folded project holding an idle lead with a waiting worker: its badge still shows the worker's state.
- A lead more urgent than its workers (lead failed, worker running): row reads *Failed*, nothing counted.
- A worker whose lead is not listed (another window's): stays listed as before, counts toward nothing.
- A narrow rail: the count truncates with the rest of the line, never pushes the row taller.

---

### Task 1: Model, row and docs

**Files:**
- Modify: `crates/app/src/rail/model.rs` (`Item`, `fold_workers`, `visible`, `badge`, `meta`; new `workers_said`)
- Modify: `crates/app/src/rail.rs:174-188` (build `rolled: row.signal`, `waiting: 0`)
- Modify: `crates/app/src/rail/session.rs:231-240` (pass `item.waiting`; a second highlight over the count)
- Modify: `docs/screens.md` (the paragraph *A worker of a lead in the window is not listed…*), `CHANGELOG.md`
- Test: `crates/app/src/rail/tests.rs`

**Interfaces:**
- `Item { …, rolled: Option<SessionSignal>, waiting: usize }`
- `pub(super) fn meta(signal, background, subagents, waiting: usize, who, age) -> String`
- `pub(super) fn workers_said(waiting: usize) -> Option<String>`: `None` for 0, `1 worker needs you`, `N workers need you`.

- [ ] **Step 1: Failing tests.** In `tests.rs`: fixtures get `rolled: signal`, `waiting: 0`; every `meta(..)` call gets `0` as `waiting`. Replace `the_lead_row_takes_its_most_urgent_worker` with `the_lead_row_keeps_its_own_state_and_counts_its_waiting_workers` (with `with_workers()`, lead uid 1: `signal == Some(AwaitingUser)`, `age == 4`, `rolled == Some(Failed)`, `waiting == 1` — uid 7 failed, uid 2 busy). Add `meta_counts_waiting_workers` (`meta(None, 0, 0, 2, "claude", 60) == "Idle · 2 workers need you · claude · 1h"`, and `1 worker needs you` for 1). Add `a_folded_project_shows_an_idle_lead_s_waiting_worker` (seed, lead uid 1 idle, uid 2 its worker awaiting, uid 6 idle: `badge(&fold_workers(items), 0) == Some(AwaitingUser)`). Keep `the_needs_attention_filter_lists_the_lead_of_a_failed_worker` and `a_lead_more_urgent_than_its_workers_keeps_its_own` (add `waiting == 0` there).
- [ ] **Step 2: Run** `cargo test -p onehand --locked --lib rail::` — expect compile failure on `rolled`/`waiting`.
- [ ] **Step 3: Implement** as *Architecture*; `visible`'s attention filter and `badge` read `rolled`; `attention_count` stays on `signal` (it runs before the fold). In `session.rs` highlight `workers_said(item.waiting)` where it sits in the line, in `status_ink(cx).warning`.
- [ ] **Step 4: Run** the rail tests, then the full checks. Expected: all pass.
- [ ] **Step 5: Docs.** `screens.md`: the lead's row keeps its own state and age; after its state its line counts *N workers need you* in the warning ink; the filter, *N need attention* and a folded project's badge still count a waiting worker. CHANGELOG: one line under the lead/workers entry.
- [ ] **Step 6: Commit** `feat(rail): a lead's row keeps its own state and counts its waiting workers`.
