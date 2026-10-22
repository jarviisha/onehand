# Harness seam, step 5: what onehand guesses about a harness, in core

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two guesses move into core, where tests and a second front end reach them:
- which config group is the model, the effort, or the fast switch (read from ACP's `category`
  first, then from the name lists the composer holds today), and whether the fast switch is on;
- that a user chunk is an adapter's "interrupted" marker, recognised where the protocol is
  parsed.

**Architecture:**
- `ConfigOption` gains `category: Option<String>`, parsed in `acp/parse.rs`.
- A new core module, `acp/roles.rs`, answers `ConfigOption::role() -> Option<Role>`, with `Role`
  one of `Model`, `Effort` or `Fast`, and `ConfigOption::switch() -> Option<Switch>`, plus
  `ConfigChoice::reads_off()`. It holds the name and word lists, moved verbatim from
  `chat/composer/presentation.rs`.
- The composer matches on `Role` and keeps no list.
- `parse::Update::Interrupted` becomes `AcpEvent::Interrupted`, and the reducer gives it its own
  arm.

**Tech Stack:** Rust (core + app).

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decision 4, step 5.

## Global Constraints

- **No behaviour change for Claude Code or Codex.** The same chips, the same rows, the same
  order, the same switch.
- **Category first, names second.** `category = "model"` → `Model`; `category =
  "thought_level"` → `Effort`. Any other category, or none, falls back to the names:
  - id or label `model` → `Model`;
  - `effort` / `reasoning_effort` → `Effort`;
  - `fast` / `fast-mode` → `Fast`.
  - Names are compared case-insensitively.
- **The word lists move verbatim:**
  - on: `["on", "true", "enabled", "yes", "fast"]`;
  - off: `["off", "false", "disabled", "no", "normal", "standard", "default"]`.
- **No list in two places.** After this step, `presentation.rs` holds no group name or on/off
  word.
- **Deliberate deviation from the spec's wording:** the role lives on `ConfigOption` (what the
  composer reads) rather than on `Selector`. Nothing reads a selector's role yet, and a `Selector`
  is built from options, so it can ask the option when something does.
