# Harness seam, step 1: config writes edit the document

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A save of `onehand.toml` writes only the settings that changed. It keeps comments,
formatting and keys this build does not know.

**Architecture:** `AppConfig::update_in_place` keeps its signature `(path, edit: FnOnce(&mut
AppConfig))`. It reads the file into a `toml_edit::DocumentMut` and still parses the typed
config. A new `config::document::merge` writes into the document the differences between the
typed config as read (unsettled) and the typed config after `edit`. Before that, `core/config.rs`
is cut along two seams, `AppState` and the font lookup, so it is under the 800-line rule.

**Tech Stack:** Rust; `toml` 0.8 (already used), `toml_edit` 0.22 (already in `Cargo.lock`
through `toml`; now a direct dependency of `onehand-core`).

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decision 1 and step 1.

## Global Constraints

- Core stays GUI-free and runtime-free: `cargo tree -p onehand-core -i gpui` must error with
  "did not match any packages".
- `toml_edit = "0.22"` exactly that line (it is what `toml` 0.8 resolves), so nothing compiles
  twice. `Cargo.lock` is committed; CI runs `--locked`.
- `write_atomic` stays the only way the file reaches disk.
- A file that fails to parse is refused with an error containing `won't parse` and left byte for
  byte as it was. A read error says `could not be read`. A missing file is a first save.
- The diff is against the typed config **as read, before `settle`**, so a save still writes the
  settled agent list and the dropped legacy `unattended.mode` (docs/agents.md, *Migration*).
- Arrays, `[[agents]]` included, are replaced whole when they differ. Mark that with a
  `ponytail:` comment naming the ceiling (an unknown key inside one agent entry is lost when the
  list changes) and the upgrade path (merge entries by position once `AgentSpec` gains a field).
- No comment, doc comment or runtime string names a document. Use `make fmt` and `make lint`,
  never bare `cargo fmt`. Tests stay in `#[cfg(test)]` modules beside their file
  (`foo.rs` + `foo/tests.rs`).

## Review Focus

1. **A save that changes nothing leaves the file byte-identical.** A no-op `edit` must not
   reformat, reorder or re-quote anything (Task 2 test `a_save_that_changes_nothing_leaves_the_file_alone`).
2. **`[[agents]]` stays an array of tables.** A changed agent list must not turn into an
   inline `agents = [{…}]` (Task 3 test `a_changed_agent_list_stays_a_list_of_tables`).
3. **A bare key added to a file holding only tables is still valid TOML.** For example, the
   first `appearance` written into a file that starts with `[[agents]]` (Task 2 test
   `a_new_bare_key_lands_above_the_tables`).
4. **Dotted keys and inline tables written by hand are merged, not duplicated.** For example
   `remote.telegram.enabled = true` at the top level, or `font = { monospace = "X" }` (Task 2
   test `dotted_and_inline_tables_are_merged_in_place`).
5. **A first launch's found agents still reach the file at the first save of any setting**
   (Task 3 test `a_first_save_of_anything_writes_the_found_agents`).

---

### Task 1: Cut `AppState` and the font lookup out of `core/config.rs`

A pure move. No behaviour changes and no test changes, except the test that moves with
`AppState`.

**Files:**
- Create: `crates/core/src/config/state.rs`. It takes `AppState`, its `impl` and its doc comments
  (today `config.rs:804-934`), and the test `a_state_that_will_not_parse_is_kept_aside`, which
  moves from `config/persist_tests.rs` to a new `crates/core/src/config/state/tests.rs`.
- Create: `crates/core/src/config/font.rs`. It takes `FontConfig`, `MONO_FALLBACKS` and
  `resolve_monospace` (today `config.rs:61-148`).
- Modify: `crates/core/src/config.rs`. Add `mod state; mod font;` and re-export what is public
  today under the same paths: `pub use state::AppState; pub use font::{FontConfig,
  resolve_monospace};`.

**Interfaces:**
- Produces: the same public paths as before (`onehand_core::config::AppState`,
  `config::FontConfig`, `config::resolve_monospace`), so no caller outside `config` changes.

- [ ] **Step 1: Move the items and add the `mod`/`pub use` lines.**
- [ ] **Step 2: Verify:** `cargo test -q -p onehand-core config` shows the same number of passing
  tests as before the move (count it first). `cargo check -q` is clean.
