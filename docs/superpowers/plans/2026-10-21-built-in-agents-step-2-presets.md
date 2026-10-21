# Built-in agents, step 2: presets — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude Code and Codex become presets that live in code. The config names a preset by id, may repeat it with its own name and `env`, and the adapter runs the person's own binary. Old configs migrate, and the stored unattended mode stops pinning every agent to `acceptEdits`.

**Architecture:**
- `AgentSpec` stays one flat struct. It gains `preset`, `version` and `env`, so its ~20 users keep compiling.
- A new core module `config/agent.rs` owns the agent definition, the presets and `AgentSpec::launch`, which turns a spec into the exact command, args and environment to start, or refuses with a sentence.
- The app's single spawn path (`AcpRuntime::start`) goes through `launch`, and ACP `connect` gains an environment.
- Migration is one core pass, `AppConfig::settle`, run wherever a config is read.
- Settings ▸ Agents learns the kinds of agent.

**Tech Stack:** Rust, serde/toml, tokio process, GPUI + gpui-component, the `onehand_ui` builders.

**Spec:** `docs/superpowers/specs/2026-10-21-built-in-agents-design.md`: decisions 1, 2, 3, decision 6 item 5, decision 8, and *Order of work* step 2.

## Global Constraints

- Read CLAUDE.md first; its rules bind every task. In particular:
  - nothing in core is `pub` unless named outside it;
  - comments never cite a document;
  - use `make fmt`, never bare `cargo fmt`;
  - core stays GUI-free;
  - split a file before it passes ~800 lines of code.
- Task 5 is a visible change. It follows `.claude/skills/ui/SKILL.md` and answers `checklist.md`.
- **Preset values, verbatim from the spec:**

  | | `claude-code` | `codex` |
  |---|---|---|
  | shown name | Claude Code | Codex |
  | adapter package | `@agentclientprotocol/claude-agent-acp` | `@agentclientprotocol/codex-acp` |
  | pin | `0.81.2` | `2.2.2` |
  | binary | `claude` | `codex` |
  | binary variable | `CLAUDE_CODE_EXECUTABLE` | `CODEX_PATH` |
  | unattended mode when none is set | `acceptEdits` | none |

