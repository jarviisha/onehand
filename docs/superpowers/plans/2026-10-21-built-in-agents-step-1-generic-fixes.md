# Built-in agents, step 1: generic fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every ACP agent, Codex in particular, gets a permission card that names the command, a status line that tells a question from an approval, effort and fast chips under Codex's group ids, and a refused session start said in words without npm noise.

**Architecture:** Four small changes, three in `onehand-core` (chat model and ACP client) and one in the app's composer presentation. No new types, no config change, no preset. Each task is one commit; the four together are one PR.

**Tech Stack:** Rust, `serde_json`, tokio (ACP client tests), GPUI app crate (composer presentation, pure functions only).

**Spec:** `docs/superpowers/specs/2026-10-21-built-in-agents-design.md` — decision 6 items 1–3 and the *Order of work* step 1.

## Global Constraints

- Core stays GUI-free; nothing here adds a dependency.
- Read CLAUDE.md before starting; its rules apply (no comment cites a document; `make fmt`, never bare `cargo fmt`).
- `crates/core/src/acp/client.rs` is already past the ~800-line split threshold; add only the lines these tasks need, no split in this PR.
- Exact on-screen words, fixed by the spec: `Waiting for your answer…` (question), `Waiting for your approval…` (permission, unchanged), `Could not start a session: <agent's message>`.
- Codex's group ids, fixed by trial: effort `reasoning_effort`, fast `fast-mode`. Claude's stay `effort`, `fast`.

## Review Focus

1. A permission whose `toolCallId` names no tool call seen yet (or has no id) keeps the request's own title. → Task 1 test `permission_without_a_known_call_keeps_its_own_title`.
2. A known tool call whose title is blank must not blank the card. → Task 1 test, same name, second case.
3. A permission and a question parked at once: the status line says approval (the stronger demand). → Task 2 test `a_question_waits_for_an_answer_and_a_permission_for_approval`.
4. An `npm error` block must survive the `notice` filter; only `npm notice` lines go. → Task 4 test `npm_notice_lines_are_not_kept`.
5. A refused `session/new` with no `message` field still says something. → Task 4 relies on `in_words`, which falls back to the raw JSON; covered by its existing behaviour, no new test.

---

### Task 1: The permission card takes the known tool call's title

**Files:**
- Modify: `crates/core/src/chat/model/apply.rs` (the `AcpEvent::Permission(req)` arm, ~line 255)
- Test: `crates/core/src/chat/model/tests.rs` (beside the `permission(title)` helper, ~line 1366)

**Interfaces:**
- Consumes: `PermissionRequest { tool_call_id: Option<String>, title: String, .. }`, `ChatItem::Tool(ToolItem { call: ToolCall { id, title, .. }, .. })`.
- Produces: nothing new; `PermItem::command()` keeps returning `req.title`, which now holds the call's title when one is known.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn permission_takes_the_title_of_the_call_it_names() {
    let mut chat = Chat::default();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "call-1".into(),
        title: "curl -sI https://example.com | head -1".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::Pending,
        content: vec![],
    }));
    let mut req = permission("Run command");
    req.tool_call_id = Some("call-1".into());
    chat.apply(AcpEvent::Permission(req));
    let Some(ChatItem::Permission(p)) = chat.items.last() else { panic!("a card") };
    assert_eq!(p.command(), "curl -sI https://example.com | head -1");
}

