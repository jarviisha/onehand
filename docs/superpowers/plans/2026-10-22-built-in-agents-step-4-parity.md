# Built-in agents, step 4: parity through presets — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining gaps between Codex and Claude Code:
- an agent that is not logged in is told so with the command that fixes it, for that entry's account;
- Codex's question card keeps its free-text note with the question it belongs to;
- the Plugins mode shows only where a Claude Code entry exists.

**Architecture:**
- **Login.**
  - The ACP client reads `-32000` (`auth_required`) as `AcpEvent::LoginRequired`, on both `session/new` and `session/prompt`.
  - `Chat` carries the entry's login hint, which core computes from the spec as `AgentSpec::login_hint`. It says that hint once, in place of the error that follows.
- **Question card.** The elicitation parser folds a property whose `_meta.codex` names it as another question's `user_note` into that question as its "Other" box. This is data-driven, exactly as Claude's `<key>_custom` is.
- **Plugins mode.** It is composed per window only when an entry is Claude Code.

**Tech Stack:** Rust, serde_json, tokio (client tests), GPUI app.

**Spec:** `docs/superpowers/specs/2026-10-21-built-in-agents-design.md`: decision 5, decision 6 items 4 and 6, *Order of work* step 4.

## Global Constraints

- CLAUDE.md binds every task. In particular:
  - our event enums are matched exhaustively, with no wildcard arm;
  - nothing in core is `pub` unless named outside it;
  - comments cite no document;
  - run `make fmt`;
  - core stays GUI-free.
- **Login words**, verbatim:
  - a preset entry: `` <entry name> is not logged in. Run `<command>` in a terminal, then restart the agent ``;
  - a declared entry: `<entry name> needs you to log in`.
- **Login commands:** Claude Code `claude auth login`, Codex `codex login`.
- **Account variables:** Claude Code `CLAUDE_CONFIG_DIR`, Codex `CODEX_HOME`.
- **The account variable in the command.** When the entry's `env` sets its preset's account variable, the command is prefixed with `VAR=value`, the value home-expanded as `launch` does it. Example: `` `CLAUDE_CONFIG_DIR=/home/me/.claude-acc2 claude auth login` ``.
- **Codex's note field**, from the 2026-10-22 trial: property `<key>_note`, carrying `_meta.codex.questionId = "<key>"` and `_meta.codex.role = "user_note"`. Answering with the note alone, or with the choice and the note, reached Codex either way.
- **The proposal is now built.** `docs/proposals/built-in-agents/` leaves in this PR, and its content now lives in `docs/agents.md`.

## Review Focus

1. **A `-32000` on a later prompt says the hint once**, with no second "The turn failed: Authentication required". → Task 2 test `a_login_required_turn_is_said_once_as_the_hint`.
2. **The same on `session/new`**: the hint, not "Disconnected: Could not start a session: Authentication required". The link still ends up lost. → Task 2 test `a_login_required_start_is_said_once_and_the_link_is_lost`.
3. **Any other error code is said as today.** → Task 2 test `another_error_is_not_read_as_login`.
4. **A note property that names no existing question stays a field of its own**, so it is not lost. → Task 3 test `a_note_naming_no_question_stays_a_field`.
5. **An entry whose env sets an unrelated variable** gets the bare command. → Task 1 test `the_login_hint_names_the_entrys_account`.

---

### Task 1: Core — the login hint

**Files:**
- Modify: `crates/core/src/config/agent.rs` (`Preset::login`, private `Preset::account_var`, `AgentSpec::login_hint`)
- Test: `crates/core/src/config/agent/tests.rs`

**Interfaces:**
- Produces: `impl AgentSpec { pub fn login_hint(&self) -> String }`. It uses `claude_or_preset`, so an old Claude default left declared gets Claude's command.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_login_hint_names_the_entrys_account() {
    let codex = AgentSpec { name: "Codex".into(), preset: Some(Preset::Codex), ..Default::default() };
    assert_eq!(codex.login_hint(), "Codex is not logged in. Run `codex login` in a terminal, then restart the agent");
    let acc2 = AgentSpec {
        name: "Claude Code (acc2)".into(),
        preset: Some(Preset::ClaudeCode),
        env: [("CLAUDE_CONFIG_DIR".into(), "/x/.claude-acc2".into()), ("OTHER".into(), "1".into())].into(),
        ..Default::default()
    };
    assert_eq!(acc2.login_hint(), "Claude Code (acc2) is not logged in. Run `CLAUDE_CONFIG_DIR=/x/.claude-acc2 claude auth login` in a terminal, then restart the agent");
    let gemini = AgentSpec { name: "Gemini".into(), command: "gemini".into(), ..Default::default() };
    assert_eq!(gemini.login_hint(), "Gemini needs you to log in");
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core the_login_hint`. Expected: compile error.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p onehand-core config`. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(agents): the words and command that log an entry's account in`

### Task 2: Login required, said once

**Files:**
- Modify: `crates/core/src/acp/types.rs`: `AcpEvent::LoginRequired`.
- Modify: `crates/core/src/acp/client.rs`: a `-32000` error emits `LoginRequired` before its existing failure, on both `NewSession` and `PromptTurn`.
- Modify: `crates/core/src/chat/model.rs` and `apply.rs`:
  - `pub login_hint: String` on `Chat`;
  - a private `login_said: bool`;
  - the `LoginRequired` arm pushes `ChatItem::failure(login_hint, Error, None)` and sets `login_said`;
  - the next `Error` or `Disconnected` arm does its state work but pushes no line while `login_said` is set, then clears it.
