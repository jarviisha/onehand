# Harness seam, step 3: Copilot as the first native built-in agent

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `preset = "copilot"` runs the GitHub Copilot CLI on the machine as a native ACP agent
(`copilot --acp`). It gets the same promises as the two adapter presets: found on first launch,
offered in Settings, a missing binary said in words, and a login hint with its account variable.

**Architecture:** One more `Preset` variant and one more `PresetInfo` row with
`Runs::Native { args: &["--acp"], tried: "1.0.63" }`. `Runs::Native` gains `tried`, the agent
version this release was tried with, which the docs and Settings name. The Settings row learns
to draw a native preset's line (`Built in · copilot --acp`), the latent gap left by step 2.

**Tech Stack:** Rust; GPUI for the one Settings line.

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decision 2 and step 3
(owner's answer: Copilot only).

## Global Constraints

- Values, exactly:
  - id `copilot`, shown name `Copilot`, binary `copilot`;
  - install `npm install -g @github/copilot`, login `copilot login`, account variable `COPILOT_HOME`;
  - args `["--acp"]`, tried `1.0.63`.
- The unattended mode is `"https://agentclientprotocol.com/protocol/session-modes#autopilot"`, or
  `None`, per the owner's answer at the plan handoff. It is recorded in the ledger as the ruling
  that settles it.
- `Preset::ALL` order: Claude Code, Codex, Copilot. A first launch lists them in that order.
- The Plugins mode stays Claude-only (`features.rs`). Nothing else in the app matches on a
  preset by name.
- Claude Code and Codex are unchanged.
- The `ui` skill's checklist is answered for the changed row. Don't launch the app; build, test,
  stop.

## Review Focus

1. **Copilot's mode ids are URLs.** The composer's mode chip and menu must show `Agent`, `Plan`,
   `Autopilot` (the mode's `name`), never the id. Check the mode label path in
   `chat/composer/presentation.rs` and add a test where the label is computed in core (Task 2).
2. **A Copilot entry's `version` is ignored at launch** and is not offered in Settings. A
   `version` written by hand must not change the command (Task 1 test).
3. **The login hint carries `COPILOT_HOME`** when the entry's env sets it (Task 1 test).
4. **The first-launch list is stable** when only `copilot` is on `PATH`: one entry, `Copilot`
   (Task 1 test).
5. **The missing-binary sentence names `copilot`** (Task 1 test).

---

### Task 1: The Copilot preset in core

**Files:**
- Modify: `crates/core/src/config/agent.rs`:
  - a `Preset::Copilot` variant (serde id `copilot` via the existing `kebab-case`);
  - a `static COPILOT: PresetInfo`;
  - `ALL` grows to 3;
  - `Runs::Native` gains `tried: &'static str`, which the existing test row sets.
- Test: `crates/core/src/config/agent/tests.rs`.

**Interfaces:**
- Produces: `Preset::Copilot`, and `Runs::Native { args: &'static [&'static str], tried:
  &'static str }`.

- [ ] **Step 1: Write the failing tests.** Use the existing `bin_dir(test, Some("copilot"))`
  helper to put a binary on `PATH`:
  ```rust
  #[test]
  fn copilot_runs_its_own_binary_in_acp_mode() {
      // spec = AgentSpec::of(Preset::Copilot, &[]) with version Some("9") and env K=v
      // launch(Some(dir)) == Launch { command: <dir>/copilot, args: ["--acp"], env: [("K","v")] }
  }
  #[test]
  fn copilot_is_found_on_first_launch_after_the_adapters() {
      // from_disk(None, Some(dir with claude, codex, copilot)) → presets [ClaudeCode, Codex, Copilot]
      // from_disk(None, Some(dir with copilot only)) → one agent named "Copilot"
  }
  #[test]
  fn a_missing_copilot_is_said_in_words() {
      // launch(Some(empty dir)) is Err containing "`copilot` is not on this machine"
  }
  #[test]
  fn copilot_logs_in_under_its_account() {
      // env COPILOT_HOME=/home/me/.copilot-work → login_hint ==
      // "Copilot is not logged in. Run COPILOT_HOME=/home/me/.copilot-work copilot login in a terminal, then restart the agent"
  }
  ```
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core copilot`. Expected: a
  compile error (no `Preset::Copilot`).
- [ ] **Step 3: Implement** the variant, the row and `tried`. `cargo check --workspace` then
  points at any exhaustive `match` on `Preset` in the app. Each one gets a Copilot arm (expected:
  none, since step 2 moved them onto `info()`).
- [ ] **Step 4: Verify:** `cargo test -q -p onehand-core` passes.
- [ ] **Step 5: Commit:** `feat(agents): Copilot is a built-in agent that speaks the protocol itself`.

### Task 2: The Settings line, the mode labels, the docs

**Files:**
- Modify: `crates/app/src/settings/agents.rs`. In `runs_line`, a native preset reads
  `Built in · <binary> <args>` (here `Built in · copilot --acp`), with ` · <env>` after it as the
  adapter line has.
- Check: `crates/app/src/chat/composer/presentation.rs`, the mode chip. Confirm that it labels a
  mode by its `name`. If it ever derives a word from the id, take the name instead.
- Modify: `docs/agents.md`:
  - *Presets*: add a `copilot` column: shown name Copilot; *Runs* "itself (`copilot --acp`)";
    *Adapter* "none; tried with 1.0.63"; binary `copilot`; variable pointing at it: none;
    unattended mode per the ruling; effort and fast groups: none.
  - *What built-in promises*: for a native agent the tried version is the agent's own and is
    not pinned.
  - *First launch*: the install command `npm install -g @github/copilot`.
  - *Not logged in*: Copilot's `copilot login`, under `COPILOT_HOME`.
  - *Known differences*: Copilot's mode ids are URLs and are shown by name; it offers an
    `allow_all` switch among its config options; it has no effort or fast group.
- Modify: `CHANGELOG.md`, under *Unreleased* (a new *Agents* line or the closest section):
  "Copilot is a built-in agent: `copilot --acp`, found on first launch and offered in Settings."

- [ ] **Step 1: Write the failing test** for the line, in `crates/app/src/settings/tests.rs`
  (where the settings module's tests live):
  `runs_line(&AgentSpec::of(Preset::Copilot, &[]))` returns `"Built in · copilot --acp"`
  (`runs_line` becomes `pub(super)` if the test module needs it).
- [ ] **Step 2: Run it to see it fail:** `cargo test -q -p onehand runs_line`. Expected: FAIL (the
  line is empty today).
- [ ] **Step 3: Implement** `runs_line`, check the mode label path, and update the docs.
- [ ] **Step 4: Answer the `ui` checklist** for the row (it reuses the existing line), and record
  it in the PR body.
- [ ] **Step 5: Full checks:**
  - `make fmt`, `cargo test -q --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`;
  - `cargo tree -p onehand-core -i gpui` (must error).
- [ ] **Step 6: Smoke test by hand:** `ACP_PRESET=copilot cargo run -q -p onehand-core --example
  acp_smoke`. Expected: `== CONNECTED`, a streamed reply, and a turn end. Record the tail in the
  PR body. A login failure counts as a valid result; record it as such.
- [ ] **Step 7: Commit:** `feat(settings): a native built-in agent says what it runs`. Open the PR
  against `main`.
