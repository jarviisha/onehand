# R3: onehand's tools for an agent, and a socket beside the lock

- Status: draft for the owner's approval.
- Source: the architecture audit of 2026-10-22 (local notes), item A2. The conversation of
  2026-10-23 settled what an agent may do and who switches it on.
- Contracts that still govern: [CONTEXT.md](../../../CONTEXT.md), [DESIGN.md](../../../DESIGN.md),
  [docs/workflows.md](../../workflows.md), [docs/tasks.md](../../tasks.md),
  [docs/unattended.md](../../unattended.md) (rule 1 above all). Every visible change follows the
  `ui` skill.

## Intent

Today only a person can drive onehand. An agent in a session cannot open another session, start
a workflow or look at the tasks, and nothing outside the process can either. A second launch of
onehand prints *already running* and exits, and the project path it was given is lost.

R3 gives an agent a small set of onehand's own tools, over MCP, in the sessions a person switches
on. It also gives the process one Unix socket. A second launch forwards its path over the socket,
and an outside program (a script, the system's cron) may use the same tools when a setting allows
it. Cron (R4) and connectors (R5) will start runs through the same tools.

## What an agent may do (the owner's list)

- Open a session in a project, with a chosen agent.
- Start a workflow on a brief, or on an issue.
- List tasks with their state, and stop a task.
- Open a file in the Workbench.

**What an agent may never do:** answer a card, a permission or an approval. Those always wait for
a person (unattended rule 1). No tool is written for them, and no tool's effect bypasses them.

## Who switches it on (the owner's calls)

- **Per session, in the composer's `+` menu**, off by default. The session's own agent gets the
  tools only while it is on.
- **An unattended run's session never has them**, whatever its agent.
- **A session the tools opened, or a run they started, starts with them off.** An agent cannot
  hand itself a second agent that controls onehand.
- **Outside control over the socket sits behind a switch in Settings**, off by default. A second
  launch forwarding its path needs no switch: it is always on.

## Goals

1. **One socket, one protocol.** The process that holds `instance.lock` binds a Unix socket
   beside it. The socket speaks MCP (JSON-RPC 2.0, one message per line) plus one onehand method
   for a second launch. Everything that controls onehand talks to this socket.
2. **The agent reaches the socket through onehand itself.** A session's ACP `session/new` and
   `session/load` carry one `mcpServers` entry of the stdio kind, which every ACP agent supports:
   `command` is onehand's own executable, and `args` are `mcp --session <token>`. `onehand mcp`
   is a thin proxy: stdin and stdout on one side, the socket on the other. *(Builder's call,
   see Decision 6: the args are really `--mcp-proxy <socket path> <token>`.)* No HTTP, and no MCP
   library: the JSON-RPC is hand-written, as ACP's is.
3. **A switch that needs no reconnect.** The entry is given to every session at connect time, so
   switching on does not restart the agent. While the session's switch is off, `tools/list` is
   empty and every `tools/call` is refused. Switching it either way sends
   `notifications/tools/list_changed` to that session's connection. An agent that ignores the
   notification still cannot call anything while the switch is off.
4. **The tools are decided in core and carried out in the app.** A core `control` module holds:
   - the tool catalogue (names, descriptions, JSON input schemas);
   - parsing a call into a `control::Call` enum, and refusing malformed input;
   - the MCP message handling (`initialize`, `tools/list`, `tools/call`, the notification) as
     pure functions from a message and the connection's standing to a reply or a `Call`.

   The app performs a `Call` and answers `Result<String, String>`. The text is what the agent
   reads.
5. **The methods a tool calls answer.** The `Shell` and app functions behind the tools return
   `Result` with a reason, instead of toasting and returning nothing. An agent hears why it was
   refused; a person still sees the toast they see today.

## Non-goals

- Answering cards, permissions or approvals: never, see above.
- Reusing the remote bridge's `RemoteCommand` vocabulary. It is shaped around one chat followed
  from Telegram and does not fit the tools. Where a tool and a Telegram command do the same
  thing (Stop), both call the same app function. The audit's "skip a `ShellCommand` enum until a
  third adapter" holds: `control::Call` exists because MCP parses into it, not as a general bus.
- Keeping onehand alive with no window, and starting runs by time. Those are R4.
- HTTP or SSE transports, or MCP resources and prompts. Tools only.
- A token or ACL scheme beyond the session tokens below. The socket is the user's own, mode
  `0600`; any process of that user could read the config anyway.

## Anti-goals (a failure even if it works)

- An agent that gets onehand's tools in a session where nobody switched them on.
- A tool that answers a card or an approval, or a run started by a tool that answers one for a
  person.
- A tool that steals focus or the caret: a session a tool opens is not shown, and a file it
  opens does not take focus from the composer.
- Two places deciding the same rule: what a tool may do lives in core once, and the socket,
  the proxy and the app only carry it.
- A tool doing something with nothing on screen saying so. A session it opens appears in the
  rail, a run it starts appears on the Tasks page, and so on.

## Constraints (from the repository)

- Core stays GUI-free and runtime-free. The `control` module is synchronous and pure; the
  listener lives in the app on a tokio runtime of its own (the `acp.rs` and `remote.rs`
  pattern), crossing to GPUI over a `futures` channel. The app's tokio gains the `net` feature.
- `SessionOpts` (`crates/core/src/acp/client.rs:63`) gains `mcp_servers`, threaded through
  `AcpRuntime::connect`/`start`. The two hard-coded `[]` (`session/new`, `session/load`) read it.
  The warm, parked adapter is connected before a session exists, so its entry is filled in when
  it is claimed by a session, or the warm path is skipped for this field. This is left to the
  builder.
- Work from outside a window goes through the window handle with the defer rule. A helper
  `with_shell(window, |shell, window, cx| ..)` replaces the hand-written walks R3 touches.
- `remote.rs` is near the 800-line rule, and core `remote/chats.rs` is past it. R3 adds nothing
  to either.

## Decisions

### 1. The socket

- Path: `instance.lock`'s directory, file `onehand.sock`. If the path is too long for a Unix
  socket address, fall back to `$XDG_RUNTIME_DIR/onehand-<hash of the config dir>.sock`.
- The process that wins the lock removes a stale socket file, then binds. Holding the lock proves
  no other process owns it. Mode `0600`.
- One line in, one line out, JSON-RPC 2.0. A connection says who it is in its first message:
  - `onehand/open { path }`: a second launch. It is answered, then closed.
  - MCP `initialize`, carrying `_meta.onehand.session = <token>` when it comes from the proxy.
    Without a token it is an outside client, refused unless Settings allows outside control.

### 2. A second launch

`main` tries the lock. When it is held, the second launch connects to the socket and sends
`onehand/open` with its path argument, or none. Then it exits 0. The running process opens the
path as a project in its focused window, or its first window, and raises that window. If the
socket cannot be reached, the old message stays, and the exit is still 1.

### 3. Session tokens

- Each `ChatSession` gets a random token when it connects. It is in the `mcpServers` entry's
  args, held by the app in a map from token to session uid, and dropped when the session closes.
- A connection with a token speaks for that session. Its standing is the session's switch, read
  on every `tools/list` and `tools/call`.
- The switch is per session and is not saved: a session reopened later starts with it off.
  That is the builder's call to revisit only if the owner asks.

### 4. The tools

Each name is a verb on onehand's own nouns (CONTEXT.md):

| Tool | Input | Does | Answers |
|---|---|---|---|
| `list_projects` | none | the projects of the window the call is placed in | name, path, whether it has a check command |
| `list_agents` | none | the agents in the New session menu | names |
| `open_session` | `project`, `agent`, optional `prompt` | opens a session there, not shown; sends `prompt` as its first message | the session's title, or why not |
| `list_workflows` | none | the workflows in the library | id and name |
| `start_workflow` | `project`, `workflow`, `title`, `brief` | a new task through `task::submit`, as the launcher makes it | the task id and whether it ran or queued, or why not |
| `start_issue` | `project`, `issue` number, optional `workflow` | a supervised task whose brief is the issue, through `task::submit` as the launcher makes one (the owner's call: not the unattended pick, so nothing is claimed on the forge and unattended need not be set up) | as above, or why not |
| `list_tasks` | optional `project` | tasks of the window's projects, with their state and next action, bounded | one line each |
| `stop_task` | `task` id | as the Tasks page's Stop, for a task of the window's projects; the task whose session is calling is refused | done, or why not |
| `open_file` | `path`, optional `line` | opens it in the Workbench of the call's window, without taking focus; a relative path is resolved against the calling session's project, and a path outside the window's projects is refused | done, or why not |

- **Where a call lands:** in the calling session's window. An outside client's call lands in the
  focused window, or the first one.
- A project is named by its name or its path. When a name matches two projects, the call is
  refused, naming both.
- Every list is capped, and an answer that was cut says so, as on screen.
- **What one session may start is bounded** *(builder's call)*: at most four sessions open at
  once that one session's tools opened. A fifth is refused, naming the four. Runs need no cap
  of their own, because they go through the task queue and its one-per-checkout rule.

### 5. The composer switch

An entry in the `+` menu: *Let the agent control onehand*, with a check when on. A session with
it on says so beside the composer, so a person can see it at a glance; its exact form follows
the `ui` skill. The entry is not offered in an unattended run's session.

### 6. The proxy's command line *(builder's call)*

`main` keeps its rule that no flag becomes a folder. The proxy is
`onehand --mcp-proxy <socket path> <token>`. The socket path is given outright, so `make dev` and
`make showcase` data roots need no environment, and a folder named `mcp` still opens. The lock
is checked before GPUI starts, so a second launch can forward its path and exit without opening
a window. It is still taken before the migration it protects.

## Steps (one PR each, in order)

| Step | What | Behaviour change |
|---|---|---|
| 1 | The socket, bound by the lock holder; a second launch forwards its path | a second launch opens its project in the running app |
| 2 | Core `control` (catalogue, parse, MCP handling) with tests; the app performs calls; outside control behind its Settings switch | an outside MCP client can use the tools when allowed |
| 3 | `onehand mcp` proxy, `SessionOpts.mcp_servers`, session tokens, the composer switch, `list_changed` | an agent in a switched-on session has the tools |

Each step updates its owning documents: a new `docs/control.md` (the tools, the socket, the
switch), CONTEXT.md if a new term is needed, and CHANGELOG.md.

## Testing

- **Core:** for every tool, a call that parses and one that is refused. `tools/list` is empty
  for a session switched off, and a `tools/call` is refused there. `initialize` without a token is
  refused while outside control is off. A malformed line is answered with a JSON-RPC error and
  does not close the connection.
- **App:** the token map drops a closed session; a second launch's `onehand/open` reaches the
  running window. A headless smoke test drives `onehand mcp` against the socket (the
  `acp_smoke` pattern).
- **By hand, with `/drive`, if the owner asks:** switch on, ask the Mock agent to call a tool,
  and see the session or the run appear.

## Left to the builder

- The warm adapter and `mcp_servers` (Constraints above).
- The exact wording of tool descriptions, which an agent reads to choose a tool. Keep them short,
  in CONTEXT.md's words.
- How `open_session`'s first prompt waits for the agent to come up: queue it on the session as a
  composer send would.
- Where in Settings the outside-control switch goes, following the `ui` skill.
