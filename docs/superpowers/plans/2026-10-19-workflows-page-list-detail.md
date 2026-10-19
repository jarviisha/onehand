# Workflows page: list beside its editor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Workflows page as a list beside its editor: the editor grouped into *About*, *How it runs* and *Steps*, and each step a one-line row that opens one at a time.

**Architecture:** All in `crates/app/src/workflows_page*`; `crates/core` is not touched. Pure helpers (ordering and stepping the list, what a step row says, which problems belong to which step, which step stays open) go in small modules with unit tests. The page entity gains a selection (`Pick`), a split state and a measured width, and draws through `onehand_plugin_host::list_detail`, as the Issues tab and the Markdown mode do.

**Tech Stack:** Rust, GPUI, gpui-component (`h_resizable`, `ResizableState`), `onehand-plugin-host` (`list_detail`, `back_link`, `hint`, `measure_width`, `card_box`, `badge`).

**Spec:** `docs/proposals/workflows-page-list-detail.md`. Read it before each task.

## Global Constraints

- Follow the `ui` skill (`.claude/skills/ui/SKILL.md`) and answer its checklist before each UI task is done.
- No hard-coded colour, radius or size: `cx.theme()`, `crate::tokens`, rems.
- Buttons via `crate::controls::action`, menu rows via `controls::menu_item`/`menu_row`, `.tall()` in the page's form.
- No comment or string names a document.
- No file past about 800 lines of code (tests not counted); tests in `foo/tests.rs` beside `foo.rs`.
- Keys are keymap commands in `crates/app/src/keymap/commands.rs`, bound `WorkflowsPage && !Dialog && !Input`.
- `crates/core` unchanged; `cargo tree -p onehand-core -i gpui` still errors.
- Checks before each commit: `cargo test -p onehand`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.
- Don't launch or screenshot the app: build, test, stop.
- Work on branch `feat/workflows-list-detail`. One PR with docs, squash-merged; commit titles `type(scope): lowercase clause`.

## Review Focus

1. Templates reload (after a save, a delete or an import) while a row is picked: the same workflow stays picked, found by its key, not its place; a deleted one clears the pick. Tested in Task 1 (`repick_*`).
2. Saving a new draft: the row it becomes is picked, so the editor stays open on it rather than jumping to the empty state. Wired in Task 3, by-hand row in Task 6.
3. ↑/↓ while the caret is in a field never steps the list (the `!Input` binding). By-hand row in Task 6.
4. Moving, removing or adding steps keeps the right one open. Tested in Task 2 (`follow_open_*`).
5. On a narrow pane, Esc or the back link from the editor returns to the list with the row still marked. By-hand row in Task 6.

---

### Task 1: The list's order, keys and selection

**Files:**
- Create: `crates/app/src/workflows_page/pick.rs`, `crates/app/src/workflows_page/pick/tests.rs`
- Modify: `crates/app/src/workflows_page.rs` (add `mod pick;`)

**Interfaces:**
- Produces:
  - `pub(super) enum Pick { Own(PathBuf), Shipped(String), New }`: a person's file by path, a shipped template by id, the unsaved new draft. Derives `Clone, PartialEq, Eq, Debug`.
  - `pub(super) fn pick_of(entry: &crate::workflow::Entry) -> Option<Pick>`: `Own(file)` when `file` is `Some` (readable or not), `Shipped(id)` when shipped and readable, else `None`.
  - `pub(super) fn listed(entries: &[Entry]) -> Vec<usize>`: indices into `entries`, the person's own first and the shipped after, each group in its existing order, at most `crate::workflow::TEMPLATES_SHOWN` long.
  - `pub(super) fn repick(entries: &[Entry], pick: &Pick) -> Option<usize>`: the index of the entry `pick` names; `None` for `New` or when it is gone.
  - `pub(super) fn step_from(at: Option<usize>, len: usize, by: isize) -> Option<usize>`: the place `by` rows from `at` in a list of `len`, clamped at both ends; from `None`, down goes to 0 and up to the last; `None` when `len == 0`.

