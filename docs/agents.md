# Agents

Every session is an ACP agent, started from an entry in the agent list (`[[agents]]` in
`onehand.toml`, edited in Settings ▸ Agents). An entry is one of two kinds.

| | **Built-in agent** | **Declared agent** |
|---|---|---|
| Which | Claude Code, Codex | any other ACP agent |
| Written as | `preset = "<id>"`, the command left to onehand | `name`, `command`, `args`, written out |
| Adapter version | chosen and tried by onehand | the person's |
| Promise | everything below | what the protocol gives, plus the generic guesses onehand makes |

## What built-in promises

- **The adapter version each release pins was tried against that agent** with the headless smoke
  test (`cargo run -p onehand-core --example acp_smoke`), run by hand before a pin moves. There is
  no scheduled check: the agent is the person's own and updates without a release, so a new agent
  that breaks the pinned adapter shows up as a report, and the fix is moving the pin.
- **The agent is the person's own.** The adapter is installed without the agent bundled inside it
  (`npm_config_omit=optional`) and pointed at the binary on `PATH`, so a session runs the same
  agent as the person's terminal. An agent update needs no onehand release.
- **A missing binary is said before anything starts**: `` `codex` is not on this machine —
  install it again, or remove Codex in Settings ▸ Agents ``. Without the check one adapter hangs at
  the first prompt and another dies with its reason on stderr only.
- **The transcript, diffs, permission and question cards, modes, the model/effort/fast chips,
  resume, workflow steps and unattended runs work alike for both**, with the known differences
  below.

## Presets

| | `claude-code` | `codex` |
|---|---|---|
| Shown name | Claude Code | Codex |
| Adapter | `@agentclientprotocol/claude-agent-acp`, pinned at 0.81.2 | `@agentclientprotocol/codex-acp`, pinned at 2.2.2 |
| Binary | `claude` | `codex` |
| Variable pointing the adapter at it | `CLAUDE_CODE_EXECUTABLE` | `CODEX_PATH` |
| Unattended mode when the config names none | `acceptEdits` | its own default (*Auto review*) |
| Effort and fast groups | `effort`, `fast` | `reasoning_effort`, `fast-mode` |

The presets live in code (`crates/core/src/config/agent.rs`). The config names one by id and never
copies its command: every settings save writes the whole file back, so a command written out there
froze the adapter version at the first save.

## The config

```toml
[[agents]]
preset = "claude-code"                      # name defaults to "Claude Code"

[[agents]]
preset = "claude-code"
name = "Claude Code (work)"
env = { CLAUDE_CONFIG_DIR = "/home/me/.claude-work" }

[[agents]]
preset = "codex"
version = "latest"                          # leave the pin; Settings says it runs outside it

[[agents]]                                  # a declared agent
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
env = { SOME_VAR = "x" }
```

- **A preset may appear more than once**, each entry with its own `name` and `env`: that is how two
  accounts of one agent sit side by side. Names are unique across the whole list.
- **`env`** is set on the adapter after onehand's own variables, so an entry can override either.
- **`version`** on a preset runs that adapter version instead of the pin.
- The first entry is the default agent everywhere (`config::agent_or_first`).

## Migration

A config is settled every time it is read, and a save writes the settled list
(`AppConfig::settle`):

- An entry that is exactly the old default, `npx --prefer-offline -y
  @agentclientprotocol/claude-agent-acp@0.81.2`, alone or behind `env VAR=value…`, becomes
  `preset = "claude-code"` with those variables in `env` and its name kept. **Only when `claude` is
  on `PATH`**: the old entry ran the agent bundled in its adapter, so without a binary it stays as
  written and keeps working. Any other entry stays as written.
- A preset with no name takes its shown name; a name already taken earlier in the list gets ` (2)`,
  ` (3)`…, so no entry is dropped.
- `unattended.mode = "acceptEdits"`, the old default written into every file, is dropped when runs
  use a built-in agent, which knows its own mode; for a declared agent it is kept.

## Known differences

- **Codex has no plan block.** It has no plan tool in its default mode and writes its steps as
  text. With `collaboration_mode = plan` it writes the plan as text, then asks *Implement this
  plan?* as a permission request.
- **Codex runs its commands itself**, not through the client's terminal: a command shows as a tool
  call with its output, without a live terminal.
- **The Plugins mode manages Claude Code's plugins for the default account only** (the onehand
  process's own `CLAUDE_CONFIG_DIR`, else `~/.claude`), whichever Claude entry a session uses.
  Codex's plugins and skills (`codex plugin`, `~/.codex/skills`) are loaded by Codex itself and not
  managed in onehand.
