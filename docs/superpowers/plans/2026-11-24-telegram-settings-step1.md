# Telegram in Settings, step 1: the bridge knows its own state

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the bridge's state (off, no token, connecting and why it is retrying, connected as
which bot, stopped and why), the chats it ignored with who sent them, and a start that no
longer needs a chat on the list, all held in `RemoteDrive` and carried out by the app. No page
yet: step 2 draws it and adds the settings that change it.

**Architecture:** the Telegram crate reports two more things (a retry's reason, and who sent
each update). `RemoteDrive` turns the channel's events into a `BridgeState`, keeps an ignored
list, and asks the app to look the token up (`Effect::Start`) and start a channel with it
(`Effect::Connect`). The app's carrier (`crates/app/src/remote.rs`) does the reading off the UI
thread, and replaces its stderr-only reports with those states.

**Tech Stack:** Rust, GPUI (app), tokio/reqwest (Telegram crate), no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-24-telegram-settings-design.md`

## Global Constraints

- Core stays GUI-free and runtime-free; nothing in core is `pub` unless named outside it.
- The token is never in `onehand.toml`, never in a log line, never on screen.
- A chat off the allowed list gets no answer and no acknowledgement, as now.
- The label (who sent an update) is shown, never used to decide who is allowed.
- `IGNORED_CAP` = 5, newest first, deduplicated by (chat, from).
- Match our own enums exhaustively (the guards in `crates/app/src/guards.rs` check it).
- Code describes, never cites a document.

## Review Focus

1. Switching off while the token read is still in flight: the late `Looked` must start nothing.
   Test: `a_look_that_lands_after_off_starts_nothing` (Task 2).
2. A poll that fails after the handshake, then recovers: state goes back to `Connected`, not
   stuck on *retrying*. Test: `connected_again_clears_the_retry` (Task 2); the Telegram crate
   re-sends `Connected` after a recovered poll (Task 1).
3. The same stranger writing ten times fills one row, not five. Test:
   `one_stranger_is_one_row` (Task 2).
4. A stopped channel's pump keeps no events flowing into a new channel's drive: the old pump
   is dropped with the old channel. Test: `a_restart_drops_the_old_pump` (Task 3).
5. A token with surrounding whitespace or a trailing newline in the file still starts
   (already `config::secret`'s rule; `token_source` must reuse it, not re-read raw).

---

### Task 1: the Telegram crate says who wrote and why it is retrying

**Files:**
- Modify: `crates/core/src/remote/types.rs` (`RemoteEvent`)
- Modify: `crates/telegram/src/telegram.rs` (`greet`, `poll_forever`, wire shapes,
  `into_event`, tests)
- Modify: `crates/telegram/src/secret.rs`
- Modify: every constructor of the changed variants (`crates/app/src/remote.rs`,
  `crates/app/src/remote/tests.rs`, `crates/core/src/remote/drive/tests.rs`)

**Interfaces:**
- Produces, in `onehand_core::remote::types`:
  - `RemoteEvent::Message { chat, from, who: String, text }`,
    `Unreadable { chat, from, who: String }`,
    `Pressed { chat, from, who: String, press_id, data }`; `who` empty when Telegram gave
    nothing.
  - `RemoteEvent::Retrying(String)`: a non-fatal failure's reason, token redacted.
  - `pub enum TokenSource { Env(String), File(std::path::PathBuf), Missing }`
    (`Debug, Clone, PartialEq, Eq`): where a channel's credential was found. `Env` holds the
    variable's name, never its value.
- Produces, in `onehand_telegram::secret`:
  `pub fn token_source(cfg: &TelegramConfig) -> (TokenSource, Option<String>)`, built on
  `onehand_core::config::secret` so trimming and the empty-is-absent rule stay one rule.
  `telegram_token` goes if nothing else calls it.

- [ ] **Step 1: Write the failing tests** in `telegram.rs`'s test module:

```rust
#[test]
fn a_message_says_who_wrote_it() {
    let event = parse_update(r#"{"update_id":1,"message":{"chat":{"id":42},
        "from":{"id":42,"first_name":"Minh","last_name":"Tran","username":"minhtran"},
        "text":"hi"}}"#);
    assert!(matches!(event, Some(RemoteEvent::Message { who, .. }) if who == "Minh Tran (@minhtran)"));
}

#[test]
fn a_group_message_names_the_group_too() {
    let event = parse_update(r#"{"update_id":1,"message":{"chat":{"id":-100,"title":"Ops"},
        "from":{"id":5,"first_name":"Minh"},"text":"hi"}}"#);
    assert!(matches!(event, Some(RemoteEvent::Message { who, .. }) if who == "Minh in Ops"));
}

