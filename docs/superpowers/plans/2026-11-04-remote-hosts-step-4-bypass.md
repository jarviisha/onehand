# Remote hosts step 4: the permission bypass switch, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A switch per host project, on by default, under which the host service answers every
card itself (permissions allowed, questions told to decide), so a session never stands still
overnight.

**Architecture:** The host keeps the switch per folder in its data directory and applies it in
`Hub`'s drive loop, where every agent event already passes. A card it answers is journaled as one
new entry, `Entry::Answered`, holding the card and the answer together, so a laptop always takes
the two in one go and never announces a card that is already answered. The laptop reads and sets
the switch over two wire calls and draws it in a *Settings* box on the host project's page.

**Tech Stack:** Rust, `onehand-core` (wire, client, chat), `onehand-host` (hub, serve), GPUI app
(project page, session loop).

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md`, *Permission bypass*
(including the decisions of 2026-11-04 listed there).

**PR boundary:** one PR, `feat/remote-hosts-bypass`, title
`feat(hosts): answer cards on the host while nobody watches`. The host side alone would be a
switch nobody can see, and the page alone would be a switch that does nothing; together they are
a few hundred lines.

## Global Constraints

- Switch label: `Answer cards on the host`.
- Line under it, always, muted ink: `While on, the host allows every permission an agent asks
  for, and tells it to decide its own questions and say what it chose. An agent can then do
  anything the host's user can on <host>, git push and deleting files included. Off, cards wait
  for you.`
- Question answer text, exact: `Nobody is watching this session to answer. Decide on your own
  judgement and carry on, and in your reply say what you chose and why.`
- Default is on: the host stores the folders switched **off**; a folder never seen is on.
- A permission is answered with the narrowest allow: `allow_once`, else `allow_always`; a card
  with no allow option is left parked.
- The rail shows no mark for the switch.
- Core stays GUI-free; nothing new in core is `pub` unless the app or host names it.
- Our own enums are matched exhaustively (the `guards.rs` rule); every new `Entry`, `Call` and
  `Reply` variant is added to every match, no wildcard arms.
- No comment or string names a document. Words on screen in English.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`, and `cargo tree -p onehand-core -i gpui-pre` still errors.

## Review Focus

1. **A card answered by the host raises no desktop notice and no Telegram message**, live or in a
   replay. Pinned by Task 4's test on `Chat::awaits` and the batch rule, and Task 1's test that
   `as_event` of `Answered` yields the card then its echo.
2. **Switching on while a card already waits answers it**, and the laptop's card resolves live.
   Pinned in Task 2 (`switching_on_answers_cards_already_waiting`).
3. **A person who answers a card at the same moment the host does**: the second answer for one
   rpc id must not reach the agent twice. Pinned in Task 2
   (`a_card_answered_by_the_person_is_not_answered_again`).
4. **A service restarted onto a new build keeps the switch** (it is read from disk in `load`).
   Pinned in Task 2 (`the_switch_survives_a_restart`).
5. **A question form with nowhere to write** (a select with no *Other* box) is declined, not left
   parked. Pinned in Task 1 (`a_form_with_nowhere_to_write_is_declined`).

---

### Task 1: The bypass answers and the wire (core)

**Files:**
- Modify: `crates/core/src/acp/types.rs` (impls on `PermissionRequest`, `Elicitation`)
- Modify: `crates/core/src/host/wire.rs` (`Entry::Answered`, `Call::Bypass`, `Reply::Bypass`,
  `busy_after`)
- Modify: `crates/core/src/host/copy.rs` (`as_event` → several events)
- Modify: `crates/core/src/host/client.rs` (`take` sends each event of an entry)
- Test: `crates/core/src/acp/tests` beside `types.rs` (wherever its tests live today),
  `crates/core/src/host/wire/tests.rs`, `crates/core/src/host/copy/tests.rs`

