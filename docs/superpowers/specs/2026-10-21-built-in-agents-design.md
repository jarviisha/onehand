# Design: Claude Code and Codex as built-in agents

- Date: 2026-10-21.
- Source: [docs/proposals/built-in-agents/](../../proposals/built-in-agents/README.md), reviewed against
  the code and re-tried against the real adapters on the same day. This document supersedes it
  where the two differ; the differences are listed under *Changes from the proposal*.
- Contracts that still govern: [CONTEXT.md](../../../CONTEXT.md), [DESIGN.md](../../../DESIGN.md),
  [screens.md](../../screens.md), [workflows.md](../../workflows.md),
  [unattended.md](../../unattended.md). Every visible change follows the `ui` skill.

## Intent

onehand is going open source. A person who installs it must be able to use Claude Code and Codex
without writing an adapter command by hand, and both must work equally well in every feature. The
owner uses both every day, including two Claude accounts side by side.

Today the app is shaped around one agent. The only default is Claude Code, written into the config
with its adapter version, and several features guess at Claude's names. A Codex user types the
adapter command by hand, then meets: an unattended run refused out of the box, a permission card
that says *Run command* instead of the command, no effort or fast chip, and a raw JSON error plus
npm noise when not logged in.

## Goals

1. **Two kinds of agent.** *Built-in* (Claude Code, Codex): found on the machine, adapter version
   chosen and tried by onehand, every feature works. *Declared* (any other ACP agent): written out
   by the person as today, gets what ACP gives plus the existing generic guesses. Nothing a
   declared agent does today is taken away.
2. **The person's own agent runs**, not the copy bundled inside the adapter. onehand pins only the
   adapter.
3. **A new user starts without configuring anything** when `claude` and/or `codex` is on `PATH`.
4. **Several accounts of one built-in agent** (e.g. two Claude Code entries pointing at different
   `CLAUDE_CONFIG_DIR`s) are first-class: each entry is a built-in agent with every promise above.
5. **A missing login is said in words, with the exact command that fixes it** for that entry.

## Non-goals

- Logging in inside onehand (ACP `authenticate`).
- Managing Codex's plugins or skills. Codex has its own (`codex plugin add/list/remove`,
  `~/.codex/skills`); because the person's own binary now runs, Codex loads them by itself. A
  Codex counterpart to the Plugins mode is a separate project.
- Fetching agents from an ACP registry.
- Moving a conversation between agents, or running one prompt on two agents side by side.
- Per-agent default *session* mode in config. The session mode stays a per-session choice on the
  composer's chip.

## Anti-goals (would be a failure even if it works)

- Any existing config, including one with hand-edited agents, loads into something that starts a
  different agent, loses an agent, or silently changes which account an entry uses.
- A built-in agent whose binary is missing hangs or fails with only stderr to explain it.
- A release bump of the adapter pin fails to reach people who already have a config file.
- An unattended run parks forever on a card nobody answers because of a mode default.

## Constraints (from the repository)

- Core stays GUI-free and runtime-free; presets, migration, the failure kind and the binary lookup
  live in core (`crates/core/src/config.rs`, `crates/core/src/acp/`).
- `AppConfig::update_in_place` rewrites the whole `onehand.toml` on every settings save; whatever
  the in-memory config holds is what gets written.
- Agents are identified **by name** everywhere: sessions (`shell/sessions.rs`), conversation
  archives (`chat/store.rs`), workflow steps, `unattended.agent`, `config::agent_or_first`.
- `AgentSpec` is used in about 20 files (settings form, `agent_check`, `Shared::saw_offer`, which
  keys offers by spec equality, unattended facts, workflow drafts).
- `config::find_command` already resolves a command on `PATH`; reuse it for every binary lookup.

## Decisions

### 1. Presets live in code; config names them by id

Core holds one preset per built-in agent:

| | Claude Code | Codex |
|---|---|---|
| id, shown name | `claude-code`, Claude Code | `codex`, Codex |
| Adapter | `@agentclientprotocol/claude-agent-acp@<pin>` (today 0.81.2) | `@agentclientprotocol/codex-acp@<pin>` (today 2.2.2) |
| Binary looked for | `claude` | `codex` |
| Variable pointing the adapter at it | `CLAUDE_CODE_EXECUTABLE` | `CODEX_PATH` |
| Account variable (for the login hint) | `CLAUDE_CONFIG_DIR` | `CODEX_HOME` |
| Login command | `claude auth login` | `codex login` |
| Unattended mode when the config sets none | `acceptEdits` | none: the agent's default (`agent`, *Auto review*) |
| Question card's free-text field | the `…_custom` property (works today) | the separate *Additional answer or note* field (proposal's finding; not re-tried) |