- [ ] **Step 1: Write the failing tests** in `pick/tests.rs`, building `Entry` values from `Template::blank(name)` with ids set:
  - `listed_puts_own_before_shipped`: entries `[shipped a, shipped b, own c, own d]` → `[2, 3, 0, 1]`.
  - `listed_caps`: `TEMPLATES_SHOWN + 3` own entries → length `TEMPLATES_SHOWN`.
  - `pick_of_unreadable_own_is_own`: `Entry { template: Err(..), file: Some(p) }` → `Some(Pick::Own(p))`.
  - `repick_finds_by_key_after_reorder`: pick `Own(c)` found at its new index after the entries are shuffled.
  - `repick_gone_is_none`: `Own(x)` not in entries → `None`; `Pick::New` → `None`.
  - `step_from_*`: `(None,3,1)→Some(0)`, `(None,3,-1)→Some(2)`, `(Some(2),3,1)→Some(2)`, `(Some(0),3,-1)→Some(0)`, `(Some(1),3,1)→Some(2)`, `(None,0,1)→None`.
- [ ] **Step 2:** `cargo test -p onehand workflows_page::pick` — FAIL (module missing).
- [ ] **Step 3:** Implement the five items above in `pick.rs`. The module header says why the pick is a key, not a place: the list is read again after every save.
- [ ] **Step 4:** `cargo test -p onehand workflows_page::pick` — PASS.
- [ ] **Step 5:** Commit `feat(workflows): the page's list orders, steps and repicks by key`.

### Task 2: What a step row says, and which step stays open

**Files:**
- Create: `crates/app/src/workflows_page/draft/tests.rs`
- Modify: `crates/app/src/workflows_page/draft.rs`