- [ ] **Step 3: Verify the size:** `config.rs` up to its `#[cfg(test)]` line is under 800 lines
  (`awk '/^#\[cfg\(test\)\]/{exit} {c++} END{print c}' crates/core/src/config.rs`).
- [ ] **Step 4: Commit:** `refactor(config): AppState and the font lookup in files of their own`.

### Task 2: `config::document::merge`

**Files:**
- Modify: `crates/core/Cargo.toml`. Add `toml_edit = "0.22"`.
- Create: `crates/core/src/config/document.rs` (`mod document;` in `config.rs`, private).
- Test: `crates/core/src/config/document/tests.rs`.

**Interfaces:**
- Produces: `pub(super) fn merge(doc: &mut toml_edit::DocumentMut, before: &toml::Table, after:
  &toml::Table)`. Afterwards `doc` holds `after`'s value at every key where `before` and `after`
  differ, and nothing else in it has changed.

Rules the body must follow:
- For each key in `after` whose value differs from `before`'s (or is absent there): if both
  sides are tables and the document already holds a table-like item there (`[table]`, dotted
  keys or an inline table, through `as_table_like_mut`), recurse into it. Otherwise replace the
  item whole.
- For each key in `before` that is absent from `after`, remove it from the document.
- A key equal on both sides is never touched, and neither is a key in neither (an unknown key).
- To build a replacement item, serialise `{ key: value }` with `toml::to_string_pretty`, parse
  that with `toml_edit`, and take the item out. That way a table comes back as `[table]` and an
  array of tables as `[[key]]`, never inline. When the item replaces one already in the document,
  keep that item's decor (the comment above it).

- [ ] **Step 1: Write the failing tests** in `document/tests.rs`. Each one parses a fixture with
  `toml_edit`, builds `before`/`after` as `toml::Table` from TOML text, calls `merge`, and
  asserts on `doc.to_string()`:

