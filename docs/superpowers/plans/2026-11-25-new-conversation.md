# New conversation in the same session: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a session on the rail can start a fresh conversation in place, from the title menu or by
typing `/clear`.

**Architecture:** one pane method, `ChatPane::new_conversation`, takes the resume picker's
fresh-start path from a live session. The title menu and the composer both reach it. Core owns the
rule for when `/clear` is onehand's command and when it belongs to the agent.

**Tech Stack:** Rust, GPUI, gpui-component.

**Spec:** `docs/superpowers/specs/2026-11-25-new-conversation-design.md`

## Global Constraints

- When the session's agent advertises a `clear` of its own, the agent's wins and the text is sent
  as before.
- The command works the same way as *Resume in this session…*: disabled mid-turn, refused while
  the session leads workers (`may_leave_conversation`), and the old conversation's name and
  settings written (`save_meta`) before it is left.
- The remote chat (Telegram) is unchanged.
- No colour, size or radius is hard-coded. Every new `match` on our own enums is exhaustive, with no
  wildcard arm (`crates/app/src/guards.rs`).
- Before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"` and
  `make fmt`.

## Review Focus

- **The old conversation's name leaks into the new one.** `connect_as` copies `custom_title` and
  `stand_in_title` from whatever session is live. `new_conversation` must `disconnect()` first, so
  the new chat starts without a name. Pinned by Task 2's test.
- **`/clear` while a turn runs** must neither be queued as a prompt nor drop the turn. It is
  refused with a toast, and the text stays in the composer. Pinned by Task 3's test.
- **`/clear` with trailing words** (`/clear now`) is not onehand's command and is sent as typed.
  Pinned by Task 1's test.
- **A worker session** has no lead guard, and a fresh conversation for it is allowed, the same as
  the picker allows it today. No new test.
- **A project page** has no session, so its composer offers no row and sends `/clear` as text.
  Out of scope, no new test.

---

### Task 1: Core rule — is this text onehand's own command

**Files:**
- Modify: `crates/core/src/completion.rs` (beside `commands`)
- Test: that module's existing `#[cfg(test)]` tests

