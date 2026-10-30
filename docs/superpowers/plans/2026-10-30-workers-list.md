# The workers' list Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The workers' region becomes a list of every worker pinned over the picked worker's detail (its card, its live transcript, a composer only on demand); every permission card shows its command whole up to a bound, then *Show all N lines*; the keys reach the list and the picked worker's card.

**Architecture:** `chat/pane/workers.rs` keeps the region's state and its frame; a pure ordering function freezes the urgency order between picks; `workers/list.rs` draws the rows (lifted from the deck's row, which the deck then draws through too) and owns the list's keys; `workers/detail.rs` replaces `workers/block.rs`. The command block in the shared permission card clips at a line bound with a toggle instead of scrolling.

**Tech Stack:** Rust, GPUI, gpui-component.

**Spec:** `docs/superpowers/specs/2026-10-30-workers-list-design.md` (step 3; *The workers' region*, *When a worker starts waiting*, *Permission cards*, builder's calls *Keys in the region*, *Which cards show the whole command*, *Freezing the order*, *The strip's title*, *The folder in the detail head*).

## Global Constraints

- Order: failed or disconnected, needs input, running (connecting with it), done, idle; ties keep the lead's opening order.
- The order re-sorts only when the person picks a worker or the region opens; a state change in between changes a row in place.
- Nothing moves, opens or is picked when a worker starts waiting.
- Opening the region by pointer takes no caret; `Ctrl+Shift+Y` puts it on the list.
- Tokens and controls through `onehand_ui` / `crate::controls`; no hard-coded colour or size; files under about 800 lines.
- The deck stays until step 4; its rows draw through the shared row.
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

- A worker closing while it is picked: the pick moves to the first row, a caret in its composer goes to the lead's.
- A worker opening while the region shows: it joins at the end of the frozen order, nothing else moves.
- The lead on screen changing while the region shows: the new lead's most urgent worker is picked, its order taken fresh.
- A command of one very long wrapped line: clipped at the bound with *Show all*, never a one-row box.
- `Alt+Y` with the caret in the region and the picked worker waiting on nothing: answers nothing, not the lead's card.

---

### Task 1: The frozen urgency order

**Files:** Modify `crates/app/src/chat/pane/workers.rs`; Modify `crates/app/src/chat/pane/deck.rs` (its `urgency` moves to `workers.rs`, the deck imports it); Test `crates/app/src/chat/pane/workers/tests.rs`.

**Interfaces:** Produces `pub(super) fn urgency(a: Option<SessionSignal>, b: Option<SessionSignal>) -> Ordering` and `fn ordered(frozen: &[u64], now: &[(u64, Option<SessionSignal>)], refresh: bool) -> Vec<u64>`: with `refresh`, `now` stably sorted by `urgency`; else `frozen` kept for the uids still in `now`, new ones appended in `now`'s order.

- [ ] Tests: `a_fresh_order_puts_the_most_urgent_first` (Busy, Failed, None, AwaitingUser → Failed, AwaitingUser, Busy, None), `ties_keep_the_opening_order`, `a_frozen_order_moves_nothing_when_a_state_changes`, `a_new_worker_joins_the_end_and_a_gone_one_leaves`.
- [ ] Run `cargo test -p onehand --locked --lib chat::pane::workers` (red), implement, run (green), commit `refactor(chat): the workers' order, frozen between picks`.

### Task 2: List over detail

**Files:** Create `crates/app/src/chat/pane/workers/list.rs`, `crates/app/src/chat/pane/workers/detail.rs`; Delete `crates/app/src/chat/pane/workers/block.rs`; Modify `workers.rs` (state: `picked: Option<u64>`, `order: Vec<u64>`, `writing: Option<u64>`, `focus: FocusHandle`; drop `tails`, `height`, `WORKER_TAIL_H`, `tails_fit`), `deck.rs` (`deck_row` body moves to `list.rs` as `worker_row`, the deck calls it).

**Interfaces:**
- `pub(super) fn worker_row(&self, worker: &Worker, signal, age: u64, picked: bool, keyed: bool, on_press: impl Fn(&mut ChatPane, &mut Window, &mut Context<ChatPane>) + 'static, cx) -> impl IntoElement`: two lines, mark, name, state word in its ink, `age · turn N · N in background`, under it `waiting_line` in the warning ink, else `activity_status`, muted; the selected fill while `picked`; *Stop* while a turn runs.
- `pub fn pick_worker(&mut self, uid: u64, window, cx)`: picks, re-sorts, closes another worker's open composer (keeping its draft).
- `pub fn open_worker(&mut self, uid, caret: bool, window, cx)` keeps its name for the shell: picks, and with `caret` opens the composer with the caret in it.
- Detail head: mark, name, the folder's last component (whole path as tooltip), *Write to <short name>…* (secondary, short), *Stop* while a turn runs, `⋯` as the block's menu had. Then `Self::cards(..)` for that worker, then its transcript filling the rest (scrollbar as before), then, while `writing == Some(uid)`, the compact composer; `Esc` in it or a send closes it (`writing = None`), the caret back to the list.
- The strip reads `Workers · N`.
- The list is capped at the region's four (`OPENED_CAP`), so no *N more* row; a `debug_assert!` names the cap.

- [ ] Tests for pure parts only: `the_short_name_is_before_the_dot` (`"b · report r1"` → `b`, `"onehand-sim-b"` → itself), used in *Write to b…*.
- [ ] Build it; `cargo test -p onehand --locked` (guards included) green; commit `feat(chat): the workers' region as a list over the picked worker`.

### Task 3: Keys

**Files:** Modify `workers.rs`/`list.rs` (actions), `crates/app/src/keymap/commands.rs`, `crates/app/src/chat/workers_panel.rs`, `crates/app/src/shell/render.rs:86`, `crates/app/src/shell/workers.rs`.

- New actions `WorkersUp`, `WorkersDown`, `WorkersWrite`, `WorkersBack` (`gpui::actions!(workers_list, …)`), bound `up`/`down`/`enter`/`escape` in `"WorkersList && !Input && !Dialog"`. Up/down pick the row above/below (re-sorting, as a pick does; the keys' row is the picked one, ringed while the list holds the caret). Enter opens *Write to…*. Esc gives the caret to the lead's composer.
- `ShowWorkers` (`Ctrl+Shift+Y`): `shell.show_workers(None, false, …)` then the list's focus, picking the most urgent. The deck's own keys stay bound for its rows until step 4 but nothing opens it by key.
- `pinned_allow`, `pinned_always`, `pinned_deny`: context `"(Chat || Workers) && !Dialog"`; `WorkersPanel` handles them by asking the pane to answer the picked worker's pinned card (`transcript::answer_pinned` on its session), nothing when it has none.
- [ ] Keymap tests already in `keymap.rs` stay green; add `ordered`-based test `a_key_step_stops_at_either_end` for the step helper reused from the deck.
- [ ] Commit `feat(chat): the keys of the workers' list`.

### Task 4: The whole command

**Files:** Modify `crates/app/src/chat/transcript/tool/command.rs`, `crates/app/src/chat/transcript/metrics.rs` (`COMMAND_H` to ten lines).

- Folded (default): `max_h(COMMAND_H)` (not the panel share), `overflow_hidden`, the fade while it overflows, no scrollbar, and under the box a quiet link *Show all N lines* (N = the command's lines; *Show all* for a single wrapped line). Open: `max_h` the panel share (`COMMAND_SHARE` of `well`), scrolling only past that, and *Show less*. The open state is keyed per card (`window.use_keyed_state(("perm-command-open", key))`).
- [ ] Test `show_all_says_how_many_lines` on the label helper (1 → `Show all`, 30 → `Show all 30 lines`).
- [ ] Commit `feat(chat): a permission card shows its command whole`.

### Task 5: Documents

- `docs/screens.md`: the composer paragraph's deck sentence loses its keys (`Ctrl+Shift+Y` now opens the region's list); the workers' region bullet rewritten for list over detail, the order and its freezing, the detail, *Write to…*, the keys, *Workers · N*. `docs/ui-patterns.md` only if the command bound is described there. CHANGELOG's lead/workers entry updated.
- [ ] `git diff --check`; commit `docs(chat): the workers' list`.