#[test]
fn permission_without_a_known_call_keeps_its_own_title() {
    let mut chat = Chat::default();
    let mut req = permission("Run command");
    req.tool_call_id = Some("never-seen".into());
    chat.apply(AcpEvent::Permission(req));
    let Some(ChatItem::Permission(p)) = chat.items.last() else { panic!("a card") };
    assert_eq!(p.command(), "Run command");

    // A known call with a blank title does not blank the card.
    let mut chat = Chat::default();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "call-2".into(),
        title: "  ".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::Pending,
        content: vec![],
    }));
    let mut req = permission("Run command");
    req.tool_call_id = Some("call-2".into());
    chat.apply(AcpEvent::Permission(req));
    let Some(ChatItem::Permission(p)) = chat.items.last() else { panic!("a card") };
    assert_eq!(p.command(), "Run command");
}
```

If `Chat::apply` on a `ToolCall` needs a live turn to keep the item, set `chat.busy = true` first, as neighbouring tests do.

- [ ] **Step 2: Run them to see the first fail**

Run: `cargo test -p onehand-core permission_takes_the_title_of_the_call_it_names permission_without_a_known_call`
Expected: the first FAILS (`"Run command"` ≠ the command); the second passes already.

- [ ] **Step 3: In the `Permission` arm, before pushing the `PermItem`, replace `req.title` with the title of the newest `ChatItem::Tool` in `self.items` whose `call.id` equals `req.tool_call_id`, when that title is not blank after `trim()`**

Search newest first (`iter().rev()`), as `apply_tool_update` does. The comment says why: one adapter puts the command in the request's title, another a generic label, while the call it names always carries the command.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p onehand-core permission_`
Expected: PASS, and the existing permission tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/chat/model/apply.rs crates/core/src/chat/model/tests.rs
git commit -m "fix(chat): a permission card names the command its tool call carries"
```

---

### Task 2: A question waits for an answer, not an approval

**Files:**
- Modify: `crates/core/src/chat/model/turn.rs` (`activity_status`, ~line 791)
- Test: `crates/core/src/chat/model/tests.rs` (beside `connecting_outranks_everything_the_status_could_say`)

**Interfaces:**
- Consumes: `ChatItem::Permission(PermItem { resolved, .. })`, `ChatItem::Ask(AskItem { resolved, .. })`, the test fixtures `ask_item()` and `permission(title)`.
- Produces: nothing new. `awaiting_permission()` keeps its meaning (either card blocks the turn); only the words differ.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_question_waits_for_an_answer_and_a_permission_for_approval() {
    let mut chat = Chat::default();
    chat.link = Link::Connected;
    chat.busy = true;
    chat.items.push(ChatItem::Ask(ask_item())); // the existing fixture, ~line 747
    assert_eq!(chat.activity_status().as_deref(), Some("Waiting for your answer…"));

    chat.items.push(ChatItem::Permission(PermItem {
        req: permission("cargo test"),
        resolved: None,
        expanded: false,
    }));
    assert_eq!(chat.activity_status().as_deref(), Some("Waiting for your approval…"));
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p onehand-core a_question_waits_for_an_answer`
Expected: FAIL on the first assertion (`Waiting for your approval…`).

- [ ] **Step 3: In `activity_status`, replace the single `awaiting_permission()` branch with: an unresolved `ChatItem::Permission` → `Waiting for your approval…`; otherwise an unresolved `ChatItem::Ask` → `Waiting for your answer…`**

Match `ChatItem` exhaustively, with no wildcard arm or partial `if let` over our own enum (`guards.rs` checks it). The wording matches the rail's `Waiting for your answer` (`crates/app/src/rail/session.rs:29`).

- [ ] **Step 4: Run the chat model tests**

Run: `cargo test -p onehand-core chat::model`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/chat/model/turn.rs crates/core/src/chat/model/tests.rs
git commit -m "fix(chat): a parked question says it waits for an answer"
```

---

### Task 3: The effort and fast chips know Codex's group ids

**Files:**
- Modify: `crates/app/src/chat/composer/presentation.rs` (`EFFORT_GROUP` ~152, `CHIP_GROUP` ~201, their users at ~176, 226, 247, 375, 485, 515)
- Test: the same file's `mod tests` (~line 524)

**Interfaces:**
- Consumes: `names(option: &ConfigOption, wanted: &str) -> bool` (id or label, case-insensitive).
- Produces: `const EFFORT_GROUPS: [&str; 2] = ["effort", "reasoning_effort"]`, `const FAST_GROUPS: [&str; 2] = ["fast", "fast-mode"]`, and `fn names_any(option: &ConfigOption, wanted: &[&str]) -> bool`. `chip_group(options, name)` becomes `fast_group(options) -> Option<&ConfigOption>`. They replace `EFFORT_GROUP`/`CHIP_GROUP` everywhere in the file.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn codex_names_its_effort_and_fast_groups_differently() {
    assert!(has_own_chip(&effort("reasoning_effort", "Reasoning effort", &["low", "high"], Some("low"))));
    assert!(names_any(&effort("fast-mode", "Fast mode", &["off", "on"], Some("off")), &FAST_GROUPS));
    assert!(names_any(&effort("effort", "Effort", &[], None), &EFFORT_GROUPS));
    assert!(!names_any(&effort("model", "Model", &[], None), &EFFORT_GROUPS));
}
```