```rust
#[test]
fn a_save_that_changes_nothing_leaves_the_file_alone() {
    let text = "# mine\nappearance = \"dark\"  # chosen\n\n[font]\nmonospace = \"X\"\n";
    let table: toml::Table = toml::from_str(text).unwrap();
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
    merge(&mut doc, &table, &table);
    assert_eq!(doc.to_string(), text);
}

#[test]
fn a_changed_value_keeps_comments_and_unknown_keys() {
    let text = "# mine\nappearance = \"dark\"\nfuture = 1  # newer build\n\n\
                [remote.telegram]\nenabled = false\nfuture_nested = \"keep\"\n";
    // before: appearance = "dark", remote.telegram.enabled = false
    // after:  appearance = "light", remote.telegram.enabled = true
    // assert: "# mine", "future = 1  # newer build" and "future_nested = \"keep\"" are still in
    // the text; it contains `appearance = "light"` and `enabled = true`; and
    // toml::from_str::<toml::Table> of the result succeeds.
}

#[test]
fn a_value_gone_from_the_config_leaves_the_file() {
    // before: [keymap] restart = ["alt-r"], toggle = ["alt-j"]; after: only toggle.
    // assert: the text has no `restart`, and still has `toggle = ["alt-j"]`.
}

#[test]
fn a_new_bare_key_lands_above_the_tables() {
    // fixture: "[[agents]]\nname = \"A\"\ncommand = \"a\"\n"; after adds appearance = "light".
    // assert: the result parses with toml and has appearance == "light" and one agent named A.
}

#[test]
fn dotted_and_inline_tables_are_merged_in_place() {
    // fixture: "remote.telegram.enabled = true\nfont = { monospace = \"X\" }\n"
    // after: remote.telegram.enabled = false, font.monospace = "Y"
    // assert: the text has `remote.telegram.enabled = false` and `font = { monospace = "Y" }`
    // (same shapes), with no `[remote` or `[font]` header added.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core document::`. Expected:
  a compile error, because `merge` does not exist yet.
- [ ] **Step 3: Implement `merge`** in `document.rs` by the rules above, with the `ponytail:`
  comment on the arrays-replaced-whole branch (see Global Constraints).
- [ ] **Step 4: Run them to see them pass:** `cargo test -q -p onehand-core document::`. Expected:
  5 passed.
- [ ] **Step 5: Commit:** `feat(config): merge a config's changes into its document`.

### Task 3: `update_in_place` writes through the document

**Files:**
- Modify: `crates/core/src/config.rs`, `AppConfig::update_in_place`.
- Test: `crates/core/src/config/persist_tests.rs`.
- Modify (words only): `docs/agents.md` (*Presets*, the sentence "every settings save writes the
  whole file back…"), `crates/core/src/config/agent.rs` (the same reason on `Preset`'s doc),
  `crates/telegram/src/secret.rs` (module doc, "It is rewritten whole by the settings dialog"),
  `CLAUDE.md` (*Telegram token*, "which settings rewrite whole"), `CHANGELOG.md` (*Fixed*).

**Interfaces:**
- Consumes: `document::merge(&mut DocumentMut, &toml::Table, &toml::Table)` from Task 2.
- Produces: `pub fn update_in_place(path: &Path, edit: impl FnOnce(&mut Self)) -> Result<(),
  String>`, unchanged in signature and in its two refusals.

Flow inside `update_in_place`:
1. Read the text: missing → `None`; any other read error → `Err("… could not be read: …")`.
2. `let before = AppConfig::parse(text.unwrap_or(""))` (no settle). A parse error → `Err("…
   won't parse: …")`.
3. `let mut cfg = AppConfig::from_disk(text, PATH)` (settled, found agents filled), then
   `edit(&mut cfg)`.
4. Parse the document from the same text (an empty `DocumentMut` when missing). Convert `before`
   and `cfg` to `toml::Table` with `toml::Table::try_from`, then `merge`.
5. `write_atomic(path, &doc.to_string())`, creating the parent directory as `save_to` does.

- [ ] **Step 1: Write the failing tests** in `persist_tests.rs`, each in a temp directory named
  for the test:

```rust
#[test]
fn a_save_keeps_what_this_build_does_not_know() {
    // file: "# my agents\nfuture = 1\n\n[[agents]]\nname = \"Mine\"\ncommand = \"x\"\n\n\
    //        [remote.telegram]\nfuture_nested = \"keep\"\n"
    // update_in_place(|c| c.appearance = Appearance::Light)
    // assert: the file still has "# my agents", "future = 1", "future_nested = \"keep\"" and
    // `name = "Mine"`, and AppConfig::parse(file).appearance == Appearance::Light.
}

#[test]
fn a_changed_agent_list_stays_a_list_of_tables() {
    // file with one [[agents]] entry; update_in_place pushes a second (name "Two", command "y")
    // assert: the file has "[[agents]]" twice and no "agents = [".
}

#[test]
fn a_first_save_of_anything_writes_the_found_agents() {
    // file: "appearance = \"dark\"\n" (no agents key); update_in_place(|c| c.appearance = Light)
    // assert: AppConfig::parse(file).agents == AppConfig::from_disk(Some(original),
    // PATH).unwrap().agents (what a first launch found on this machine).
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core persist_tests`. Expected:
  `a_save_keeps_what_this_build_does_not_know` fails (the comment and the unknown keys are gone
  today).
- [ ] **Step 3: Implement the flow above** in `update_in_place`, and drop `to_toml` if nothing else
  calls it (`save_to` may keep a whole-file write for tests; check with grep).
- [ ] **Step 4: Run the whole crate:** `cargo test -q -p onehand-core`. Expected: all pass, the four
  existing `persist_tests` (`keymap_round_trips…`, `unreadable_existing…`,
  `a_broken_config_is_never_overwritten`, `a_missing_config_starts_from_defaults`) included.
- [ ] **Step 5: Update the words** in the five files listed under *Files*. Each sentence that
  said a save writes the whole file now says a save writes what it changes. The *reasons* that
  still hold stay (a preset is named by id because a written command freezes at the first save
  of the agent list; the token stays out because the file is pasted into bug reports and is a
  preference file). CHANGELOG *Fixed*: "A settings save writes only what it changed: comments,
  formatting and keys from a newer build stay in `onehand.toml`."
- [ ] **Step 6: Full checks:** `make fmt`, `cargo test -q --workspace`, `make clippy
  CLIPPY_EXTRA="-- -D warnings"`, and `cargo tree -p onehand-core -i gpui` (it must error).
- [ ] **Step 7: Commit:** `feat(config): a save writes what it changed and keeps the rest of the file`.
  Then open the PR against `main` (one PR for the step), titled the same way.
