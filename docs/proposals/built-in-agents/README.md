# Proposal: Claude Code and Codex as built-in agents

- Date: 2026-10-21, after a brainstorm and a day of trials against the real adapters.
- Status: proposal. Nothing here is a requirement until a task asks for it; once a piece is built,
  its text moves into the owning documents and leaves this folder. The contracts are
  [CONTEXT.md](../../../CONTEXT.md), [DESIGN.md](../../../DESIGN.md),
  [screens.md](../../screens.md), [workflows.md](../../workflows.md) and
  [unattended.md](../../unattended.md).

## The gap

onehand already hosts any ACP agent: the agent list is config, each session has its own agent, and
*New ▸ With agent* starts one with any of them. But the app is shaped around one agent. The only
default is Claude Code, written into the config with its adapter version, and several features
guess at Claude's names. A person who uses Codex has to type an adapter command by hand, and then
meets an unattended run refused out of the box, a permission card that does not say the command,
and no effort chip.

The goal, in one sentence: **Claude Code and Codex are supported officially, found on the machine
and equal in every feature; every other agent is declared and gets what ACP itself gives.**

## Two kinds of agent

| | **Built-in agent** | **Declared agent** |
|---|---|---|
| Which | Claude Code, Codex | any other ACP agent |
| Added by | found on the machine, or picked in Settings ▸ Agents | the person, writing `name`, `command`, `args` |
| Adapter version | chosen and tried by onehand | the person's |
| Promise | every feature below works | what standard ACP gives, plus the generic guesses already in place |

What *built-in* promises, to be written down in one owning document (a new `docs/agents.md`):

1. Each onehand release names an adapter version per built-in agent that passed the headless smoke
   test against that agent.
2. The transcript, diffs, permission and question cards, modes, the model/effort/fast chips,
   resume, workflow steps and unattended runs work alike for both.
3. The first launch finds them, and a missing login is said with the command that fixes it.

A declared agent keeps everything that works for it today. Nothing is taken from it; nothing more
is promised.

## The pieces

### 1. Presets: a built-in agent lives in code

Core holds one **preset** per built-in agent. The config names a preset by id and never copies its
command:

```toml
[[agents]]
preset = "claude-code"

[[agents]]
preset = "codex"

[[agents]]                 # a declared agent, written out in full as today
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
```

**Why by id.** Every settings save rewrites the whole `onehand.toml` (`AppConfig::update_in_place`),
the agent list included. Today that freezes `claude-agent-acp@0.81.2` in the person's file at their
first save, and a later release that bumps the pin never reaches them.

What a preset carries:

| | Claude Code | Codex |
|---|---|---|
| id, shown name | `claude-code`, Claude Code | `codex`, Codex |
| Adapter | `@agentclientprotocol/claude-agent-acp@<pin>` | `@agentclientprotocol/codex-acp@<pin>` |
| Binary looked for | `claude` | `codex` |
| Variable pointing the adapter at it | `CLAUDE_CODE_EXECUTABLE` | `CODEX_PATH` |
| Login command shown | to be confirmed | `codex login` |
| Effort group id | `effort` | `reasoning_effort` |
| Fast group id | `fast` | `fast-mode` |
| The question card's free-text field | the `…_custom` property | the separate *Additional answer or note* field |
| Unattended mode | `acceptEdits` | to be decided by trial (see *Open*) |

**Migration.** An agent entry whose command and args are exactly the old Claude default
(`npx … @agentclientprotocol/claude-agent-acp@…`) becomes `preset = "claude-code"` at the next
save. An entry the person edited stays as it is, as a declared agent.

### 2. The agent is the person's own; onehand pins only the adapter

Both adapters ship their agent inside: `claude-agent-acp@0.81.2` carries Claude Code 2.1.280 and
`codex-acp@2.2.2` carries Codex 0.160.1, while the machine these trials ran on had 2.1.306 and
0.162.1. So onehand today runs an older agent than the person's terminal.

- A built-in agent runs the binary found on the machine. The preset sets the variable in the
  table above to its absolute path when it starts the adapter. `AgentSpec` has no environment
  today; this is new.
- The adapter is installed without the bundled agent (`npm_config_omit=optional`). Tried: Claude's
  adapter is 65 MB instead of 281 MB and Codex's 18 MB instead of 444 MB, and both ran a full turn
  on the machine's agent.
- **onehand checks the binary is there before starting the adapter**, and when it is not, says
  *"`codex` is not on this machine — install it again, or remove Codex in Settings ▸ Agents"* and
  starts nothing. Tried: with no binary, Claude's adapter hangs silently at the first prompt and
  Codex's app-server dies with the reason only on stderr; neither reaches onehand as a failure.
- The adapter stays pinned, as `DEFAULT_ACP_ADAPTER` is today. A person can leave the pin with
  `version = "latest"` or an exact version beside `preset`; Settings ▸ Agents then says the agent
  runs outside the tried version.
- Settings ▸ Agents shows each built-in agent's own version, read from `--version`.
- An agent update needs no onehand release. A release is needed only when the adapter must move:
  an ACP feature onehand wants, or an agent the pinned adapter no longer drives.

### 3. First launch

Only when no config file exists:

1. The window opens at once; no wizard. In the background, onehand looks for `claude` and `codex`
   on `PATH`.
2. Each one found is added as its preset. Claude Code comes first when both are found.
3. None found: the agent pane shows an empty state saying no agent is set up, with how to install
   each built-in agent and a way to Settings ▸ Agents to declare another.

A person with a config is never rescanned. Settings ▸ Agents instead offers a built-in agent that is
on the machine but not in the list (*Codex is on this machine — Add*).

**The default agent.** The first agent in the list is the default (`config::agent_or_first`):
the main part of *New session*, `Ctrl+Shift+O`, a new project's first session, an unattended run
with no `unattended.agent`, and every workflow step that names no agent. Reordering the list
changes all of them, so Settings ▸ Agents marks the first row *Default*.

### 4. Not logged in

Tried: a logged-out Codex answers `session/new` with `{"code":-32000,"message":"Authentication
required"}`, and onehand shows that JSON followed by npm's stderr (`npm notice run npx · …`).

- Core reads `-32000` (ACP's `auth_required`) as a failure kind of its own, not a string.
- The agent pane says *"Codex is not logged in. Run `codex login` in a terminal, then Restart the
  agent."*, the command from the preset. A declared agent gets *"<name> needs you to log in."*
- npm's `notice` lines are dropped from a failure's text.
- No login inside onehand (ACP `authenticate`) in this proposal.

### 5. Codex at Claude's level

Found by running Codex in the app:

1. **The permission card says *Run command*, not the command.** `parse_permission` takes the
   request's own title; Claude puts the command there, Codex a generic label. When the request's
   `toolCallId` matches a known tool call, the card uses that call's title. Generic, no preset.
2. **The question card splits Codex's free-text field into a second question** (*Question 1 of 2*).
   The preset says how Codex sends it and the card folds it into the question it belongs to.
3. **The status line under a question card says *Waiting for your approval…*.** It says the
   question is waiting for an answer. Generic.
4. **No effort or fast chip for Codex.** The chips read the group ids from the preset; a declared
   agent keeps today's guess by name.
5. **Unattended runs are refused for Codex.** `UnattendedConfig::default()` sets
   `mode = "acceptEdits"` and the preflight blocks an agent that does not offer it. The default
   mode comes from the preset; a declared agent runs in its own default mode.
6. **The Plugins mode** manages Claude Code's plugins only. It shows when the list holds the
   `claude-code` preset and not otherwise.
7. **Unattended permissions** in [unattended.md](../../unattended.md) are told for Claude only
   (`.claude/settings.json`). Codex gets its own part.

Generic fallbacks that stay as they are: `_meta.claudeCode.title` for a tool's description, the
fence stripping on tool output, the `question_N` field order.

### 6. A weekly check against the newest agents

With the agent the person's own, the one early warning that a new agent breaks the pinned adapter is
a scheduled CI job: install the newest `claude` and `codex`, run the smoke test on each through the
pinned adapter, fail loudly. It needs the agents' credentials as CI secrets.

## Order of work

Each step is one pull request with its document changes in it.

1. **The generic fixes** (piece 5 items 1 and 3, and piece 4's wording): the permission card's
   title, the question's status line, a refused `session/new` said in the agent's words
   (*Could not start a session: Authentication required*) and npm's `notice` lines dropped from
   the failure. No preset needed; every agent gains.
2. **Presets** (pieces 1 and 2): the preset table, `preset = "…"` and `version` in the config, the
   migration, the environment on start, the install without bundled agents and the binary check.
   The glossary gains *built-in agent*, *declared agent* and *preset*; `docs/agents.md` is written.
3. **First launch** (piece 3): the scan, the empty state, *on this machine — Add* and the *Default*
   mark. Follows the `ui` skill.
4. **Parity through presets** (piece 4's failure kind and login command, piece 5 items 2, 4, 5,
   6, 7): needs the trials under *Open* first.
5. **The weekly check** (piece 6).

**Specs.** Each step gets a spec issue before it is built, written from this folder as it stands
then, one step at a time and in this order. Publishing one writes its number here.

| Step | Spec | Built |
|---|---|---|
| 1 | none: small enough to be specified here | |
| 2 | | |
| 3 | | |
| 4 | | |
| 5 | | |

## Open

To settle by trial before step 4:

- **Codex's unattended mode.** Its modes are *Read-only*, *Workspace access*, *Auto review* and
  *Full access*. Read-only asks before writing a file; Auto review (its default) edited and ran a
  command without asking. Which one finishes an issue that edits files, runs a networked command
  and pushes, without parking on a card nobody answers?
- **Codex's plan.** It said no plan tool was available in its default mode; it has a `/plan` command
  and a *Collaboration mode* setting. Does the plan block render?
- **Claude's login.** Does a logged-out Claude adapter answer `-32000` too, and which command does
  it need?
- **Claude without its binary.** It hung for the 90 s it was watched; whether it ever fails was not
  seen. The binary check makes this moot for built-in agents.

## Not in this proposal

- Logging in inside onehand.
- Managing Codex's plugins or skills.
- Fetching agents from an ACP registry.
- Moving a conversation from one agent to another, or one prompt run by two agents side by side.