**Interfaces:**
- Produces:
  - `pub enum OwnCommand { NewConversation }`
  - `pub fn own_command(text: &str, advertised: &[crate::acp::SlashCommand]) -> Option<OwnCommand>`
  - `pub fn offers_clear(advertised: &[crate::acp::SlashCommand]) -> bool`
  - `pub const CLEAR: &str = "clear";`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn clear_alone_is_ours_unless_the_agent_has_one() {
    let none: Vec<SlashCommand> = vec![];
    let theirs = vec![SlashCommand { name: "clear".into(), description: String::new() }];
    assert_eq!(own_command("/clear", &none), Some(OwnCommand::NewConversation));
    assert_eq!(own_command("  /clear \n", &none), Some(OwnCommand::NewConversation));
    assert_eq!(own_command("/clear now", &none), None);
    assert_eq!(own_command("clear", &none), None);
    assert_eq!(own_command("/clear", &theirs), None);
    assert!(offers_clear(&none));
    assert!(!offers_clear(&theirs));
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core clear_alone_is_ours`. Expected: FAIL (not defined).
- [ ] **Step 3: Implement.** `offers_clear` checks that no advertised name equals `CLEAR`.
  `own_command` trims the text and matches it exactly against `/clear`. Doc comments give the
  reason (the Claude Code adapter does not advertise `clear`) in their own words, without citing
  a document.
- [ ] **Step 4: Run** the same test. Expected: PASS. Also confirm `cargo test -p onehand-core` passes.
- [ ] **Step 5: Commit** `feat(chat): say when /clear is onehand's own command`.

### Task 2: Pane — start a new conversation in place, and the title menu item

**Files:**
- Modify: `crates/app/src/chat/pane/history.rs` (new method beside `show_history`)
- Modify: `crates/app/src/chat/pane/header.rs` (menu item after *Resume in this session…*)
- Test: `crates/app/src/chat/pane/tests.rs`

**Interfaces:**
- Produces: `pub(super) fn new_conversation(&mut self, cx: &mut Context<Self>) -> bool` (true when started), acting on
  `self.active`.

- [ ] **Step 1: Write the failing test**

```rust
/// A new conversation replaces the live session with a fresh one, which
/// carries no name from the old.
#[gpui::test]
fn a_new_conversation_replaces_the_session_without_its_name(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);
    open(&pane, 1, cx);
    let old = session(&pane, 1, cx).expect("a session");
    old.update(cx, |s, _| s.chat_mut_for_test().custom_title = Some("old".into()));
    pane.update(cx, |pane, cx| pane.new_conversation(cx));
    cx.run_until_parked();
    let new = session(&pane, 1, cx).expect("a session");
    assert_ne!(old.entity_id(), new.entity_id());
    assert!(old.upgrade().is_none(), "the old adapter is let go");
    assert_eq!(new.read_with(cx, |s, _| s.chat().custom_title.clone()), None);
}
```

  (Use whatever way the existing tests already use to set a title on a session. If none exists,
  assert only the first two, and check the title by reading `connect_as`.)

- [ ] **Step 2: Run** `cargo test -p onehand a_new_conversation_replaces`. Expected: FAIL.
- [ ] **Step 3: Implement `new_conversation`.**
  - Return when nothing is active.
  - On `crate::control::drive().may_leave_conversation(uid)`'s `Err`, emit
    `ChatPaneEvent::ResumeRefused(resume_refused(why))`.
  - When `chat().busy`, emit `ChatPaneEvent::ResumeRefused("Wait for the turn to end before
    starting a new conversation".into())`.
  - Otherwise call `save_meta` on the live session, call `disconnect()` on the conversation, then
    `self.start(uid, None, cx)` and `cx.notify()`, and return `true` (`false` on every refusal).
- [ ] **Step 4: Add the title menu item.** In `header.rs`, add *New conversation* after *Resume in
  this session…*, disabled while `busy` in the same way (`menu_still` + `.disabled(busy)`). Its
  click calls `pane.new_conversation(cx)`.
- [ ] **Step 5: Run** the test and `cargo test -p onehand chat::pane`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(chat): start a new conversation in the same session`.

### Task 3: Composer — `/clear` in the list and when sent

**Files:**
- Modify: `crates/app/src/chat/composer/presentation.rs` (`Act::NewConversation`)
- Modify: `crates/app/src/chat/composer/complete.rs` (`act_rows` row and pick arm)
- Modify: `crates/app/src/chat/composer.rs` (`ComposerEvent::NewConversation`)
- Modify: `crates/app/src/chat/pane.rs` (event arm → `new_conversation`)
- Modify: `crates/app/src/chat/pane/turns.rs` (`submit` intercept)
- Test: `crates/app/src/chat/pane/tests.rs` (and the composer's tests, if a row test lives there)

**Interfaces:**
- Consumes: `onehand_core::completion::{own_command, offers_clear, OwnCommand, CLEAR}` (Task 1),
  `ChatPane::new_conversation` (Task 2).
- Produces: `Act::NewConversation`, `ComposerEvent::NewConversation`.

- [ ] **Step 1: Write the failing test.** Type `/clear` into the pane's composer and call
  `pane.submit(...)`. Assert that the session entity changed, no prompt was sent
  (`chat().prompts_sent == 0` on the new one) and the composer is empty. Second case: with the
  old chat `busy`, `/clear` leaves the same session, sends nothing and keeps the text in the
  composer.
- [ ] **Step 2: Run** the test. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **`act_rows`:** a row `(CLEAR, "Start a new conversation in this session",
    Act::NewConversation)`, offered when `offers_clear(&chat.commands)` and the chat is not busy.
  - **Pick arm:** `(None, Act::NewConversation)` emits `ComposerEvent::NewConversation`.
  - **`Act::opens`:** returns `None` for `NewConversation`. Add the variant to every exhaustive
    match on `Act`.
  - **`pane.rs`:** `ComposerEvent::NewConversation => pane.new_conversation(cx)`.
  - **`ChatPane::submit`:** before `submit_from`, when `own_command(text.trim(), &chat.commands)`
    is `Some(NewConversation)` and nothing is staged, clear the composer and call
    `new_conversation`, clearing the composer only when it returns `true`.
- [ ] **Step 4: Run** the test and `cargo test -p onehand`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(composer): /clear starts a new conversation`.

### Task 4: Docs, checks, cleanup

- [ ] **Step 1:** in `docs/screens.md`'s *Agent pane header*, add *New conversation*, refused
  mid-turn, after *Resume in this session…*. Where the composer's `/` list is described there (or in
  `docs/ui-patterns.md`), add the `clear` row and the rule that the agent's own `clear` wins.
- [ ] **Step 2:** delete this plan and the spec (this is the work's only PR).
- [ ] **Step 3:** run `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"` and
  `cargo test --workspace --locked`. Expected: all pass.
- [ ] **Step 4: Commit** `docs(screens): new conversation in the session's menu and /clear`.
