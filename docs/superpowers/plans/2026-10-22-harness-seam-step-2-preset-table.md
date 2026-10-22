# Harness seam, step 2: the preset table, and unknown presets kept

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each built-in agent is one row of a table that says whether it runs through an npm
adapter or as a native ACP program. A config that names a preset this build does not know loads
anyway: the entry is kept, refuses to start with a sentence, and is written back as read.

**Architecture:** `Preset` keeps its two variants. Each one answers `info() -> &'static
PresetInfo`, with `runs: Runs` set to `Adapter { package, pin, binary_var }` or `Native { args }`,
and that table replaces the per-field `match` methods. `AgentSpec.preset` becomes
`Option<PresetId>`, where `PresetId` is `Known(Preset)` or `Unknown(String)` (serde `untagged`),
read through `spec.preset() -> Option<Preset>`. Settings ▸ Agents draws an unknown entry as not
supported, with only *Delete…*.

**Tech Stack:** Rust, serde (`untagged`), GPUI + gpui-component for the Settings row.

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decisions 2 and 3, step 2.

## Global Constraints

- No new preset in this step. Copilot is step 3. `Runs::Native` is reached here only through
  tests.
- Behaviour for `claude-code` and `codex` is unchanged: same command, args, env, error words,
  pins (`0.81.2`, `2.2.2`) and packages.
- An unknown preset id is kept **verbatim** in memory and in the file. A save of any setting,
  including another agent's edit, writes it back exactly (`preset = "kimi"`).
