# Telegram in Settings, step 2: the page and the settings behind it

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a *Telegram* group on Settings ▸ Connections: the switch, the bridge's state with
*Try again*, the token (entered, replaced, removed, written to `telegram.token` 0600), the
allowed chats (added, removed), and the ignored chats with *Allow*, all in force at once.

**Architecture:** the switch and the allowed chats are settings in `SettingsDrive`, shown at
once and written after, as *Outside control* is; what they put in force on the bridge goes
through `RemoteDrive::shown`. The token's write is a `RemoteDrive` effect carried out off the
UI thread by the app, whose outcome comes back as an input. The page reads `RemoteDrive`; its
fields are a `TelegramDraft` on the shell, as `HostDraft` is.

**Tech Stack:** Rust, GPUI, gpui-component (`InputState::masked`), no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-24-telegram-settings-design.md`. Step 1 is merged
(#399): `BridgeState`, `Ignored`, `RemoteDrive::start`, `Effect::{Start, Connect, ShutDown}`.

## Global Constraints

- The token is never in `onehand.toml`, never logged, never read back onto the screen.
- An environment variable still wins over the file; while one holds the token the page has no
  token field and says *Read from $NAME*.
- The allowed list's edits never restart the channel; the switch, a token change and *Try
  again* do.
- Lists capped: allowed chats at `LIST_SHOWN` (the settings pages' cap, 50) with *N more not
  shown*; ignored at `IGNORED_CAP` (5).
- Every button secondary and `short`; fields `tall`; state at rest in the muted ink, a failure
  in the warning ink. Settings holds no primary.
- Copy, verbatim: heading *Telegram*; about *A bot that tells you when an agent needs you, and
  takes your answer back*; switch *Let a Telegram bot reach onehand*; *No chat is allowed yet;
  the bot answers nobody*; *A chat id is a number*; *<id> is already allowed*; *Remove the bot
  token?*; *Ignored: messaged the bot and got no answer*; *Try again*; *Replace…*; *Remove…*;
  *Save*; *Add*; *Allow*.
- The last PR of this work: the spec and both plans leave `docs/superpowers/`.

## Review Focus

1. Switch off while the token look is in flight: the late `Looked` starts nothing. Test
   `off_while_looking_starts_nothing` (Task 1).
2. A failed config write of the switch puts it back, and the bridge follows (off again or on
   again). Test `a_failed_write_puts_the_switch_back` (Task 1).
3. A chat taken off the list loses its `/use` binding and follows, and no announcement reaches
   it. Test `a_chat_taken_off_loses_what_it_followed` (Task 1).
4. *Allow* on a group row adds the sender too, or the group stays refused. Test
   `allowing_a_group_member_lists_both` (Task 1).
5. The token file is owner-only from its first byte, and a write that fails leaves the old file
   whole. Tests `a_secret_is_written_owner_only` and `a_secret_write_replaces_whole` (Task 1).

---

### Task 1: core: the settings, the bridge's half of them, and the token write

**Files:**
- Modify: `crates/core/src/config/drive.rs` (`Setting`, `Value`, `Edit`, two setters)
- Modify: `crates/core/src/config/secret.rs` (writer, remover)
- Modify: `crates/core/src/remote/drive.rs`, `crates/core/src/remote/chats.rs`
- Modify: `crates/core/src/control/link.rs` (exhaustive arms for the new values)
- Test: `crates/core/src/remote/drive/tests.rs`, `crates/core/src/config/drive/tests.rs` (or
  the config drive's existing test module), `secret.rs`'s tests

**Interfaces:**
- Produces in `onehand_core::config::drive`:
  - `Setting::TelegramOn`, `Setting::TelegramChats`;
    `Value::TelegramOn(bool)`, `Value::TelegramChats(Vec<String>)`;
    `Edit::TelegramOn(bool)` (writes `cfg.remote.telegram.enabled`),
    `Edit::TelegramChats(Vec<String>)` (writes `.allowed_chats`). `doing()`: *Saving the
    Telegram switch*, *Saving the allowed chats*.
  - `SettingsDrive::set_telegram_on(&mut self, before: bool, on: bool) -> Vec<Effect>` and
    `set_telegram_chats(&mut self, before: Vec<String>, after: Vec<String>) -> Vec<Effect>`,
    shaped as `set_outside`: nothing when unchanged, else `[Show(after), Write]`.
- Produces in `onehand_core::config` (re-exported from `secret`):
  - `pub fn write_secret(file: &Path, value: &str) -> std::io::Result<()>`: creates the
    folder, writes `value.trim()` plus a newline to `<file>.tmp` opened with mode 0600 (Unix;
    `OpenOptionsExt::mode`) and `create_new` after removing a stale temp, then renames over
    `file`.
  - `pub fn remove_secret(file: &Path) -> std::io::Result<()>`: `NotFound` is `Ok`.
- Produces in `onehand_core::config`: `pub fn chat_refusal(text: &str, allowed: &[String]) ->
  Option<String>`: `None` for an id that parses as `i64` and is not listed; *A chat id is a
  number* for empty or non-numeric; *<id> is already allowed*. Trims first.
- Produces on `RemoteDrive`:
  - `pub fn shown(&mut self, value: &Value) -> Vec<Effect>`: `TelegramOn(on)` →
    `self.switch(on)`; `TelegramChats(list)` → `self.set_allowed(list)`, no effect; other
    values → nothing.
  - `fn switch(&mut self, on: bool) -> Vec<Effect>`: on: `self.on = true; self.start()`. Off:
    `on = false`, `looking = false`, `away = false`, state `Off`, ignored cleared, and
    `[ShutDown]` when it was live, else nothing.
  - `fn set_allowed(&mut self, allowed: Vec<ChatId>)`: through `Chats::set_allowed`, which
    drops `pointed` and `followed` for chats no longer listed; ignored rows that are now
    allowed (`allows(chat, from)`) leave the list.
  - `pub fn allowed(&self) -> &[ChatId]` (via `Chats`, add `pub fn allowed()` there).
  - `pub fn allowing(&self, row: &Ignored) -> Vec<ChatId>`: the allowed list plus `row.chat`,
    plus `row.from` when it differs and is not empty, deduplicated, in that order.
  - `pub fn retry(&mut self) -> Vec<Effect>`: `self.start()`.
  - `pub fn save_token(&mut self, token: String) -> Vec<Effect>` /
    `pub fn remove_token(&mut self) -> Vec<Effect>`: token write state `Writing`, then
    `[Effect::WriteToken(Some(token))]` / `[Effect::WriteToken(None)]`. Nothing while one is
    `Writing`.
  - `pub fn token_write(&self) -> Option<&TokenWrite>`, with
    `pub enum TokenWrite { Writing, Failed(String) }`.
  - `Input::TokenWritten(Result<(), String>)`: `Ok` → write state cleared, `self.start()`'s
    effects (a new token may be a new bot); `Err(why)` → `Failed(why)`, nothing else.
  - `Effect::WriteToken(Option<String>)`: write the token (or remove it) off the UI thread and
    answer `Input::TokenWritten`.

- [ ] **Step 1: Write the failing tests.**
  - `off_while_looking_starts_nothing`: `new(vec![], true)`, `start()`, `switch(false)` via
    `shown(&Value::TelegramOn(false))`, then `Looked` with a token gives nothing; state `Off`.
  - `switching_off_a_live_bridge_shuts_it_down`: on a connected bridge, `TelegramOn(false)`
    gives `[ShutDown]`, state `Off`, `away()` false.
  - `switching_on_looks_for_the_token`: `new(vec![], false)`, `TelegramOn(true)` gives
    `[Start]`.
  - `a_chat_taken_off_loses_what_it_followed`: chat `7` `/use 1`, then
    `TelegramChats(vec![])`; `bound("7")` is `None` and `announce` for session 1 is empty.
  - `allowing_a_group_member_lists_both`: allowed `["7"]`, row `{chat: "-5", from: "6"}` gives
    `["7", "-5", "6"]`; a row with `chat == from` adds one id.
  - `an_allowed_chat_leaves_the_ignored_list`: stranger `9` ignored, then
    `TelegramChats(vec!["7","9"])`; `ignored()` is empty.
  - `a_token_saved_starts_again` / `a_token_write_that_fails_is_said`: `save_token("t")` gives
    `[WriteToken(Some("t"))]` and `token_write()` is `Writing`; a second call gives nothing;
    `TokenWritten(Ok)` on a bridge switched on gives `[ShutDown, Start]` (live) or `[Start]`;
    `TokenWritten(Err("disk full"))` leaves `Failed("disk full")`.
  - `a_failed_write_puts_the_switch_back` (config drive tests): `set_telegram_on(false, true)`
    gives `Show(TelegramOn(true))` and a `Write`; feeding `Input::Written` with an `Err` gives
    `Show(TelegramOn(false))` back. Follow the existing *Outside* put-back test's shape.
  - `chat_refusal`: `""` and `"abc"` → *A chat id is a number*; `" 7 "` with `["7"]` → *7 is
    already allowed*; `"-1001"` with `[]` → `None`.
  - `a_secret_is_written_owner_only` (`#[cfg(unix)]`): write to a temp dir, mode `& 0o777` is
    `0o600`, content `"tok\n"` for input `" tok "`.
  - `a_secret_write_replaces_whole`: an existing file holding `old` is replaced by `new`; no
    `.tmp` left beside it. `remove_secret` on a missing file is `Ok`.