**Interfaces:**
- Produces:
  - `pub const BYPASS_ANSWER: &str` (the question answer text above), in `acp/types.rs`.
  - `impl PermissionRequest { pub fn bypass_answer(&self) -> Option<AcpRequest> }`:
    `PermissionResponse { rpc_id, option_id: Some(id) }` of `keyed(AllowOnce)` else
    `keyed(AllowAlways)`; `None` when neither exists.
  - `impl Elicitation { pub fn bypass_answer(&self) -> AcpRequest }`: `ElicitationResponse` with
    `Accept` of, per field, `(key, Text(BYPASS_ANSWER))` for a `Text` field, `(custom_key,
    Text(BYPASS_ANSWER))` for a field with an *Other* box, nothing otherwise; `Decline` when that
    leaves no pair.
  - `Entry::Answered { card: Option<AcpEvent>, answer: AcpRequest }`: a card the host answered
    itself; `card` is `Some` when the card is journaled in the same line, `None` when it was
    journaled earlier (switched on while it waited).
  - `Call::Bypass { cwd: PathBuf, on: Option<bool> }` (None reads, Some sets) answered
    `Reply::Bypass { on: bool }` (the value now in force).
  - `copy::as_event(entry) -> Vec<AcpEvent>` (was `Option`): `Answered` gives the card (if any)
    then `AcpEvent::Echo(answer)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_permission_is_allowed_once_before_always() {
    let req = permission_with(&[("r", "reject_once"), ("a", "allow_always"), ("o", "allow_once")]);
    assert!(matches!(req.bypass_answer(),
        Some(AcpRequest::PermissionResponse { option_id: Some(id), .. }) if id == "o"));
}
#[test]
fn a_permission_with_no_allow_is_left_alone() {
    assert!(permission_with(&[("r", "reject_once")]).bypass_answer().is_none());
}
#[test]
fn a_question_is_answered_in_its_other_box_and_text_fields() {
    // fields: Select with custom_key "q0_custom", Text "q1"
    let AcpRequest::ElicitationResponse { outcome: ElicitOutcome::Accept(pairs), .. } =
        form().bypass_answer() else { panic!() };
    assert_eq!(pairs, vec![
        ("q0_custom".into(), ElicitValue::Text(BYPASS_ANSWER.into())),
        ("q1".into(), ElicitValue::Text(BYPASS_ANSWER.into())),
    ]);
}
#[test]
fn a_form_with_nowhere_to_write_is_declined() {
    // one Select field, custom_key None
    assert!(matches!(select_only().bypass_answer(),
        AcpRequest::ElicitationResponse { outcome: ElicitOutcome::Decline, .. }));
}
#[test]
fn an_answered_card_reads_as_the_card_then_its_echo() {
    let events = as_event(Entry::Answered { card: Some(AcpEvent::Permission(p())), answer: allow() });
    assert!(matches!(events.as_slice(), [AcpEvent::Permission(_), AcpEvent::Echo(_)]));
}
#[test]
fn an_answer_does_not_change_busy() {
    assert!(busy_after(true, &Entry::Answered { card: None, answer: allow() }));
    assert!(!busy_after(false, &Entry::Answered { card: None, answer: allow() }));
}
```

Plus a wire round-trip of `Call::Bypass { on: None }` and `Reply::Bypass { on: true }` in
`wire/tests.rs`, as its other calls have.

- [ ] **Step 2:** `cargo test -p onehand-core bypass answered` → FAIL (not defined).
- [ ] **Step 3:** Implement the interfaces above. `busy_after(Answered)` keeps `busy`.
  `client.rs` `take`: for each event of `as_event(entry)`, send `At` once before the first, as
  today, and stop on `Disconnected`. `ElicitField::custom_key` stays `pub(crate)`.
