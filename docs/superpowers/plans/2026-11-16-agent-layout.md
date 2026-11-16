# Agent Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redraw onehand's window as borderless rounded regions on the window's fill, with the terminal spanning under the conversation and the Workbench, a quieter title bar and rail, an empty-session greeting and one Workbench row.

**Architecture:** The contract (`DESIGN.md`, `docs/screens.md`) changes first. Surfaces change in the dock skin and the shared `onehand-ui` tab helpers. The Workbench leaves `DockPlacement::Right` for the right slot of an `h_split` in the `DockArea`'s centre, behind a small shell-side API, so the bottom dock (the terminal) is drawn under both. Persistence stays `PanelLayout`'s fields.

**Tech Stack:** Rust, GPUI (`gpui-pre`), gpui-component / gpui-base 0.8 dock, `onehand-ui`.

**Spec:** `docs/superpowers/specs/2026-11-16-agent-layout-design.md`

## Global Constraints

- The composer is unchanged in every part.
- No hex literal, hard-coded colour, radius or size: theme tokens, `tokens::*` and rem steps only (`.claude/skills/ui/tokens.md`).
- Region radius `cx.theme().radius_lg` (8px); region gap the `1p5` step (6px), named `tokens::REGION_GAP`.
- Boxes: fill `theme.background`; window, title bar and rail: `theme.sidebar`. No edge on a box, no hairline between regions or inside a box's strips.
- Weights: *New session*, *Sessions* heading `font_semibold` in `foreground`-strong (`tab_active_foreground`); page and project rows `font_medium`; session title regular, its line regular muted at `MICRO_TEXT`; conversation name `font_medium`; empty heading `font_semibold` at `text_xl`; tabs, tree rows, terminal rows regular.
- Empty heading copy, exact: `What are we building?`
- Persistence: `PanelLayout { workbench_w, workbench_open, terminal_h, terminal_open, rail_w }` only; never `DockAreaState`/`dump`/`load`.
- Code never cites a document; comments give reasons in their own words.
- Any touched file past ~800 non-test lines is split along a seam in the same task (`shell/sessions.rs` 849, `terminal.rs` 843, `shell/boot.rs` 807, `rail.rs` 800).
- Every task ends green on: `make fmt`, `cargo test -p onehand`, `cargo test -p onehand-ui`, `make clippy CLIPPY_EXTRA="-- -D warnings"`. Never launch the app to self-verify; the owner looks after each task (a stop).
- One PR on `style/agent-layout`; commit titles `type(scope): summary`, each ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Narrow window with the Workbench open: the conversation keeps `CHAT_MIN` and, below the threshold, the Workbench takes the area under *← Conversation*, unchanged from today. Pinned in Task 5.
2. Restart with a saved `workbench_w` and `workbench_open = false`: the Workbench comes back closed and reopens at the saved width, not a 50/50 split. Pinned in Task 5.
3. Closing the Workbench while it holds focus: the keymap stays alive (`ChatPane::reclaim_focus` runs before it leaves the frame). Pinned in Task 5.
4. A session that connects, fails, or receives a permission card before its first item: no greeting over a card or an error. Pinned in Task 4.
5. A terminal with a command running in its second shell while the first is chosen: the live dot shows on the second shell's row in the side list. Pinned in Task 7.

---

### Task 1: Contract