- Modify: `crates/app/src/chat/session.rs`: `spawn` sets `chat.login_hint = spec.login_hint()`.
- Modify: every exhaustive match on `AcpEvent` that the compiler names.
- Test: `crates/core/src/acp/client/tests.rs`, `crates/core/src/chat/model/tests.rs`.

**Interfaces:**
- Produces: `AcpEvent::LoginRequired` (unit) and `Chat.login_hint: String`. An empty hint falls back to `"<agent> needs you to log in"`.

- [ ] **Step 1: Failing tests**

The client tests use the scripted `Agent`:
- `a_login_required_answer_is_read_as_such`: the `session/new` error `{code:-32000}` gives `LoginRequired` before `Disconnected`.
- `another_error_is_not_read_as_login`: `{code:-32603}` gives no `LoginRequired`.

The chat tests:

```rust
#[test]
fn a_login_required_turn_is_said_once_as_the_hint() {
    let mut chat = Chat::default();
    chat.login_hint = "Codex is not logged in. Run `codex login` in a terminal, then restart the agent".into();
    chat.busy = true;
    chat.apply(AcpEvent::LoginRequired);
    chat.apply(AcpEvent::Error { text: "The turn failed: Authentication required".into(), detail: None });
    let texts: Vec<_> = chat.items.iter().filter_map(|i| match i { ChatItem::Notice { text, .. } => Some(text.clone()), _ => None }).collect();
    assert_eq!(texts, [chat.login_hint.clone()]);
}

#[test]
fn a_login_required_start_is_said_once_and_the_link_is_lost() {
    let mut chat = Chat::default();
    chat.login_hint = "hint".into();
    chat.apply(AcpEvent::LoginRequired);
    chat.apply(AcpEvent::Disconnected("Could not start a session: Authentication required".into()));
    assert_eq!(chat.link, Link::Lost);
    assert_eq!(chat.items.iter().filter(|i| matches!(i, ChatItem::Notice { .. })).count(), 1);
}
```

Adjust the `ChatItem::Notice` pattern to the variant's real fields.

- [ ] **Step 2: Run** `cargo test -p onehand-core login`. Expected: compile errors.
- [ ] **Step 3: Implement.** Match `AcpEvent` exhaustively everywhere the compiler asks.
- [ ] **Step 4: Run** `cargo test -p onehand-core && cargo test -p onehand`. Expected: PASS, the guards included.
- [ ] **Step 5: Commit**: `feat(chat): an agent that is not logged in says the command that logs it in`

### Task 3: Codex's note joins its question

**Files:**
- Modify: `crates/core/src/acp/client.rs` (`parse_elicitation`)
- Test: `crates/core/src/acp/client/tests.rs`

**Interfaces:**
- The custom key of a field `k` is `<k>_custom` when present. Otherwise it is the property whose `_meta.codex.questionId == k` and `_meta.codex.role == "user_note"`. A property used as a custom key is not a field of its own.

- [ ] **Step 1: Failing tests**
  - `codex_note_is_its_questions_other_box`: use the trial's schema (`add_change` and `add_change_note` as captured). Expect exactly one field `add_change`, with `custom_key == Some("add_change_note")`.
  - `a_note_naming_no_question_stays_a_field`: a `x_note` whose `questionId` is `"missing"`. Expect it to stay a field.
- [ ] **Step 2: Run** `cargo test -p onehand-core codex_note`. Expected: FAIL (two fields).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p onehand-core acp::client`. Expected: PASS.
- [ ] **Step 5: Commit**: `fix(acp): Codex's note on a question is that question's Other box`

### Task 4: The Plugins mode follows Claude Code

**Files:**
- Modify: `crates/app/src/features.rs` (`workbench_modes`: the Plugins mode only when `Shared.agents` holds an entry with `claude_or_preset() == Some(ClaudeCode)`)
- Modify: `crates/core/src/config/agent.rs` (`claude_or_preset` becomes `pub`, since the app names it)

The modes are built once per window. A Claude entry added later shows Plugins in the next window. This is a known gap; record it in `docs/known-gaps.md`.

- [ ] **Step 1: Implement.** It has no unit test: composition happens in GPUI. Record that as a ruling.
- [ ] **Step 2: Run** `cargo test -p onehand && make clippy CLIPPY_EXTRA="-- -D warnings"`. Expected: PASS.
- [ ] **Step 3: Commit**: `feat(workbench): the Plugins mode shows where a Claude Code agent is set up`

### Task 5: Documents, the proposal leaves, checks

**Files:**
- Modify: `docs/agents.md`:
  - a *Not logged in* part with the words, the two commands and the account prefix;
  - the question card's note;
  - Plugins showing only with a Claude entry.
- Modify: `docs/known-gaps.md`: Plugins appears in a window made after a Claude entry is added.
- Delete: `docs/proposals/built-in-agents/`.
- Modify: `docs/superpowers/specs/2026-10-21-built-in-agents-design.md`: its *Source* line says the proposal was built and removed, with no link.
- [ ] **Step 1:** Write the documents. Run `git diff --check` and grep for any remaining link to `docs/proposals/built-in-agents`.
- [ ] **Step 2:** Run `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo test --locked && cargo tree -p onehand-core -i gpui`. Expected: all clean, and the last command errors with "did not match".
- [ ] **Step 3:** Smoke test the login hint for real. Run `CLAUDE_CONFIG_DIR=/tmp/empty ACP_PRESET=claude-code cargo run -p onehand-core --example acp_smoke`. Expected: a `LoginRequired` line in its output; add one print arm for it to the example.
- [ ] **Step 4: Commit**: `docs(agents): logging in, the question note and Plugins; the proposal is built`