- [ ] **Step 4:** `cargo test -p onehand-core` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): the answers a host gives a card by itself`.

### Task 2: The host answers cards (hub, serve)

**Files:**
- Modify: `crates/host/src/hub.rs`
- Modify: `crates/host/src/serve.rs` (dispatch `Call::Bypass`)
- Test: `crates/host/src/hub/tests.rs`, `crates/host/src/serve/tests.rs`

**Interfaces:**
- Consumes: Task 1's `bypass_answer`, `Entry::Answered`, `Call::Bypass`, `Reply::Bypass`.
- Produces: `Hub::bypass(&self, cwd: &Path) -> bool`,
  `Hub::set_bypass(&mut self, cwd: PathBuf, on: bool) -> Result<bool, String>`.

Design (my calls):
- `Hub` gains `off: BTreeSet<PathBuf>`, kept in `<dir>/bypass.json` as `{"off":[...]}`, written
  whole through a temporary file on each change and read in `Hub::load`. A file that does not
  parse is logged and read as empty (all on), which is the default the owner chose.
- `Session` gains `parked: Vec<AcpEvent>`: cards journaled and not yet answered. A
  `PermissionResponse`/`ElicitationResponse` request for an rpc id drops it; `TurnEnded`,
  `Interrupted`, `Disconnected` clear it.
- In `drive`, a `Permission`/`Elicitation` event on a session whose cwd is not in `off` and whose
  card has an answer: send the answer on `tx`, then record
  `Entry::Answered { card: Some(event), answer }`, live to the watcher. Otherwise record the card
  as today and push it to `parked`. A send that fails (agent gone) records the card plain.
- `set_bypass(cwd, true)` answers each parked card of every session on that cwd the same way,
  recorded as `Entry::Answered { card: None, answer }`.
- `Hub::request` drops a person's card answer whose rpc id is no longer parked (already answered
  by the host) with `Ok(())`, journaling nothing.

- [ ] **Step 1: Write the failing tests** (fake connector, as the hub tests already use):
  - `a_card_is_answered_while_the_switch_is_on`: open a session, the fake agent sends a
    `Permission`; the agent's request channel receives `PermissionResponse` with the allow-once
    id; the journal's last line is `Answered { card: Some(_), .. }`; a watcher received it live.
  - `a_card_waits_while_the_switch_is_off`: `set_bypass(cwd, false)` first; the agent receives
    nothing; the journal's last line is `Event(Permission(_))`.
  - `switching_on_answers_cards_already_waiting`: off, card parks, `set_bypass(cwd, true)`; the
    agent receives the answer; the journal ends `Answered { card: None, .. }`.
  - `a_card_answered_by_the_person_is_not_answered_again`: off, card parks, the person answers
    via `request`, then `set_bypass(cwd, true)`; the agent received exactly one answer.
  - `the_switch_survives_a_restart`: `set_bypass(cwd, false)`, a fresh `Hub::new` + `load` on the
    same dir reads `bypass(cwd) == false`; another folder reads `true`.
  - In `serve/tests.rs`: `Call::Bypass { on: None }` answers `Reply::Bypass { on: true }` for a
    folder never set; `Some(false)` then `None` answers `false`.
- [ ] **Step 2:** `cargo test -p onehand-host` → the new tests FAIL.
- [ ] **Step 3:** Implement the design above; `Call::Bypass` is answered inline on the loop
  (`lock(hub)`), not off it: it touches one small file.
- [ ] **Step 4:** `cargo test -p onehand-host` → PASS.
- [ ] **Step 5:** Commit `feat(hosts): the host answers cards while the switch is on`.

### Task 3: The client calls (core)

**Files:**
- Modify: `crates/core/src/host/client.rs` (`Answer` slot `bypass: Option<bool>`, filled from
  `Reply::Bypass`)
- Modify: `crates/core/src/host/client/files.rs` (beside `home`)
- Test: `crates/core/src/host/client/tests.rs`

**Interfaces:**
- Produces: `HostConn::bypass(&self, cwd: &Path) -> Result<bool, HostError>` and
  `HostConn::set_bypass(&self, cwd: &Path, on: bool) -> Result<bool, HostError>`, both through
  `ask_for(Call::Bypass { .. }, |a| a.bypass)`.

- [ ] **Step 1:** Test `bypass_asks_and_reads_the_answer` in the style of the `Home` test
  (line ~1170): the call sent is `Call::Bypass { cwd: "/srv/app", on: None }`, a scripted
  `Reply::Bypass { on: false }` gives `Ok(false)`; and `set_bypass` sends `on: Some(true)`.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** run → PASS.
- [ ] **Step 5:** Commit `feat(hosts): read and set a folder's switch from the laptop`.