Update the existing tests that import `EFFORT_GROUP`/`CHIP_GROUP` to the new names; their assertions stay the same.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p onehand codex_names_its_effort_and_fast_groups_differently`
Expected: FAIL to compile (`EFFORT_GROUPS` not found).

- [ ] **Step 3: Introduce the two arrays and `names_any`, and route every former `names(_, EFFORT_GROUP)` / `chip_group(_, CHIP_GROUP)` / `names(_, CHIP_GROUP)` through them**

Rewrite the two constants' doc comments in place: the reasoning holds, but it now names the ids the app knows rather than one, and says a group under any other id still lands among the model's rows.

- [ ] **Step 4: Run the composer tests**

Run: `cargo test -p onehand composer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/chat/composer/presentation.rs
git commit -m "feat(chat): the effort and fast chips also read Codex's group ids"
```

---

### Task 4: A refused session start is said in words, without npm's notices

**Files:**
- Modify: `crates/core/src/acp/client.rs` (`Pending::NewSession` error branch ~line 682; `LastWords::remember` ~line 184)
- Test: `crates/core/src/acp/client/tests.rs`

**Interfaces:**
- Consumes: `in_words(err: &Value) -> (String, Option<String>)`, `LastWords::remember(&mut self, line: &str)`, the test harness `client()` / `Agent::heard` / `Agent::say`.
- Produces: the `Disconnected` text for a refused `session/new` begins `Could not start a session: `. The chat pane already prefixes `Disconnected: `; leave that prefix alone.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn a_refused_session_is_said_in_the_agents_words() {
    let (mut agent, mut events) = client();
    let hello = agent.heard().await;
    agent.say(json!({"jsonrpc": "2.0", "id": hello["id"],
        "result": {"protocolVersion": 1, "agentCapabilities": {}}})).await;
    let new = agent.heard().await;
    agent.say(json!({"jsonrpc": "2.0", "id": new["id"],
        "error": {"code": -32000, "message": "Authentication required"}})).await;
    let text = loop {
        match events.recv().await.expect("an event") {
            AcpEvent::Disconnected(text) => break text,
            _ => continue,
        }
    };
    // The scripted transport has no child, so no stderr tail follows.
    assert_eq!(text, "Could not start a session: Authentication required");
}

#[test]
fn npm_notice_lines_are_not_kept() {
    let mut words = LastWords::default();
    for line in [
        "npm notice run npx · @agentclientprotocol/codex-acp@2.2.2",
        "npm error code ETARGET",
        "NPM NOTICE something",
    ] {
        words.remember(line);
    }
    assert_eq!(words.say().as_deref(), Some("npm error code ETARGET"));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p onehand-core a_refused_session_is_said npm_notice_lines_are_not_kept`
Expected: both FAIL (raw JSON in the first; the notice kept in the second).

- [ ] **Step 3: Change the `NewSession` error to `format!("Could not start a session: {}", in_words(err).0)`, and make `remember` skip a line whose trimmed text starts with `npm notice`, ignoring case, without counting it as dropped**

Log the whole error object with `eprintln!` first, as the `PromptTurn` branch does, so the code still reaches the log. Extend `remember`'s doc comment: a notice is npm narrating its own start, never a reason, and keeping it costs a slot a reason needs.

- [ ] **Step 4: Run the ACP client tests**

Run: `cargo test -p onehand-core acp::client`
Expected: PASS, including `a_real_npm_error_block_survives_the_noise_printed_ahead_of_it`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/acp/client.rs crates/core/src/acp/client/tests.rs
git commit -m "fix(acp): a refused session start says why in words and drops npm notices"
```

---

### Task 5: Whole-workspace checks and the spec table

**Files:**
- Modify: `docs/proposals/built-in-agents/README.md` (the *Specs* table: step 1's *Built* cell)

- [ ] **Step 1: Run the CI set**

Run: `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo test`
Expected: no diff from fmt, no clippy warnings, every test passes.

- [ ] **Step 2: Check core is still GUI-free**

Run: `cargo tree -p onehand-core -i gpui`
Expected: error "did not match any packages".

- [ ] **Step 3: Mark step 1 built in the proposal's table** (`| 1 | none: … | yes |`)

- [ ] **Step 4: Commit**

```bash
git add docs/proposals/built-in-agents/README.md
git commit -m "docs(proposals): built-in agents step 1 is built"
```
