run: 2026-11-26-ui

# UI rules step 1: lints, guards and the census. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The UI rules a machine can hold (where a colour, a size, a button and the pointer may be written) become clippy bans and source guards, and the shared sizes, local sizes and palette are locked by a census test, with nothing a person sees changing.

**Architecture:** `clippy.toml` at the workspace root bans the calls; every lawful or existing site carries an item- or statement-level `#[expect(clippy::disallowed_methods, reason = "…")]`. The new source scans live in a child module of the guards, `crates/app/src/guards/visual.rs`, which adds the test-skipping helper the existing guards lack. The census compares gathered lines with `crates/app/src/guards/sizes.txt` and prints what to add and drop.

**Tech Stack:** Rust 1.99.1, clippy 0.1.99, gpui (package `gpui-pre` =0.3.9, lib name `gpui`), gpui-component =0.8.0.

**Spec:** `docs/superpowers/specs/2026-11-26-ui-rules-design.md` (copied unchanged from the run's `spec.md`), sections *Lints*, *Guards* and step 1.

## Global Constraints

- Nothing a person sees changes: no value, size, colour or layout. A site that cannot meet a lint without a visible change takes an `expect` and is listed in the PR.
- No rule id, spec or document name in code, comments, test names or lint reasons.
- Inside `crates/ui` an `expect` sits on the item (or a statement), never on a module.
- An existing `cursor_pointer` site outside `crates/ui` takes an `expect` with its reason; it is not moved onto a builder.
- No `debug_selector`, no layout fix (job b); no change to the `ui` or `drive` skills or the verify rule (job c); no rule text in `DESIGN.md` or `docs/ui-patterns.md`.
- `make fmt`, never bare `cargo fmt`. Never `cargo check`/`cargo run`; `cargo clippy`, `cargo test -p <crate> <filter>` only, one at a time.
- Commits: `type(scope): summary`, plain body, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **An `expect` wider than its call.** One `expect` on a large `fn render` also licenses a `cursor_pointer` added to that function later. Builder places each on the narrowest stable place: a `let` statement holding the chain if there is one, else the enclosing `fn`. Task 1 lists each site's place.
2. **clippy's cache does not see `clippy.toml`.** Probe: after writing `clippy.toml`, `make clippy CLIPPY_EXTRA="-- -D warnings"` finished in 10 s and passed with no warning, because nothing was re-linted; only touching a source made the bans fire. CI starts clean, so it is not fooled; a local run is. Task 1 touches the crate roots before its check, and *Guard coverage* says so.
3. **The test-skipping helper and brace depth.** It counts braces to find the end of a `#[cfg(test)]` item; a `'{'` char or an unbalanced brace in a string would end it early or late. Its unit test (Task 2) carries a format string with braces; the unbalanced literal is a written limit.
4. **A `#` string that is not a colour.** `"#100"` and `"#409"` (issue references) exist in core today. The colour guard reads six or eight hex digits only, so a reference is never taken for a colour; the three-digit colour it misses is a written limit. Its sample test (Task 2) holds both.
5. **The census on a multi-line or computed constant, and a name in two files.** A value is written as the source has it, joined onto one line with single spaces; the path keeps two `ROW_H`s apart. Task 3's sample test holds both.

---

### Task 0: Spec and plan into the repository

**Files:**
- Create: `docs/superpowers/specs/2026-11-26-ui-rules-design.md` (the run's `spec.md`, byte for byte)
- Create: `docs/superpowers/plans/2026-11-26-ui-rules-step-1.md` (the approved revision of this plan, byte for byte)

- [ ] **Step 1:** copy both; `cmp` each against its source. Expected: no output.
- [ ] **Step 2:** commit `docs(ui): the spec and plan for the UI rules' lints and census`.

### Task 1: clippy.toml and the expects

**Files:**
- Create: `clippy.toml`
- Modify: `crates/ui/src/toggle.rs`, `crates/ui/src/lib.rs` (export), `crates/ui/src/button.rs`, `crates/ui/src/menu.rs`, `crates/ui/src/row.rs`, `crates/ui/src/tabs.rs`, `crates/ui/src/tokens.rs`
- Modify: `crates/app/src/theme.rs`, `crates/terminal-ui/src/tests.rs`
- Modify (cursor_pointer expects): `crates/app/src/{board_page.rs, rail.rs, rail/pages.rs, rail/workspace.rs, rail/project.rs, rail/session.rs, schedules_page.rs, schedules_page/form.rs, settings.rs, settings/pages.rs, settings/telegram.rs, tasks_page.rs, workspace_page.rs, chat/transcript/parts.rs, chat/transcript/parts/activity.rs, chat/transcript/strip.rs, chat/transcript/subagent.rs, chat/transcript/user.rs, workflows_page/step.rs, workflows_page/grid.rs}`, `crates/claude-plugins/src/view/rows.rs`, `crates/issues/src/view/detail.rs`

**Interfaces:**
- Produces: `onehand_ui::Locked` — `pub trait Locked: Sized { fn locked(self, locked: bool) -> Self; }`, implemented for `gpui_component::switch::Switch` in `toggle.rs` by calling `Disableable::disabled`, with the one `expect` for switches. Its doc comment says, in its own words, that it is the switch a person cannot flip now, drawn as the library draws a disabled one. Re-exported beside `toggle` in `lib.rs`.

`clippy.toml` holds `disallowed-methods`, each entry `{ path, reason }`:

| path | reason (as clippy prints it) |
|---|---|
| `gpui::rgb`, `gpui::rgba`, `gpui::hsla`, `gpui::black`, `gpui::white`, `gpui::red`, `gpui::green`, `gpui::blue`, `gpui::yellow`, `gpui::opaque_grey` | a colour is written in the app's palette and read from the theme |
| `gpui_component::button::Button::new` | a button is made with `onehand_ui::action`, which gives it the pointer and the corner |
| `gpui::Styled::cursor_pointer` | the pointer is given by the shared builders, so it means a press will act |
| `gpui_component::Disableable::disabled` | a button refuses with its reason (`Refuses::refuses`); a switch is locked with `Locked::locked` |

`gpui::transparent_black` and `gpui::transparent_white` stay free: they draw nothing (builder's call, the spec leaves it open).

Expects (reason in the item's own words, no document named):

| Site | Place of the `expect` |
|---|---|
| `crates/ui/src/button.rs` `action` (Button::new, cursor_pointer) | the fn |
| `crates/ui/src/button.rs` `impl Refuses for Button` `refuses` | the fn |
| `crates/ui/src/toggle.rs` `impl Locked for Switch` `locked` | the fn |
| `crates/ui/src/menu.rs:52`, `row.rs:74`, `tabs.rs:104` | the enclosing fn |
| `crates/ui/src/tokens.rs:188` `lift` (`gpui::black`) | the fn |
| `crates/app/src/theme.rs:244` `composer_shades`, `:373` (a test) | the fn |
| `crates/terminal-ui/src/tests.rs:34` (a test needs some ink) | the test fn |
| the 27 `cursor_pointer` calls outside `crates/ui` (probe list below) | a `let` statement holding the chain if there is one, else the enclosing fn; one `expect` per item however many calls it holds |

The three `.disabled(` calls on a switch (`workflows_page/step.rs:408`, `:435`, `claude-plugins/src/view/rows.rs:58`) become `.locked(…)`, and the `Disableable as _` imports there go. Behaviour is identical: `locked` is `disabled`.

- [ ] **Step 1:** write `clippy.toml`; touch `crates/*/src/lib.rs crates/app/src/main.rs`; run `make clippy CLIPPY_EXTRA="-- -D warnings"`. Expected: FAIL, 40 `disallowed method` errors at the probe's sites (the run stops at the first failing crate, so fewer may print).
- [ ] **Step 2:** add `Locked` and the expects; replace the three switch `.disabled(`.
- [ ] **Step 3:** `make fmt`; touch the crate roots; `make clippy CLIPPY_EXTRA="-- -D warnings"`. Expected: PASS, no `unfulfilled_lint_expectations`.
- [ ] **Step 4:** `cargo test -p onehand-ui --locked` and `cargo test -p onehand-claude-plugins --locked`. Expected: PASS.
- [ ] **Step 5:** commit `chore(ui): clippy bans where a colour, a button and the pointer are made`.

### Task 2: The test-skipping helper, the colour guard, the size guard

**Files:**
- Modify: `crates/app/src/guards.rs` (declare `#[cfg(test)] mod visual;`, nothing else)
- Create: `crates/app/src/guards/visual.rs`
- Modify (sizes into constants, values unchanged): `crates/app/src/board_page/column.rs:111` → `const SCROLLBAR_PULL: Rems = rems(-0.75);`; `crates/app/src/chat/pane/page_draft.rs:462` → `const BRANCH_FIELD_W: Rems = rems(16.);`; `crates/ui/src/choice.rs:142-143` → `const LIFT_DROP: Pixels = px(0.5);` and `const LIFT_BLUR: Pixels = px(1.);`; `crates/ui/src/stepper.rs:89` → `const RAIL_W: Pixels = px(1.);`. Each with a one-line comment saying what it is, kept from the comment already beside the call.

**Interfaces:**
- Consumes: `super::ui_sources() -> Vec<(String, String)>` (path, text).
- Produces (private to `visual`, used by Task 3):
  - `fn test_only_files(sources: &[(String, String)]) -> Vec<String>` — for every `mod NAME;` under `#[cfg(test)]` (with or without a visibility, other attributes between allowed) in `sources`, the two path prefixes `<dir>/NAME.rs` and `<dir>/NAME/`, where `<dir>` is the declaring file's own directory for a `lib.rs`, `main.rs` or `mod.rs`, else that file's path without `.rs`. Today in the UI sources: `crates/app/src/guards` (from `lib.rs`) and `crates/app/src/shell/rig` (from `shell.rs:50-51`).
  - `fn without_tests(path: &str, source: &str, test_only: &[String]) -> Option<Vec<(usize, String)>>` — `None` for a file named `tests.rs`, under a `tests/` directory, or whose path starts with one of `test_only`'s prefixes; else every line with its 1-based number, minus the lines of each item under `#[cfg(test)]` (a `mod … { }`, a `fn`, a `mod …;` line), found by brace depth from the item's first line.

Tests in `visual.rs`:

- `test_code_is_left_out_of_a_scan`: on a sample holding a plain fn, a `#[cfg(test)] mod tests { fn t() { let s = format!("{x}"); } }` and a `#[cfg(test)] fn helper() {}` followed by a plain fn, the kept lines are both plain fns' lines with their original numbers; `without_tests("x/tests.rs", …)` and `without_tests("x/tests/a.rs", …)` are `None`. On a sample pair, `x/src/shell.rs` declaring `#[cfg(test)]\npub(crate) mod rig;` and `x/src/lib.rs` declaring `#[cfg(test)]\nmod guards;`, `test_only_files` gives `x/src/shell/rig.rs`, `x/src/shell/rig/`, `x/src/guards.rs`, `x/src/guards/`, and `without_tests` is `None` for `x/src/shell/rig.rs` and `x/src/shell/rig/strain.rs`, `Some` for `x/src/shell/roots.rs`.
- `a_colour_is_written_in_the_palette_alone`: over `ui_sources()` minus `crates/app/src/theme.rs` and `crates/terminal-ui/src/ansi.rs`, through `without_tests` with `test_only_files(&ui_sources())`, skipping `//` lines, no line holds a string `"#` + 6 or 8 hex digits + `"`, or a struct literal `Rgba {`/`Hsla {` (the type name not preceded by `->`, `impl`, `for` or `struct`). Floor: the scan reads more than 50 hex strings in `theme.rs` (proves the pattern still matches). Failure names `path:line`. A sample check in the same test: `"#18181800"` matches, `"#100"` and `-> Hsla {` do not.
- `a_size_is_written_once_in_a_constant`: same sources through `without_tests`, skipping `//` lines; a `px(` or `rems(` whose whole argument is a number literal other than zero (`0.`, `0.0`, `0` are free) stands only inside a `const NAME:` item, read from its line to the `;` that ends it. Floor: more than 150 constant sizes seen. Failure names `path:line` and says to give the size a name in a `const`.

- [ ] **Step 1:** write the three tests; `cargo test -p onehand --locked --lib guards::visual`. Expected: the helper test PASS; the size guard FAIL naming exactly `column.rs:111`, `page_draft.rs:462`, `choice.rs:142`, `choice.rs:143`, `stepper.rs:89`; the colour guard PASS (0 offenders today).
- [ ] **Step 2:** move the five literals into the four places' constants.
- [ ] **Step 3:** `make fmt`; the same test command. Expected: PASS. `git diff` shows only the literals renamed, the values as they were.
- [ ] **Step 4:** commit `test(ui): guards that a colour and a size are written in one place`.

### Task 3: The size census

**Files:**
- Modify: `crates/app/src/guards/visual.rs`
- Create: `crates/app/src/guards/sizes.txt` (read with `include_str!`)

**Interfaces:**
- Consumes: `test_only_files` and `without_tests` (Task 2), `super::ui_sources()`.
- Produces: `fn census() -> Vec<String>`, sorted, one line per entry: `<workspace-relative path>  <NAME> = <value as written>` (two spaces after the path).

What it gathers (builder's calls on what the spec leaves open):
- every `const NAME: Rems|Pixels` (`gpui::` prefix allowed) in `ui_sources()` outside test code: 194 after Task 2 (189 today: 42 in `crates/ui`, 147 local, plus Task 2's five);
- every `pub const` in `crates/ui/src/tokens.rs` whatever its type (adds `LEADING` and `LEADING_TIGHT`, the two `f32`s);
- every palette slot in `crates/app/src/theme.rs`'s `palette` macro, `slot = "#dark", "#light"` (59 today), in the same file: one list, one `+n −m` in a PR.
- A value is the text between `=` and `;`, whitespace runs joined to one space, so a computed or multi-line constant is written as the source has it. A name repeated in two files is two lines, told apart by the path.

Tests:
- `the_census_reads_a_constant_as_written`: on a sample, `const A: Rems = rems(1.);`, a two-line `const B: Pixels =\n    px(2.);` and `const C: Rems = rems(D.0 * 2.);` give `A = rems(1.)`, `B = px(2.)`, `C = rems(D.0 * 2.)`.
- `the_sizes_are_the_listed_ones`: `census()` equals the non-empty lines of `sizes.txt`. On a difference it panics with the lines to add, then the lines to drop, each prefixed `+ ` / `- `, and one sentence: a shared size or a palette value changes only when the owner asked for it; bring the list up to date by hand from these lines. Floor: more than 200 lines.

- [ ] **Step 1:** write both tests and an empty `sizes.txt`; `cargo test -p onehand --locked --lib guards::visual::the_`. Expected: the sample test PASS; the census FAIL printing 255 `+ ` lines and no `- ` line.
- [ ] **Step 2:** write `sizes.txt` from the failure's lines, by hand (the test writes nothing).
- [ ] **Step 3:** same command. Expected: PASS.
- [ ] **Step 4:** commit `test(ui): a census of the sizes and the palette`.

### Task 4: The documents

**Files:**
- Modify: `docs/rules-and-gotchas.md` (*Guard coverage* only)
- Modify: `AGENTS.md` (two sentences)
- Modify: `.claude/skills/ui/catalog.md` (the `toggle` row and the `Switch (always through toggle)` line)
- Modify: `.claude/skills/ui/checklist.md` (item 7's clause on `.disabled(`, one hunk; job c owns the rest of the skill)

- [ ] **Step 1:** *Guard coverage* gains one row per check, each with its limit:
  - colour calls (lint): `gpui`'s colour constructors and named colours outside the palette and the lift | `transparent_black`/`transparent_white` are free; a colour made by a library helper the list does not name.
  - `Button::new` (lint): outside the wrapper | a button made by another library constructor.
  - `cursor_pointer` (lint): outside the shared builders | an `expect` on a function also licenses a later call in that function; the existing hand-made rows carry one each.
  - a button refuses (lint on `Disableable::disabled`): every `Disableable` type, so a switch goes through `Locked::locked` | a menu item's and a field's `disabled` are their own and are not seen.
  - colour literal (guard): hex strings of six or eight digits and `Rgba {`/`Hsla {` literals outside `theme.rs` and `ansi.rs` | a three-digit hex, a colour built from numbers some other way.
  - size literal (guard): a number in `px(`/`rems(` outside a `const` | a size computed from numbers, a bare `f32`, the spacing and type steps.
  - size census (guard): every `Rems`/`Pixels` constant, every `tokens` constant, every palette slot | a size typed `f32` outside `tokens`.
  - The last row loses "no source guard scans the visual rules": colour, size, button and pointer move to the rows above; corner, icons, motion and the rest stay by review.
  - One sentence under the table: clippy keeps its last result and does not notice a changed `clippy.toml`; touch a source to re-lint locally (CI starts clean).
  - The test-skipping helper's limit: an unbalanced brace in a string or char inside a `#[cfg(test)]` item; a test-only module declared some other way than a plain `#[cfg(test)]` on `mod NAME;` (through a `#[path]`, or a cfg expression that is not plain `test`).
- [ ] **Step 2:** AGENTS.md: in *Rules*, after "Never hard-code a colour, radius or size…", the sentence: "A shared size or a palette value changes only when the owner asked for it, and a new local size is said in the PR; the size census among the guards fails until its list agrees." In *Invariants*, "No guard scans the UI's visual rules: they are reviewed by hand." becomes "`clippy.toml` and the guards hold where a colour, a size, a button and the pointer are written; the rest of the UI's visual rules are reviewed by hand." No other line reflowed.
- [ ] **Step 2a:** `.claude/skills/ui/catalog.md`: `Locked` is listed where `toggle` is: the row *A setting turned on or off* names `.locked(bool)` for a switch a person cannot flip now, and the line "`Switch` (always through `toggle`)" adds "and locked with `.locked(…)`". `.claude/skills/ui/checklist.md` item 7: the clause "which only a menu item or a switch takes" becomes "which only a menu item takes; a switch takes `.locked(`", and nothing else in the file changes.
- [ ] **Step 3:** `guards::tests::code_never_cites_a_document` and `documents_are_written_in_english` still pass: `cargo test -p onehand --locked --lib guards`. Expected: PASS.
- [ ] **Step 4:** commit `docs(ui): the new lints and guards, with their limits` (the skill's two hunks in the same commit).

### Task 5: The whole check

- [ ] **Step 1:** touch the crate roots; run `/home/jarviisha/development/rust-projects/onehand-ui-rules-lead/check.sh` on the committed SHA. Expected: exit 0. Record `check: check.sh pass sha=<sha>`.
- [ ] **Step 2:** `cargo tree -p onehand-core -i gpui-pre` (and `-p onehand-io`, `-p onehand-host`) still errors "did not match any packages".
- [ ] **Step 3:** `git push -u origin chore/ui-lints-census`. No PR until a decision says so.

## Builder's calls

- Named colours banned: `black`, `white`, `red`, `green`, `blue`, `yellow`, `opaque_grey`, with `rgb`, `rgba`, `hsla`. `transparent_black` and `transparent_white` stay free (alpha zero; used 8 times as "no edge").
- The button rule is the lint, not a chain scan: today every switch's `disabled` is called on `onehand_ui::toggle`'s result, so a `Locked` trait beside `toggle` makes the shared toggle the one lawful way. The alternative, an `expect` at each of the three switch sites, needs no new name; say if you prefer it.
- The new guards go in `crates/app/src/guards/visual.rs` so `guards.rs` (967 lines) does not grow, and the census list sits beside them as `guards/sizes.txt`, the palette in the same file.
- The census skips test code, test-only modules included; no test file or test-only module holds a size constant today.
- An `expect` outside `crates/ui` sits on the narrowest stable place (a `let` statement, else the fn), with a reason naming what the hand-made part is.

## The probe (2026-11-26, on the base d12ee956)

Run with the routed `cargo` (the run's rule overrides the spec's "the machine's own cargo"), `make clippy` as CI runs it, against a scratch `clippy.toml` holding the table of Task 1. The scratch file was removed afterwards; the worktree is clean.

1. **The bans fire.** A full re-lint (`make clippy`, sources touched) printed `use of a disallowed method` at 40 distinct sites: `gpui::Styled::cursor_pointer` 31 (27 outside `crates/ui`, 4 inside), `gpui_component::button::Button::new` 1, `gpui_component::Disableable::disabled` 4 (`button.rs:81`, `step.rs:408`, `step.rs:435`, `rows.rs:58`: a trait method's ban reaches every impl), `gpui::hsla` 2 (`theme.rs:244`, `:373`), `gpui::black` 2 (`tokens.rs:188`, `terminal-ui/src/tests.rs:34`). `rgb`, `rgba`, `white`, `red`, `green`, `blue`, `yellow`, `opaque_grey`: 0. The menu items' and inputs' own `disabled` are not hit.
2. **An `expect` is fulfilled.** Under `-D warnings`, an `expect` on `action` silenced both its `Button::new` and its `cursor_pointer`; one on a `let` statement silenced the `disabled` in it; one on `resting`, which calls nothing banned, failed: `error: this lint expectation is unfulfilled`.
3. **Crates without gpui stay quiet.** `onehand-core`, `onehand-io` and `onehand-host` were checked in the same run with no warning at all, about an unresolved path or otherwise.
4. **The cache.** The first `make clippy CLIPPY_EXTRA="-- -D warnings"` after writing `clippy.toml` passed in 10.59 s with no warning: nothing was re-linted. Touching `crates/ui/src/lib.rs` made the bans fire. (Review Focus 2.)

Today's offenders by my own scan (`ui_sources`, test code left out). Recounted with the test-only modules left out as well (`crates/app/src/guards.rs`, `crates/app/src/shell/rig.rs`; neither holds a size constant, a size literal or a colour): no number moves.

| Check | Offenders |
|---|---|
| lint, colour calls | 4 (above), all lawful or test code: each takes an `expect` |
| lint, `Button::new` | 1, the wrapper: `expect` |
| lint, `cursor_pointer` | 31: 4 builders in `crates/ui`, 27 hand-made sites outside |
| lint, `Disableable::disabled` | 4: the refusal, and 3 switches moved to `Locked` |
| guard, colour literal | 0 (core's `"#100"`/`"#409"` are outside the UI sources and are not six digits) |
| guard, size literal | 4 places, 5 literals: `board_page/column.rs:111`, `chat/pane/page_draft.rs:462`, `ui/src/choice.rs:142-143`, `ui/src/stepper.rs:89` |
| census | 250 lines today (189 sizes, 2 `f32` tokens, 59 palette slots); 255 after Task 2 |

The 27 `cursor_pointer` sites outside `crates/ui`: `app/src/board_page.rs:557`, `chat/transcript/parts/activity.rs:265`, `chat/transcript/parts.rs:309`, `chat/transcript/strip.rs:318`, `chat/transcript/subagent.rs:54`, `chat/transcript/user.rs:471`, `rail/pages.rs:102`, `rail/project.rs:269`, `rail.rs:424`, `rail/session.rs:352`, `rail/workspace.rs:218`, `schedules_page/form.rs:235`, `:466`, `schedules_page.rs:520`, `:546`, `settings/pages.rs:256`, `settings.rs:479`, `settings/telegram.rs:74`, `tasks_page.rs:372`, `workflows_page/grid.rs:254`, `workflows_page/step.rs:144`, `:401`, `:431`, `workspace_page.rs:385`; `claude-plugins/src/view/rows.rs:173`, `:225`; `issues/src/view/detail.rs:314`.

The access check: `check.sh` passed on the base d12ee956 (artifacts/check-base.log); `git commit --allow-empty` and `git reset --soft HEAD~1` worked.