#[test]
fn a_press_with_no_presser_has_no_name() {
    // A press's `who` is the presser's, empty when absent.
}
```

Also check `token_source` in `secret.rs`: an env var named by `token_env` that is set to
whitespace falls through to the file; a file holding `"tok\n"` gives `(File(path), Some("tok"))`.
Run the env case with a variable name unique to the test.

- [ ] **Step 2: Run** `cargo test -p onehand-telegram --locked`. Expected: FAIL to compile on `who`.

- [ ] **Step 3: Implement.**
  - Wire shapes: `TgChat` gains `title: Option<String>`; a new `TgUser { id, first_name:
    Option<String>, last_name: Option<String>, username: Option<String> }` replaces `TgChat`
    for every `from`. All name fields optional so one odd update cannot fail its batch.
  - `who`: `"First Last (@user)"`, dropping the parts absent; `" in <title>"` appended when the
    chat has a title and differs from the sender.
  - `greet(client, bot, out)`: every transient failure sends `RemoteEvent::Retrying(reason)`
    (keep the `eprintln!` beside it). A failed send of that event is ignored: it is only news.
  - `poll_forever(client, bot, name, out)`: a transient failure sends `Retrying`; the first
    successful poll after one or more failures sends `Connected { name }` again.
  - Every reason passes through `Telegram::redact` as now.

- [ ] **Step 4: Run** `cargo test -p onehand-telegram --locked`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(remote): the Telegram channel says who wrote and why it is retrying`.

---

### Task 2: the drive holds the bridge's state and the chats it ignored

**Files:**
- Modify: `crates/core/src/remote/drive.rs`, `crates/core/src/remote/mod.rs` (re-exports)
- Test: `crates/core/src/remote/drive/tests.rs`

**Interfaces:**
- Consumes: Task 1's `RemoteEvent` fields and `TokenSource`.
- Produces, in `onehand_core::remote` (re-exported from `drive`):
  - `pub enum BridgeState { Off, NoToken, Connecting { retrying: Option<String> },
    Connected { name: String }, Stopped { why: String } }` (`Debug, Clone, PartialEq, Eq`).
  - `pub struct Ignored { pub chat: ChatId, pub from: ChatId, pub who: String }`.
  - `pub const IGNORED_CAP: usize = 5;`
  - `RemoteDrive::new(allowed: Vec<ChatId>, on: bool) -> Self` (state `Off` until `start`);
    `RemoteDrive::off()` stays, as `new(Vec::new(), false)`.
  - `pub fn start(&mut self) -> Vec<Effect>`: on → state `Connecting { retrying: None }`,
    ignored cleared, `vec![Effect::Start]`; off → state `Off`, nothing.
  - `pub fn state(&self) -> &BridgeState`, `pub fn source(&self) -> Option<&TokenSource>`,
    `pub fn ignored(&self) -> &[Ignored]`.
  - `Input::Message`/`Unreadable`/`Pressed` gain `who: String`.
  - New inputs: `Input::Looked { source: TokenSource, token: Option<String> }`,
    `Input::Connected { name: String }`, `Input::Retrying(String)`;
    `Input::Disconnected` becomes `Input::Disconnected(String)`.
  - New effects: `Effect::Start` (look the token up, off the UI thread, and answer
    `Input::Looked`) and `Effect::Connect(String)` (start a channel with this token, replacing
    any running one). `Effect::ShutDown` stays.
  - `live` stops being a field: a bridge is live while its state is `Connecting` or
    `Connected`, which is what `announce` reads.

- [ ] **Step 1: Write the failing tests** (names and what each asserts):
  - `a_bridge_switched_off_starts_nothing`: `new(vec![], false).start()` is empty, state `Off`.
  - `a_bridge_on_looks_for_its_token`: `new(vec![], true).start()` is `[Start]`, state
    `Connecting { retrying: None }`.
  - `no_token_is_said_and_nothing_starts`: after `start`, `Looked { Missing, None }` gives no
    effect and state `NoToken`; `source()` is `Some(&Missing)`.
  - `a_token_found_connects`: `Looked { File(p), Some("t") }` gives `[Connect("t")]`.
  - `a_look_that_lands_after_off_starts_nothing`: drive built with `on = false`, `Looked` with
    a token gives nothing and state stays `Off`.
  - `an_empty_list_still_connects`: `new(vec![], true)`, start, `Looked` with a token gives
    `[Connect(..)]`.
  - `connected_names_the_bot` / `connected_again_clears_the_retry`: `Retrying("x")` →
    `Connecting { retrying: Some("x") }`; then `Connected { name: "@b" }` → `Connected`.
  - `a_refusal_stops_with_its_reason`: `Disconnected("token refused")` → `[ShutDown]`, state
    `Stopped { why: "token refused" }`, away cleared (the existing away test, updated).
  - `a_stranger_is_remembered_and_not_answered`: a message from chat `"9"` on a list holding
    `"7"` gives no effect, and `ignored()` is `[Ignored { chat: "9", from: "9", who: "Minh" }]`.
    A press and an unreadable from a stranger are remembered the same way.
  - `one_stranger_is_one_row`: the same (chat, from) ten times is one row, moved to the front.
  - `the_ignored_list_keeps_the_newest_five`: seven strangers leave the last five, newest first.
  - `a_restart_forgets_who_was_ignored`: `start()` empties it.
  - Update existing tests and the `sent`/`shown` helpers for the new variants and fields.

