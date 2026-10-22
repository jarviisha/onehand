# Design: the harness seam (R1)

- Date: 2026-10-22.
- Source: the architecture audit of the same day (local notes), items A3 and A5, and bugs B12 and B13.
  The harness ground was re-read against `main` at `a09e97e`. The native agents on the owner's
  machine were probed with a raw `initialize` + `session/new` (no prompt sent).
- Contracts that still govern: [CONTEXT.md](../../../CONTEXT.md), [docs/agents.md](../../agents.md)
  (which this changes and must be updated with), [DESIGN.md](../../../DESIGN.md). Every visible
  change follows the `ui` skill.

## Intent

onehand should be able to host the coding agents people actually use, not only the two that come
through an npm adapter. Today every built-in agent is `npx <adapter>@<pin>` pointed at a binary,
while Copilot, OpenCode and Gemini speak ACP themselves. Two more things get in the way. A config
naming an agent this build does not know fails to load entirely. And every settings save rewrites
the whole file, so an older build erases what a newer one wrote. R1 makes the seam fit native
agents, makes the config safe across builds, and moves the harness guesses the app makes into
core. The later refactors (runs off windows, MCP tools for agents, connectors) build on this.

## Goals

1. **A built-in agent is either an adapter or a native ACP program.** One table in core says,
   per preset, how it runs: `Adapter { package, pin, binary_var }` or `Native { args }`.
2. **A config from a newer build still loads.** A preset id this build does not know keeps its
   entry: it is shown as not supported by this build, cannot start, and is written back exactly
   as it was read.
3. **A save changes only what it changed.** `onehand.toml` is edited as a document: keys, tables
   and comments this build does not know survive, and so does everything outside the changed
   setting.
4. **What onehand guesses about a harness lives in core**, where a second front end and tests
   reach it: which config group is the model, the effort or the fast switch; that a user chunk is
   an adapter's interruption marker.
5. **The serve loop is a value, not twelve parameters**, so authentication and capability checks
   (and R3's per-session MCP servers) have one place to land.

## Non-goals

- Logging in inside onehand (ACP `authenticate`). The login stays a terminal command, said in
  words. Only the source of those words grows (decision 6).
- One adapter process serving several sessions.
- MCP servers handed to agents (R3). R1 only leaves the place for them (decision 5).
- An ACP registry, or fetching agents.
- Changing how sessions, archives, workflow steps or `unattended.agent` name an agent (by name).

## Anti-goals (a failure even if it works)

- Any existing config loads into something that starts a different agent, loses an entry,
  changes which account an entry uses, or drops a comment the person wrote outside the setting
  being saved.
- An unknown preset is silently rewritten to something else, or dropped at the next save.
- A native agent that is not installed hangs, or fails with only stderr to say why. The same
  missing-binary sentence the adapters get applies.
- A harness name list ends up in two places, one in core and one in the app.

## Constraints (from the repository)

- Core stays GUI-free and runtime-free. Presets, the document edit, selector roles and the serve
  loop are all core (`config/agent.rs`, `config.rs`, `chat/model/meta.rs`, `acp/client.rs`).
- `AppConfig::update_in_place(path, edit)` is the one write path. `crate::saves::write_config`
  orders the writes and puts a value back when its write fails. Its callers pass a closure over
  the typed `AppConfig`, and that API is kept.
- A config is settled on every read (`AppConfig::settle`), and docs/agents.md promises that a
  save writes the settled list (the migration lands that way).
- Settings applies one `AgentEdit` (since #198), found by whole spec, to the list re-read at save
  time.
- `Preset` is matched by name in about ten app files (settings form, project page install
  offers, the Plugins mode gate, the unattended report).
- `toml` 0.8 already pulls in `toml_edit` 0.22 (`Cargo.lock`). Depend on that line so nothing is
  compiled twice. CI resolves with `--locked`.
- Files near the 800-line rule: `core/config.rs` is already past it (934). Splitting it along
  `AppState` and the document edit is part of step 1. `acp/client.rs` (about 1,050 lines of code
  before its tests) splits as the serve loop becomes `Conn` (step 4).

## What the probe found

| Agent | Command | `initialize` | `session/new` | Notes |
|---|---|---|---|---|
| Copilot 1.0.63 | `copilot --acp` | answers | answers | `loadSession`, image prompts; `authMethods` with the description *Run `copilot login` in the terminal*; mode ids are URLs (`https://agentclientprotocol.com/protocol/session-modes#agent`, `#plan`, `#autopilot`) |
| OpenCode 2.0.3 | `opencode acp` | answers | answers | `loadSession`, image prompts; `authMethods`: *Run `opencode auth login` in the terminal*; config options carry `category` (`"model"`) |
| Gemini 0.36.0 | `gemini --acp` | **no answer in 20 s** | — | Not understood yet. It may need a login, a trusted folder, or other framing. |

## Decisions

### 1. Config writes edit the document

`update_in_place` reads the file as a `toml_edit::DocumentMut` and parses the typed config from
it. It applies `edit` to the typed config, then writes back only the differences between the
typed config **as read, before settling** and the typed config after the edit. Diffing against
the unsettled read keeps the current promise that a save writes the settled agent list and the
dropped legacy mode.

- **Top-level keys and tables are merged key by key**, recursively. A key this build does not
  know is never touched, and comments on untouched keys stay.
- **Arrays (including `[[agents]]`) are replaced whole when they differ.** This is a known
  ceiling. A newer build's unknown key *inside* one agent entry is lost when the list changes.
  Mark it `ponytail:` and lift it when a field is added to `AgentSpec`.
- A missing file starts from an empty document, as a first save does today. A file that fails to
  parse is still refused and reported (the existing guard).
- `write_atomic` stays the only way the file reaches disk.

### 2. Presets become one table in core

`Preset` stays a closed enum of the ids this build knows. Each one answers `fn info(self) ->
&'static PresetInfo`: shown name, binary, install command, login command, account variable,
unattended mode, and `runs: Runs`. `Runs` is either `Adapter { package, pin, binary_var }` or
`Native { args }`. `launch()` reads the table. A native preset runs the binary it found on
`PATH` with `args`. An adapter preset runs as today.

- `version` (run another adapter version) means something only for `Adapter`. On a native entry
  it is kept in the file, ignored at launch, and Settings does not offer it.
- docs/agents.md's promise for a native preset is narrower and says so: onehand tried the agent
  version named in the table. The agent updates itself, so a break shows up as a report, the same
  as today with adapters.

### 3. An unknown preset is kept and refused

The entry's `preset` field keeps the id verbatim whether or not this build knows it (e.g. an
enum `Known(Preset) | Unknown(String)`, or an id string plus a lookup; the builder chooses). A
helper returns `Option<Preset>` for the app to match on. An unknown entry:

- loads with the rest of the file (B13: today one such entry throws the whole file away);
- shows in Settings ▸ Agents as *Not supported by this build* and offers no Test or Edit, only
  Remove;
- refuses to start with a sentence: *`kimi` is not an agent this build of onehand knows; update
  onehand, or remove it in Settings ▸ Agents*;
- writes back exactly as read.

### 4. Harness guesses move into core

- **Selector roles.** `Selector` (core) gains `role: Option<Role>`, with `Role` one of `Model`,
  `Effort`, `Fast`. It is read first from ACP's `category` on the config option (`model`,
  `thought_level`), parsed in `acp/parse.rs`. When there is no category, it falls back to the
  name lists now in `chat/composer/presentation.rs` (`effort`/`reasoning_effort`,
  `fast`/`fast-mode`), moved verbatim into core. The fast switch's on/off word lists move too.
  The composer matches on `Role` and keeps no list.