**Files:**
- Modify: `DESIGN.md` (*Principles* 1 wording on docks if it names the right dock; *Layout* prose, diagram, region list; *Components* where it names a tab's primary stroke)
- Modify: `docs/screens.md` (*Title bar*, *Status bar* → removed, *Rail*, *Agent pane header* + a new *Empty session* paragraph, *Docks*, *Persistence*)
- Modify: `docs/ui-patterns.md` only where it describes a tab bar's look or the status bar
- Modify: `.claude/skills/ui/tokens.md` (add `REGION_GAP` under *Spacing*, the box's fill/radius under *Corners*)

- [ ] **Step 1:** Rewrite the *Layout* diagram (plain ASCII + box-drawing, aligned) to the spec: title bar with three toggles; rail on the window fill; agent pane and Workbench as boxes side by side; the terminal box under both; no status bar.
- [ ] **Step 2:** Write each goal 1-9 of the spec into the owning section at the level the contract writes (structure and behaviour, not values beyond tokens). Remove "Nothing else in the title or status bar" for the status bar; keep "no right toolbar".
- [ ] **Step 3:** `grep -n "status bar\|Status bar\|workspace bar\|right dock\|stroke of the primary" DESIGN.md docs/*.md .claude/skills/ui/*.md` — every hit either rewritten or still true.
- [ ] **Step 4:** Commit `docs(design): draw the window as boxed regions over a full-width terminal`. **Stop for the owner.**

### Task 2: Boxes, surfaces and tabs

**Files:**
- Modify: `crates/ui/src/tokens.rs` (add `pub const REGION_GAP: Rems = rems(0.375);`)
- Modify: `crates/ui/src/surface.rs` (`dock_surface` returns `cx.theme().background`; add `pub fn region(cx: &App) -> Div` — `size_full`, `bg(background)`, `rounded(radius_lg)`, `overflow_hidden`)
- Modify: `crates/ui/src/tabs.rs` (`tab_bar`: no fill, no `border_b_1`, `px_1`/`gap_1`, items centred; `panel_tab_frame`: no `border_r_1`, no primary stroke, chosen = `rounded(radius)` + `bg(secondary)` + `chosen_ink`, height a step short of the bar)
- Modify: `crates/app/src/shell/dock_skin.rs:86-96` (no `border_l_1`/`border_t_1`; content wrapped in `onehand_ui::region(cx)`; implement `split_frame`/`render_split_handle` so the handle sits in the `REGION_GAP`)
- Modify: `crates/app/src/shell/render.rs` (body `p(REGION_GAP)` without top padding, `gap(REGION_GAP)`, on `sidebar`; the chat's centre panel in a `region`)
- Modify: `crates/editor/src/split.rs`, `crates/markdown/src/view.rs` (no hairline between tree and detail; the tree parts from the detail by standing on `theme.sidebar`, one step off the box's fill)
- Test: `crates/app/src/theme.rs` tests

- [ ] **Step 1: Pin the approved fill gap** in `crates/app/src/theme.rs` tests (it passes today: dark 1.077, light 1.062; it fails if a later palette edit sinks the boxes into the window):

```rust
/// A box (the page fill) stands apart from the window's fill it sits on
/// by at least the gap the owner approved in the dark palette's mock-up.
const BOX_ON_WINDOW: f32 = 1.06;

#[test]
fn a_box_stands_apart_from_the_window() {
    for (name, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        let theme = installed(mode);
        let ratio = contrast(theme.background, theme.sidebar);
        assert!(ratio >= BOX_ON_WINDOW, "{name}: box on window {ratio:.3}");
    }
}
```

- [ ] **Step 2:** `cargo test -p onehand a_box_stands_apart` — PASS.
- [ ] **Step 3:** Implement the file changes above. The terminal grid and Neovim read `dock_surface`, so their background follows with no further edit.
- [ ] **Step 4:** `cargo test -p onehand-ui && cargo test -p onehand` — PASS, including `theme.rs` contrast tests (if a pair names `sidebar` as a dock's surface, move it to `background`).
- [ ] **Step 5:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 6:** Commit `style(shell): draw each region as a box on the window's fill`. **Stop.**

### Task 3: Title bar, status bar, rail, weights

**Files:**
- Modify: `crates/app/src/shell/frame.rs` (`dock_toggles` :210 leads with a rail toggle, `IconName::PanelLeft`, dispatching the existing hide-rail command, on/off ink as `panel_toggle`; delete `status_bar` :191-204)
- Modify: `crates/app/src/shell/render.rs:222` (drop `.child(self.status_bar())`)
- Modify: `crates/app/src/rail/search.rs` (`search_field` leads with the workspace name segment: a focusable button, `ChevronDown`, opening `workspace_menu` via `menu_below` on press and Enter; `sessions_header` gains a search `quiet_icon` button focusing the field as `Ctrl+K` does; heading `font_semibold`)
- Modify: `crates/app/src/rail/workspace.rs` (delete `workspace_bar`; make `workspace_menu` `pub(super)` for `search.rs`; `new_button` → `new_session_row`: a page-like row, `Plus`, *New session* `font_semibold`, the keymap's hint for the new-session command at its end, a `ChevronDown` at its end on hover / keyboard row / always on right-click opening `new_session_menu`)
- Modify: `crates/app/src/rail.rs:252-385` (mount order: `new_session_row`, pages, sessions header, list, foot; no workspace bar, no hide button); page rows and project rows `font_medium`; split `rail.rs` (800) by moving `pages()` and the foot into `rail/pages.rs`
- Modify: `crates/app/src/rail/session.rs` (title regular; second line `MICRO_TEXT`)
- Modify: `crates/app/src/chat/pane/header.rs` (name `font_medium`, if not already)
- Test: `crates/app/src/rail/tests.rs`, `crates/app/src/shell/frame.rs` tests

- [ ] **Step 1: Write the failing test** in `frame.rs` tests: `fn the_title_bar_toggles_rail_terminal_workbench_in_order()` asserting a new pure `fn toggle_order() -> [Toggle; 3]` equals `[Toggle::Rail, Toggle::Terminal, Toggle::Workbench]`, where `enum Toggle { Rail, Terminal, Workbench }` drives `dock_toggles`.
- [ ] **Step 2:** `cargo test -p onehand the_title_bar_toggles` — FAIL (no `Toggle`).
- [ ] **Step 3:** Implement the files above. `every_menu_item_is_a_command` must still pass: the View menu already offers the rail's toggle.
- [ ] **Step 4:** `cargo test -p onehand` — PASS; guards green (`no_field_is_written_and_never_read` catches a leftover workspace-bar field).
- [ ] **Step 5:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 6:** Commit `style(rail): move the workspace into the title bar and drop the status bar`. **Stop.**

### Task 4: Empty-session greeting

**Files:**
- Create: `crates/app/src/chat/pane/greeting.rs` (the rule and the heading)
- Modify: `crates/app/src/chat/pane/body.rs:478+` (when `greets(..)`, mount header, then a `v_flex().flex_1().justify_center()` holding the heading and the composer, the list not mounted)
- Test: `crates/app/src/chat/pane/greeting.rs` tests module

**Interfaces:**
- Produces: `pub(super) fn greets(connected: bool, items: usize, over_composer: bool) -> bool` — true only for `connected && items == 0 && !over_composer`. `over_composer` is whatever `over_composer.rs` would draw (reconnect, queue, staged files, cards, lead strip, worker band) being non-empty; `body.rs` already computes it.
- Produces: `pub(super) const GREETING: &str = "What are we building?";`

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn greets_only_a_connected_empty_session_with_nothing_over_the_composer() {
    assert!(greets(true, 0, false));
    assert!(!greets(false, 0, false)); // connecting or failed
    assert!(!greets(true, 1, false));  // first item moves the composer down
    assert!(!greets(true, 0, true));   // a card or banner stands over the composer
}
```

- [ ] **Step 2:** `cargo test -p onehand greets_only` — FAIL.
- [ ] **Step 3:** Implement `greets`, `GREETING` and the body branch; heading `text_xl`, `font_semibold`, centred, the composer's own max width unchanged.
- [ ] **Step 4:** `cargo test -p onehand` — PASS.
- [ ] **Step 5:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 6:** Commit `feat(chat): greet an empty session with the composer centred`. **Stop.**

### Task 5: The Workbench into the centre (terminal spans)

**Files:**
- Create: `crates/app/src/shell/workbench_slot.rs` (+ `workbench_slot/tests.rs`)
- Modify: `crates/app/src/shell/boot.rs:120-150` (mount through `workbench_slot::mount`; this moves code out of `boot.rs`, bringing it under 800)
- Modify: `crates/app/src/shell/docks.rs` (:12 `workbench_open`, :153-159 `show_workbench`, :184-192 `hide_workbench`, :427 and :590-603 zoom)
- Modify: `crates/app/src/shell/presentation.rs` (:109-186: `dock_size(Right)`, `toggle_dock(Right)`, `set_dock_size(Right)`, `zoom_in(Right)` → slot API; pure `presentation()` and its tests untouched)
- Modify: `crates/app/src/shell/issues_page.rs:238`, `crates/app/src/shell/roots.rs:127-130,416-418`, `crates/app/src/shell/sessions.rs:704-708` (open/close → slot API; split `sessions.rs` (849) along a seam it already has, e.g. the page-docks helpers into `sessions/pages.rs`)
- Modify: `crates/app/src/shell/dock_skin.rs:71,92` (no Right case left to draw)
- Modify: `crates/app/src/workbench/panel.rs` (`Panel::visible` reads a `shown: bool`; `set_shown(&mut self, bool, cx)`)
- Modify: `crates/app/src/chat/pane.rs` (same `shown` + `visible` for the chat's panel, used by `WorkbenchFocus` and maximize); remove the vestigial `docks_open` field (pane.rs:279,534) if nothing reads it
- Test: `crates/app/src/shell/workbench_slot/tests.rs`

**Interfaces:**
- Produces, in `workbench_slot.rs`, all `pub(super)`:
  - `fn mount(area: &mut DockArea, chat: Arc<dyn PanelView>, workbench: Arc<dyn PanelView>, layout: &PanelLayout, window: &mut Window, cx: &mut Context<DockArea>)` — centre = `h_split` of `tabs(chat)` and `tabs(workbench)`, sizes from `layout.workbench_w`.
  - `fn set_open(shell: &mut Shell, open: bool, window: &mut Window, cx: &mut Context<Shell>)` — flips `Workbench::set_shown`; on close runs `ChatPane::reclaim_focus` first when the Workbench holds focus.
  - `fn set_width(area: &mut DockArea, width: Pixels, window: &mut Window, cx: &mut Context<DockArea>)` — `set_split_sizes` on the centre split.
  - `fn width(area: &DockArea, cx: &App) -> Option<Pixels>` — the Workbench slot's size from `area.layout(DockPlacement::Center)`.
  - `fn set_chat_shown(shell: &mut Shell, shown: bool, cx: &mut Context<Shell>)` — used by `Presentation::WorkbenchFocus` and by maximizing the Workbench (maximize no longer calls `zoom_in`; the terminal stays).
- `Shell::workbench_open(&self, cx) -> bool` keeps its name, now `self.workbench.read(cx).shown()`.
- `DockEvent::LayoutChanged` from a drag writes `width(..)` into `PanelLayout.workbench_w` where `presentation.rs:121-127` writes it today.

- [ ] **Step 1: Write the failing tests** in `workbench_slot/tests.rs`, `#[gpui::test]`, building a `DockArea` with the app's `DockSkin` and probe panels as gpui-component's `the_centre_and_the_bottom_dock_share_the_column` does:
  - `fn the_terminal_spans_the_conversation_and_the_workbench` — after `mount` with the Workbench shown and a bottom dock set, the bottom probe's width equals the chat probe's + the Workbench probe's + the gap (± 1px).
  - `fn a_hidden_workbench_gives_the_conversation_the_width` — after `set_shown(false)` and a frame, the chat probe's width equals the centre's.
  - `fn the_saved_width_comes_back_when_reopened` — mount with `workbench_open: false, workbench_w: 480.`, show it, assert its probe ≈ 480px.
  - `fn hiding_the_chat_leaves_the_terminal` — `set_chat_shown(false)`: Workbench probe width = centre width, bottom probe still measured > 0.
- [ ] **Step 2:** `cargo test -p onehand workbench_slot` — FAIL (no module).
- [ ] **Step 3:** Implement `workbench_slot.rs`. If `Panel::visible` is not re-read on the panel's `cx.notify()` (the second test still fails), call `cx.notify()` on the `DockArea` entity inside `set_open`/`set_chat_shown` as well; do not fall back to `set_center` rebuilds, which drop the tab panels' focus.
- [ ] **Step 4:** `cargo test -p onehand workbench_slot` — PASS.
- [ ] **Step 5:** Move every caller off `DockPlacement::Right` (list in *Files*). `grep -rn "DockPlacement::Right" crates/` returns nothing.
- [ ] **Step 6:** `cargo test -p onehand` — PASS, `presentation/tests.rs` and `dock_rules` tests unchanged; add to `presentation/tests.rs` `fn a_narrow_window_still_gives_the_workbench_the_area()` driving `Presentation::WorkbenchFocus` through `set_chat_shown(false)` and back on *← Conversation*.
- [ ] **Step 7:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`; guards green (`no_flag_is_only_ever_assigned_false` on `stepped_aside`).
- [ ] **Step 8:** Commit `refactor(shell): set the Workbench beside the conversation so the terminal spans both`. **Stop.**

### Task 6: One Workbench row; trees on the right

**Files:**
- Modify: `crates/workbench/src/lib.rs` (trait method below)
- Create: `crates/app/src/workbench/panel/picker.rs` (`ModePicker` entity: `Editor ▾` button + menu of offered modes, calls `Workbench::set_mode`; `StripEnd` entity: *← Conversation* when the Workbench has the area, maximize, hide — moved from `strip.rs`)
- Modify: `crates/app/src/workbench/panel/strip.rs` (delete `mode_tab`, `mode_select`, `MODES_MIN_W`; the row for a mode that takes no ends is `tab_bar` + picker + filler + end)
- Modify: `crates/app/src/workbench/panel.rs` (hand `BarEnds` to each mode at mount; draw the fallback row only for modes that return false)
- Modify: `crates/editor/src/view.rs:481-505`, `crates/editor/src/buffers.rs:90` (the file strip leads with `ends.lead` and ends with `ends.end`; the tree toggle stays after the lead)
- Modify: `crates/editor/src/split.rs`, `crates/markdown/src/view.rs:495` (tree slot second, detail first)
- Test: `crates/app/src/workbench/panel` tests

**Interfaces:**
- Produces in `onehand-workbench`: `pub struct BarEnds { pub lead: AnyView, pub end: AnyView }` and on `WorkbenchMode`: `fn take_bar_ends(&mut self, _ends: BarEnds, _cx: &mut App) -> bool { false }` — true means the mode draws the ends in its own bar. Editor returns true; every other mode keeps the default.

- [ ] **Step 1: Write the failing test** `fn only_the_editor_draws_the_workbench_row_itself()` in the panel tests: mount the composed modes from `features.rs` and assert the set returning true from `take_bar_ends` is exactly `{ModeId::Editor}`.
- [ ] **Step 2:** `cargo test -p onehand only_the_editor_draws` — FAIL.
- [ ] **Step 3:** Implement. The picker is one control at every width, so no narrow select remains.
- [ ] **Step 4:** `cargo test -p onehand && cargo test -p onehand-editor -p onehand-markdown -p onehand-workbench` — PASS; `our_event_enums_are_matched_exhaustively` green.
- [ ] **Step 5:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 6:** Commit `style(workbench): one row, a mode picker then the mode's tabs`. **Stop.**

### Task 7: Terminal shell list

**Files:**
- Create: `crates/app/src/terminal/shells.rs` (the side list; first move the tab-bar code out of `terminal.rs` ≈:583-800 into it, bringing `terminal.rs` under 800)
- Modify: `crates/app/src/terminal.rs` (top bar: the open shell's name, filler, `+`, maximize, hide; the body an `h_flex` of the grid and, when `shells > 1`, the list)
- Modify: `crates/ui/src/tokens.rs` (`pub const SHELL_LIST_W: Rems = rems(10.);`)
- Test: `crates/app/src/terminal/tests.rs`

**Interfaces:**
- Produces: `pub(super) fn lists_shells(count: usize) -> bool` (`count > 1`).
- A row: `tree_row`-height, `SquareTerminal` glyph, the name capped, the live dot (as the tab's today) when that shell runs a command, `tab_close` on hover and on the keyboard's row, chosen on the rounded `secondary` fill; the chosen row scrolled into view.

- [ ] **Step 1: Write the failing tests:** `fn one_shell_draws_no_list()` (`!lists_shells(1)`, `lists_shells(2)`); `fn a_busy_shell_that_is_not_chosen_keeps_its_dot()` asserting the row model for shell 2 of 2, running, not chosen, has `live: true` (extract the row's facts as `struct ShellRow { name, live, chosen }` built by `fn shell_rows(..) -> Vec<ShellRow>` from the same state the tabs read today).
- [ ] **Step 2:** `cargo test -p onehand one_shell_draws_no_list a_busy_shell` — FAIL.
- [ ] **Step 3:** Implement. `Terminal` key context, focus tracking and the title bar's live dot are untouched.
- [ ] **Step 4:** `cargo test -p onehand` — PASS. Read `.claude/rules/terminal.md` before this task.
- [ ] **Step 5:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 6:** Commit `style(terminal): list shells down the dock's side`. **Stop.**

### Task 8: Close-out

**Files:**
- Modify: `CHANGELOG.md` (`[Unreleased]`: the layout entry; under *Breaking* that the status bar and the rail's workspace bar are gone)
- Delete: `docs/superpowers/specs/2026-11-16-agent-layout-design.md`, `docs/superpowers/plans/2026-11-16-agent-layout.md`
- Check: `docs/known-gaps.md` for anything this made true or false

- [ ] **Step 1:** Re-read `DESIGN.md` and `docs/screens.md` against the built app's code paths changed in Tasks 2-7; fix any drift in the docs.
- [ ] **Step 2:** `cargo test && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo tree -p onehand-core -i gpui-pre` (the last must error "did not match any packages").
- [ ] **Step 3:** Run the `ui` skill's checklist over the whole diff.
- [ ] **Step 4:** Commit `docs(changelog): note the agent layout` and open the PR (title `style(shell): draw the window as boxed regions over a full-width terminal`, plain-text body, squash).
