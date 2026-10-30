# Lead workers, PR 5: the person's view of the workers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a lead's workers leave the rail and are seen from the lead: its rail row carries their most urgent state, a deck tucked behind its composer counts them and slides up to a row each, and a workers' region in the Workbench's place stacks a block per worker where the person reads, answers, stops, types, gives the worker back to the lead or closes it.

**Architecture:** `control` answers who is whose (`lead_of`, `workers_of`, opens in flight included) and moves `sent` for *Give back*. The rail folds workers into their lead in its pure model. The pane never shows a worker of an open lead: the one place every "show this session" goes through redirects to the lead with the region open at that worker. The region is a dock panel of its own (`WorkersPanel`) swapped into the right dock in the Workbench's place, as the terminal is mounted and removed; it draws through the `ChatPane`, which owns the conversations, their viewports and their list states, so a worker's list is still painted in one place only. Each open block gets a compact `Composer` of its own. The deck is `onehand_ui::tucked`, the one builder allowed to animate. The rules a test needs (rail folding, the deck's summary, the region following the lead, focus hand-back, the stack's heights) are pure functions with tests; there are no GPUI tests in the app.

**Tech Stack:** Rust, GPUI, gpui-component 0.8.

**Two pull requests** (decided 2026-10-30): **5a** is Tasks 1–7 and Task 9 for what they build (the rail, the region, the blocks, *Give back*), so `main` never hides a worker with nowhere to see it; **5b** is Task 8 (the deck and *Show the workers*) and Task 9 for it.

**Spec:** `docs/proposals/lead-workers/spec.md` section 7 and section 5's *Give back to the lead*; `onehand.md` section 8. Sketch: `.superpowers/brainstorm/lead-workers.html`.

## Global Constraints

