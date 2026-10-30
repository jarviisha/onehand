# Worker names Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A worker is named for its job: `open_session` takes a `name` from a session's agent, and a worker opened without one is named for its folder, never its prompt.

**Architecture:** Core parses and checks `name` and gives `Chat` a title that stands in for the prompt's (not saved). The app passes `name` through the worker open, refuses it from an outside program, and on start sets the worker's custom title (the given name) and its stand-in title (the folder's last component).

**Tech Stack:** Rust, `serde_json`, GPUI (app side only).

**Spec:** `docs/superpowers/specs/2026-10-30-workers-list-design.md` (step 1, *Worker names* and the builder's calls *The `name` argument*, *Where the name lives*).

## Global Constraints

- `name` is for a session's agent only; an outside program naming it is refused like `dirs`.
- `name` is whitespace-collapsed (as `Chat::rename` does), not empty, at most 40 characters.
- A given name is the conversation's custom title: saved, and a person's rename wins over it.
- The folder fallback is never saved; *Reset title* on a worker goes back to it, not to the prompt.
- Core stays GUI-free; nothing in core is `pub` unless something outside the crate names it.
- No comment or string cites a document (AGENTS.md rule).
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

- A name of 41+ characters, or only spaces: refused with words naming `name`, nothing opened.
- A retried open with the same `request` but another `name`: refused like any other changed argument.
- A worker opened in a folder whose path ends in `/` or is a project root: the stand-in is still the last component, not empty.
- A worker renamed by the person, then *Reset title*: shows the folder, not *Turn 2…*.
- An outside program sending `name`: refused, and the refusal lists `name` with any other worker-only argument.

---

### Task 1: Core: `name` on `open_session`, and a stand-in title on `Chat`

**Files:**
- Modify: `crates/core/src/control.rs` (`Call::OpenSession`, the `open_session` schema in `catalogue`, the `known` list and the `open_session` arm of `parse`)
- Modify: `crates/core/src/chat/model.rs:122` (field beside `custom_title`)
- Modify: `crates/core/src/chat/model/meta.rs:275-296` (`conversation_title`)
- Test: `crates/core/src/control/tests.rs`, `crates/core/src/chat/model/tests.rs`

**Interfaces:**
- Produces: `Call::OpenSession { …, name: Option<String> }`, already collapsed and checked.
- Produces: `pub stand_in_title: Option<String>` on `Chat` (doc: the title shown when nobody named the conversation, ahead of the prompt's; never saved), and `conversation_title()` = `custom_title`, else `stand_in_title`, else `derived_title()`.

- [ ] **Step 1: Write the failing tests**

In `control/tests.rs`: add `name: None` to every existing `Call::OpenSession` expectation, and add to the parse test:

```rust
assert_eq!(
    performs("open_session", json!({"project": "/wt", "agent": "Claude", "name": "  a ·  review #265 "})),
    Call::OpenSession { project: "/wt".into(), agent: "Claude".into(), prompt: None, dirs: Vec::new(),
        request: None, turn: None, model: None, name: Some("a · review #265".into()) }
);
```

and to `every_tool_refuses_malformed_input`'s cases:

```rust
("open_session", json!({"project": "app", "agent": "Claude", "name": "   "}), "name"),
("open_session", json!({"project": "app", "agent": "Claude", "name": 3}), "name"),
("open_session", json!({"project": "app", "agent": "Claude", "name": "x".repeat(41)}), "`name` must be at most 40 characters"),
```

In `chat/model/tests.rs`:

```rust
#[test]
fn a_stand_in_title_comes_before_the_prompt_and_after_a_rename() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::User(UserMsg::text("Turn 2: write the report")));
    chat.stand_in_title = Some("onehand-sim-b".into());
    assert_eq!(chat.conversation_title().as_deref(), Some("onehand-sim-b"));
    assert!(chat.rename("b · report r1"));
    assert_eq!(chat.conversation_title().as_deref(), Some("b · report r1"));
    chat.reset_title();
    assert_eq!(chat.conversation_title().as_deref(), Some("onehand-sim-b"));
}
```

Plus one assertion that the saved snapshot carries no stand-in: build the snapshot the way the existing persist tests do and check its `title` is `None` when only `stand_in_title` is set.

- [ ] **Step 2: Run them, expect compile failures**

Run: `cargo test -p onehand-core --locked control:: chat::model::`
Expected: FAIL (`name` and `stand_in_title` unknown).

- [ ] **Step 3: Implement**

- `parse`: add `"name"` to `open_session`'s `known`; `name: optional("name")?` then collapse with `split_whitespace().join(" ")`; over 40 chars (`chars().count()`) is `Err("`name` must be at most 40 characters")`. A non-string or blank falls to `optional`'s existing words.
- Schema: `"name": text("A short name for the session, saying its job, shown wherever it is listed; for a session's agent only")`.
- `Chat`: the field, and `conversation_title` as in *Interfaces*. Do not add it to `persist.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p onehand-core --locked`
Expected: PASS.

- [ ] **Step 5: Commit** (with the spec and this plan)

```bash
git add docs/superpowers crates/core
git commit -m "feat(control): open_session takes a worker's name"
```

### Task 2: App: pass the name through and name the worker on start; docs

**Files:**
- Modify: `crates/app/src/control.rs:670-695` (destructure and pass `name`)
- Modify: `crates/app/src/control/workers.rs` (`Open.name`, `Open::asked`, `worker_only`, `finish`)
- Modify: `crates/app/src/control/workers/requests.rs:15-22` (`Asked.name`)
- Modify: `docs/control.md` (the `open_session` row; *A worker*: a bullet on the name)
- Test: `crates/app/src/control/workers/tests.rs`

**Interfaces:**
- Consumes: `Call::OpenSession.name`, `Chat::stand_in_title` (Task 1).
- Produces: `pub(super) name: Option<String>` on `Open`, `pub(in crate::control) name: Option<String>` on `Asked`.

- [ ] **Step 1: Write the failing tests**

In `workers/tests.rs`: add `name` to both `Open` literals and to `asked(..)`. In the first, set `name: Some("a".into())` and assert `why.contains("`name`")`; the second keeps `name: None`. Add:

```rust
#[test]
fn the_same_request_with_another_name_is_refused() {
    let mut requests = Requests::default();
    let mut first = asked("go");
    first.name = Some("a · one".into());
    requests.see(1, "a", &first).unwrap();
    let mut second = asked("go");
    second.name = Some("a · two".into());
    assert!(requests.see(1, "a", &second).is_err());
}
```

(match the construction the neighbouring request tests use if `Requests` is built otherwise.)

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p onehand --locked control::workers`
Expected: FAIL to compile on `name`.

- [ ] **Step 3: Implement**

- Thread `name` from `Call::OpenSession` into `workers::Open`, into `asked()`, and into `worker_only`'s list as `("name", open.name.is_some())`.
- Carry `name` into `finish` (a parameter beside `spec`). In its `Ok((uid, session))` arm, before returning, `session.update(cx, |s, _| { s.chat.stand_in_title = <last component of resolved.root>; s.chat.custom_title = name; })`. The last component: `resolved.root.file_name()` lossily as a string, falling back to the whole path's display when there is none. Setting `custom_title` directly is right here: no conversation file exists yet, and the first turn's end writes the metadata.

- [ ] **Step 4: Run the tests and the checks**

Run: `cargo test --workspace --locked && make clippy CLIPPY_EXTRA="-- -D warnings" && make fmt`
Expected: all pass; `git diff --stat` after `make fmt` shows no new changes.

- [ ] **Step 5: Docs**

`docs/control.md`: in the `open_session` row add optional `name` to a session's agent's arguments; in *A worker* add one bullet: the name the lead gives (collapsed, at most 40 characters) is the worker's title wherever it is listed and the answer's `name:`, saved as a rename is and replaced by the person's own; without one, the worker is named for its folder's last component, which is not saved and is what *Reset title* goes back to. Add `name` to the list an outside program is refused for.

- [ ] **Step 6: Commit**

```bash
git add crates/app docs/control.md
git commit -m "feat(control): a worker is named for its job or its folder"
```
