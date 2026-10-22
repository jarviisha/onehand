# Built-in agents, step 3: first launch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A person who opens onehand for the first time finds Claude Code and Codex already set up when they are installed. When neither is, they are told how to install them. When one is installed later, Settings ▸ Agents offers to add it.

**Architecture:**
- **Core** gains `found_agents(path)`, the presets whose binary is on `PATH`, and `AppConfig::from_disk(text, path)`. The latter is the one read path behind `load_resolved` and `update_in_place`. It gives a file with no `agents` key, or no file at all, the found agents, then settles.
- **The app** keeps the presets found at boot in `Shared::on_machine`, so Settings never looks on disk when it opens.
- **The agent pane** draws a no-agent state while the list is empty.
- **Settings ▸ Agents** offers each preset on the machine that has no entry yet.

**Tech Stack:** Rust, serde/toml, GPUI + gpui-component, `onehand_ui` builders.

**Spec:** `docs/superpowers/specs/2026-10-21-built-in-agents-design.md`, decision 4 and *Order of work* step 3.

## Global Constraints

- CLAUDE.md binds every task, and its rules apply throughout:
  - nothing in core is `pub` unless something outside the crate names it;
  - comments never cite a document;
  - run `make fmt`;
  - core stays GUI-free;
  - files stay under ~800 lines.
- Tasks 3 and 4 are visible changes. They follow `.claude/skills/ui/SKILL.md` and answer `checklist.md` in the commit body.
- **First launch** means one of these: no config file is read, the file has no `agents` key, or the file fails to parse (in which case it is never written, as today). An `agents = []` written by the person is never rescanned.
- **Found order:** Claude Code first, then Codex. A found entry is a bare preset (`name` is the shown name, no `env`).
- **Install commands, one per preset.** npm for both, since npm is already what runs the adapters:
  - Claude Code: `npm install -g @anthropic-ai/claude-code`
  - Codex: `npm install -g @openai/codex`
- **On-screen words:**
  - the no-agent state's line: `No agent is set up`;
  - each install line: `<shown name>: <install command>`;
  - its button: `Open the agent settings`;
  - the line under the install lines: `Then restart onehand, or add another agent in the settings`;
  - the Settings offer: `<shown name> is on this machine`, with the button `Add`.

## Review Focus

1. **A file that names `agents = []` keeps an empty list**, even with `claude` installed. → Task 1 test `an_empty_list_the_person_wrote_is_kept`.
2. **A file with other sections and no `agents` key gets the found agents, and the rest of the file survives a save.** → Task 1 test `a_file_without_agents_gets_the_found_ones_and_keeps_the_rest`.
3. **The scan writes nothing by itself.** The found list reaches disk only with the person's first save, which every other first-launch default also does. → covered by reading `load_resolved`: it never writes. No new test.
4. **Adding an offered preset when an entry already has its name** numbers it rather than duplicating. → Task 2 test `an_added_preset_takes_a_free_name`.
5. **The no-agent state is drawn whether or not a project exists.** With no agent nothing can start, and that is the first thing to fix. → Task 3, by its placement ahead of `project_home`'s project branch. A UI path, so it is checked by the reviewer and the person.

---

### Task 1: Core — the found agents on first launch

**Files:**
- Modify: `crates/core/src/config/agent.rs` (`found_agents`, `Preset::ALL`)
- Modify: `crates/core/src/config.rs` (`from_disk`; `load_resolved` and `update_in_place` call it)
- Test: `crates/core/src/config/agent/tests.rs`