- The refusal sentence, exactly: ``` `kimi` is not an agent this build of onehand knows; update
  onehand, or remove it in Settings ▸ Agents ``` (with the entry's id in place of `kimi`).
- The Settings line for an unknown entry, exactly: `Not supported by this build of onehand`.
- Core stays GUI-free. `#![warn(unreachable_pub)]`: only what the app names is `pub`.
- Every visible change follows the `ui` skill (`.claude/skills/ui/`) and its checklist.
  Don't launch the app to check; build, test, stop.
- `make fmt` / `make lint`; CI's `make clippy CLIPPY_EXTRA="-- -D warnings"` must be clean.

## Review Focus

1. **Another agent's edit writes an unknown entry back untouched**, including its `env` and
   `version` (Task 2 test `an_unknown_preset_survives_a_save_of_another_agent`).
2. **An unknown entry with no `name`** gets its id as its name, so it can still be found and
   removed (Task 2 test `an_unknown_preset_with_no_name_is_called_by_its_id`).
3. **The legacy `unattended.mode = "acceptEdits"` is not dropped because of an unknown preset.**
   Only a known preset knows its own mode (Task 2 test
   `an_unknown_preset_does_not_drop_the_old_run_mode`).
4. **The old Claude default migration leaves an unknown preset alone** (`old_default_env` returns
   `None` for any `preset`). Covered by `an_unknown_preset_survives_a_save_of_another_agent`,
   which carries `claude` on `PATH` via the test helper `bin_dir`.
5. **A known preset id is never read as `Unknown`.** `untagged` tries `Known` first (Task 2 test
   `known_ids_still_read_as_known`).

---

### Task 1: The preset table

**Files:**
- Modify: `crates/core/src/config/agent.rs`. Add `PresetInfo` and `Runs`; replace `shown_name`,
  `binary`, `pin`, `package`, `install`, `login`, `account_var`, `binary_var` and
  `unattended_mode` with one `info()`; make `launch()`, `adapter()`, `outside_pin()` and
  `login_hint()` read the table.
- Modify: `crates/core/src/config.rs`. Re-export `PresetInfo` and `Runs`; `settle` and `run_mode`
  read `info()`.
- Modify (callers, mechanical): `crates/core/examples/acp_smoke.rs`,
  `crates/app/src/settings/agents.rs`, `crates/app/src/shell/settings_page.rs`,
  `crates/app/src/chat/pane/project_page.rs`, and any other `preset.<method>()` the compiler
  points at. Each `preset.shown_name()` becomes `preset.info().shown_name`, and likewise for the
  others.
- Test: `crates/core/src/config/agent/tests.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct PresetInfo {
      pub shown_name: &'static str,
      pub binary: &'static str,
      pub install: &'static str,
      pub(crate) login: &'static str,
      pub(crate) account_var: &'static str,
      pub(crate) unattended_mode: Option<&'static str>,
      pub runs: Runs,
  }
  pub enum Runs {
      Adapter { package: &'static str, pin: &'static str, binary_var: &'static str },
      Native { args: &'static [&'static str] },
  }
  impl Preset { pub fn info(self) -> &'static PresetInfo }
  /// What a preset runs once its binary is found: pure, so a test can hand it any table row.
  fn preset_launch(info: &PresetInfo, version: Option<&str>, binary: &Path,
                   own: impl Iterator<Item = (String, String)>) -> Launch
  ```
  `Runs::Adapter` → `npx --prefer-offline -y <package>@<version or pin>`, env
  `npm_config_omit=optional`, `<binary_var>=<binary>`, then `own` (today's behaviour).
  `Runs::Native` → command `<binary>` (the found path, displayed), args `args`, env `own` only;
  `version` is ignored.
- `adapter()` is `Some` only for `Runs::Adapter`. `outside_pin()` is true only for an adapter
  whose `version` differs from its pin.

- [ ] **Step 1: Write the failing tests:**
  ```rust
  #[test]
  fn a_native_preset_runs_its_own_binary_with_its_args() {
      let info = PresetInfo { shown_name: "Native", binary: "native", install: "", login: "",
          account_var: "", unattended_mode: None, runs: Runs::Native { args: &["--acp"] } };
      let own = [("K".to_string(), "v".to_string())].into_iter();
      let launch = preset_launch(&info, Some("9.9"), Path::new("/bin/native"), own);
      assert_eq!(launch, Launch { command: "/bin/native".into(), args: vec!["--acp".into()],
          env: vec![("K".into(), "v".into())] });
  }

  #[test]
  fn an_adapter_preset_launches_as_it_did() {
      // claude-code through preset_launch with version None and binary /bin/claude:
      // command "npx", args ["--prefer-offline", "-y", "@agentclientprotocol/claude-agent-acp@0.81.2"],
      // env [("npm_config_omit","optional"), ("CLAUDE_CODE_EXECUTABLE","/bin/claude")].
  }
  ```
  Every existing test in `agent/tests.rs` must keep passing unchanged, apart from the method to
  field renames.
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core config::agent`.
  Expected: a compile error (`PresetInfo`, `preset_launch` not found).
- [ ] **Step 3: Implement** the table (one `static` per preset, matched in `info()`), then
  `preset_launch`, and rewrite `launch()` as: find the binary (unchanged error words), then
  `preset_launch`. Update the callers.
- [ ] **Step 4: Verify:** `cargo test -q -p onehand-core` and `cargo check -q` are both clean.
- [ ] **Step 5: Commit:** `refactor(agents): every built-in agent is a row of one table`.

### Task 2: An unknown preset is kept and refused

**Files:**
- Modify: `crates/core/src/config/agent.rs`. Add `PresetId`; change the `AgentSpec.preset`
  field; add `preset()` and `unknown_preset()`; make `launch()` refuse unknown entries; make
  `name_uniquely`, `old_default_env`, `claude_or_preset` and `login_hint` read through
  `preset()`.
- Modify: `crates/core/src/config.rs`. `settle` drops the old mode only for a known preset
  (`preset().is_some()`); re-export `PresetId`.
- Modify (callers, mechanical): every `spec.preset` read becomes `spec.preset()`, and every
  `preset: Some(p)` construction becomes `preset: Some(p.into())`. The compiler lists them:
  `acp_smoke.rs`, `settings.rs`, `settings/agents.rs`, `shell/settings_page.rs`,
  `unattended/report.rs` (test), `features.rs`.
- Test: `crates/core/src/config/agent/tests.rs`, `crates/core/src/config/persist_tests.rs`.

**Interfaces:**
- Consumes: `Preset::info()` from Task 1.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(untagged)]
  pub enum PresetId { Known(Preset), Unknown(String) }
  impl From<Preset> for PresetId
  impl AgentSpec {
      pub fn preset(&self) -> Option<Preset>          // Known only
      pub fn unknown_preset(&self) -> Option<&str>    // Unknown's id
  }
  ```
  `AgentSpec.preset: Option<PresetId>` (the field keeps its name and its serde attributes).

- [ ] **Step 1: Write the failing tests:**
  ```rust
  // agent/tests.rs
  #[test]
  fn known_ids_still_read_as_known() {
      let cfg = AppConfig::parse("[[agents]]\npreset = \"codex\"\n").unwrap();
      assert_eq!(cfg.agents[0].preset(), Some(Preset::Codex));
  }

  #[test]
  fn an_unknown_preset_loads_and_refuses_to_start() {
      let cfg = AppConfig::parse(
          "[[agents]]\npreset = \"kimi\"\nname = \"Kimi\"\n\n[[agents]]\npreset = \"codex\"\n",
      ).unwrap();
      assert_eq!(cfg.agents.len(), 2);
      assert_eq!(cfg.agents[0].unknown_preset(), Some("kimi"));
      assert_eq!(cfg.agents[0].preset(), None);
      assert_eq!(
          cfg.agents[0].launch(None).unwrap_err(),
          "`kimi` is not an agent this build of onehand knows; update onehand, or remove it in Settings ▸ Agents"
      );
  }

  #[test]
  fn an_unknown_preset_with_no_name_is_called_by_its_id() {
      // AppConfig::from_disk(Some("[[agents]]\npreset = \"kimi\"\n"), None) → agents[0].name == "kimi"
  }

  #[test]
  fn an_unknown_preset_does_not_drop_the_old_run_mode() {
      // from_disk of "[[agents]]\npreset = \"kimi\"\nname = \"K\"\n\n[unattended]\nmode = \"acceptEdits\"\n"
      // → cfg.unattended.mode == Some("acceptEdits")
  }

  // persist_tests.rs
  #[test]
  fn an_unknown_preset_survives_a_save_of_another_agent() {
      // file: [[agents]] preset = "kimi", name = "Kimi", version = "2", env = { K = "v" }
      //       [[agents]] name = "Mine", command = "x"
      // update_in_place(|c| AgentEdit::Remove(c.agents[1].clone()).apply(&mut c.agents))
      // assert: the file still has `preset = "kimi"`, `version = "2"`, `K = "v"`, and no "Mine".
  }
  ```
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core preset`. Expected: a
  compile error (`unknown_preset`, `PresetId` not found).
- [ ] **Step 3: Implement.** In `launch()`, `Some(PresetId::Unknown(id))` returns the refusal.
  `name_uniquely` gives a nameless `Unknown(id)` the name `id`.
- [ ] **Step 4: Verify:** `cargo test -q -p onehand-core` and `cargo check -q` are both clean.
- [ ] **Step 5: Commit:** `fix(agents): a preset this build does not know keeps its entry and refuses to start`.

### Task 3: Settings shows it; the docs say it

**Files:**
- Modify: `crates/app/src/settings/agents.rs`:
  - `runs_line`: an unknown entry reads `Not supported by this build of onehand`, and its `env`
    is not drawn after it.
  - `agent_row`: an unknown entry gets no *Make default*, no *Test* and no *Edit*; *Delete…*
    stays.
  - The *Kind* choice and its `about` are built from `Preset::ALL` and `info()`:
    - `Runs::Adapter` → "Runs the {binary} on this machine through the adapter onehand was
      tried with".
    - `Runs::Native` → "Runs the {binary} on this machine, which speaks the protocol itself".
  - The *Another agent* choice stays last.
- Modify: `docs/agents.md`:
  - *Presets*: the table gets a row "Runs" (`adapter` for both today).
  - *The config*: one paragraph. "A `preset` this build does not know (written by a newer one)
    keeps its entry: Settings shows it as not supported by this build, it refuses to start, and
    a save writes it back as it was."
- Modify: `CHANGELOG.md` (*Fixed*): "A config naming an agent this build does not know loads:
  the entry is kept, said to be unsupported, and written back as it was. Before, the whole file
  was set aside for defaults."

- [ ] **Step 1: Read the `ui` skill** (`.claude/skills/ui/`) and its checklist. The row reuses
  the existing muted line and `list_row`. No new colour, size or control.
- [ ] **Step 2: Implement** the three changes in `settings/agents.rs`.
- [ ] **Step 3: Answer the `ui` checklist** for the changed row and the *Kind* field. Record the
  answers in the PR body.
- [ ] **Step 4: Full checks:**
  - `make fmt`, `cargo test -q --workspace`, `make clippy CLIPPY_EXTRA="-- -D warnings"`;
  - `cargo tree -p onehand-core -i gpui` (must error);
  - `cargo run -p onehand-core --example acp_smoke` builds (`cargo build -p onehand-core --examples`).
- [ ] **Step 5: Update** `docs/agents.md` and `CHANGELOG.md` as above.
- [ ] **Step 6: Commit:** `feat(settings): an agent this build does not know is shown as such`. Open
  the PR for step 2 against `main`.
