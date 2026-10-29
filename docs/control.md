# Control

How onehand is driven from outside a window: the socket the running process listens on, what a
second launch does with it, and onehand's own tools, which a program outside onehand may call over
it when Settings allows, or the agent of a session a person has switched on.

**No tool answers a card, a permission or an approval.** Those always wait for a person; no tool
is written for them.

## The socket

- **Who binds it:** the process holding `instance.lock` (one per config directory), the moment it
  takes the lock and before GPUI starts; it is served once GPUI is up, and a launch in between
  waits in the socket's backlog. Holding the lock proves no other process owns the socket, so the
  holder removes a stale file before it binds. A process that could not take the lock at all
  binds nothing.
- **Where:** beside the lock, `<config dir>/onehand.sock`. A path too long for a Unix socket
  address (more than 107 bytes) moves to `$XDG_RUNTIME_DIR/onehand-<hash>.sock`, the temp
  directory when that is unset; the hash is FNV-1a over the config directory, so every build on
  that directory finds the same name.
- **Mode `0600`:** the user's own, as the config directory beside it is.
- **What is said:** JSON-RPC 2.0, one message per line. A connection says what it is in its first
  line: `onehand/open` is a second launch, answered and closed; anything else starts an MCP
  conversation, carried on until the client hangs up. A first line that does not come within five
  seconds, or any line past 64 KiB without a newline, drops the connection.
- **Where the rule lives:** core's `instance` reads and writes a second launch's lines, core's
  `control` every MCP message; the app's `instance` and `control` only carry them, on a tokio
  runtime of their own, and cross to GPUI over a channel only to carry out a call.

## A second launch

`main` takes the lock before GPUI starts, right after `--version`. When another process holds it,
the launch sends

```json
{"jsonrpc":"2.0","id":1,"method":"onehand/open","params":{"path":"/abs/path"}}
```

with its first argument made absolute (no argument: no `path`), waits up to five seconds for the
answer, and exits.

| Answer | The running process | The second launch |
|---|---|---|
| a folder | opens it as a project in the window in front (the first one when none is), selecting it if it is one already, and raises that window; a line that lands before the first window is up waits for it | exits 0 |
| no path | raises that window | exits 0 |
| not a folder (`-32602`) | nothing | prints `onehand: <path> is not a folder`, exits 1 |
| a path that is not UTF-8 | (never sent) | prints `onehand: <path> is not a UTF-8 path`, exits 1 |
| nobody on the socket | (an older build, or one that could not bind) | prints `onehand is already running on <dir>`, exits 1 |

## The MCP conversation

- **Tools only:** `initialize`, `ping`, `tools/list`, `tools/call`. Resources, prompts and other
  methods are answered `-32601`; a notification is not answered.
- **`initialize` decides who the connection is.** Without a token it is an outside client,
  refused (`-32001`, saying where the switch is) while outside control is off. A token
  (`_meta.onehand.session`) is refused as an unknown session until sessions are given tokens.
- **Asked on every message:** turning outside control off refuses the next `tools/list` or
  `tools/call` of a client already connected (`-32001`). A call before `initialize` is `-32002`.
- **A refusal the caller can act on is the tool's answer**, marked `isError`: an argument missing
  or of the wrong type, a project or task not found, a file outside the project on screen. An
  unknown tool is `-32602`; a malformed line `-32700`, and the conversation goes on.
- **Where a call lands:** the window in front, or the first one. A call that arrives before the
  first window is up waits for it.

## The tools

