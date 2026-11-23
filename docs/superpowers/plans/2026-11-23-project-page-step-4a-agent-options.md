# Project page, step 4a: what each agent offers, remembered and picked on the page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Onehand keeps, across launches, what each agent last offered (its modes, its config
groups with their labels and the values in force, its `/` commands). A project's page draws the
picked agent's model, effort, ⚡ and mode from it, and what the person picks there is set on
the session before its first prompt goes.

**Architecture:** Core gains `AgentFacts` (modes, config options, commands, as the chat holds
them) and a file of them by agent name, `<config_dir>/agents-seen.toml`, held by the settings'
drive beside its per-process `offers_seen`. A session that connects, or whose offer changes,
tells the drive its facts; the drive writes the file through an effect. The chat gains *start
picks*: values armed before a fresh session connects and set when it does, before a held prompt
goes; one no longer offered is skipped with a notice. The page reads the facts for its agent
chip's agent, keeps its picks in the page draft, and hands them with the send.

**Tech Stack:** Rust; `onehand-core` (config drive, chat model); `onehand` app.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *Remembered agent facts*.
Step 4a of the plan's six (4 split in two). 4b brings `+`, attachments and `/` completion to
the page. *Cards* and *Continue* are later.

## Global Constraints

- The file is not `state.toml` (the spec's guess): that file belongs to the recents' drive and
  every window writes it; a file of its own keeps the two writers apart. Written whole through
  `write_atomic`, read once at start; one that does not parse is kept as `.bad`, as `state.toml`
  is.
- Facts are keyed by the agent's name, as the page's agent chip names agents; the per-process
  `offers_seen` stays keyed by spec.
- Model, effort and fast are found by `onehand_core::acp::roles` as the session composer finds
  them; the page draws only those three and the mode. Other groups are not offered on the page.
- An agent with no facts yet: its chips are one muted *Agent's default* label, no menu.
- A pick the agent no longer offers when the session connects is not sent; the transcript says
  *<group> <value> is no longer offered; the session runs on the agent's default* (an Info
  notice).
- Words: chip menus headed by the group's own name (*Model*, *Effort*, *Mode*); ⚡ as the
  session's (tooltip *Fast*).
- `make fmt`, workspace tests, clippy `-D warnings`; no comment cites a document; files under
  ~800 lines (`chat/pane.rs` and `shell/sessions.rs` are near: put new code in new files).

## Review Focus

1. An agent renamed in Settings: its facts are under the old name and are not shown for the new
   one (falls back to *Agent's default*), never someone else's.
2. Two windows connecting sessions of one agent at once: the file is written whole each time,
   never interleaved.
3. A pick armed for a session that is resumed instead (a Resume through the same path): the
   resume's own prefs win; start picks apply only to `Connected { resumed: false }`.
4. The held first prompt goes *after* the picks' requests on the agent's channel.
5. A corrupt `agents-seen.toml` does not stop the app and is kept as `.bad`.

---

### Task 1: start picks in the chat

**Files:** `crates/core/src/chat/model/meta.rs` (`arm_start_picks`, `apply_start_picks`),
`crates/core/src/chat/model/apply.rs:260-271` (the `Connected` arm), `crates/core/src/chat/drive.rs`
(`SessionDrive::arm_start_picks(&mut self, picks: Vec<(String, String)>)`, group `"mode"` meaning
the mode), tests in `crates/core/src/chat/model/tests.rs`.

- [ ] Failing tests: `start_picks_are_set_when_a_fresh_session_connects` (offers `model` with
  `opus`/`sonnet` current `opus`, mode `default`/`plan`; arm `[("model","sonnet"),("mode","plan")]`;
  apply `Connected { resumed: false }` → requests `SetConfigOption model=sonnet` and
  `SetMode plan`), `a_start_pick_no_longer_offered_is_said_and_skipped` (arm `("model","gone")`
  → no request; a notice naming it), `a_resumed_session_ignores_start_picks`.
- [ ] Implement; `cargo test -p onehand-core chat` → PASS; commit
  `feat(chat): picks armed for a fresh session are set as it connects`.

### Task 2: agent facts, kept across launches

**Files:** Create `crates/core/src/config/agent_facts.rs` (`AgentFacts { modes: Vec<Mode>,
options: Vec<ConfigOption>, commands: Vec<SlashCommand> }`, serde; `FactsFile` load/save at
`config_dir().join("agents-seen.toml")`, `.bad` on a parse failure); `crates/core/src/chat/model/meta.rs`
(`Chat::facts(&self) -> AgentFacts`); `crates/core/src/chat/drive.rs` (`OfferChanged` also when
the commands change); `crates/core/src/config/drive.rs` (`Settings::facts: BTreeMap<String,
AgentFacts>`, read in `SettingsDrive::new`'s caller; `SettingsDrive::saw_facts(&mut self, name:
String, facts: AgentFacts) -> Vec<Effect>` → `Effect::WriteFacts(BTreeMap<..>)` only when they
differ); `crates/app/src/chat/session.rs:649` (the `OfferChanged` arm tells it), the settings
effects' carrier (write on the background executor).

- [ ] Failing tests: `facts_round_trip_through_the_file`, `a_bad_facts_file_is_kept_aside`,
  `saw_facts_writes_only_what_changed`.
- [ ] Implement; tests → PASS; commit `feat(agents): what each agent offered is kept across launches`.

### Task 3: the page's chips

**Files:** Create `crates/app/src/chat/pane/page_options.rs` (the model, effort, ⚡ and mode
chips from `Settings::facts[agent]`, their menus, picks written into the page draft);
`crates/app/src/chat/pane/page_draft.rs` (`PageDraft.picks: Vec<(String, String)>`; picking another
agent clears them; `PageSend.picks`); `crates/app/src/chat/composer/page_card.rs` (a row of
chips after the agent chip); `crates/app/src/shell/page_start.rs` (`arm_start_picks(picks)` on the
session right after `open_unshown`, before `prompt_when_up`).

- [ ] Failing tests: `the_page_offers_what_its_agent_last_said` (facts for the rig agent with a
  model group; the page's model chip lists them and starts on the current),
  `a_page_pick_goes_with_the_send` (pick `sonnet`; `StartFromPage` carries `("model","sonnet")`).
- [ ] Implement; `cargo test -p onehand` → PASS.
- [ ] Docs: `docs/screens.md` (the page's card: the chips), `docs/agents.md` (what is kept of each
  agent and where).
- [ ] Full checks; commit `feat(projects): the page picks the agent's model and mode`.

## After the tasks

Opus review; PR `feat(projects): the page offers what each agent last said`.
