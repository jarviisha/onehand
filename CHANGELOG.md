# Changelog

Notable changes to onehand, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `0.x`, and `0.x` here means what it says: the config file, the on-disk transcript
format, the window layout and the keymap have all changed without migration and will again. A
minor bump may break any of them.

## [Unreleased]

### Breaking

- `pipelines/` in the config directory moves to `workflows/`, and `pipeline-runs/` to `tasks/`, at
  the first start of this build. The move can be cut short and run again, but there is no way back:
  an older build does not read what it leaves.
- Only one onehand runs on a config directory. A second instance says so and exits; more windows
  still open in the one process.
- In a Telegram group on `remote.telegram.allowed_chats`, only members whose own ids are listed
  too reach the app. Before, every member of a listed group could prompt and answer permissions.

### Workflows and tasks

- A workflow takes a brief through named steps: agent steps judged by gates onehand checks itself,
  command steps, approvals, and forge steps that push the commit the check passed on, open a draft
  pull request and repair it until its status checks pass. Three ship read-only; the rest are
  edited, imported and exported in Settings ▸ Workflows, and carry an id and a version.
- Every run belongs to a task, kept as history (the newest 200 finished per project), with each
  step visit pinned as a commit under `refs/onehand/`. One task works in a checkout at a time; the
  rest queue.
- A Tasks page lists what needs attention, runs, queues and has finished, opens a task's detail
  with each visit's output and diffs, and offers Resume, Retry (from an earlier step too) and
  Dismiss. A project's check command runs as a task of its own.
- An agent step can name the agent's mode and config options (model, effort, fast…), set before
  each visit's first prompt and recorded on the visit. A value the agent does not offer blocks the
  start, or fails the run at that step; it never runs on something else.

### Unattended runs

- A project switched on works issues carrying a trigger label, or one picked by hand, as a task in
  a worktree and session of its own, off screen. Only issues the user opened are taken.
- Issues come from the forge (GitHub, through `gh`) or are kept in onehand, in a new Issues mode of
  the Workbench, optionally in step with the forge both ways.
- A parked question waits for a person and gives up its slot meanwhile; a prompt from a person
  takes the run over. The report on the issue is kept until it is delivered, and putting the label
  back answers a review on the pull request.

### The window

- A workspace overview of sessions, projects and recent conversations as cards.
- A Plugins mode in the Workbench manages Claude Code's plugins per project and globally.
- Settings is one paged modal: Appearance, Workspace, Agents (with a default agent and a test),
  Connections, Workflows and Shortcuts. Shortcuts are edited there and persisted under `[keymap]`.
- The transcript reads the agent's work as one line per stretch, and a turn ends on the files it
  changed. The composer is one surface above the conversation.
- The rail keeps the user's order, splits into two lists, and fades a name that runs out of room.
- `Ctrl+Shift+O` starts a new session on the current project with the default agent, and
  `Shift+Tab` in the composer steps through the session's modes.
- Terminal tabs size to their names and scroll; the terminal caches rows and coalesces redraws.
- The interface font is Be Vietnam Pro, shipped inside the binary in place of Inter.
- The palette is VS Code's Dark Modern and Light Modern, the rail on its side bar's grey.

### Fixed

- An agent's file write that names no content is refused instead of emptying the file.
- An adapter that answers a turn with an error and exits says why, instead of only that it exited.
- Releasing an agent's terminal stops a command that ignores the hangup, and what it started.
- A transcript line torn by a crash no longer swallows the next save's first message; a save that
  fails is carried by the next one; a line this build cannot read is never rewritten away.
- A save keeps the config's permissions and writes through a symlink.
- An unattended claim whose comment fails still runs, instead of leaving the issue taken and idle.
- A run's report says it could not start only when it never worked, not when it failed after a
  resume.
- Close or Restart pressed mid-turn and cancelled works again on the next press.
- A lost adapter settles the steps it left running, and a step saved running comes back stopped.
- A turn the agent ended on a refusal or a limit is a failed turn, and a run fails on any failed
  turn instead of judging work that was never done.
- A fork's pull request on the run's branch name is never taken for the run's own.
- An adapter runs in a process group of its own and goes with everything it started.
- Reading the git status takes no lock, so an agent's commit does not trip over it.
- A folder cut at the file tree's cap is counted in its "more not shown".
- An agent left by its adapter gets a moment to finish before what is left of the group is killed.
- A picture goes inline only to an agent that says it reads pictures, and as a link otherwise.
- A file read asked for a range of lines gets that range.
- A method onehand does not serve is answered as missing, and a session the agent did not name is a
  failed start.
- A free unattended slot is filled at once rather than at the next tick, and labelled issues are
  read oldest first so the oldest are never the ones past the list's limit.
- A cancelled turn the agent never ends lets go of its slot after 30 seconds.
- Stop waits for a push or a pull request in flight, and no pull request is opened after it.
- Publishing an issue that timed out says it may have been created, so it is not published twice.
- Each project's issues file has its own lock, so one slow sync holds no other project.
- The maximize key never blows up a panel that is off screen; a removed project's check field goes
  with it; a file opened from a maximized conversation is shown; and a session that cannot start
  leaves the panels on the project the rail selected.
