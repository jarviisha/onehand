run: 2026-11-26-ui

# The strain tests (step 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tests that draw the real views under strain (narrow and wide, long words, zoom, counts) with no screen and read their parts' bounds, one per seed layout rule a structure test can hold; a view that fails is fixed with the smallest change and listed.

**Architecture:** Views gain `debug_selector` marks on the parts a test reads (a no-op outside a debug test build). A helper module beside the shell's rig (`crates/app/src/shell/rig/strain.rs`) builds a shell on a workspace the test names, sets the window's width, and reads bounds by selector with assertions in plain words. Each view's strain tests live in that view's own `tests.rs`. `page_head` in `crates/ui` gets the same few lines locally, since `crates/ui` cannot reach the app's rig.

**Tech Stack:** Rust, gpui-pre 0.3.9 (`VisualTestContext::debug_bounds`, `simulate_resize`, `TestPlatform` with `NoopTextSystem`), gpui-component 0.8.0.

**Spec:** /home/jarviisha/development/rust-projects/onehand-ui-rules-lead/spec.md, sections *Strain tests* and *Layout rules* (step 2). Not committed by this job.

## Global Constraints

- No rule id, spec or document name in code, comments, test names or anything drawn. A test name says the behaviour in plain words.
- No clippy.toml, guards or census (job a); no skill, drive script or verify rule (job c); no rule text in DESIGN.md or docs/ui-patterns.md.
- No view moves onto a new slot-owning builder. A failing view gets the smallest fix, values unchanged where possible; each fix is appended to ask.md when found and listed in the PR body.
- Before fixing a view a test fails on, read the comment on that code and the view's entry in docs/screens.md and DESIGN.md. Where either says the present behaviour is meant, the test is wrong, not the view: change the test and say so in ask.md. A fix that changes what a person sees with ordinary content at a width they can reach is an entry in ask.md that waits for a decision, while the other tasks go on; only a fix that shows under strain alone (a 200-character name, the narrowest window, an overfull list) goes in without waiting, and is listed.
- No new size constant unless a fix needs one, and then it is said in ask.md first. Test data (words, counts, widths read from existing constants) is not a size constant.
- Sizes in tests come from the code's own constants (`WINDOW_MIN`, `PanelLayout::RAIL_MIN`/`RAIL_MAX`, `CHAT_MIN`, `COLUMN_W`, `HOME_ROWS`), never retyped numbers, so a changed value does not leave a test asserting the old one.
- Tests live in `#[cfg(test)]` modules (`foo.rs` + `foo/tests.rs`); no `tests/` directory. Files stay under about 800 lines of code.
- `make fmt`, never bare `cargo fmt`. Cargo through the routed name only; `cargo check`/`cargo run` are not run. One cargo command at a time; `-p onehand --lib <filter>` while iterating.
- The build is done when check.sh passed on the committed SHA.

## The probe (run in turn 1; evidence in artifacts/probe-r1.rs and probe-r1.out)

Run as `cargo test -p onehand --lib --locked strain_probe -- --nocapture --test-threads=1` (routed, debug test build), with a temporary test module and two temporary marks in `shell/frame.rs`, all removed after.