- A worker is a session with a relation **or** an open in flight (`Requests` `Seen::Opening(Some(uid))`), so a connecting worker is in the deck and the region (head spinner, no body) and never flashes in the rail.
- `Sessions` is a mutex shared with the socket's runtime: every accessor copies out and drops the guard before any entity is read or updated.
- A worker's `ChatSession` is drawn only in the region while its lead is open; its viewport's `ListState` is painted only there.
- Focus leaves a block, the region or the deck **before** it leaves the frame (`ChatPane::reclaim_focus`).
- `nothing_animates` stays as it is: only `crates/ui/src/tucked.rs` animates.
- Bounded: at most four workers per lead (the cap), so the deck and the region need no further cap; the folded tail has a named height.
- Every size in rems from `onehand_ui::tokens` or a named constant; every ink from `cx.theme()` / `status_ink`; every control through `controls::action`, every menu row through `menu_item`.
- Files stay under ~800 lines: `control.rs` (729) gets nothing but a module line; the pane's new code goes in new modules.
- Comments never cite documents. Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui-pre` erroring.

## Builder's calls (said here, changeable at review)

- **The worker's name** is the rail's own label (`Shell::session_label`); onehand does not know the job letter.
- **A project left with nothing but hidden workers** is left out of the rail when it is transient (a worktree the open added); a project the person added still shows, with *No sessions*.
- **The lead row's age** is the most urgent one's: when a worker's state wins, its `since`.
- **The deck stays while the region is open**; it is where the count lives.
- **The header button** sits between the terminal's and the Workbench's, a bot-like `IconName` picked in Task 6; a dot in the warning ink while a worker waits on the person, as the terminal's dot.
- **The region and the Workbench share the right dock's width** (`workbench_w`); the region's open state is not saved.
- **The compact composer** has no completion popup (`@`, `/`) in this PR: those draw in the pane's overlay. Typed text is sent as written.
- ***Show the workers*** is bound to `ctrl-shift-y` (free in `keymap/commands.rs`), context `Shell && !Dialog`.

## Review Focus

- A session shown by any route (rail search, MRU, a toast, the remote bridge, `show_session`) that is a worker of an open lead lands on the lead with the region at that worker. Pinned in Task 3.
- A worker whose lead closes reappears in the rail under its project, and its block leaves the region with focus handed back first. Pinned in Tasks 2 and 7.
- *Need attention* counts a waiting worker even though its row is hidden. Pinned in Task 2.
- *Give back* is not offered once the lead is switched off, and does nothing when the relation is gone by the time it is pressed. Pinned in Task 1.
- The Workbench opened while the region holds the dock (keymap, a file opened from the chat, step aside, a page) moves focus out of the region first, and every `is_dock_open(Right)` reader still means what it meant. Pinned in Task 6.

---

### Task 1: who is whose, and *Give back*

**Files:** Create `crates/app/src/control/workers/whose.rs`; modify `control/workers.rs` (module line), `control/workers/requests.rs` (`Requests::opening_of(lead) -> Vec<u64>`, `lead_opening(uid) -> Option<u64>`), `control/workers/opening.rs` (notify the window's shell once the relation is written); tests in `control/workers/tests.rs`.

**Interfaces (crate-visible):**
- `pub(crate) struct Worker { pub(crate) uid: u64, pub(crate) turn: Option<u64>, pub(crate) opening: bool, pub(crate) sent: Option<usize> }`.
- `pub(crate) fn lead_of(worker: u64) -> Option<u64>` — the relation's lead, else an open in flight's.
- `pub(crate) fn workers_of(lead: u64) -> Vec<Worker>` — relations and opens in flight, by uid (creation order).
- `pub(crate) fn give_back_offered(worker: u64, prompts_sent: usize, cx: &App) -> bool` — a relation, `prompts_sent > sent`, the lead switched on and still a session of some window.
- `pub(crate) fn give_back(worker: u64, prompts_sent: usize, cx: &App) -> bool` — the same check, then `sent = prompts_sent`; false when no longer offered. Nothing sent, nothing written in the transcript.
- The pure core of the offer: `fn offered(relation: Option<&Relation>, prompts_sent: usize, lead_on: bool, lead_open: bool) -> bool`.

- [ ] **Step 1: Failing tests:** `an_open_in_flight_is_a_worker`; `a_relation_names_its_lead`; `a_lead_forgotten_lets_its_workers_go` (via `forget`); `give_back_is_offered_only_while_the_person_holds_it`; `give_back_is_not_offered_once_the_lead_is_off`; `give_back_moves_sent_to_now` (and `session_state` then reads `held by: lead`, through core's `state`).
- [ ] **Step 2–4:** FAIL, implement, PASS (`cargo test -p onehand --locked control::`).
- [ ] **Step 5:** Commit `feat(control): which lead a worker has, and a worker given back`.

### Task 2: the rail folds workers into their lead

**Files:** Modify `crates/app/src/rail/model.rs` (`Item.lead: Option<u64>`, `fn fold_workers(items: Vec<Item>, lead_open: &dyn Fn(u64) -> bool, transient: &dyn Fn(usize) -> bool) -> (Vec<Item>, Vec<usize>)` returning the items kept and the roots to leave out; `attention_count` taken before the fold), `crates/app/src/rail.rs` (`List::of` fills `lead` from `control::lead_of`, folds, passes the hidden roots to `rows`), `crates/app/src/shell.rs` + `shell/sessions.rs` (`RailSession.lead`, so `rail_sessions`, the repaint key, changes with the relation); tests in `rail/tests.rs`.

Rules: a worker's item is dropped; its lead's item takes `SessionSignal::most_urgent` of its own and its workers' signals, and the winner's `since`; `background`/`subagents` stay the lead's own. A transient root left with no items is left out of `rows`. With the lead gone (`lead_of` none) nothing is folded.

- [ ] **Step 1: Failing tests:** `a_worker_of_an_open_lead_is_not_listed`; `the_lead_row_takes_its_most_urgent_worker`; `a_waiting_worker_counts_as_needing_attention`; `the_workers_come_back_when_the_lead_goes`; `a_worktree_left_with_only_hidden_workers_is_left_out`; `a_project_the_person_added_still_shows_empty`; the *Needs attention* filter lists the lead of a waiting worker.
- [ ] **Step 2–4:** FAIL, implement, PASS (`cargo test -p onehand --locked rail::`).
- [ ] **Step 5:** Commit `feat(rail): a lead's workers fold into its row`.

### Task 3: the pane never shows a worker of an open lead

**Files:** Modify `crates/app/src/shell/roots.rs` (`arrive_at_active_root`, where every route to a shown session ends), `shell/sessions.rs`; create the pure rule in `crates/app/src/workers.rs` (new private module, listed in `lib.rs`): `fn arrival(active: u64, lead_of: Option<u64>) -> Arrival { Show(u64), Lead { lead: u64, worker: u64 } }`.

On `Lead`, the shell selects the lead's root and session (the worker's root stays where it is), then opens the region at the worker (Task 6; until then the lead alone shows).