- **The interruption marker** (`[Request interrupted by user…]`) is recognised in the ACP parse
  step and becomes its own event, instead of a check inside the reducer.
- **Mode ids that are URLs** (Copilot) get their label from the mode's `name`, as today. Nothing
  slices an id. This is called out because Copilot is the first agent whose ids are not words.
- The permission card's command-vs-action guess (audit A3) stays where it is in R1. Only Claude
  and Codex exercise it, and moving it without a third agent to test against is churn.

### 5. The serve loop becomes `Conn`, and a session is opened with `SessionOpts`

`handle_message`'s state (`pending`, `next_id`, `ready`, `session_id`, `images`, the terminal
registry and the channels) becomes one struct, `Conn`, owned by the loop. Its methods are the
current branches. `connect(command, args, env, cwd, resume)` becomes `connect(launch, opts:
SessionOpts { cwd, resume })`. `mcp_servers` is **not** added in R1. R3 adds the field where it
is used. This is a pure refactor: the existing client tests and the smoke test pass unchanged in
behaviour.

### 6. The login sentence can come from the agent

`initialize`'s `authMethods` is parsed. When an agent answers `auth_required`, a built-in entry
still gets its table's command, under its account variable (unchanged). A declared entry gets the
first auth method's `description` when the agent sent one (*Run `copilot login` in the
terminal*). Without one it gets today's *X needs you to log in*.

### 7. Secrets: one rule in core

`pick_token` and `warn_if_shared` move from `crates/telegram/src/secret.rs` into core as
`config::secret(name, env_override) -> Option<String>`. The rule is: an environment variable
first, then `<config_dir>/onehand/<name>.secret` (or the existing `telegram.token` name for
Telegram, kept for compatibility), then a warning when that file is readable by others. Telegram
calls it with no change in behaviour. Agent `env` stays plain values; it never names a secret.

## Steps (one PR each, in order)

| Step | What | Depends on |
|---|---|---|
| 1 | Document edit for `onehand.toml` (decision 1); split `core/config.rs` | — |
| 2 | Preset table with `Adapter`/`Native` (2), unknown preset kept (3) | 1 (step 3's write-back relies on it) |
| 3 | Copilot as the first native built-in preset | 2 |
| 4 | `Conn` + `SessionOpts` (5), `authMethods` login sentence (6); split `acp/client.rs` | — |
| 5 | Selector roles and the interruption event in core (4) | 4 |
| 6 | `config::secret` in core (7) | — |

Each step updates the document that owns what it changes (docs/agents.md for 2, 3 and 6, the
config comments for 1) and `CHANGELOG.md`.

## Testing

- Step 1: round-trip tests on a file with a comment, an unknown top-level key, an unknown key in
  `[remote.telegram]` and an unknown preset. Each must survive a save of another setting.
- Step 2: `launch()` for each `Runs` kind and for a missing binary; an unknown id loads, refuses
  to start, and writes back verbatim.
- Step 3: each new preset was smoke-tested by hand with `ACP_PRESET=<id> cargo run -p
  onehand-core --example acp_smoke`, recorded in the PR.
- Steps 4 and 5: the existing client and reducer tests unchanged; new tests for `category`-first
  roles, the URL mode id, and the interruption event.

## Left to the builder

- The exact Rust shape of the preset id (decision 3) and of `Conn`'s methods.
- Where the document diff lives (a `config/document.rs` sibling is the obvious seam).
- The Settings words for an unknown or native entry, within the `ui` skill and its checklist.

## Answered by the owner

1. **Step 3 adds Copilot only** (`copilot --acp`, binary `copilot`, login `copilot login`).
   OpenCode is declared by hand as today. Gemini waits until its silence is understood.
2. **Agent `env` does not name secrets**, in R1 or as part of this design. Step 6 only moves the
   rule into core for Telegram and the connectors to come.