**Interfaces:**
- Produces:
  - `impl Preset { pub const ALL: [Preset; 2] = [Preset::ClaudeCode, Preset::Codex]; }`
  - `pub fn found_agents(path: Option<&OsStr>) -> Vec<AgentSpec>`, bare preset entries for each preset in `ALL` whose `binary()` `find_command` finds.
  - `impl AppConfig { pub(crate) fn from_disk(text: Option<&str>, path: Option<&OsStr>) -> Result<Self, toml::de::Error> }`:
    - parse `text`, or use `Self::default()` when there is none;
    - if there is no text, or its top-level table has no `agents` key, replace `agents` with `found_agents(path)`;
    - then `settle(path)`.
  - `load_resolved` and `update_in_place` read only through it. The not-found branch and the final fallback pass `None`; a parse error still leaves the file alone.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn no_file_finds_the_installed_agents_claude_first() {
    let dir = bin_dir("first-both", Some("codex"));
    std::fs::write(dir.join("claude"), "").unwrap();
    let cfg = AppConfig::from_disk(None, Some(dir.as_os_str())).unwrap();
    let presets: Vec<_> = cfg.agents.iter().map(|a| a.preset).collect();
    assert_eq!(presets, [Some(Preset::ClaudeCode), Some(Preset::Codex)]);
    assert_eq!(cfg.agents[1].name, "Codex");
}

#[test]
fn no_file_and_no_agent_installed_is_an_empty_list() {
    let empty = bin_dir("first-none", None);
    assert!(AppConfig::from_disk(None, Some(empty.as_os_str())).unwrap().agents.is_empty());
}

#[test]
fn a_file_without_agents_gets_the_found_ones_and_keeps_the_rest() {
    let dir = bin_dir("first-partial", Some("codex"));
    let cfg = AppConfig::from_disk(Some("[font]\nmonospace = \"Test Mono\"\n"), Some(dir.as_os_str())).unwrap();
    assert_eq!(cfg.agents.len(), 1);
    assert_eq!(cfg.agents[0].preset, Some(Preset::Codex));
    assert_eq!(cfg.font.monospace.as_deref(), Some("Test Mono"));
}

#[test]
fn an_empty_list_the_person_wrote_is_kept() {
    let dir = bin_dir("first-kept-empty", Some("claude"));
    let cfg = AppConfig::from_disk(Some("agents = []\n"), Some(dir.as_os_str())).unwrap();
    assert!(cfg.agents.is_empty());
}
```

Keep the existing `settle` tests as they are; they call `settle` directly.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p onehand-core config::agent`
Expected: compile errors for `from_disk` and `found_agents`.

- [ ] **Step 3: Implement**, then route `load_resolved` and `update_in_place` through `from_disk`.
  - Detect the `agents` key with `toml::from_str::<toml::Table>(text)`. A text that does not parse as a table never reaches this, because `parse` failed first.
  - Rewrite `load_resolved`'s doc comment and `config.rs`'s module header. They say a file with no `agents` key keeps the default agents; it now gets the agents found.

- [ ] **Step 4: Run** `cargo test -p onehand-core`. Expected: PASS. The persist test that settles against the machine's `PATH` should still hold. If it now compares against the found list, keep its intent: other settings survive edits.

- [ ] **Step 5: Commit**: `feat(config): a first launch finds Claude Code and Codex on the machine`

---

### Task 2: Core and state — what is on the machine, and adding it

**Files:**
- Modify: `crates/core/src/config/agent.rs` (`Preset::install`, `AgentSpec::of`)
- Modify: `crates/app/src/state.rs` (`Shared::on_machine`)
- Test: `crates/core/src/config/agent/tests.rs`

