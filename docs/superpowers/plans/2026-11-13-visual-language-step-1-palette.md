# Visual language, step 1: palette and selection

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Both modes take onehand's own palette, and the chosen item of several reads blue.

**Architecture:** `crate::theme::install` writes the palette into the library's two theme configs,
so library parts read it too. `accent` stays the neutral hover and highlight the library uses it
for. The chosen fill and ink are derived in `onehand_ui::ink` from `primary`, and every chosen
row, rail entry and tab reads them through its shared builder.

**Tech Stack:** Rust, GPUI, gpui-component 0.8.0 (`ThemeConfigColors`, `Hsla::mix_oklab`).

**Spec:** `docs/superpowers/specs/2026-11-13-visual-language-design.md`

## Global Constraints

- A colour value is written only in `crates/app/src/theme.rs`; call sites read `cx.theme()` or a helper.
- Hover stays neutral; blue means the primary action and the chosen item, nothing else.
- Light and dark follow one rule set.
- No ink and surface pair falls under the threshold its test holds today.
- Type sizes, radii, heights, the frame, the terminal's colours do not change in this step.
- Code never cites a document. Use `make fmt`, never bare `cargo fmt`.

## Review Focus

- A chosen row on each surface it lands on (page, rail and dock, floating): its ink reads on the tint in both modes. Pinned in Task 2.
- A hovered row beside the chosen one stays visibly different. Pinned in Task 2.
- A row the keys are on in a popup stays neutral (the composer's rows): by review, Task 2.
- A quieter ink on a chosen row (a second line, an issue's meta) stays readable. Pinned in Task 2.
- A library list or menu mounted in the app (popup menus, the command list): hover neutral, by the owner's eye in `make dev`.

---

### Task 1: The palette

**Files:**
- Modify: `crates/app/src/theme.rs` (`install`, `primary_overrides`, derived steps, tests)

**Interfaces:**
- Produces: the theme slots at the spec's starting values in both modes; `primary` is the blue
  (`#669df1` dark, `#1868db` light) with its paired ink.

- [ ] **Step 1:** Add tests in `theme.rs`'s test module, both modes, from `installed(mode)`:
  `the_regions_stand_a_step_apart` (`background != sidebar`, and dark `background` lightness
  above 0.1, so it is not near black); `the_inks_read_on_every_neutral_surface`
  (`foreground`, `muted_foreground` and `meta_ink` at AA on `background`, `sidebar`, `popover`,
  `muted`; keep whatever lower threshold an existing test already holds for a pair);
  `hover_stays_neutral` (`accent.s`, `list_hover.s`, `sidebar_accent.s` each under 0.1).
- [ ] **Step 2:** Run `cargo test -p onehand --locked theme` and see the first fail.
- [ ] **Step 3:** Replace `primary_overrides` with `fn palette(colors: &mut ThemeConfigColors, dark: bool)`
  setting every slot in the spec's table plus the slots the library derives a surface from
  (`list`, `table`, `accordion`, `group_box`, `tab`, `tab_active`, `sidebar_border`,
  `secondary_hover`/`_active`, `list_active` as the primary at the chosen fill's alpha,
  `list_hover`, `sidebar_accent`, `accent`). Rewrite the doc comments: the palette is the app's own.
- [ ] **Step 4:** Re-derive `waiting_card_of`, `floating_well_of`, `composer_shades` where their
  comments assume a near-black page; keep each test's threshold.
- [ ] **Step 5:** `cargo test -p onehand --locked theme` passes. Commit `feat(theme): give both modes onehand's own palette`.

### Task 2: The chosen role

**Files:**
- Modify: `crates/ui/src/ink.rs`, `crates/ui/src/lib.rs`, `crates/ui/src/row.rs`,
  `crates/ui/src/tabs.rs`, `crates/ui/src/stepper.rs`
- Modify: `crates/app/src/theme.rs` (`rail_chosen` removed, tests), `crates/app/src/rail/row.rs`,
  `crates/app/src/rail/workspace.rs`, `crates/app/src/workflows_page/step.rs`,
  `crates/app/src/dialogs/issue.rs`, `crates/issues/src/view/{list.rs,detail.rs,page/row.rs}`
- Leave: `crates/app/src/chat/composer/rows.rs` (the keys' cursor in a popup stays `accent`)

**Interfaces:**
- Produces, in `onehand_ui`: `pub fn chosen_fill(cx: &App) -> Hsla`, `pub fn chosen_ink(cx: &App) -> Hsla`,
  and for the app's tests `pub fn chosen_fill_of(primary: Hsla, dark: bool) -> Hsla`,
  `pub fn chosen_ink_of(primary: Hsla, foreground: Hsla, dark: bool) -> Hsla`.
  The fill is the primary thinned (it lies on whatever surface the row is on); the ink is the
  primary, pulled toward the foreground only as far as its test needs.

- [ ] **Step 1:** Test in `theme.rs`: `the_chosen_row_reads_and_stands_off_a_hovered_one`, both
  modes: `chosen_ink` over `chosen_fill` composited on `background`, `sidebar` and `popover` at
  AA; `middle_ink(chosen_ink, muted_foreground)` on the same at 3.0 or the figure the existing
  middle-ink test holds; the composited fill differs from `list_hover` / `sidebar_accent` in hue
  (`s > 0.2`).
- [ ] **Step 2:** Run it, see it fail to compile.
- [ ] **Step 3:** Add the four functions; re-export from `lib.rs`.
- [ ] **Step 4:** Move each chosen site to them: `row(picked)`, the tab chip, the stepper's picked
  mark, the rail's `chosen_fill`/`page_fill`/`row_inks`, the workspace row, the workflow step,
  the issue dialog's row, the three issues inks. Read each site first: one that means "pointed
  at" stays on `accent`. Rewrite the doc comments that name `accent` as the selected step.
- [ ] **Step 5:** `cargo test --workspace --locked` passes. Commit `feat(ui): mark the chosen item in blue`.

### Task 3: The contract

**Files:**
- Modify: `DESIGN.md` (intro, Principle 2, *Colour and state*, the tab label under *Type, space and shape*),
  `.claude/skills/ui/tokens.md` (*Colour*, slots, helpers, fills and inks),
  `docs/ui-patterns.md` (where a selected row is said to take `accent`),
  `docs/known-gaps.md` (the contrast pairs), `.claude/skills/ui/{catalog,checklist}.md` if they name `accent` as selection.

- [ ] **Step 1:** Rewrite each rule the spec's *Palette* and *Blue marks where you are* change; grep for `accent`, `Default Dark`, `near black`, `rail_chosen`, `library's own` to find them.
- [ ] **Step 2:** `git diff --check`; answer the UI skill's checklist.
- [ ] **Step 3:** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`. Commit `docs: say the palette is onehand's own and blue marks the chosen item`.