1. **`debug_bounds` works on the shell's rig.** A `debug_selector` on the title bar's end toggles read `Some(1832,8 88×24)` right after `rig::shell` returned, with no `run_until_parked` needed: the rig's `add_window` already drew a frame (the test window is 1920 wide). After `cx.simulate_resize(size(px(700.), px(600.)))` and `run_until_parked`, the same selector read `612,8 88×24` and the start group `12,8 151×24`: a resize redraws, and the bounds follow. A model change made through `cx.update` needs a `run_until_parked` (or `window.refresh()` then park) before the next read, since the map is the last rendered frame's.
2. **`NoopTextSystem` measures and wraps text.** Every char is one glyph advancing 0.6 em (`advance = 600 × glyph id`, glyph id = UTF-16 length, 1000 units per em). At 10 px in a 300 px box: 200 `a` with no space = 1200 px, broken into 4 lines (64 px at its 16 px line); 40 × `word ` likewise 4 lines, broken at spaces; `.truncate()` held one line (16 px); a `flex_1 min_w_0 truncate` name beside a `flex_none` 40 px tail took 260 px and the tail kept 40 px at the row's end. So with the test text a long word does wrap (by character), a wrapped line is taller by whole lines, and truncation and shrinking are laid out as in the app. What it cannot show is where the real font breaks: its widths are a fixed 0.6 em per char, while Be Vietnam Pro's vary per glyph.
3. **A real text system can be given on Linux, with no new dependency.** `gpui_platform::current_platform(true).text_system()` (gpui_platform is already the app's dependency) passed to `TestAppContext::build_with_text_system(gpui::TestDispatcher::new(0), None, ts)`, then `crate::fonts::register`, drew the same view: the 200-char word took 5 lines (80 px) instead of 4, the rest matched. It saw 21 font names on the build host, so any glyph outside the bundled faces falls back to whatever the machine has; such a test cannot use `#[gpui::test]` (it builds its own context and must `quit` it), and on Linux each call builds a platform whose threads are not reclaimed.

**Builder's call from the probe:** the matrix runs on `NoopTextSystem`. It is deterministic on every machine, already wraps and truncates, and is wider per char than the bundled UI face's average, so a control that stays on one line under it has room to spare under the real face for Latin words. The real text system is not used in this step; it is recorded here so a later step can take it if a picture shows a wrap the test text missed. Say so in the PR.

## Seeds a structure test cannot hold with the test platform's text

- *A long name is cut by a fade or an ellipsis*: the test holds the half about width (the name stays inside its bounded slot and neighbours do not move); whether a fade band or an ellipsis is actually painted over the cut is paint, not bounds. Picture.
- *An icon is never clipped by its box*: the test holds that the icon's svg bounds stand inside its button's; a glyph whose stroke touches its own viewBox edge is inside the svg's bounds and still looks clipped. Picture for that half.
- *A control's words never wrap*, under the real face: held for the test text (one line height), and for the real face only as far as point 2 above argues. Where a real line breaks is the pictures'.
- *Its words shorten, or the least needed go behind a menu*: the test holds that the row stays on one line; which of the two a view chose is review.

Every other seed gets a test below.

## The matrix, per view (builder's call: widths from each region's own minimum)

| View | Narrowest | Wide | Words | Zoom | Count |
|---|---|---|---|---|---|
| Title bar | window at `WINDOW_MIN.width` | 1920 (the test window) | workspace name short / 200 / 200 unbroken | none: the title bar is not zoomed | — |
| Rail project row | rail at `RAIL_MIN` | rail at `RAIL_MAX` | project label short / 200 / 200 unbroken | none: the rail is not zoomed | — |
| Pinned permission card's footer | window at `WINDOW_MIN.width`, rail hidden, so the pane is at its least | 1920 | the answers as the app words them; a longer wording constructed for the test (two options of one kind, so each answer shows the agent's name) | 100%, 150% (the pane's zoom) | — |
| Project page's conversation list (in the scrolling box) | as the card | 1920 | conversation title short / 200 / 200 unbroken | 100%, 150% | none, one, `HOME_ROWS + 1` |
| Board columns | as the card | 1920 | task title short / 200 unbroken | 100%, 150% | steps that fit; steps past the width |
| `page_head` (crates/ui) | a box 20 rems wide | 120 rems | title short / 200 / 200 unbroken | rem 16 / 24 | actions none, one, three |

Words: `SHORT = "Notes"`, `LONG` = "word " repeated to 200 chars, `UNBROKEN` = `"x".repeat(200)`. Zoom 150% is set through the pane's own zoom (`ChatPane::zoom_mut`, stepped with `zoom_in` five times, which lands on 1.5 by its snapping), so it is the zoom the person reaches.

## Review Focus

1. A selector that a view no longer draws makes `bounds` panic with the selector's name, never a silent `None` that passes; every assertion reads through it.
2. Repeated rows (rail, history, columns) need one selector each: views mark them `format!("…-{i}")` and tests leak the string to `&'static str` (`strain::selector`); a test that reads row 0 only would miss the last row's inset.
3. A redraw is needed after a model change: every helper that changes state ends in `run_until_parked`; a test that reads before it would compare two frames' worth of nothing.
4. Release builds: `debug_selector` on a view must compile in `cargo build --release` (the no-op variant) and cost nothing; check with clippy's normal build in check.sh, and keep the closures to a `format!`.
5. Zoom applies to the pane body only: a test at 150% that reads a part outside `zoom.scale` (the pane's header) would wrongly expect it scaled; tests compare parts inside the zoomed body with each other, not with the chrome.

---

### Task 0: The plan in the repository

**Files:** Create `docs/superpowers/plans/2026-11-26-ui-rules-step-2.md` (the approved revision, copied unchanged).

- [ ] Copy, commit `docs(ui): the plan of the strain tests`.

### Task 1: The helper beside the rig

**Files:**
- Modify: `crates/app/src/shell/rig.rs` (becomes `rig.rs` + `rig/strain.rs`)
- Create: `crates/app/src/shell/rig/strain.rs`

**Interfaces (Produces):**
- `rig::shell_on(cx: &mut TestAppContext, store: PathBuf, agents: Vec<AgentSpec>, ws: Workspace) -> (AnyWindowHandle, Entity<Shell>, &mut VisualTestContext)`; `rig::shell` becomes a call to it with `Workspace::seeded("/tmp")` and its storage, unchanged for its callers. `shell_on` sets `ws.storage_dir` as `shell` does today.
- `strain::SHORT: &str`, `strain::LONG: &str` (a `LazyLock<String>` or fn `long() -> String`, builder's choice), `strain::unbroken() -> String`.
- `strain::width(cx: &mut VisualTestContext, w: Pixels)`: resize the window to `w` × 800 and park.
- `strain::selector(name: String) -> &'static str` (leaks; tests only).
- `strain::bounds(cx: &mut VisualTestContext, sel: &'static str) -> Bounds<Pixels>`: panics `"nothing drawn as {sel}"` when absent.
- `strain::assert_inside(part: Bounds<Pixels>, within: Bounds<Pixels>, what: &str)`, `strain::assert_one_height(parts: &[Bounds<Pixels>], what: &str)`, `strain::assert_one_line(parts: &[Bounds<Pixels>], what: &str)` (same top and same height), with a tolerance of `px(0.5)` for sub-pixel rounding.

- [ ] **Step 1: Test the helper on a view of its own** in `rig/strain.rs`'s test module: `the_bounds_of_a_marked_part_follow_the_window` (a `flex_1` div marked `"strain-probe"` in `add_window_view`; at `width(cx, px(700.))` its width is 700, at 1200 it is 1200) and `an_unmarked_part_is_said_by_name` (`#[should_panic(expected = "nothing drawn as strain-missing")]`).
- [ ] **Step 2: Run** `cargo test -p onehand --lib --locked strain` — fails (no module), then implement, then passes.
- [ ] **Step 3: Run** `cargo test -p onehand --lib --locked shell::` — every existing rig caller still passes.
- [ ] **Step 4: Commit** `test(ui): a rig for drawing a view under strain`.

### Task 2: The title bar

**Files:** Modify `crates/app/src/shell/frame.rs` (marks: `title-start`, `title-rail`, `title-workspace` on the workspace control's name, `title-palette` on the palette field's box, `title-toggles`); Create `crates/app/src/shell/frame/tests.rs` (the two existing tests in `frame.rs` move there first).

Tests, each at `WINDOW_MIN.width` and 1920, the workspace named by `shell_on`:
- [ ] `a_long_workspace_name_leaves_the_toggles_in_place`: for `LONG` and `unbroken()`, `title-toggles` and `title-rail` bounds equal those with `SHORT`.
- [ ] `the_palette_stays_at_the_centre_whatever_the_name`: `title-palette`'s centre x equals the window's centre for `SHORT` and `LONG` at 1920; at `WINDOW_MIN.width` it stays inside the bar and does not overlap `title-start` or `title-toggles`.
- [ ] `a_long_workspace_name_stays_inside_its_slot`: `title-workspace` inside `title-start`, `title-start` ends before `title-palette` starts, every part one line (height equal to `SHORT`'s).
- [ ] `the_title_bar_controls_share_one_height`: `title-rail`, the three toggles (marked on their buttons) one height; each toggle's icon (marked on the `Icon`'s svg if it takes a selector, else the button's only child) inside its button.
- [ ] Run `cargo test -p onehand --lib --locked frame::`; a failure is fixed in `frame.rs`/`rail/search.rs` with the smallest change and appended to ask.md. Commit `test(ui): the title bar under a long name and a narrow window`.

### Task 3: A rail row

**Files:** Modify `crates/app/src/rail/project.rs` (marks `rail-project-{i}`, `rail-project-name-{i}`, `rail-project-tail-{i}` on the row, its name and its trailing slot); Test `crates/app/src/rail/tests.rs`.

The workspace given to `shell_on` holds two roots whose `label` is set to the words; the rail width is set through the workspace's `layout` (`PanelLayout`'s rail width) at `RAIL_MIN` and `RAIL_MAX`.
- [ ] `a_long_project_name_truncates_and_its_tail_keeps_its_width`: for `LONG`/`unbroken()`, the tail's width equals its width with `SHORT`, the name ends at or before the tail starts, the row's height is `ROW_H` (the row's own constant) and equals the short row's.
- [ ] `a_project_row_stays_inside_the_rail`: the row inside the rail's bounds (mark `rail` on the rail's list box) at both widths.
- [ ] Run `cargo test -p onehand --lib --locked rail::`; fix and list as in Task 2. Commit `test(ui): a rail row under a long name`.

### Task 4: The pinned permission card's footer

**Files:** Modify `crates/app/src/chat/transcript/permission.rs` (marks `perm-foot` on `card_foot()`'s div, `perm-answer-{i}` on each answer) and `crates/app/src/chat/transcript/ask.rs` (marks `ask-foot`, `ask-answer-{i}`); Modify `crates/app/src/shell/rig.rs` (gains `canned`) and `crates/app/src/control/workers/tests.rs` (its private `canned` removed, its callers use the rig's); Test `crates/app/src/chat/transcript/tests.rs` (or a new `transcript/strain_tests.rs` mounted beside it, if `tests.rs` would pass 800 lines).

**Interfaces (Produces):** `rig::canned(cx: &mut VisualTestContext) -> futures::channel::mpsc::Sender<AcpEvent>`, the body of today's `control/workers/tests.rs:29` moved unchanged: the next session opened plays what is sent on it.

The view's promise, from `card_foot`'s own comment: its answers wrap onto a second line before a label is cut, since a cut grant is one nobody can read the reach of. So the tests hold that promise, and the spec's seed *a row of actions never wraps* is not asserted on this view (the lead takes it to the owner and the rules step). Nothing in `card_foot` or the answers changes in this task.

Built on the rig with `rig::canned`: the session is opened and shown, the canned stream sends a permission request with three options (allow once, allow always, reject once), and for the longer wording a request with two options of one kind, whose answers then show the agent's names.
- [ ] `a_cards_answers_as_worded_stand_on_one_line`: the answers as the app words them, at the pane's least width and at 1920, at 100%: every `perm-answer-*` shares one top and one height (`assert_one_line`), and each is inside `perm-foot`. If this fails at the pane's least width, it is a finding for ask.md and nothing is fixed.
- [ ] `answers_that_do_not_fit_wrap_whole`: at 150%, and with the longer wording, at the pane's least width: each answer is one line tall (its height equals its height at 1920), as wide as at 1920 (not cut), and inside `perm-foot`.
- [ ] The same two for the question card's foot (`ask.rs`, same `card_foot`): `a_question_cards_answers_as_worded_stand_on_one_line`, `a_question_cards_answers_that_do_not_fit_wrap_whole`, its question sent through the canned stream as its own tests do.
- [ ] Run `cargo test -p onehand --lib --locked transcript::` and `control::workers`. Commit `test(ui): a parked card's answers under a narrow pane`.

### Task 5: The project page's list in its scrolling box

**Files:** Modify `crates/app/src/chat/pane/project_page.rs` (marks `project-page` on the scrolling box, `home-row-{i}` on each history row, `home-more-{i}` on its trailing slot, `home-hidden` on the "N more" line); Test `crates/app/src/chat/pane/tests.rs` (or `project_page/tests.rs`).

History is written as `projects/tests.rs`'s `kept_conversation` writes it (move that helper into `rig.rs` as `rig::kept_conversation` so both use it), then the page is shown with `show_project_page`.
- [ ] `a_project_page_with_no_conversations_draws_no_rows`: count none, no `home-row-0`.
- [ ] `past_the_cap_the_page_says_how_many_more`: `HOME_ROWS + 1` conversations draw `HOME_ROWS` rows and `home-hidden`.
- [ ] `every_history_row_keeps_its_height_and_its_trailing_slot`: one, and `HOME_ROWS + 1`, titled `LONG`/`unbroken()`, at the pane's least and 1920, 100% and 150%: all rows one height (equal to a `SHORT` row's), every `home-more-{i}` the same width as with `SHORT`, each row inside `project-page`.
- [ ] `the_last_row_stands_off_the_end_of_its_box`: the last row's bottom stands above the content's end by the box's own bottom inset (read as the difference between `project-page`'s content end and the last row, compared with the inset the first row stands off the top), not flush.
- [ ] `rows_in_a_scrolling_box_keep_a_lane_beside_its_scrollbar`: with the page taller than the window (`HOME_ROWS + 1` long rows at a short window height), each row's right edge stands left of `project-page`'s right edge by at least the scrollbar's width as `onehand_ui` draws it (its constant, not a number).
- [ ] Run, fix, list. Commit `test(ui): a project page's conversations under strain`.

### Task 6: The board's columns

**Files:** Modify `crates/app/src/board_page.rs` / `board_page/column.rs` (marks `board-columns` on the sideways scroller, `board-column-{i}` on each column, `board-row-{c}-{r}` on rows); expose the sideways `ScrollHandle`'s `max_offset` to tests through a `#[cfg(test)]` accessor on `BoardPage`; Test `crates/app/src/board_page/tests.rs` (new).

A project with a board (set up as `projects/tests.rs` and `shell/board_settings` tests do) and a workflow with more steps than fit the pane's least width, shown with `show_board`.
- [ ] `columns_past_the_width_scroll_sideways_and_keep_their_width`: at the pane's least, every `board-column-{i}` is `COLUMN_W` wide (at the rem size in force), and the scroller's `max_offset().x > 0`; at 1920 with the steps that fit, `max_offset().x == 0`.
- [ ] `a_long_task_title_stays_inside_its_column`: a row titled `unbroken()` stays inside its column, at 100% and 150%.
- [ ] Run, fix, list. Commit `test(ui): the board's columns under a narrow pane`.

### Task 7: `page_head` in crates/ui

**Files:** Modify `crates/ui/Cargo.toml` (dev-dependency `gpui = { workspace = true, features = ["test-support"] }`, with a comment saying why, as the other crates do; no change to Cargo.lock expected, check with `--locked`); Modify `crates/ui/src/page.rs` (marks `page-head`, `page-head-title`, `page-head-actions`); Create `crates/ui/src/page/tests.rs`.

The test view draws `page_head(title, line, actions)` in a box of the given width inside `window.with_rem_size(Some(rem), …)`, in a `TestAppContext` with `gpui_component::init`. The bounds helpers are three local functions (`bounds`, `assert_inside`, `assert_one_height`), not a shared crate: the app's rig is unreachable from here and three helpers do not earn one.
- [ ] `a_long_page_title_leaves_its_actions_in_place`: for `LONG`/`unbroken()` at 20 and 120 rems, rem 16 and 24: `page-head-actions` has the same bounds as with `SHORT`, the title one line and inside the head, ending before the actions start.
- [ ] `a_page_heads_actions_share_one_height_and_one_line`: with one and three `.short()` actions, one top, one height.
- [ ] `a_page_head_with_no_actions_gives_the_title_the_row`: none, the title's box spans to the head's end.
- [ ] Run `cargo test -p onehand-ui --locked page`; fix and list. Commit `test(ui): a page's head under a long title`.

### Task 8: The whole check, the list of fixes, the push

- [ ] `make fmt`, then check.sh on the committed SHA; record `check:` in status.md.
- [ ] ask.md holds one entry per view fix (file, the change, the test that found it); status.md's build entry names them.
- [ ] `git push -u origin test/ui-strain-tests`. No pull request until a decision says so; its body (when asked) lists every fix and the seeds left to pictures, above.

## Builder's calls

- `NoopTextSystem` for the whole matrix; the real text system recorded, not used (probe, point 3).
- The widths: narrowest window `WINDOW_MIN.width`; rail `RAIL_MIN`/`RAIL_MAX`; the pane at its least by hiding the rail at the narrowest window; `page_head` at 20 and 120 rems.
- No zoom axis for the title bar and the rail: neither is zoomed in the app.
- Selectors named by the part (`title-toggles`, `home-row-3`), never by a rule; repeated parts by index.
- The palette popup seed (*a popup never covers the field it serves*) is held in Task 2 only if the palette's list can be opened in the rig by its action and is drawn into the frame the map reads; if either fails, it is left to pictures and said in the PR. Builder tries it as `the_palettes_list_opens_below_its_field`.
- Fixes are the smallest change found; any that would change a value, add a size constant or reach beyond the view stops as a question in ask.md first.
