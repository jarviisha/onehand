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