- [ ] **Step 2: Run** `cargo test -p onehand-core --locked remote config`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** per the Interfaces block. `drive.rs` passes ~700 lines with this;
  if it passes 800, move `BridgeState`, `Ignored`, `TokenWrite` and the methods above into
  `remote/drive/bridge.rs` as an `impl RemoteDrive` block.
- [ ] **Step 4: Run** `cargo test -p onehand-core --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(remote): the Telegram switch, its chats and its token are settings`.

---

### Task 2: the app: carry the settings, draw the group

**Files:**
- Modify: `crates/app/src/saves.rs` (`carry_settings`: `Show(TelegramOn | TelegramChats)`)
- Modify: `crates/app/src/remote.rs` (`Effect::WriteToken`; entry points for the page)
- Create: `crates/app/src/settings/telegram.rs` (the group and `TelegramDraft`)
- Create: `crates/app/src/shell/telegram.rs` (the shell's actions: the draft, *Add*, *Remove…*,
  *Save*, *Replace…*, *Allow*, the switch)
- Modify: `crates/app/src/settings/pages.rs` (`connections_page` gains the group after
  `outside_section`), `crates/app/src/settings.rs` (module, re-export), `crates/app/src/shell.rs`
  + `shell/boot.rs` (the draft field, built as `HostDraft` is)