- [ ] **Step 2: Run** `cargo test -p onehand-core --locked remote`. Expected: FAIL to compile.

- [ ] **Step 3: Implement** in `drive.rs`. The refusal arm (`Input::Message { .. } |
  Input::Unreadable { .. } | Input::Pressed { .. } => Vec::new()`) becomes the one place a
  stranger is remembered. If `drive.rs` passes ~800 lines, move `BridgeState`, `Ignored` and
  their handling to `drive/bridge.rs`. Update the module header: the drive now also owns what
  the bridge is doing and who it ignored.

- [ ] **Step 4: Run** `cargo test -p onehand-core --locked remote`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(remote): the bridge's drive knows its state and whom it ignored`.

---

### Task 3: the app's carrier starts the channel from the drive

**Files:**
- Modify: `crates/app/src/remote.rs` (`boot`, `receive`, `carry`, `RemoteBridge`)
- Test: `crates/app/src/remote/tests.rs`

**Interfaces:**
- Consumes: Task 2's `start`, `Effect::Start`, `Effect::Connect`, `Input::Looked`, the new
  inputs; Task 1's `token_source`.
- Produces: `RemoteBridge { pub drive, channel: Option<Channel>, pump: Option<Task<()>> }`
  (the pump moves from `Shared::_remote_pump` onto the bridge, so one place drops both).
  `pub(crate) fn boot(cfg: &RemoteConfig, cx: &mut App)` keeps its signature; step 2 adds
  the restart entry points the page needs.

- [ ] **Step 1: Write the failing tests:**
  - `a_refusal_stops_the_channel_and_says_why`: the existing Disconnected test, now also
    asserting `drive.state()` is `Stopped { why: "token refused" }`.
  - `a_restart_drops_the_old_pump`: start a bridge on a `Fake` channel; carry
    `[Effect::Connect("t")]` with the fake factory again; the first channel's request side is
    closed (the old `Fake` sees its receiver end) and `pump` is the new one.
  - The test `bridge()` helper builds `RemoteDrive::new(vec!["7".into()], true)` and sets
    state through `Input::Connected` so `announce` still speaks.

  To test `Connect` without the network, `carry` reaches the channel factory through
  `crate::features::remote_channel` as `boot` does today; the test registers a `Fake` the same
  way the existing helper starts one. If the factory cannot be swapped in a test, take the
  factory as a parameter of a private `connect(token, factory, cx)` and test that.

- [ ] **Step 2: Run** `cargo test -p onehand --locked remote`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - `boot`: builds `RemoteDrive::new(cfg.telegram.allowed_chats.clone(), cfg.telegram.enabled)`,
    stores it, and carries `drive.start()`. The early returns for an empty list and a missing
    token go: the first is no longer a stop, the second is `NoToken`. The missing-channel and
    could-not-start cases become `Input::Disconnected(why)` fed to the drive.
  - `Effect::Start`: on the background executor, `secret::token_source(&cfg.telegram)` (the
    config read from `Shared` at the press, cloned in); back on the UI thread, feed
    `Input::Looked { source, token }`.
  - `Effect::Connect(token)`: take and `shut_down` any running channel and drop its pump, then
    start the factory's channel and a new pump, as `boot` does now.
  - `Effect::ShutDown`: as now, and drop the pump too.
  - `receive`: `Connected` and `Retrying` become inputs (the `eprintln!` for each may stay);
    `Disconnected(why)` carries `why`; the `who` fields pass through.
  - `cx.refresh_windows()` after a state change, so a page reading it redraws.

- [ ] **Step 4: Run** `cargo test -p onehand --locked`. Expected: PASS.

- [ ] **Step 5: Whole checks:** `cargo test --workspace --locked`,
  `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, and
  `cargo tree -p onehand-core -i gpui-pre` erroring with "did not match any packages".

- [ ] **Step 6: Commit** `feat(remote): the bridge starts from its drive and keeps no chat list requirement`.