- A large edit's diff is bounded by its size on the UI thread, and a card's +N/−M always matches
  the rows it draws.
- A closed step no longer reads its whole output on every frame; Copy on a read or a search takes
  every line, not the sixty shown; and ACP events already waiting are applied in one update.
- Settings writes only the agent or shortcut it changed, so an entry added to `onehand.toml` by hand
  while the app runs survives the next save.
- A `state.toml` that will not parse is kept as `state.toml.bad` before recents start over.
- Closing a terminal tab no longer holds the window while its shell is stopped, and an agent's
  terminal is never hung up after its process was reaped.
- Telegram keeps at most 20 messages waiting on a slow network; older ones are dropped and counted
  in one message per chat.
- An allowed chat written with spaces around its id hears announcements too.
- Tasks read at start come before any added meanwhile, and none is listed twice.
- Stopping a queued task is recorded in the run's history like any other stop.
- A step the agent announces twice is one card.
- A folded step stays folded on the right card after a resumed conversation's replay is dropped.
- A label holding a comma, a quote or a leading dash syncs as one label.
- A run's branch skips a name whose worktree folder is still on disk, and a branch past `-99` is an
  error rather than a guess.
- A file whose name is not UTF-8 gets its git badge and counts in a run's work.
- On macOS, removing a worktree sees a shell or Neovim working in it.
- One tool output keeps at most 1 MiB in the transcript and says how much more there was.
- A settings save writes only what it changed: comments, formatting and keys from a newer build
  stay in `onehand.toml`.

### Project

- Built-in plugins are composed at compile time from three ordered lists; the startup registry is
  gone. GitHub moved out of core into a connector plugin.

## [0.1.0] - 2026-09-07

First release. Everything below is new, so it is grouped by what it is rather than listed as one
wall of additions.

### Sessions and agents

- Every session is an [ACP](https://agentclientprotocol.com) agent, rendered as a native chat.
  Claude Code over `npx` is the default; the adapter command is configuration, so anything that
  speaks the protocol can take its place.
- A workspace groups project roots, and a root runs as many concurrent sessions as it is asked to.
  Sessions connect lazily — a workspace with a dozen roots does not launch a dozen agents at boot.
- Agent-run commands come back over ACP's terminal extension and render inline in the transcript.
- Permission requests and multiple-choice questions are answered from cards in the transcript.
- Agent-advertised pickers — mode, model, effort — are drawn in the composer.
- `@` mentions files and `/` opens the agent's own slash commands; images and files pasted onto the
  composer become attachments.

### The window

- A session-first left rail: every project row lists its sessions, each named by its conversation,
  each carrying a mark of its own shape while it is busy, waiting, finished unseen or disconnected.
- A Workbench dock with four modes — a quick editor with tree-sitter highlighting, the project's
  file tree with git status, a live Markdown reader, and Neovim in a real PTY.
- A terminal dock, one login shell per tab, whose open state is remembered per project.
- A status bar carrying only what nothing else on screen carries: the project, its branch and change
  count, the running agent, unsaved buffers and each panel's zoom.
- Per-panel zoom, two directions of maximize, and a keymap in an exact `Ctrl+Shift` namespace so
  plain `Ctrl` keys stay usable inside a PTY. The Help dialog is the whole keymap.
- Light, dark and system appearance, following the desktop while set to system.
- Multi-window: one window hosts one workspace, and opening a workspace already on screen focuses
  the window showing it.

### Persistence

- Every conversation is a directory: metadata rewritten whole, items appended, images stored by
  content hash. Written at the end of every turn, so a crash costs the turn in flight.
- Conversations can be resumed, renamed, exported as Markdown and deleted. Nothing is ever deleted
  on the app's own initiative.
- Panel arrangement, the recents list and per-workspace settings persist; sessions respawn.

### Remote bridge

- An optional Telegram bridge for when nobody is at the machine, off unless the config asks for it.
  The token is read from an environment variable or a file of its own, never from the config file.
- A chat is told about a session only after it follows one, and a chat not on `allowed_chats` is
  answered with nothing at all.
- `/sessions`, `/use`, `/follow`, `/unfollow`, `/status`, `/stop`, `/options`, `/archive`, `/open`,
  `/away` and `/here`; anything else is a prompt for the bound session.
- Parked permissions and questions go out with inline buttons and can be answered from the chat.

### Terminal

- A vendored grid plus the interaction layer upstream never had: scrollback, selection, copy and
  paste, bracketed paste, mouse reporting with a `Shift` bypass, cursor shapes, focus reporting,
  terminal replies going back to the PTY, and `OSC 52` writes.

### Project

- Built-in features are plugins registered once at startup behind a versioned Rust contract.
- `onehand-core` carries the GUI-free half and depends on no UI framework, enforced by a test.
- CI runs formatting, core tests, app tests and clippy with warnings denied.

[Unreleased]: https://github.com/jarviisha/onehand/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jarviisha/onehand/releases/tag/v0.1.0