- Test: `crates/app/src/remote/tests.rs`

**Interfaces:**
- Consumes: Task 1's whole block.
- Produces in `crate::remote`:
  - `pub(crate) fn shown(value: &Value, cx: &mut App)`: `drive.shown(value)` then `carry`.
  - `pub(crate) fn retry(cx)`, `pub(crate) fn save_token(token: String, cx)`,
    `pub(crate) fn remove_token(cx)`: each the drive's call, then `carry`.
  - `Effect::WriteToken(token)`: `write_secret(&secret::token_file(), &t)` or
    `remove_secret(..)` on the background executor, the error as its `to_string()`, then feed
    `Input::TokenWritten`.
- Produces `pub struct TelegramDraft { pub token: Entity<InputState> /* masked */, pub chat:
  Entity<InputState>, pub replacing: bool, _typing: [Subscription; 2] }`, built like
  `HostDraft::new` (each key redraws, since *Add* and *Save* refuse by what the fields hold).
- Shell, in `shell/telegram.rs`: `telegram_draft()`, `set_telegram_on(on, window, cx)`
  (`set_telegram_on(drive.on-ness, on)` through `carry_settings`, as `control::set_outside`
  does), `add_telegram_chat(window, cx)` (refusal from `chat_refusal`; on none, the list plus
  the trimmed id through `set_telegram_chats`; the field empties), `remove_telegram_chat(id,
  window, cx)` (no question), `allow_telegram(row, window, cx)` (`drive.allowing(row)` through
  `set_telegram_chats`), `save_telegram_token(window, cx)` (the field's value, trimmed, through
  `crate::remote::save_token`; the field empties and `replacing` goes false),
  `confirm_remove_telegram_token(window, cx)` (`Confirm { id: "remove-telegram-token", title:
  "Remove the bot token?", description: "The bridge stops until another is saved.", act:
  "Remove", .. }`, then `crate::remote::remove_token`), `replace_telegram_token(cx)`
  (`replacing = true`).