### Task 4: No notice for a card already answered (core + app session loop)

**Files:**
- Modify: `crates/core/src/chat/model/turn.rs` (or wherever `pending_permissions` lives)
- Modify: `crates/app/src/chat/session.rs` (the batch loop around line 472)
- Test: `crates/core/src/chat/model/tests.rs`

**Interfaces:**
- Produces: `Chat::awaits(&self, ask: UserAsk) -> bool`: a permission or a question of that kind
  is still unresolved.

The rule: `asked_user` no longer ends a batch. The loop keeps the last `asked_user` of the batch
and, once the batch ends, emits `ChatEvent::AwaitingUser(ask)` only when `chat.awaits(ask)`. An
`Answered` entry puts its card and its echo on the channel together (Task 1), so the echo is
always in the same batch as its card. `session.rs` is past 800 lines already: if this change
adds to it, move the batch loop's body into `crates/app/src/chat/session/events.rs` in the same
commit.

- [ ] **Step 1:** Core tests: `awaits_a_card_until_it_is_answered` (Permission applied →
  `awaits(Permission)`; its `Echo(PermissionResponse)` applied → not), and the same for a
  question with `Echo(ElicitationResponse)`.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement `awaits`, then the loop change.
- [ ] **Step 4:** `cargo test -p onehand-core` and `cargo test -p onehand` → PASS.
- [ ] **Step 5:** Commit `fix(chat): say a card waits only while it still does`.

### Task 5: The switch on the host project's page (app) and the documents

**Files:**
- Modify: `crates/app/src/chat/pane/project_page/host.rs` (read with the listing, the box)
- Modify: `crates/app/src/chat/pane/project_page.rs` (the box under the host sessions box)
- Modify: `docs/hosts.md`, `docs/screens.md`, `docs/known-gaps.md`, `CONTEXT.md`,
  the spec's *Status* line and *Order of work* item 4

**Interfaces:**
- Consumes: Task 3's `HostConn::bypass`, `HostConn::set_bypass`.

Behaviour (follow the `ui` skill and answer its checklist):
- The page's project gains `bypass: Option<bool>`, `None` until read. `list_host_sessions` reads
  it on the same attach right after the listing (a failure to read it leaves `None`, and the
  listing still shows).
- With `Some(on)`, under *Sessions on <host>*, `page_card("Settings", ..)` holds one row: the label
  *Answer cards on the host* with `onehand_ui::toggle("host-bypass", on)` at the row's end, the
  muted line from Global Constraints under it (with `<host>` filled). With `None` (not attached,
  or not read yet) the box is not drawn.
- A press sets it optimistically, sends `set_bypass` on the ACP runtime as the listing does, and
  takes the value the host answers. A failure puts the old value back and toasts *Could not
  change the switch on <host>: …* (warning).
- The box goes when the page leaves; a newer ask drops an older answer (the `asked` counter
  already guards the listing; reuse it).

Documents (describe as built, no code citing documents):
- `docs/hosts.md`: a new section *The permission bypass* (the switch, default, where it is kept,
  what it answers and with what, switching on answers waiting cards, no notice for a card the
  host answered); drop the item from *Not built*.
- `docs/screens.md`: the host project page bullet gains the *Settings* box.
- `CONTEXT.md`: *Permission bypass* as a term, if the glossary has host terms (it does since
  step 1).
- `docs/known-gaps.md`: an offloaded project's switch comes with step 5.
- Spec: *Status* says steps 1 to 4 built; *Order of work* item 4 says built, as built in
  `docs/hosts.md`.

- [ ] **Step 1:** Implement the page, then `cargo check` and `cargo test -p onehand`.
- [ ] **Step 2:** Update the documents.
- [ ] **Step 3:** Run the full checks from Global Constraints.
- [ ] **Step 4:** Commit `feat(hosts): a switch on the host project's page for answering cards`.

### Finish

- [ ] Opus subagent reviews `origin/main..HEAD`; fix what it confirms.
- [ ] Push, open the PR (house style: plain body, `-` bullets), wait for CI, report in
  Vietnamese, ask the owner before merging.