- The adapter start for a preset is `npx --prefer-offline -y <package>@<version>`, with `npm_config_omit=optional` and the binary variable set to the absolute path `find_command` returns. The entry's own `env` is applied after both, so it can override either.
- The words for a missing binary (the entry name is the entry's own): `` `<binary>` is not on this machine — install it again, or remove <entry name> in Settings ▸ Agents ``.
- The old default that migration recognises is exactly command `npx` with args `--prefer-offline -y @agentclientprotocol/claude-agent-acp@0.81.2`. The env form is `env VAR=value… npx` followed by the same args.

## Review Focus

1. **A config written by this build is read back as the same list** (round trip, both kinds). A preset entry is not written with `command`/`args`, and a declared one is not written with `preset`. → Task 1 test `a_preset_and_a_declared_agent_round_trip`.
2. **A migration only happens when `claude` is on `PATH`.** With no binary, the old default stays declared and keeps launching as before. → Task 4 test `the_old_default_stays_declared_without_claude`.
3. **An `env VAR=value` entry whose value holds `=` or spaces keeps its value whole** after migration. → Task 4 test `the_env_form_keeps_values_whole`.
4. **A save never deletes an entry whose name is a duplicate**; it renames it. → Task 4 test `duplicate_names_are_numbered_not_dropped`.
5. **An unattended run on a declared agent with a stored `acceptEdits` keeps `acceptEdits`.** → Task 4 test `the_stored_mode_is_cleared_for_a_preset_and_kept_for_a_declared_agent`.

---

### Task 1: The agent module, the presets and `launch`

**Files:**
- Create: `crates/core/src/config/agent.rs`. Move out of `config.rs`, unchanged, everything about the agent definition: `AgentSpec`, `agent_or_first`, `join_args`, `find_command`, `split_args`, `DEFAULT_ACP_ADAPTER`, `default_adapter_args`, `default_agents`, with their doc comments. `config.rs` re-exports them, so every `onehand_core::config::…` path still resolves.
- Modify: `crates/core/src/config.rs` (`mod agent;` plus the re-exports).
- Test: `crates/core/src/config/agent/tests.rs` (new).

**Interfaces:**
- Produces:
  - `pub enum Preset { ClaudeCode, Codex }`, serde `rename_all = "kebab-case"` (`"claude-code"`, `"codex"`), with `Copy, Clone, Debug, PartialEq, Eq`.
  - Methods on `Preset`:
    - `pub fn shown_name(self) -> &'static str`
    - `pub fn binary(self) -> &'static str`
    - `pub fn pin(self) -> &'static str`
    - `pub(crate) fn unattended_mode(self) -> Option<&'static str>`
    - plus private `package` and `binary_var`.
  - New `AgentSpec` fields, all `#[serde(default, skip_serializing_if = …)]`:
    - `pub preset: Option<Preset>`
    - `pub version: Option<String>`
    - `pub env: BTreeMap<String, String>`
  - `command` and `args` become `#[serde(default, skip_serializing_if = …)]`, so a preset entry writes neither.
  - Field order: `name`, `preset`, `version`, `command`, `args`, `env`.
  - `pub struct Launch { pub command: String, pub args: Vec<String>, pub env: Vec<(String, String)> }`.
  - `impl AgentSpec`:
    - `pub fn launch(&self, path: Option<&OsStr>) -> Result<Launch, String>`
    - `pub fn outside_pin(&self) -> bool`: a preset whose `version` is set and differs from its pin.

- [ ] **Step 1: Move the agent items into `config/agent.rs` with no change in behaviour**, run `cargo test -p onehand-core config`, and expect PASS. Commit: `refactor(config): the agent definition moves to a module of its own`.

- [ ] **Step 2: Write the failing tests**

```rust
/// A fresh directory under the system temp dir, unique to `test`, holding an
/// empty file named `binary` when one is given. Core has no tempfile crate.
fn bin_dir(test: &str, binary: Option<&str>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(binary) = binary {
        std::fs::write(dir.join(binary), "").unwrap();
    }
    dir
}

#[test]
fn a_preset_and_a_declared_agent_round_trip() {
    let text = r#"
[[agents]]
name = "Claude Code (acc2)"
preset = "claude-code"
env = { CLAUDE_CONFIG_DIR = "/home/me/.claude-acc2" }

[[agents]]
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
"#;
    let cfg = AppConfig::parse(text).unwrap();
    assert_eq!(cfg.agents[0].preset, Some(Preset::ClaudeCode));
    assert!(cfg.agents[0].command.is_empty());
    assert_eq!(cfg.agents[1].preset, None);
    let again = AppConfig::parse(&cfg.to_toml().unwrap()).unwrap();
    assert_eq!(again.agents, cfg.agents);
    assert!(!cfg.to_toml().unwrap().contains("command = \"\""));
}

#[test]
fn a_preset_launches_the_pinned_adapter_on_the_machines_binary() {
    let dir = bin_dir("case-1", Some("codex"));
    let spec = AgentSpec { name: "Codex".into(), preset: Some(Preset::Codex), ..Default::default() };
    let launch = spec.launch(Some(dir.as_os_str())).unwrap();
    assert_eq!(launch.command, "npx");
    assert_eq!(launch.args, ["--prefer-offline", "-y", "@agentclientprotocol/codex-acp@2.2.2"]);
    assert!(launch.env.contains(&("npm_config_omit".into(), "optional".into())));
    assert!(launch.env.contains(&("CODEX_PATH".into(), dir.join("codex").display().to_string())));
}

#[test]
fn an_entrys_env_and_version_win() {
    let dir = bin_dir("case-2", Some("claude"));
    let spec = AgentSpec {
        name: "Claude Code (acc2)".into(),
        preset: Some(Preset::ClaudeCode),
        version: Some("latest".into()),
        env: [("CLAUDE_CONFIG_DIR".into(), "/x".into()), ("npm_config_omit".into(), "".into())].into(),
        ..Default::default()
    };
    let launch = spec.launch(Some(dir.as_os_str())).unwrap();
    assert_eq!(launch.args[2], "@agentclientprotocol/claude-agent-acp@latest");
    // Applied after the preset's own, so the entry's value is the last one set.
    assert_eq!(launch.env.iter().rev().find(|(k, _)| k == "npm_config_omit").unwrap().1, "");
    assert!(spec.outside_pin());
}

#[test]
fn a_missing_binary_is_said_and_nothing_is_launched() {
    let empty = bin_dir("case-8", None);
    let spec = AgentSpec { name: "Codex".into(), preset: Some(Preset::Codex), ..Default::default() };
    assert_eq!(
        spec.launch(Some(empty.as_os_str())).unwrap_err(),
        "`codex` is not on this machine — install it again, or remove Codex in Settings ▸ Agents"
    );
}

#[test]
fn a_declared_agent_launches_as_written() {
    let spec = AgentSpec {
        name: "Gemini".into(),
        command: "gemini".into(),
        args: vec!["--experimental-acp".into()],
        env: [("A".into(), "1".into())].into(),
        ..Default::default()
    };
    let launch = spec.launch(None).unwrap();
    assert_eq!((launch.command.as_str(), launch.args.as_slice()), ("gemini", &["--experimental-acp".to_string()][..]));
    assert_eq!(launch.env, [("A".to_string(), "1".to_string())]);
    assert!(!spec.outside_pin());
}
```

`AgentSpec` derives `Default` for these tests and for Task 5's form. `bin_dir` lives in `config/agent/tests.rs` as `pub(super)`; Task 4's tests in the same file use it.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p onehand-core config::agent`
Expected: compile errors for `Preset`, `launch`, the new fields.

- [ ] **Step 4: Implement `Preset`, the fields and `launch` in `config/agent.rs`**

- A declared agent's launch never looks on `PATH`: a missing command still fails at spawn with the OS's words, as today.
- `default_agents()` stays the old declared default; Task 4 migrates it.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p onehand-core config`
Expected: PASS, the existing config tests included.

- [ ] **Step 6: Commit**: `feat(config): presets for Claude Code and Codex, and an agent's environment`

---

### Task 2: ACP `connect` sets the adapter's environment

**Files:**
- Modify: `crates/core/src/acp/client.rs` (`connect` ~line 62, `Transport::spawn` ~line 247)
- Modify: `crates/core/examples/acp_smoke.rs` (pass an empty env)
- Test: `crates/core/src/acp/client/tests.rs`

**Interfaces:**
- Produces: `pub fn connect(command: String, args: Vec<String>, env: Vec<(String, String)>, cwd: PathBuf, resume: Option<String>)`. The parameter order is fixed here, and Task 3 calls it so. `Transport::spawn` takes the same `env` and calls `.envs(env)` on the `Command`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn the_adapter_is_started_with_its_environment() {
    let stream = super::connect(
        "sh".into(),
        vec!["-c".into(), "echo seen-$ONEHAND_PROBE >&2".into()],
        vec![("ONEHAND_PROBE".into(), "42".into())],
        std::env::temp_dir(),
        None,
    );
    futures::pin_mut!(stream);
    let mut text = None;
    while let Some(event) = stream.next().await {
        if let AcpEvent::Disconnected(why) = event { text = Some(why); break; }
    }
    assert!(text.expect("the adapter dies").contains("seen-42"));
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p onehand-core the_adapter_is_started_with_its_environment`
Expected: compile error (too many arguments).

- [ ] **Step 3: Add the `env` parameter through `connect` → `Transport::spawn`**, and update `acp_smoke.rs` to pass `Vec::new()`.

- [ ] **Step 4: Run the client tests**

Run: `cargo test -p onehand-core acp::client && cargo build -p onehand-core --examples`
Expected: PASS; the example builds.

- [ ] **Step 5: Commit**: `feat(acp): an adapter starts with the environment its agent sets`

---

### Task 3: Sessions, warm adapters, unattended runs and *Test* start through `launch`

**Files:**
- Modify: `crates/app/src/acp.rs`: `Warm` (~line 50), `warm` (~102), `start` (~172).
- Modify: `crates/app/src/unattended/report.rs`: `wrapped` (~34), and the `ponytail:` note above `spec_for`, which this fixes.
- Modify: `crates/app/src/shell/settings_page.rs`: `check_agent` (~152).
- Modify: `crates/app/src/settings/agents.rs`: `check_key` (~157).
- Test: a `#[cfg(test)] mod tests` in `crates/app/src/unattended/report.rs` (add one if none exists).

**Interfaces:**
- Consumes: `AgentSpec::launch`, `Preset::binary` (Task 1); `acp::connect(command, args, env, cwd, resume)` (Task 2).
- Produces:
  - `wrapped(base)` returns `base` with `CARGO_TARGET_DIR` inserted into `env`. `command` and `args` are left alone, and `env` is not used as the command.
  - `check_key(spec)` covers `preset`, `version`, `command`, `args`, and `env`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_run_puts_the_build_directory_in_the_agents_environment() {
    let base = AgentSpec { name: "Codex".into(), preset: Some(Preset::Codex), ..Default::default() };
    let run = wrapped(base.clone());
    assert_eq!(run.preset, base.preset);
    assert!(run.command.is_empty());
    if let Some(dir) = onehand_core::target_dir() {
        assert_eq!(run.env.get("CARGO_TARGET_DIR"), Some(&dir.display().to_string()));
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p onehand a_run_puts_the_build_directory`
Expected: FAIL (`command` is `env`).

- [ ] **Step 3: Implement**
  - **`AcpRuntime::start`**: call `spec.launch(std::env::var_os("PATH").as_deref())`.
    - On `Err(why)`, send `AcpEvent::Disconnected(why)` into the channel and return it, starting nothing.
    - On `Ok`, pass the launch's command, args and env to `acp::connect`.
  - **`Warm`**: store the spec with its `name` cleared, and match on equality with the asked spec, `name` cleared. Keep the reason in the existing comment: a rename does not change the process.
  - **`wrapped`**: as above. Delete the POSIX `ponytail:` note, since the variable now goes through the spawn.
  - **`check_agent`**:
    - For a preset, look for `preset.binary()` on `PATH` and, if found, read `<binary> --version` (first line, trimmed) on the background executor.
    - `AgentCheck::Found` gains an `Option<String>` version: `Found(PathBuf, Option<String>)`.
    - A declared agent checks `command` as today.

- [ ] **Step 4: Run the app tests**

Run: `cargo test -p onehand`
Expected: PASS, the guards included.

- [ ] **Step 5: Commit**: `feat(agents): sessions, runs and Test start an agent through its launch`

---

### Task 4: `AppConfig::settle` — migration, names, and the unattended mode

**Files:**
- Modify: `crates/core/src/config/agent.rs`: migration of one entry, name filling and numbering.
- Modify: `crates/core/src/config.rs`:
  - `UnattendedConfig.mode` becomes `Option<String>`, `skip_serializing_if = "Option::is_none"`, default `None`;
  - `settle`;
  - calls from `load_resolved` and `update_in_place`.
- Modify: `crates/app/src/unattended.rs`: `boot` (~line 246) resolves the mode.
- Test: `crates/core/src/config/agent/tests.rs`, `crates/core/src/config/tests.rs`.

**Interfaces:**
- Produces:
  - `impl AppConfig { pub(crate) fn settle(&mut self, path: Option<&OsStr>) }`, idempotent. It runs after every parse in `load_resolved` (with `std::env::var_os("PATH")`) and in `update_in_place`, before `edit`, the missing-file branch included.
  - `pub fn run_mode(mode: Option<&str>, agent: Option<&AgentSpec>) -> String`:
    - `Some(m)` returns `m`;
    - `None` returns the agent's preset `unattended_mode()`, or `""`.
    - The result keeps today's meaning: `""` is the agent's own default mode.

**What `settle` does, in order:**
1. **Migrate the old default, only if `find_command("claude", path, None)` finds it.** An entry matching the old default, or its env form, becomes:
   - `preset: Some(ClaudeCode)`
   - `command` and `args` cleared
   - the `VAR=value` pairs moved into `env` (split on the first `=` only)
   - `name` kept
2. **Fill names.** A preset entry with a blank name takes `shown_name()`.
3. **Number duplicates.** The second and later entries with a name already taken get ` (2)`, ` (3)`… appended, skipping any number already in use.
4. **Clear the stored default mode.** If `unattended.mode == Some("acceptEdits")` and the unattended agent (`unattended.agent`, or the first entry) has a preset, set it to `None`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_old_default_becomes_the_preset_where_claude_is_installed() {
    let dir = bin_dir("case-3", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.agents[0].preset, Some(Preset::ClaudeCode));
    assert_eq!(cfg.agents[0].name, "Claude Code");
    assert!(cfg.agents[0].command.is_empty() && cfg.agents[0].args.is_empty());
}

#[test]
fn the_old_default_stays_declared_without_claude() {
    let empty = bin_dir("case-9", None);
    let mut cfg = AppConfig::default();
    cfg.settle(Some(empty.as_os_str()));
    assert_eq!(cfg.agents, default_agents());
}

#[test]
fn the_env_form_keeps_values_whole() {
    let dir = bin_dir("case-4", Some("claude"));
    let mut cfg = AppConfig::default();
    let mut args = vec!["CLAUDE_CONFIG_DIR=/home/me/.claude acc2".to_string(), "X=a=b".into(), "npx".into()];
    args.extend(default_adapter_args());
    cfg.agents = vec![AgentSpec { name: "Claude Code (acc2)".into(), command: "env".into(), args, ..Default::default() }];
    cfg.settle(Some(dir.as_os_str()));
    let spec = &cfg.agents[0];
    assert_eq!(spec.preset, Some(Preset::ClaudeCode));
    assert_eq!(spec.name, "Claude Code (acc2)");
    assert_eq!(spec.env.get("CLAUDE_CONFIG_DIR").map(String::as_str), Some("/home/me/.claude acc2"));
    assert_eq!(spec.env.get("X").map(String::as_str), Some("a=b"));
}

#[test]
fn an_edited_entry_is_left_as_written() {
    let dir = bin_dir("case-5", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.agents[0].args.push("--verbose".into());
    let before = cfg.agents.clone();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.agents, before);
}

#[test]
fn duplicate_names_are_numbered_not_dropped() {
    let mut cfg = AppConfig::default();
    let one = |name: &str| AgentSpec { name: name.into(), command: "x".into(), ..Default::default() };
    cfg.agents = vec![one("A"), one("A"), one("A (2)"), one("A")];
    cfg.settle(None);
    let names: Vec<_> = cfg.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["A", "A (3)", "A (2)", "A (4)"]);
}

#[test]
fn the_stored_mode_is_cleared_for_a_preset_and_kept_for_a_declared_agent() {
    let dir = bin_dir("case-6", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = Some("acceptEdits".into());
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.unattended.mode, None);

    let empty = bin_dir("case-10", None);
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = Some("acceptEdits".into());
    cfg.settle(Some(empty.as_os_str()));
    assert_eq!(cfg.unattended.mode.as_deref(), Some("acceptEdits"));
}

#[test]
fn a_run_mode_comes_from_the_preset_when_none_is_set() {
    let claude = AgentSpec { preset: Some(Preset::ClaudeCode), ..Default::default() };
    let codex = AgentSpec { preset: Some(Preset::Codex), ..Default::default() };
    assert_eq!(run_mode(None, Some(&claude)), "acceptEdits");
    assert_eq!(run_mode(None, Some(&codex)), "");
    assert_eq!(run_mode(Some(""), Some(&claude)), "");
    assert_eq!(run_mode(Some("plan"), Some(&codex)), "plan");
    assert_eq!(run_mode(None, None), "");
}

#[test]
fn settling_twice_changes_nothing_more() {
    let dir = bin_dir("case-7", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.settle(Some(dir.as_os_str()));
    let once = cfg.clone();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg, once);
}
```

Fix whatever existing `config/tests.rs` assertions read `unattended.mode` as a `String`, keeping their intent.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p onehand-core config`
Expected: compile errors for `settle`, `run_mode`, `Option` mode.

- [ ] **Step 3: Implement `settle` and `run_mode`, and call `settle` from `load_resolved` and `update_in_place`.** In `unattended::boot`, set `Unattended.mode` to `run_mode(cfg.mode.as_deref(), <the unattended agent>)`, where the agent is `agent_or_first(&shared.agents, cfg.agent.as_deref())`. Everything downstream keeps reading a `String`.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p onehand-core && cargo test -p onehand`
Expected: PASS.

- [ ] **Step 5: Commit**: `feat(config): old Claude entries become the preset, names stay unique, and runs take the preset's mode`

---

### Task 5: Settings ▸ Agents knows the kinds of agent

Follows the `ui` skill. Peers: the workflow step settings' `choice_track` (`crates/app/src/workflows_page/step/settings.rs:302`); the existing agent rows and form in `settings/agents.rs`.

**Files:**
- Modify: `crates/app/src/settings.rs` (`AgentDraft`)
- Modify: `crates/app/src/settings/agents.rs` (`agent_row`, the form)
- Modify: `crates/app/src/shell/settings_page.rs` (picking a kind)
- Test: `crates/app/src/settings/tests.rs`

**Interfaces:**
- Consumes: `Preset`, `AgentSpec { preset, version, env, .. }`, `outside_pin` (Task 1); `AgentCheck::Found(PathBuf, Option<String>)` (Task 3).
- Produces:
  - `AgentDraft` gains `kind: Option<Preset>` (`None` = another agent) and `env: Entity<InputState>`.
  - `to_spec`:
    - for a preset, needs a non-blank name only; it writes `preset`, `name`, `env`, and keeps the edited entry's `version`;
    - for another agent, needs name and command, as today, plus `env`.
  - `env` is one line of `KEY=value` words, parsed with `split_args` and joined with `join_args`, split on the first `=`. A word with no `=` refuses Save with the words `Write each variable as NAME=value`.

**What the person sees:**
- **The form opens with *Kind***, a `choice_track` of *Claude Code*, *Codex*, *Another agent*:
  - *Claude Code* and *Codex* show *Name* (its placeholder is the preset's name) and *Environment* (about: `Variables set for this agent, as NAME=value; a second account sets its config directory here`).
  - *Another agent* shows *Name*, *Command*, *Arguments*, *Environment*, as today plus *Environment*.
  - Editing an entry opens on its kind.
- **A preset row's mono line** reads `Built in · <package>@<version or pin>`.
- **When `outside_pin()`**, a second line in the warning ink says `Runs adapter <version>, outside the version this release was tried with`.
- **A declared row** reads as today, its env words appended to the line.
- ***Test* on a preset row**:
  - found: `<binary> <version> found at <path>`, or without the version if `--version` gave nothing;
  - missing: the same sentence `launch` refuses with.

- [ ] **Step 1: Write the failing tests**

```rust
// draft_spec(kind, name, command, args, env, version) -> Result<AgentSpec, &'static str>
#[test]
fn a_preset_draft_needs_only_a_name() {
    let spec = draft_spec(Some(Preset::Codex), "Codex", "", "", "", None).unwrap();
    assert_eq!(spec.preset, Some(Preset::Codex));
    assert!(spec.command.is_empty() && spec.args.is_empty());
    assert!(draft_spec(Some(Preset::Codex), " ", "", "", "", None).is_err());
    assert!(draft_spec(None, "Gemini", "", "", "", None).is_err(), "another agent still needs its command");
}

#[test]
fn an_env_word_without_equals_refuses_save() {
    assert_eq!(
        draft_spec(Some(Preset::ClaudeCode), "Claude Code (acc2)", "", "", "CLAUDE_CONFIG_DIR", None).unwrap_err(),
        "Write each variable as NAME=value"
    );
    let spec = draft_spec(Some(Preset::ClaudeCode), "Claude Code (acc2)", "", "", "CLAUDE_CONFIG_DIR='/x y' A=b=c", None).unwrap();
    assert_eq!(spec.env.get("CLAUDE_CONFIG_DIR").map(String::as_str), Some("/x y"));
    assert_eq!(spec.env.get("A").map(String::as_str), Some("b=c"));
}

#[test]
fn editing_a_preset_keeps_its_version() {
    let spec = draft_spec(Some(Preset::Codex), "Codex", "", "", "", Some("latest".into())).unwrap();
    assert_eq!(spec.version.as_deref(), Some("latest"));
}
```

`draft_spec` is a free function in `settings.rs`: `fn draft_spec(kind: Option<Preset>, name: &str, command: &str, args: &str, env: &str, version: Option<String>) -> Result<AgentSpec, &'static str>`. `AgentDraft::to_spec` reads its inputs and calls it, keeping the edited entry's `version`. The `Err` string is the refusal Save shows; a blank name or command keeps today's `Name it and give its command first` (`Name it first` for a preset).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p onehand settings`
Expected: compile errors for the new function and fields.

- [ ] **Step 3: Implement the draft, the form and the rows.** Answer `.claude/skills/ui/checklist.md` item by item in the commit body.

- [ ] **Step 4: Run the checks the ui skill names**

Run: `cargo test -p onehand --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`
Expected: PASS, the guards included.

- [ ] **Step 5: Commit**: `feat(settings): an agent is Claude Code, Codex or another agent, each with its environment`

---

### Task 6: The documents

**Files:**
- Create: `docs/agents.md`: what *built-in* promises, the preset table, `preset`/`name`/`version`/`env` with the example TOML from the spec, the migration rules, and the known differences listed in the spec's decision 6. It promises the following:
  - the adapter version per release passed the headless smoke test against that agent, run by hand before a pin moves;
  - the features in the spec's decision 6 work alike for both;
  - a missing binary is said before anything starts.
- Modify: `CONTEXT.md`: a new `## Agents` section with *Built-in agent*, *Declared agent* and *Preset*, each with an *Avoid* line, in the existing format.
- Modify: `docs/unattended.md:134`: `mode` is optional, absent means the agent's preset mode (`acceptEdits` for Claude Code, Codex's own default), and `""` means the agent's own default mode. Also add a short part on Codex's permissions under unattended runs: *Auto review* runs without asking, from the trial.
- Modify: `CLAUDE.md`: one line under *Where the rest lives* pointing to `docs/agents.md`.
- Modify: `docs/proposals/built-in-agents/README.md`: step 2 built.

- [ ] **Step 1: Write the documents.** No code comment cites them.
- [ ] **Step 2: Run** `git diff --check`. Expected: no output.
- [ ] **Step 3: Commit**: `docs(agents): built-in and declared agents, presets, and the unattended mode`

---

### Task 7: Whole-workspace checks

- [ ] **Step 1:** Run `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo test --locked`. Expected: no fmt diff, no warnings, every test passes.
- [ ] **Step 2:** Run `cargo tree -p onehand-core -i gpui`. Expected: "did not match any packages".
- [ ] **Step 3:** Run a real smoke test on the machine's Codex through the new launch. Run `cargo run -p onehand-core --example acp_smoke`, with `ACP_CMD` unset, after pointing the example at `AgentSpec { preset: Some(Codex), .. }.launch(..)` when `ACP_PRESET=codex` is set. Expected: `== CONNECTED`, a reply, `TURN END`. Commit the example change with this task.
- [ ] **Step 4: Commit** any fmt change as `style: make fmt over the step 2 changes`.