| Tool | Input | Does | Answers |
|---|---|---|---|
| `list_projects` | none | the window's projects | `name — path — check: yes/no`, one per line |
| `list_agents` | none | the agents a session can be opened with | one name per line |
| `open_session` | `project`, `agent`, optional `prompt`; for a session's agent, `request` (required), `turn` (required with `prompt`), optional `model` and `dirs`, and `project` may also be a worktree's top level | opens a session on the project with that agent, not shown; it appears in the rail; `prompt` is sent once the agent is up. A session's agent opens a worker: see *A worker* below | for an outside program, what it opened; for a session's agent, the worker's handle and what it runs with; or why not |
| `list_workflows` | none | the library's workflows | `id — name`; an unreadable file says why |
| `read_workflow` | `workflow` id | the workflow's file as TOML, a shipped one's too | the TOML, or why not |
| `save_workflow` | `toml` | `workflow::store::save_text_blocking`: read as a file is (a key onehand does not read refuses), checked as the editor's *Save* is, and refused when another workflow has its name. It goes over the person's own file with its `id`, else into a new file: a shipped workflow is copied, never changed. The library is read again | `Saved <name> as <id>, version <n>`, or every problem, in the editor's words |
| `start_workflow` | `project`, `workflow` id, `title`, optional `brief` | a new task through `task::submit`, as the launcher makes it: on the checkout or on a worktree cut first, as the workflow's place says; what blocks the launcher's *Run* blocks it, in the same words | `Task <id> started: <title>`, or `Task <id> queued: …`, or why not |
| `start_issue` | `project`, `issue` number, optional `workflow` id | the same, the brief being the issue: one of the project's own issues, as its Issues tab numbers them. A supervised task, not the unattended pick: nothing is claimed or said on the forge, unattended runs need not be set up, and the brief does not tell the agent it is alone. Without `workflow`, the one unattended runs use | as above |
| `list_tasks` | optional `project` | the tasks of the window's projects, or of one | `id — title — state — step — next: …`, the next actions as the Tasks page offers them |
| `stop_task` | `task` id | as the Tasks page's Stop, for a task of the window's projects | `Stopping <title>`, or why not |
| `open_file` | `path` | opens the file in the Workbench and shows the Workbench, without taking focus | what it opened, or why not |
| `list_issues` | optional `project` | the project's own open issues, as its Issues tab keeps them (a synced project's imported ones too); the calling session's project when none is named | `#<n> — <title>`, one per line |
| `create_issue` | optional `project`, `title`, optional `body` | writes one issue into the project's own issue file, read again wherever the issues are on screen; an open issue with the same title (case and spaces aside) is not written again. It takes no labels: the issue gets those of the calling session's schedule, so no agent labels its own issue for an unattended run | `Wrote issue #<n>: …`, or `Issue #<n> is already open with that title` |