```toml
[[agents]]
preset = "claude-code"                      # name defaults to "Claude Code"

[[agents]]
preset = "claude-code"
name = "Claude Code (acc2)"
env = { CLAUDE_CONFIG_DIR = "/home/me/.claude-acc2" }

[[agents]]
preset = "codex"
version = "latest"                          # optional: leave the pin

[[agents]]                                  # declared, as today, now with env too
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
env = { SOME_VAR = "x" }
```

- A preset may appear **more than once**. `name` is optional; it defaults to the preset's shown
  name. **Names are unique across the whole list**, presets and declared agents together; the
  settings form refuses a duplicate, and a file that already holds one is renamed on load
  (decision 3).
- **`env` is a new field on every agent**, built-in or declared: a map of variables set on the
  adapter process. It replaces the `command = "env"` trick.
- `version` is optional on a preset entry: absent means the pin; `"latest"` or an exact version
  leaves it, and Settings ▸ Agents says that entry runs outside the tried version.
- The adapter is started as `npx --prefer-offline -y <package>@<version>` with
  `npm_config_omit=optional` set, so the bundled agent is not installed, and with the preset's
  binary variable set to the absolute path found by `find_command`. Any `env` the entry sets is
  applied after, so a person can override either.

### 2. The binary is checked before the adapter starts