- [ ] **Step 1: Failing tests** in `crates/app/src/workers/tests.rs`: `a_plain_session_shows_itself`; `a_worker_shows_its_lead_at_it`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5:** Commit `feat(shell): a worker is shown through its lead`.

### Task 4: drawing a conversation by uid

**Files:** Modify `crates/app/src/chat/pane/runs.rs` (`reproject_for(uid, room)`, `run_element_for(uid, ix, …)`; `reproject` and `run_element` become the active session's calls of them), `chat/pane/body.rs` (`pinned_for(uid, …)` from `pinned`).

No behaviour change for the agent pane. A refactor of reads from `active_conversation()` to `conversations.get(&uid)`.

- [ ] **Step 1:** `cargo test -p onehand --locked` green before and after; the guards included.
- [ ] **Step 2:** Commit `refactor(chat): draw a conversation's runs by its uid`.

### Task 5: the compact composer

**Files:** Modify `crates/app/src/chat/composer/card.rs` (a `Shape { compact: bool }` argument to `card`: compact leaves out `+`, *Fast* and the model and effort chips; Send / Stop / Queue drawn by `send_controls` as now), `chat/pane/sessions.rs` (`submit` becomes `submit_to(uid, composer)`, the main composer's press its call for the active session; Stop the same: `stop(uid)`), `chat/pane.rs` (`worker_composers: HashMap<u64, Entity<Composer>>`, subscribed as the main one is; dropped when the worker leaves).

- [ ] **Step 1:** A pure test where the shape decides what is drawn, if `card`'s chip list can be read without a window (`fn chips(shape, offered) -> Vec<Chip>`); otherwise none, and Task 9's checklist covers it.
- [ ] **Step 2:** Commit `feat(chat): a compact composer for a session not on screen`.

### Task 6: the region, its header button and its place in the dock

**Files:** Create `crates/app/src/chat/workers_panel.rs` (`WorkersPanel`: `Panel` (`panel_name` "Workers"), `Focusable`, `track_focus` in `render`; holds a `WeakEntity<ChatPane>` and draws through `ChatPane::workers_region`), `crates/app/src/chat/pane/workers.rs` (the region: strip and the stack, Task 7 fills the blocks); modify `shell/docks.rs` (`RightPanel { Workbench, Workers }` on the shell; `show_workers(at: Option<u64>)`, `hide_workers`, `toggle_workers`; the swap mounts one panel in the right dock with `set_dock`, keeping the dock's size; `FocusedPanel::Workers`), `shell/boot.rs`, `chat/pane/header.rs` (the button, filled while open, present only while the lead on screen has workers; `ChatPaneEvent::ToggleWorkers`), `chat/pane.rs` (`DocksOpen.workers`), `shell/roots.rs` (the region follows the lead on screen).

**Every `DockPlacement::Right` reader is read** (27 today, in `shell/{boot,docks,roots,sessions,presentation,issues_page,dock_skin}.rs`): each one that means "the Workbench is open" asks `RightPanel::Workbench` too; `show_workbench` puts the region away first and the region the Workbench, focus moved out of the one leaving to the lead's composer, the one arriving taking none.

Pure rules in `crates/app/src/workers.rs`:
- `fn follow(open: bool, lead_on_screen: Option<u64>, has_workers: bool) -> Follow { Stay, Close }` — the lead on screen changing keeps the region on the new lead's workers or closes it.
- `fn hand_back(focus_in_leaving: bool) -> bool` — trivial, kept only if a second caller appears; otherwise inline at each site.

- [ ] **Step 1: Failing tests:** `the_region_follows_a_lead_with_workers`; `the_region_closes_on_a_session_without_workers`; `the_region_closes_on_a_page`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Then the dock audit: list each `Right` site in the commit body as read and kept, or changed.
- [ ] **Step 5:** Commit `feat(shell): the workers' region in the Workbench's place`.

### Task 7: a block per worker

**Files:** Create `crates/app/src/chat/pane/workers/block.rs`; modify `chat/pane/workers.rs`, `chat/pane.rs` (`region: Region { open: Option<u64>, scroll: ScrollHandle }`); add `WORKER_OPEN_MIN` and `WORKER_TAIL_H` (rems) to `onehand_ui::tokens` and `tokens.md`.

- Head (`ROW_H`-based, as the rail row): `signal_mark`, the name, the state word (`rail::model::word`), *held by person* while `prompted_since(sent)`, *Stop* (secondary, short) while a turn runs, `⋯` (`menu_item`s: *Give back to the lead* refusing with its reason when not offered, *Send the last prompt again* while failed or lost, as the rail's resend, *Close*). Pressing the head opens the block.
- Folded: the worker's list at `WORKER_TAIL_H`, kept at its end, and `pinned_for(uid)`. Open (one at most; the first opened by default): the list at the room left, never under `WORKER_OPEN_MIN`, the pinned card, the compact composer, which takes the caret when opened from the deck or the strip.
- Connecting: the head's `Spinner`, no body. Failed and lost: the failure's words in the body.
- Heights: `fn fit(avail, heads, tails, pinned, open_min) -> Fit { tails: bool, scroll: bool }`: tails dropped first, then the stack scrolls as a whole, the open block keeping its minimum.
- Leaving: before a block leaves while it holds focus (closed by the person or elsewhere, its lead gone), `reclaim_focus` first, then the block goes; the region closes with its last block.

- [ ] **Step 1: Failing tests:** `fit_keeps_every_tail_when_there_is_room`; `fit_drops_the_tails_first`; `fit_scrolls_when_even_the_heads_do_not_fit`; `the_first_worker_opened_is_open`; a worker leaving the open block opens none (the person picks next).
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5:** Commit `feat(chat): a block per worker in the region`.

### Task 8: the deck

**Files:** Create `crates/ui/src/tucked.rs` (`pub fn tucked(id, open: bool, edge: AnyElement, body: AnyElement, cx) -> impl IntoElement`: the edge always drawn, the body sliding up from under the card over ~200ms eased when `open` turns true and back when false, the animation keyed on the open state so it plays only on a press; no `TabBar`); list it in `crates/ui/src/lib.rs` and `catalog.md`. Create `crates/app/src/chat/pane/deck.rs`; modify `chat/pane/overlay.rs` (the deck between the pinned card and the composer card, narrower than the card), `keymap/commands.rs` + `shell.rs` (`ShowWorkers`), `chat/pane.rs` (`deck: Deck { open: bool, row: usize, focus: FocusHandle }`, `track_focus`).

- Folded edge: *N workers*, then one count per state that has any, most urgent first, each with its mark and ink, then the most urgent worker's name and age.
- Open: a row each (mark, name, state word and age, turn, background count; under them its latest activity, or the question it waits on in the warning ink); *Stop* while it runs. Pressing a row, or `Enter`, opens the region at that worker; `↑`/`↓` move; `Esc` folds and gives the caret to the lead's composer. Opening or folding with the pointer leaves the caret where it was; *Show the workers* opens it with focus on its first row.
- Shown only while the lead on screen has workers.
- Pure: `fn summary(signals: &[Option<SessionSignal>]) -> Vec<(Option<SessionSignal>, usize)>` most urgent first; `fn step(row, len, up) -> usize`.

- [ ] **Step 1: Failing tests:** `the_summary_counts_each_state_most_urgent_first`; `the_summary_names_the_most_urgent_worker`; `the_deck_steps_and_stops_at_its_ends`.
- [ ] **Step 2–4:** FAIL, implement, PASS; `nothing_animates` passes with `tucked.rs` the only animating file.
- [ ] **Step 5:** Commit `feat(chat): the workers' deck tucked behind the lead's composer`.

### Task 9: the contract and the checklist

**Files:** `docs/screens.md` (*Rail*: the lead row and hidden workers; *Agent pane header*: the workers' button; *Composer*: the deck; *Docks*: the region, its strip and blocks), `DESIGN.md` *Layout* (one line for the region), `docs/control.md` *A worker* (*Give back*, what the person sees), `.claude/skills/ui/tokens.md` and `catalog.md` (the two tokens, `tucked`); spec section 7 and 5 marked "Built in PR 5".

- [ ] **Step 1:** Answer `.claude/skills/ui/checklist.md`, item by item, in the PR body's notes.
- [ ] **Step 2:** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, the core-is-GUI-free check.
- [ ] **Step 3:** An independent opus review of `origin/main..HEAD`; every finding fixed, minors included.
- [ ] **Step 4:** Commit `docs(lead-workers): the person's view of the workers`; PR, CI, ask before merging.

## Visual check for the owner (`make showcase` or `/drive` when asked)

Both themes; the region narrow and wide; zoom 80% and 150%; a lead with one, then four workers; a worker connecting, running, waiting on a permission, failed, held by the person and given back; the lead closed with the region open and focus in a block; the Workbench opened from the keymap while the region is open; the deck opened by pointer and by `ctrl-shift-y`.
