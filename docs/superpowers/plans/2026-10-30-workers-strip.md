# The workers' strip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The deck goes; one line above the lead's composer counts its workers and names the most urgent one waiting, and pressing it opens the region at that worker.

**Architecture:** `chat/pane/deck.rs` becomes `chat/pane/lead_strip.rs`: no state of its own, no rows, no slide. The rail's clock (`since`) moves to the region's state, which its rows read. With the deck gone the tucked card has no caller, so `onehand_ui::tucked` and the guard's exception for it go too, and the contract's motion rule loses it.

**Tech Stack:** Rust, GPUI.

**Spec:** `docs/superpowers/specs/2026-10-30-workers-list-design.md` (step 4, *The strip above the lead's composer*).

## Global Constraints

- The strip stays while the region shows.
- Its line: *N workers*, a count per state that has any (mark and ink, most urgent first), and at the end *<name> needs you* for the most urgent worker waiting on the person, when one does.
- A press leaves the caret where it was, and opens the region with the most urgent worker picked (picking it while the region shows).
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

- A lead with workers none of which waits: no *needs you*, the counts alone.
- The lead's composer list (`@`, `/`) open: the strip does not cover it.
- The lead on screen changing to a session without workers: the strip is gone the same frame.
- A long worker name: the end truncates, the counts never do.

---

### Task 1: The strip

**Files:** Create `crates/app/src/chat/pane/lead_strip.rs` (+ `lead_strip/tests.rs`); Delete `crates/app/src/chat/pane/deck.rs`, `deck/tests.rs`; Modify `chat/pane.rs` (mod, field, the Esc branch for the deck), `chat/pane/overlay.rs:290`, `chat/pane/workers.rs` (`since` field, `set_since`, `worker_age`).

- `fn summary(signals) -> Vec<(Option<SessionSignal>, usize)>` keeps its test; `fn waiting_first(workers: &[(u64, Option<SessionSignal>)]) -> Option<u64>`: the most urgent that `needs_attention`, ties to the first. Tests: `the_summary_counts_each_state_most_urgent_first` (moved), `no_one_waiting_names_no_one`, `the_most_urgent_waiting_is_named`.
- `pub(super) fn lead_strip(&self, cx) -> Option<AnyElement>`: the old edge's line, no chevron, on the old card's surface (raised fill, composer lift, top corners, `px_3` narrower than the composer), `ROW_H` tall; the end in the warning ink. Press: `ChatPaneEvent::ShowWorker(most urgent by urgency)`; mouse-down stopped so the caret stays.
- [ ] Tests red, implement, green; commit `feat(chat): one line over the lead's composer for its workers`.

### Task 2: The tucked card goes

**Files:** Delete `crates/ui/src/tucked.rs`; Modify `crates/ui/src/lib.rs`, `crates/app/src/guards.rs` (`nothing_animates` without its exception).

- [ ] `cargo test -p onehand --locked` (guards) and `cargo test -p onehand-ui --locked` green; commit `refactor(ui): the tucked card goes with the deck`.

### Task 3: Documents

`docs/screens.md` (composer paragraph: the strip in place of the deck; region bullet: the row described in place of "as the deck draws them"), `DESIGN.md` *Principles* (the motions: a `Spinner` only), `docs/ui-patterns.md` (the tucked card's lines), `docs/rules-and-gotchas.md` (guard row), `.claude/skills/ui/catalog.md` (the `tucked` row), `docs/known-gaps.md:249` (deck → strip), `CHANGELOG.md`.

- [ ] `git diff --check`; commit `docs(chat): the workers' strip`.