If the preset's binary is not on `PATH`, onehand starts nothing and the agent pane says
*"`codex` is not on this machine — install it again, or remove Codex in Settings ▸ Agents"* (with
the entry's own name). Settings ▸ Agents shows each built-in entry's version from `<binary>
--version`, read in the background.

### 3. Migration of existing configs

Applied when a file is loaded, written at the next save:

- **Only when `claude` is found on `PATH`** (design call: the old default runs the agent bundled
  in the adapter, so migrating without the binary would turn a working agent into a refusal):
  - An entry whose command is `npx` with exactly the old default arguments
    (`--prefer-offline -y @agentclientprotocol/claude-agent-acp@0.81.2`) becomes
    `preset = "claude-code"`, keeping its `name`.
  - An entry whose command is `env` followed by one or more `VAR=value` arguments and then exactly
    that `npx` default becomes the same preset with those pairs moved into `env`, keeping its
    `name`. (The owner's `Claude Code (acc2)` is this case.)
- Without `claude` on `PATH`, those entries stay declared and keep working as today.
- Anything else stays as written, as a declared agent.
- **Duplicate names** in an existing file (nothing prevented them before): the second and later
  get ` (2)`, ` (3)`… appended on load, so a save writes every entry back and the list becomes
  unique (design call). References by name already resolved to the first; they still do.
- `unattended.mode` becomes optional, and absent and empty stay distinct (design call):
  absent = the preset's unattended mode, or the agent's own default for a declared agent;
  `""` = the agent's own default, as today. A stored `"acceptEdits"`, the old default, becomes
  absent **only when the unattended agent resolves to a preset entry**; for a declared agent it is
  kept, so its behaviour does not change. The clearing and the "absent" meaning ship together.

The only adapter pin that ever shipped is 0.81.2 (`git log -S` on `config.rs`), so matching that
exact version is enough.

### 4. First launch

Only when no config file exists, or the file has no `agents` key (design call: today both get
the built-in default, so both get the scan instead). A file that fails to parse is still never
written, as today; its in-memory list is the scan.

1. When building the initial config, look up `claude` and `codex` with `find_command` (a few file
   stats, done synchronously while the config loads; no race with a settings save). Each one found
   becomes its preset entry; Claude Code first when both are found.
2. The same rule applies to `update_in_place`'s missing-file branch, so no save can write a list
   that differs from the scan.
3. None found: the agent pane shows an empty state saying no agent is set up, how to install each
   built-in agent, and a way to Settings ▸ Agents to declare another.

A person with a config is never rescanned. Settings ▸ Agents offers a built-in agent that is on the
machine but has no entry (*Codex is on this machine — Add*). The first row is marked *Default*,
since the first agent is the default everywhere (`config::agent_or_first`).

### 5. Not logged in

Tried: Codex refuses `session/new` with `{"code":-32000,"message":"Authentication required"}`;
Claude accepts `session/new` and refuses the **first `session/prompt`** with the same error.

- Core reads `-32000` (ACP `auth_required`) as its own failure kind, on both requests.
- For a preset entry the pane says *"Codex is not logged in. Run `codex login` in a terminal, then
  Restart the agent."*, with the entry's account variable prefixed when its `env` sets one:
  `CLAUDE_CONFIG_DIR=/home/me/.claude-acc2 claude auth login`. A declared agent gets *"<name>
  needs you to log in."*
- npm's `notice` lines are dropped from any failure's text.

### 6. Parity fixes

Generic (every agent gains, no preset):

1. **Permission card title.** When the request's `toolCall.toolCallId` matches a known tool call,
   the card uses that call's title. Tried: Codex sends `title="Run command"` while the matching
   tool call carries the command itself.
2. **Status line under a question card** says the question is waiting for an answer, not *Waiting
   for your approval…* (`Turn::activity_status` treats both kinds the same today).
3. **Effort and fast chips** also recognise Codex's group ids, `reasoning_effort` and
   `fast-mode`, by adding them to the names the composer already matches (`EFFORT_GROUP`,
   `CHIP_GROUP` in `composer/presentation.rs`). Declared agents using those ids gain too.

Through presets:

4. **Codex's question card** folds the separate free-text field into the question it belongs to.
5. **Unattended mode** comes from the preset when the config sets none (decision 3; ships in
   step 2 with the migration that clears the old default).
6. **The Plugins mode** shows only when the list holds at least one `claude-code` entry. The
   Workbench has no way to hide a mode today; that mechanism is new. It keeps managing the
   **default account only** (the onehand process's own `CLAUDE_CONFIG_DIR`, else `~/.claude`),
   whichever Claude entry a session uses. Decided by the owner; documented as a known difference.
7. **Unattended permissions** in `unattended.md` gain a part for Codex.

Known differences, documented in `docs/agents.md`, nothing to build:

- Codex has no plan tool: no plan block renders. With `collaboration_mode = plan` it writes the
  plan as text and asks *"Implement this plan?"* as a permission request, which the existing card
  shows.
- Codex runs its commands itself rather than through the client's terminal (no
  `terminal/create` was seen); its commands appear as tool calls.

### 7. No CI check against the agents

Decided by the owner: no scheduled CI job (it would need paid credentials as repository secrets).
Instead, before an adapter pin is bumped, the headless smoke test is run by hand against each
built-in agent through the new pin, and `docs/agents.md` says so as part of the release promise.

### 8. Documents

A new `docs/agents.md` owns what *built-in* promises (the adapter version per release passed the
smoke test; the features above work alike; first launch finds them; a missing login is said with
its command) and the known differences. CONTEXT.md gains *built-in agent*, *declared agent* and
*preset*. When this work is done the proposal folder leaves `docs/proposals/`.

## Order of work

Each step is one pull request with its document changes.

1. **Generic fixes**: decision 6 items 1–3, a refused request said in the agent's words, npm
   `notice` lines dropped. No preset needed.
2. **Presets**: decisions 1, 2, 3 and decision 6 item 5; `env` on `AgentSpec`; glossary;
   `docs/agents.md`.
3. **First launch**: decision 4. Follows the `ui` skill.
4. **Parity through presets**: decision 5 and decision 6 items 4, 6, 7.

## Trial record (2026-10-21)

Machine agents: Claude Code 2.1.306, codex-cli 0.162.1. Adapters installed with
`--omit=optional`: both together 58 MB. A throwaway ACP driver advertised the same client
capabilities as onehand and auto-allowed permissions, counting them.

| Trial | Result |
|---|---|
| Codex via `CODEX_PATH`, no bundled agent | full turn |
| Codex modes offered | `read-only`, `workspace-write`, `agent` (default), `agent-full-access` |
| Codex config groups | `mode`, `collaboration_mode[default/plan]`, `model`, `reasoning_effort`, `fast-mode` |
| Claude config groups | `mode`, `model`, `effort`, `fast` |
| Codex in `agent`: fix a file, `curl` out, commit and push | 0 permission requests; sandbox refusals self-escalated through *Guardian Review*; pushed |
| Codex in `workspace-write`, same task | 2 permission requests (the `curl`, the push) |
| Codex plan, default mode | "No plan/todo tool is available"; steps as text |
| Codex plan, `collaboration_mode = plan` | plan as text, then a permission request *Implement this plan?* |
| Claude logged out (empty `CLAUDE_CONFIG_DIR`) | `session/new` ok; first prompt → `-32000 Authentication required`; `authMethods: []` |

## Changes from the proposal

- A preset may repeat, with its own name and `env`; `env` is a field on every agent.
- Migration also recognises the `env VAR=… npx …` form.
- `unattended.mode` becomes optional and the stored old default is cleared for preset agents; the
  proposal missed that the default is already written into every existing file.
- First-launch lookup is synchronous at load; the proposal's background scan raced settings saves.
- Codex's unattended mode is settled (its default) and needs no preset value.
- `-32000` is handled on `session/prompt` too; Claude's login command is `claude auth login`.
- The effort and fast chips become generic (step 1) rather than preset-driven.
- No weekly CI check; a manual smoke test before each pin bump.
- The Plugins mode stays on the default Claude account.
- Codex's missing plan block is a documented difference, not work.

## Left to the builder

- The Rust shape of `AgentSpec` (enum of preset/declared, or optional fields) and how it serializes
  back to the TOML above.
- How the Workbench hides a mode.
- The exact wording of empty states and hints, within the `ui` skill.
- Where the migration runs (on parse, or on load before first use), as long as a save writes it.