- `RemoteDrive` needs a read of whether it is switched on for the page and the setter's
  `before`: add `pub fn on(&self) -> bool` in Task 1 if it is not there.

The group (sketch from the spec; rows in this order):

1. The switch (`onehand_ui::toggle("telegram-on", on)`), held to its width as
   `outside_section`'s is.
2. The state line: `Off` → *Off*; `NoToken` → *No bot token* (warning); `Connecting { None }`
   → *Connecting…*; `Connecting { Some(why) }` → *Connecting… <why>* (warning);
   `Connected { name }` → *Connected as <name>*; `Stopped { why }` → *Stopped: <why>*
   (warning) and *Try again*.
3. Token: by `drive.source()`: `Env(name)` → *Read from $<name>*; `File(path)` and not
   `replacing` → *Saved in* `<path>` in mono, *Replace…*, *Remove…*; otherwise the masked field
   and *Save* (refusing while empty, loading while `TokenWrite::Writing`); a `Failed(why)`
   under it in the danger ink. `None` (never looked, the switch off at launch) reads as the
   field, since saving is how one sets the bridge up before turning it on.
4. *Allowed chats*: `list_rows` of the ids (` (group)` after a negative one), each with
   *Remove*, capped at `LIST_SHOWN` with *N more not shown*; the empty line when none; then
   the chat field and *Add*, refusing with `chat_refusal`'s words.
5. *Ignored: messaged the bot and got no answer*: only when `ignored()` is not empty; each row
   *<who> · <chat>* (just the chat when `who` is empty), *Allow*.

- [ ] **Step 1: Write the failing test** in `remote/tests.rs`:
  `a_token_written_starts_the_bridge`: a bridge switched on with no token; carry
  `[Effect::WriteToken(Some("t"))]` with the token file pointed at a temp path is awkward, so
  test the drive side through `feed(Input::TokenWritten(Ok(())), cx)` and assert the state is
  `Connecting { retrying: None }`; and `shown(&Value::TelegramOn(false), cx)` on a live bridge
  leaves `channel` and `pump` `None`.
- [ ] **Step 2: Run** `cargo test -p onehand --locked remote`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the carrier, the shell actions, the draft and the group. The
  `ui` skill's checklist applies: peer is `outside_section` and the Hosts page's add field.
- [ ] **Step 4: Run** `cargo test -p onehand --locked -- --skip a_projects_folders_share_one_issues_file`
  (that test hangs locally on main too). Expected: PASS.
- [ ] **Step 5: Commit** `feat(settings): the Telegram bridge on the Connections page`.

---

### Task 3: documents, and the spec and plans leave

**Files:**
- Modify: `docs/screens.md` (the Connections sentence), `crates/telegram/src/secret.rs` header,
  `AGENTS.md` (*Telegram token* line), `CHANGELOG.md` (`[Unreleased]` ▸ *Remote bridge*)
- Delete: `docs/superpowers/specs/2026-11-24-telegram-settings-design.md`,
  `docs/superpowers/plans/2026-11-24-telegram-settings-step1.md`, this plan

- [ ] **Step 1:** screens.md: *The Connections page lists each connector and who it acts as,
  with Check again, then an Outside control group …, then a Telegram group: its switch, what
  the bridge is doing (and Try again once it stopped), where the token comes from or a field
  for it, the allowed chats (at most 50, N more not shown) with a field to add one, and, while
  any wrote and were ignored, the last five with who wrote and Allow.*
- [ ] **Step 2:** secret.rs header and AGENTS.md: the token is read from the environment or
  `telegram.token`, and written only to that file, owner-only, by Settings; never to
  `onehand.toml`.
- [ ] **Step 3:** CHANGELOG `[Unreleased]` ▸ *Remote bridge*: the Telegram bridge is set up and
  watched from Settings ▸ Connections.
- [ ] **Step 4:** delete the spec and plans; `git diff --check`; whole checks:
  `cargo test --workspace --locked` (skipping the known hang), `make clippy CLIPPY_EXTRA="-- -D warnings"`,
  `make fmt`.
- [ ] **Step 5: Commit** `docs(remote): the Telegram bridge's settings, written into their documents`.