- **A task a tool starts is driven off screen**, the way an issue's run is: its session is in the
  rail (a schedule's that ends done leaves it, `docs/schedules.md`) and the task on the Tasks page,
  and nothing the person is looking at moves. Only its first
  drive: a Resume or Retry a person presses later shows, as theirs does. A person still sees the
  toasts the launcher's start shows (queued, over the cap).
- A project is named by its name or its path. A name two projects share is refused, naming both.
- Every list answers at most 50 lines and ends `… and N more not listed` when it was cut.
- `open_file` takes an absolute path without `..`, inside the project on screen; another
  project's file is refused rather than switching projects under the person. It takes no line:
  the editor cannot go to one.

## Outside control

Settings ▸ Connections, *Outside control*: off by default, kept as `control.outside` in
`onehand.toml`. The section names the socket's path and says what a program may and may never do.
Turning it on or off holds at once, for clients already connected too; a write that fails puts
it back and says so.

## A session's agent

- **Every session's agent is given one MCP server**, in `session/new` and `session/load`: onehand
  itself, the stdio kind every ACP agent takes, as `onehand --mcp-proxy <socket> <token>`. `main`
  runs the proxy before the instance lock and exits when the socket closes; it carries the agent's
  lines to the socket and back, putting the session's token on `initialize` whatever the agent
  wrote there. No flag becomes a folder: `--mcp-proxy` takes exactly its two arguments.
- **A token per started agent.** It is minted when the agent is started, which for the warm
  adapter is before any session exists, and passed to the session that claims it; it goes with
  the session. A token no session holds yet is known and switched off, so an agent that starts its
  servers at `session/new` is connected, offered nothing, and told when that changes. A token
  whose session closed is refused as no such session. The token is on the proxy's command line,
  so another process of the same user can read it, as it can read the config.
- **The switch is the session's**, in the composer's `+` menu: *Agent controls onehand*, under
  a heading of its own, *This session*. Off by default, kept by the session through a restart of
  its agent and not saved, so a session reopened later starts off. While it is on, *Controls onehand* stands in the composer's status
  row; pressing it switches it off. A session the tools opened, or a run they started, starts
  off; an unattended run's session is never offered it.
- **A set of tools.** Beside all and none, a session may stand with a set of tools by name: a
  schedule's task's sessions stand with the schedule's, copied onto the task, and an issue they
  write gets the schedule's labels. Only those tools are listed, and a call to any other is
  refused naming it, whatever asked for it. Switching the session on in the composer gives it all
  of them; switching it off goes back to the set. See [schedules.md](schedules.md).
- **No reconnect.** While the switch is off, `tools/list` is empty and every `tools/call` is
  refused. Switching either way sends `notifications/tools/list_changed` to each of that session's
  connections; an agent that ignores it still cannot call anything while off.
- **A session's call lands in its own window.** `open_file` resolves a relative path against the
  session's project. `stop_task` refuses the task whose session is calling. A session's tools may
  have at most four sessions open at once; the fifth `open_session` is refused, naming the four.

## A worker

What a session's agent gets from `open_session` beyond what an outside program does. An outside
program naming a folder that is no project of the window, or `dirs`, is refused.

- **The folder**: a project of the window, or the top level of a git worktree of a repository one
  of the window's projects belongs to. Every path is canonicalized first (symlinks resolved),
  and the repository is told by git's common directory, read the same way from the worktree and
  from each project, main checkout, linked worktree or a folder inside one. A folder inside a
  checkout is refused, naming the top level; any other folder, naming why. A worktree that is no
  project of the window opens as a transient project, selecting nothing.
- **What it may write besides its folder**, sent as ACP's `additionalDirectories` in
  `session/new`, `session/load` and `session/fork`: the repository's common git directory when
  the folder is a linked worktree, where its commits go, then each of `dirs`, absolute, existing
  and canonicalized, and never `/`, the home folder, the worker's folder or one inside it, or a
  folder holding it or the repository's main checkout. The first that fails refuses the call,
  naming it as given and as resolved. The answer names them. A session with any never takes the
  warm adapter, which opened its session without them, and a restart of its agent sends them again.
- **One worker per folder.** A folder another session of the window is open on is refused. So is
  one a worker is opening or open on: it is reserved, by its canonical path, in the same update
  that checks it, until the open fails or the worker closes.
- **Nothing else working in it.** Every process of the user is read for one whose directory is in
  the folder (`/proc` on Linux, `lsof` elsewhere), leaving out onehand itself and its terminal's
  shells: an earlier run's worker, left by a crash, is what this finds. Any found refuses the
  open, naming each by pid and command; a process whose directory could not be read for any
  other reason than being gone, or than the system refusing it to its own user (a process it may
  not trace, such as `systemd --user` or `ssh-agent`, which an agent never is), refuses it as
  unknown, as does a process table that could not be read at all. Nothing is opened either way.
- A worker whose window closed holds its folder no longer: the next open lets it go.
- The checks read git, the file system and the process table off the UI thread.
- **The answer comes once the worker is up**, within 120 seconds of the call: its agent connected,
  put in its mode and on its model one pick at a time, each answered, then its prompt accepted.
  - **The mode** is not the lead's to name: it is `[unattended] mode` when the worker's agent is
    the one unattended runs use (a mode is the adapter's own id), else what that agent is known
    to need, else the agent's own.
  - **The model**, when named, is a value of the agent's model option. An agent that offers no
    model option, or not that value, refuses the open, naming what it offers; one that refuses
    the pick does too. A worker never runs on another model.
  - A `prompt` needs a `turn`, the caller's number for it, and a `turn` needs a prompt.
- **What it answers**, one fact per line: `session:` the handle, `<run>-<uid>`, which names this
  run of the app since uids count from 1 in every run; `name:` as the rail shows it; `project:`;
  `mode:` and `model:` in force, or *the agent's own*; `dirs:` or *none*; and `turn: <n>
  accepted` with a prompt, which means the agent was handed it, nothing more.
- **`request` is the caller's id for the open**, per lead and for this run of the app. The same
  id with the same arguments, while the open runs, answers that it is still opening; once it
  answered, the same answer again; once it failed, it opens afresh. The same id with other
  arguments is refused. Nothing is opened, sent or counted under the cap twice.
- **A failure leaves nothing behind and touches nothing else**: past the 120 seconds, an agent
  that never came up, a mode or model refused, a prompt not taken. Only the open's own session is
  closed, ending its agent; its project goes only if the open added it and no session is left on
  it; its folder, its place under the cap and its request id are freed; the answer says why.
- **The lead is asked about at every step.** A lead whose session closed, or switched off, while
  its worker was opening abandons the open: it fails as above, its prompt never sent.
