# Agents

Every session is an ACP agent, started from an entry in the agent list (`[[agents]]` in
`onehand.toml`, edited in Settings ▸ Agents). An entry is one of two kinds.

| | **Built-in agent** | **Declared agent** |
|---|---|---|
| Which | Claude Code, Codex, Copilot | any other ACP agent |
| Written as | `preset = "<id>"`, the command left to onehand | `name`, `command`, `args`, written out |
| Adapter or agent version | chosen and tried by onehand | the person's |
| Promise | everything below | what the protocol gives, plus the generic guesses onehand makes |

## What built-in promises

- **The adapter version each release pins was tried against that agent** with the headless smoke
  test (`cargo run -p onehand-core --example acp_smoke`), run by hand before a pin moves. There is
  no scheduled check: the agent is the person's own and updates without a release, so a new agent
  that breaks the pinned adapter shows up as a report, and the fix is moving the pin.
- **An agent that speaks the protocol itself has no adapter and no pin.** Each release names the
  agent version it was tried with; the agent updates itself, so a break shows up as a report, as
  with an adapter.
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

| | `claude-code` | `codex` | `copilot` |
|---|---|---|---|
| Shown name | Claude Code | Codex | Copilot |
| Runs | through an npm adapter | through an npm adapter | itself (`copilot --acp`) |
| Adapter | `@agentclientprotocol/claude-agent-acp`, pinned at 0.81.2 | `@agentclientprotocol/codex-acp`, pinned at 2.2.2 | none; tried with Copilot 1.0.102, which updates itself |
| Binary | `claude` | `codex` | `copilot` |
| Variable pointing the adapter at it | `CLAUDE_CODE_EXECUTABLE` | `CODEX_PATH` | none |
| Unattended mode when the config names none | `acceptEdits` | its own default (*Auto review*) | its own default (*Agent*), which asks before every step; see unattended runs |
| Effort and fast groups | `effort`, `fast` | `reasoning_effort`, `fast-mode` | none |

The presets live in code (`crates/core/src/config/agent.rs`). The config names one by id and never
copies its command: every save of the agent list writes the whole list back, so a command written
out there froze the adapter version at the first save. A save writes only what it changed, so
comments and keys from a newer build stay in the file.

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
- **A `preset` this build does not know** (written by a newer one) keeps its entry: Settings shows
  it as *Not supported by this build of onehand* with only *Delete…*, and it refuses to start. A
  save keeps its id as written, with the fields this build knows (`name`, `version`, `env`…); a key
  only the newer build knows is lost when the agent list is saved. With no `name` it is called,
  and saved, by its id.

## First launch

When no config file is read, or the file names no `agents`, the list is the built-in agents
installed on the machine, Claude Code first (`AppConfig::from_disk`). It is looked up while the
config loads, a few file checks on `PATH`, so no settings save can race it, and it reaches the file
with the first save. A list the person wrote, even `agents = []`, is never looked up again.

- **No agent in the list:** the agent pane says *No agent is set up*. Each built-in agent installed
  on the machine is offered there with *Add*; each one that is not shows the npm command that
  installs it (`npm install -g @anthropic-ai/claude-code`, `npm install -g @openai/codex`,
  `npm install -g @github/copilot`). A way to
  Settings ▸ Agents follows. The offer is what makes a restart after installing enough even when an
  unrelated save already wrote the empty list.
- **Installed later:** the built-in agents on the machine are looked up once at start
  (`Shared::on_machine`), and Settings ▸ Agents offers each one with no entry: *Codex is on this
  machine*, *Add*. Opening Settings looks at nothing on disk.
- **The default:** the first row, marked *Default*, is what *New session*, a new project's first
  session, an unattended run with no `unattended.agent` and a workflow step naming no agent use.

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
  use a built-in agent, which knows its own mode; for a declared agent it is kept. The old default
  left declared still counts as Claude Code for that mode, and the mode is read when each run starts,
  so making another agent the default changes it.

## Not logged in

An agent that answers a request with ACP's `auth_required` (`-32000`) is not logged in: Codex
answers `session/new` so, Claude Code only its first prompt. The transcript says it once, in place
of the failure that follows (`AgentSpec::login_hint`):

- a built-in agent: *Codex is not logged in. Run codex login in a terminal, then restart the
  agent*. Claude Code's command is `claude auth login`, Copilot's `copilot login`. When the
  entry's `env` sets its account variable (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME`), the
  command carries it, so the login reaches that
  account: `CLAUDE_CONFIG_DIR=/home/me/.claude-work claude auth login`, the directory in single
  quotes when it holds a space or anything else a shell would read. The line is drawn as text, so
  the command carries no backticks;
- a declared agent: *Gemini needs you to log in*.

Logging in inside onehand (ACP `authenticate`) is not built.

## Questions

A question's free-text answer is its card's *Other* box. Claude Code sends it as `<key>_custom`;
Codex as a property of its own whose `_meta.codex` names the question (`questionId`) and the role
`user_note`. Both are folded into the question they belong to, read from the request itself, so a
declared agent sending either gets the same card.

## Known differences

- **Codex has no plan block.** It has no plan tool in its default mode and writes its steps as
  text. With `collaboration_mode = plan` it writes the plan as text, then asks *Implement this
  plan?* as a permission request.
- **Codex runs its commands itself**, not through the client's terminal: a command shows as a tool
  call with its output, without a live terminal.
- **The Plugins mode is in the Workbench only where a Claude Code agent is set up**, and manages
  Claude Code's plugins for the default account only (the onehand process's own
  `CLAUDE_CONFIG_DIR`, else `~/.claude`), whichever Claude entry a session uses.
  Codex's plugins and skills (`codex plugin`, `~/.codex/skills`) are loaded by Codex itself and not
  managed in onehand.
- **Copilot's mode ids are URLs** (`https://agentclientprotocol.com/protocol/session-modes#agent`,
  `#plan`, `#autopilot`). The chip and its menu show each mode's name; a workflow step or
  `unattended.mode` naming one writes the whole id.
- **Copilot offers an *Allow All* switch** (`allow_all`) among its options, and no effort or fast
  group.