**Interfaces:**
- Produces:
  - `pub(super) fn step_facts(at: usize, steps: &[StepSpec]) -> Vec<String>`: muted facts for step `at`. A `Command` or `StatusChecks` whose `on_fail` names an earlier step at `k` → `"on failure ↑ {k+1}"`. An `Agent` with `keep_answer` → `"keeps answer"`. Otherwise nothing.
  - `pub(super) fn split_problems(problems: &[(Option<usize>, String)], steps: usize) -> Vec<Option<String>>`: per step, the first problem's text for that step, `steps` long; a problem for an index ≥ `steps` is dropped.
  - `pub(super) enum StepEdit { Added, Moved { from: usize, to: usize }, Removed(usize) }`
  - `pub(super) fn follow_open(open: Option<usize>, edit: StepEdit, len_after: usize) -> Option<usize>`: `Added` → `Some(len_after - 1)`. `Moved` → follows the open step, or the step it swapped with. `Removed(at)` → `None` if it was the open one, one less if it was after `at`, unchanged if before.
  - On `WorkflowDraft`: a new field `pub open_step: Option<usize>`, `None` from `load`; a new field `pub shipped: bool`, set from a new `load` parameter; `pub fn problem_list(&self, cx: &App) -> Vec<(Option<usize>, String)>` (`core::validate`'s `step` and `said`; a `to_template` error is `(None, why)`). `problems()` stays, built from `problem_list` with `"Step {n}: "` prefixed as core's `Display` does. `add_step`, `move_step` and a new `pub fn remove_step(&mut self, at: usize)` update `open_step` through `follow_open`; `toggle_step(&mut self, at: usize)` opens `at`, or closes it if it is open.
- Consumes: `onehand_core::workflow::{StepSpec, StepKind}`.

- [ ] **Step 1: Write the failing tests** in `draft/tests.rs`:
  - `facts_command_points_back`: steps `[agent "plan", command on_fail "plan"]`, `step_facts(1, ..) == ["on failure ↑ 1"]`.
  - `facts_unknown_target_says_nothing`: `on_fail "nope"` → `[]`.
  - `facts_agent_keeps_answer`: → `["keeps answer"]`.
  - `split_problems_first_per_step`: `[(Some(1),"a"),(Some(1),"b"),(None,"c"),(Some(9),"d")]`, 3 steps → `[None, Some("a"), None]`.
  - `follow_open_added`, `follow_open_moved_open_step`, `follow_open_moved_neighbour`, `follow_open_removed_open`, `follow_open_removed_before`, `follow_open_removed_after`: with the values the signature implies.
- [ ] **Step 2:** `cargo test -p onehand workflows_page::draft` — FAIL.
- [ ] **Step 3:** Implement. Add `#[cfg(test)] mod tests;` to `draft.rs`. Update the `WorkflowDraft::load` callers in `actions.rs` to pass `shipped: false` for now.
- [ ] **Step 4:** `cargo test -p onehand workflows_page` — PASS.
- [ ] **Step 5:** Commit `feat(workflows): a step row knows its facts and its problem, and one step stays open`.

### Task 3: The page picks: selection, keys, and actions on what is open

**Files:**
- Modify: `crates/app/src/workflows_page.rs`, `crates/app/src/workflows_page/actions.rs`, `crates/app/src/keymap/commands.rs`

**Interfaces:**
- Consumes: Task 1 (`Pick`, `pick_of`, `listed`, `repick`, `step_from`), Task 2 (`WorkflowDraft.shipped`, `open_step`).
- Produces, on `WorkflowsPage`:
  - The field `picked: Option<Pick>`.
  - `pub(crate) fn pick(&mut self, pick: Pick, window, cx)`: through `unless_editing("Opening another workflow", ..)`. `Own` with a readable template → a draft on it with its file. `Shipped` → a draft with `shipped: true` and file `None`. An unreadable `Own` → no draft, only `picked` set. Picking the one already picked does nothing.
  - `fn step_pick(&mut self, by: isize, window, cx)`: over `listed()` (with the `New` row first when picked) via `step_from`, then `pick`.
  - `close_workflow_form` also clears `picked`. *Cancel* calls it, as Esc does.
  - `new_workflow`, `duplicate_open` (replaces `duplicate_workflow(at)`, copying the open draft's `original`) and `import_workflow` set `picked = Some(Pick::New)`.
  - `export_open` and `confirm_delete_open` replace the by-index `export_workflow(at)` and `confirm_delete_workflow(at)`, acting on the picked entry (delete works for an unreadable own file too). `edit_workflow(at)` goes.
  - After a save lands, `picked` becomes `Some(Pick::Own(path))`. After a delete lands, `picked` clears. In render, a `picked` that `repick` no longer finds (and is not `New`) clears.
  - Actions `WorkflowsUp` and `WorkflowsDown` beside `CloseWorkflowForm`. Commands `workflows_up` ("Pick the workflow above", `["up"]`) and `workflows_down` ("Pick the workflow below", `["down"]`), context `"WorkflowsPage && !Dialog && !Input"`, group `"Workflows page"`.

- [ ] **Step 1:** Make the changes above. The list still renders with the old layout but marks the picked row with `onehand_plugin_host::row(selected, cx)`. Drop the rows' *Edit*, *Duplicate*, *Export…* and *Delete…*; keep *Run…* until Task 5 moves it.
- [ ] **Step 2:** `cargo check -p onehand`, then `cargo test -p onehand` — PASS. The keymap tests (if any check that commands are unique or documented) pass.
- [ ] **Step 3:** Commit `feat(workflows): picking a row opens it, and the keys step the list`.

### Task 4: The page splits into a list beside its editor

**Files:**
- Create: `crates/app/src/workflows_page/list.rs` (the list column: its cut-down head, the slots, the rows, the cap line, the `New` draft row)
- Modify: `crates/app/src/workflows_page.rs` (render; `list`, `run_menu`, `row_*` move out or go)

**Interfaces:**
- Consumes: Task 3's `picked`, `pick`, `new_workflow`, `import_workflow`.
- Produces:
  - On `WorkflowsPage`: `split: Entity<ResizableState>` and `width: onehand_plugin_host::Measured`.
  - `const LIST: ListWidths = ListWidths::new(15., 12., 22.)`, the builder's call within the spec.
  - `pub(super) fn list_column(handle, page: &WorkflowsPage, cx) -> AnyElement`.

- [ ] **Step 1:** Write `list_column`.
  - The head: *Workflows* at the title size, with `"{n} workflows"` under it in meta ink. At the end, *New* (`.tall()`, primary; `secondary()` while `picked.is_some()`) and a ⋯ menu holding *Import…*.
  - Then a slot label for the person's own, their rows, a *Built in* slot label, and the shipped rows. A slot with no rows draws no label. Each row is just the name, truncated, using `row(selected, cx)` and the focus ring as today. An unreadable own row says *Cannot be read* in `status_ink(cx).warning`.
  - The `New` draft row is *Untitled* (or the draft's typed name) at the top of the own slot.
  - Keep the cap line as today.
- [ ] **Step 2:** Rewrite `Render`, following `plugins/builtin/workbench-issues/src/view/tab.rs:30-95`. Use `list_detail(self.split.read(cx), &LIST, self.width.get(), window.rem_size())`.
  - Side by side: `h_resizable("workflows-split")` with the list panel `flex_none`, a `border_r_1` in `cx.theme().border`, and the detail panel.
  - One at a time with `picked.is_some()`: `back_link("workflows-back", "Workflows", cx)` over the detail. Its click calls `close_workflow_form`.
  - Otherwise the list alone.
  - The detail region scrolls by itself and centres its child up to `crate::tokens::PAGE_PROSE` with `px_8`/`py_6`. With nothing picked it shows `onehand_plugin_host::hint("Pick a workflow to see and change it, or make a new one", cx)`.
  - Put `measure_width(self.width.clone(), cx.entity().downgrade())` in a `relative` box sized by the page. Keep `key_context("WorkflowsPage")`, `track_focus` and the action handlers. Bind `WorkflowsUp`/`WorkflowsDown` to `step_pick(∓1)`.
- [ ] **Step 3:** `cargo test -p onehand` and `make clippy CLIPPY_EXTRA="-- -D warnings"` — PASS. `wc -l crates/app/src/workflows_page*.rs crates/app/src/workflows_page/*.rs` — each file under about 800.
- [ ] **Step 4:** Commit `feat(workflows): the page sets its list beside the editor, and says what to pick while nothing is`.

### Task 5: The editor: its head, three groups, read-only and unreadable

**Files:**
- Modify: `crates/app/src/workflows_page/form.rs`
- Create, if `form.rs` would pass about 800: `crates/app/src/workflows_page/form/head.rs`

**Interfaces:**
- Consumes: Task 3 (`duplicate_open`, `export_open`, `confirm_delete_open`, `picked`), Task 2 (`shipped`, `problem_list`), `run_menu` (moved here from the row, renamed `run_open`).
- Produces: `pub(super) fn form(handle, draft, writing, choices, cx) -> AnyElement`, and `pub(super) fn unreadable(handle, why: &str, file: &Path, cx) -> AnyElement` for an unreadable own pick: its name, the reason in the warning ink, and *Delete…* in the danger ink.

- [ ] **Step 1:** Head with `page_parts::page_head`. Title: the draft's name, or *Untitled*.
  - Facts: `"{n} steps · {place label}"`. For a shipped draft, add `" · Built in, read-only: duplicate it to change it"`.
  - Actions for an own or new draft: *Run…* (`.tall()`, secondary; `.refuses(Some("Save it first: a run starts from what is saved"))` while `draft.dirty(cx) || draft.file.is_none()`), then a ⋯ (`quiet_icon`) with *Duplicate*, *Export…*, a separator and *Delete…* in the danger ink (*Delete…* only when `file.is_some()`).
  - Actions for a shipped draft: *Run…*, then *Duplicate to edit* as primary, then a ⋯ with *Export…*.
- [ ] **Step 2:** Groups with `crate::settings::section(Some(title), None, cx)`, children in the spec's order:
  - *About*: Name and Description in an `h_flex().flex_wrap().gap_4()`, each `flex_1().min_w(rems(14.))`.
  - *How it runs*: the place switch, then Misses allowed and Timeout side by side, as today.
  - *Steps*: the variables hint goes on the heading line as `about`; if `section` cannot take an element there, put the hint on the line right under the heading in meta ink. Under it the step rows (Task 6), then one *Add step* button (`.tall()`, quiet) through `menu_below`, with one `menu_item` per `Kind::ALL` label.
  - The foot stays: problems, then *Save* and *Cancel*. For a shipped draft there is no foot.
- [ ] **Step 3:** A shipped draft draws every input with `.disabled(true)`. The switch, the pickers and the gates take no press; refuse through the control's own disabled state. Show no *Add step* and no step ⋯.
- [ ] **Step 4:** `cargo test -p onehand`, then clippy — PASS.
- [ ] **Step 5:** Commit `feat(workflows): the editor names its workflow, groups its fields, and opens a shipped one read-only`.

### Task 6: Steps as rows, then the documents

**Files:**
- Modify: `crates/app/src/workflows_page/step.rs`, `crates/app/src/workflows_page/form.rs` (drop `margin`, `MARGIN`)
- Modify: `docs/screens.md` (*Workflows* bullet), `docs/workflows.md` (the page paragraph at "Workflows are written on a page of their own", and the by-hand rows *The Workflows page*, *A draft kept across pages*, *Run from the page*), `docs/ui-patterns.md` (*Pages*, *Column pages* list: Workflows leaves `PAGE_PROSE`; *List and detail* names it), `CHANGELOG.md` if the repo's practice adds an entry per PR
- Delete: `docs/proposals/workflows-page-list-detail.md`, this plan file

**Interfaces:**
- Consumes: Task 2 (`step_facts`, `split_problems`, `toggle_step`, `remove_step`, `open_step`).
- Produces: `pub(super) fn steps_box(handle, draft, place, cx) -> AnyElement`, replacing the per-step `step_box` loop in `form`.

- [ ] **Step 1:** `steps_box` is one `card_box` holding `page_parts::ruled` rows, one per step.
  - Each row is the header line, a pressable `div` with the pointer cursor and the row hover step, whose click calls `toggle_step(i)`. It holds, in order:
    - the number in meta ink
    - `onehand_plugin_host::badge(kind.label(), Tint { fill: secondary, ink: secondary_foreground })`
    - the label (falling back to the id), truncated
    - `step_facts` joined by `" · "` in meta ink
    - the problem (from `split_problems`) after a danger dot in the danger ink
    - a chevron icon (`ChevronDown`/`ChevronUp`)
    - the existing ⋯, whose *Remove* now calls `remove_step`
  - The open row's body follows under its header line: Kind, Id and Label in one wrapping `h_flex().gap_4()`, then the kind's fields as today.
  - Remove the *Step N* head and the margin.
- [ ] **Step 2:** Update the docs to the built behaviour, in the existing wording style, with no document citing code it doesn't need. Rewrite the three by-hand rows, and add these:
  - Pick a workflow and edit its name, then pick another: *Discard the workflow being edited?*. *Keep* leaves the first picked.
  - *New*, save: the new row stays picked.
  - ↑/↓ with the caret in Name type in the field.
  - A narrow pane: picking shows the editor under *← Workflows*; Esc returns to the list with the row marked.
  - Pick a shipped workflow: fields disabled, *Duplicate to edit* opens a copy as *… copy*, picked.
  - Pick an unreadable file of the person's own: the reason, and *Delete…*.

  Delete the proposal and this plan.
- [ ] **Step 3:** `cargo test`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui` (errors as required) — all PASS.
- [ ] **Step 4:** Commit `feat(workflows): each step is one row that opens alone, and the documents say the page as built`.
- [ ] **Step 5:** Hand back for the by-hand rows; the user runs the app.