**Interfaces:**
- Produces:
  - `impl Preset { pub fn install(self) -> &'static str }`, the install command from Global Constraints.
  - `pub fn on_machine(path: Option<&OsStr>) -> Vec<Preset>`: the presets in `ALL` whose binary is found. `found_agents` maps this.
  - `impl AgentSpec { pub fn of(preset: Preset, taken: &[AgentSpec]) -> AgentSpec }`: a bare preset entry, its name the shown name, or the first ` (n)` of it no entry in `taken` has.
  - `Shared.on_machine: Vec<Preset>`, computed in `Shared::from_config` from `std::env::var_os("PATH")`, once.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_added_preset_takes_a_free_name() {
    let taken = vec![
        AgentSpec { name: "Codex".into(), command: "x".into(), ..Default::default() },
        AgentSpec { name: "Codex (2)".into(), command: "x".into(), ..Default::default() },
    ];
    let added = AgentSpec::of(Preset::Codex, &taken);
    assert_eq!((added.name.as_str(), added.preset), ("Codex (3)", Some(Preset::Codex)));
    assert_eq!(AgentSpec::of(Preset::Codex, &[]).name, "Codex");
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core an_added_preset`. Expected: compile error.
- [ ] **Step 3: Implement.** `found_agents` folds into `on_machine` plus `of`, and `name_uniquely` keeps its own numbering. `Shared::from_config` sets `on_machine`.
- [ ] **Step 4: Run** `cargo test -p onehand-core && cargo check -p onehand`. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(agents): what is on the machine, and an entry for it under a free name`

---

### Task 3: The agent pane's no-agent state

Follows the `ui` skill. Peers: `onehand_ui::hint` (the no-project line), and the project page's head actions for the button's size and variant.

**Files:**
- Modify: `crates/app/src/chat/pane/project_page.rs` (`project_home`: an empty agent list draws the no-agent state first)
- Modify: `crates/app/src/chat/pane/events.rs` (`ChatPaneEvent::OpenAgentSettings`)
- Modify: `crates/app/src/shell/boot.rs` (handle it: `show_settings_page(SettingsPage::Agents)`, then `open_settings`)

**Interfaces:**
- Consumes: `Preset::ALL`, `shown_name`, `install` (Task 2).
- Produces: `ChatPaneEvent::OpenAgentSettings`. The shell matches it with no wildcard arm.

**What it draws:** centred in the body, as `hint` is, at the same meta size, one column, `gap_2`:
1. the line `No agent is set up`, in the muted ink;
2. one line per preset, `<shown name>: <install>`, the command in the code face;
3. the line `Then restart onehand, or add another agent in the settings`;
4. a secondary `.short()` button `Open the agent settings` that emits the event.

It is not a page: no head, no boxes. It replaces both the no-project hint and the project page while the list is empty.

- [ ] **Step 1: Implement**, with the event and its handler. There is no unit test: the body is drawn by GPUI and the guards scan it. Record this as a ruling.
- [ ] **Step 2: Run** `cargo test -p onehand --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`. Expected: PASS, the guards included.
- [ ] **Step 3: Commit**: `feat(chat): with no agent set up the pane says how to install one`. Put the ui checklist answers in the body.

---

### Task 4: Settings ▸ Agents offers what is on the machine

Follows the `ui` skill. Peers: the agent rows and the *Default* badge in `settings/agents.rs`.

**Files:**
- Modify: `crates/app/src/settings/agents.rs` (the list section)
- Modify: `crates/app/src/shell/settings_page.rs` (`add_found_agent(preset)`: push `AgentSpec::of(preset, &agents)`, then `persist_agents`)

**What it draws:** after the rows, for each preset in `Shared.on_machine` with no entry whose `preset` is it, a line like this:
- the words `<shown name> is on this machine`, in chrome text;
- an `Add` button (`.secondary().short()`) at the line's end.

These lines are not capped: there are at most two presets.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Run** `cargo test -p onehand --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`. Expected: PASS.
- [ ] **Step 3: Commit**: `feat(settings): a built-in agent on the machine without an entry is offered`, with the ui checklist answers.

---

### Task 5: Documents and checks

**Files:**
- Modify: `docs/agents.md`: a *First launch* section with the scan and its trigger, the no-agent state, the offer in Settings, and the *Default* mark. The mark already exists; the first row is the default.
- Modify: `docs/screens.md`:
  - under *Pages without a session*, what the pane shows with no agent;
  - under Settings, the Agents page's offer line.
- Modify: `docs/ui-patterns.md` *Nothing to show*: one sentence. An empty state that blocks every action may carry, under its line, what fixes it: commands in the code face, and one secondary button to where it is fixed.
- Modify: `docs/proposals/built-in-agents/README.md`: step 3 is built.

- [ ] **Step 1: Write them.** Run `git diff --check`. Expected: no output.
- [ ] **Step 2: Run** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo test --locked && cargo tree -p onehand-core -i gpui`. Expected:
  - no fmt diff;
  - clippy clean;
  - every test passes;
  - "did not match any packages".
- [ ] **Step 3: Commit**: `docs(agents): first launch, the no-agent state and the offer in Settings`
