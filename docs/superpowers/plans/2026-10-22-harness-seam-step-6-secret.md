# Harness seam, step 6: one rule for reading a secret, in core

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The rule for reading a credential moves from `crates/telegram/src/secret.rs` into core as
`config::secret`, so the connectors to come (Slack, Jira) read theirs the same way. The rule:
- an environment variable first, then a file of its own;
- blank is no answer, and whitespace is trimmed;
- a warning when the file is readable by more than its owner.

Telegram calls it with no change in behaviour.

**Architecture:** A new `crates/core/src/config/secret.rs` with `pub fn secret(env_var: &str, file:
&Path) -> Option<String>`. Its tests move with it, to `config/secret/tests.rs`. The Telegram crate
keeps what is Telegram's: `DEFAULT_TOKEN_ENV`, `token_file()`, and the module doc on why a bot
token never sits in `onehand.toml`. `telegram_token` becomes a call to `config::secret`.

**Tech Stack:** Rust (core + telegram crate).

**Spec:** `docs/superpowers/specs/2026-10-22-harness-seam-design.md`, decision 7, step 6 (owner's
answer: agent `env` never names a secret).

## Global Constraints

- **No behaviour change for Telegram:** the same variable (`$ONEHAND_TELEGRAM_TOKEN`, or the one
  `token_env` names), the same file (`<config_dir>/onehand/telegram.token`), the same order and
  trimming, and the warning under the same condition (the file answered and the variable was
  unset).
- **The warning's wording becomes generic:** `It holds a secret; run: chmod 600 <path>` instead of
  `It holds a bot token`. This is the one visible difference, in stderr only.
- **Deliberate deviation from the spec's wording:** the caller passes the file path. The spec's
  `<config_dir>/onehand/<name>.secret` naming rule has no second user yet, and Telegram's file is
  named its own way.
- Nothing about agent `env` changes.
- `#![warn(unreachable_pub)]`: `secret` is `pub` (the telegram crate names it). `pick` and
  `mode_is_private` are private.

## Review Focus

1. **A blank variable and a file:** the file answers, and no warning is printed (the warning
   needs the variable unset, as today). Covered by the moved `a_blank_source_falls_through` for
   the answer; the warning condition is checked by inspection.
2. **A missing file, with the variable unset:** `None`, and the caller's message is unchanged
   (`remote.rs:169-176`).

---

### Task 1: `config::secret`

**Files:**
- Create: `crates/core/src/config/secret.rs` (`mod secret; pub use secret::secret;` in
  `config.rs`) and `crates/core/src/config/secret/tests.rs`.
- Modify: `crates/telegram/src/secret.rs`:
  - delete `pick_token`, `mode_is_private`, both `warn_if_shared` and their tests;
  - `telegram_token` becomes `onehand_core::config::secret(env_name, &token_file())`;
  - the module doc keeps the why-not-the-config reasoning, and points at `config::secret` for
    the rule.
- Modify: `CHANGELOG.md`: nothing visible changed except a stderr line, so no entry.

**Interfaces:**
- Produces: `pub fn secret(env_var: &str, file: &Path) -> Option<String>`.

- [ ] **Step 1: Move the five tests** (`the_environment_wins_over_the_file`,
  `a_file_answers_when_nothing_is_in_the_environment`, `a_blank_source_falls_through`,
  `the_trailing_newline_of_a_token_file_is_not_the_token`, `only_an_owner_only_mode_is_private`)
  into `config/secret/tests.rs`, calling `pick` and `mode_is_private`. Add:
  ```rust
  #[test]
  fn a_secret_is_read_from_its_file_when_the_variable_is_unset() {
      // a temp file holding "s3cret\n", an env var name nobody sets
      // ("ONEHAND_TEST_SECRET_<pid>") → secret(var, &file) == Some("s3cret")
  }
  ```
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core config::secret`.
  Expected: a compile error.
- [ ] **Step 3: Implement** `secret.rs` by moving the bodies, and switch Telegram to it.
- [ ] **Step 4: Full checks:**
  - `make fmt`, `cargo test -q --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`;
  - `cargo tree -p onehand-core -i gpui` (must error).
- [ ] **Step 5: Commit:** `refactor(config): reading a secret is one rule in core`. Open the PR
  against `main`.