- Every `AcpEvent` match stays exhaustive, with no wildcard arms (the app's guards check this).
- `make fmt`; `make clippy CLIPPY_EXTRA="-- -D warnings"` must be clean.

## Review Focus

1. **A `category` this build does not know** (`"_vendor_x"`, `"mode"`) falls back to names, so
   it does not lose a chip (Task 1 test `an_unknown_category_falls_back_to_the_names`).
2. **A category and a name that disagree**: the category wins, e.g. id `effort` with category
   `model` is the model (Task 1 test `the_category_wins_over_the_name`).
3. **The interrupted marker split over two chunks** is not recognised, as today. Recognising it
   would need buffering, which today's code does not do either (state it, no test).
4. **The interrupted marker inside a replayed history**: the reducer's arm still consumes the
   replay before pushing the notice, as today (Task 3: the moved arm keeps `consume_replay`).
5. **OpenCode's `category: "model"` option with id `model`**: it reads as `Model` either way
   (covered by 1 and 2).

---

### Task 1: Roles in core

**Files:**
- Modify: `crates/core/src/acp/types.rs`. `ConfigOption` gains `pub category: Option<String>`.
  Update every `ConfigOption { … }` literal that the compiler lists.
- Modify: `crates/core/src/acp/parse.rs`. Read `category` (a string) in the config-option parser.
- Create: `crates/core/src/acp/roles.rs` (`mod roles;` in `acp/mod.rs`, with `pub use
  roles::{Role, Switch}`). Create `crates/core/src/acp/roles/tests.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Role { Model, Effort, Fast }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct Switch { pub on: bool, pub flip_to: String }
  impl ConfigOption {
      pub fn role(&self) -> Option<Role>;
      /// The choice in force, when the agent named one this list holds.
      pub fn in_force(&self) -> Option<&ConfigChoice>;
      /// The group as a switch: exactly two choices, one named on or off.
      pub fn switch(&self) -> Option<Switch>;
  }
  impl ConfigChoice { pub fn reads_off(&self) -> bool; }
  ```
  `switch` and `reads_off` are today's `toggle_of` and `says(…, &OFF_WORDS)`, moved without
  change. `in_force` is today's composer helper, moved.

- [ ] **Step 1: Write the failing tests** in `roles/tests.rs`:
  - Move these app tests here, on the new methods:
    - `a_group_is_found_by_id_or_by_label` → `role()`;
    - `codex_names_its_effort_and_fast_groups_differently` → `role()`;
    - `fast_is_a_switch_only_when_its_two_values_say_which_is_on` → `switch()`;
    - `a_fast_picker_is_lit_unless_its_value_reads_as_off` → `in_force()` + `reads_off()`;
    - `a_value_the_agent_never_offered_lights_nothing` → `in_force()`.
  - Add:
  ```rust
  #[test]
  fn the_category_wins_over_the_name() {
      // id "effort", name "Effort", category Some("model") → Some(Role::Model)
      // id "x", name "Thinking", category Some("thought_level") → Some(Role::Effort)
  }
  #[test]
  fn an_unknown_category_falls_back_to_the_names() {
      // id "fast-mode", category Some("_vendor_x") → Some(Role::Fast)
      // id "agent", category Some("mode") → None
  }
  ```
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core acp::roles`.
  Expected: a compile error.
- [ ] **Step 3: Implement** `roles.rs` and the `category` field and parse. Literals in tests and
  in the app get `category: None`.
- [ ] **Step 4: Verify:** `cargo test -q -p onehand-core` passes, and `cargo check -q --workspace
  --tests` is clean.
- [ ] **Step 5: Commit:** `feat(acp): a config group's role is core's to say, from its category first`.

### Task 2: The composer reads roles

**Files:**
- Modify: `crates/app/src/chat/composer/presentation.rs`:
  - Delete `EFFORT_GROUPS`, `LEAD_GROUP`, `FAST_GROUPS`, `ON_WORDS`, `OFF_WORDS`, `names`,
    `names_any`, `says`, `toggle_of`, `lit` and `in_force`.
  - `has_own_chip` = `role() == Some(Role::Effort) && choices.len() >= 2`.
  - `fast_group` finds `role() == Some(Role::Fast)` with choices.
  - `fast_toggle` maps `option.switch()` into `FastToggle { on, config_id, flip_to }`.
  - `fast_action` reads `in_force()` and `reads_off()`.
  - `options_action` finds `Role::Model`; `listed` and `config_rank` match on `role()`.
  - Doc comments that named the lists now say the composer asks core for the group's role.
- Test: the tests that stay in `presentation.rs` are `a_promoted_group_that_cannot_be_drawn_stays_in_the_list`,
  `effort_and_fast_each_have_a_control_and_leave_no_row` and
  `model_then_effort_lead_the_combined_popup`, on the same fixtures with `category: None`.

- [ ] **Step 1: Refactor** as listed. The five tests moved in Task 1 are removed here.
- [ ] **Step 2: Verify:**
  - `cargo test -q -p onehand --locked`; the guards and the 3 remaining presentation tests pass;
  - `grep -nE '"effort"|reasoning_effort|"fast-mode"|"normal"' crates/app/src/chat/composer/presentation.rs`
    finds only test fixtures.
- [ ] **Step 3: Commit:** `refactor(composer): the model, effort and fast chips ask core which group is which`.

### Task 3: The interrupted marker is parsed, not reduced

**Files:**
- Modify: `crates/core/src/acp/parse.rs`. In the `user_message_chunk` arm, a chunk that is the
  marker (today's `is_interruption`, moved here) yields `Update::Interrupted`, not
  `Update::User`.
- Modify: `crates/core/src/acp/types.rs`. Add `AcpEvent::Interrupted`, with a doc comment: the
  adapter's own marker for a turn the person cut short, sent in the person's voice.
- Modify: `crates/core/src/acp/client.rs`. Map `Update::Interrupted` → `AcpEvent::Interrupted`.
- Modify: `crates/core/src/chat/model/apply.rs`. The `UserChunk(s) if is_interruption(&s)` arm
  becomes `AcpEvent::Interrupted`, with the same body. Delete `is_interruption`. Check the
  metadata and `user_chunk_open` `matches!` lists: `Interrupted` is a content event, as the user
  chunk it replaces was.
- Modify (exhaustive matches the compiler lists): `crates/app/src/agent_check.rs`,
  `crates/core/examples/acp_smoke.rs`, and any other.
- Test: `crates/core/src/acp/parse.rs` tests, `crates/core/src/chat/model/tests.rs`.

- [ ] **Step 1: Write the failing tests:**
  ```rust
  // parse.rs tests
  #[test]
  fn the_interrupted_marker_is_its_own_update() {
      // {"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"[Request interrupted by user for tool use]"}}
      // → vec![Update::Interrupted];  "hi" → vec![Update::User("hi")]
  }
  ```
  The existing reducer test(s) that send the marker as a `UserChunk` now send
  `AcpEvent::Interrupted` and keep their assertions (find them with `grep -n "interrupted"
  crates/core/src/chat/model/tests.rs`).
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core interrupted`.
  Expected: a compile error.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Full checks:**
  - `make fmt`, `cargo test -q --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`;
  - `cargo tree -p onehand-core -i gpui` (must error);
  - `ACP_PRESET=copilot cargo run -q -p onehand-core --example acp_smoke` (Expected: CONNECTED …
    TURN ENDED).
- [ ] **Step 5:** Add a `CHANGELOG.md` line only if behaviour changed. Expected: the one change
  is that a declared agent's option with an ACP `category` gets its chip. Say that line under
  *Fixed*.
- [ ] **Step 6: Commit:** `refactor(acp): an adapter's interrupted marker is read where the protocol is`. Open the
  PR against `main`.
