# Changelog

Notable changes to onehand, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `0.x`, and `0.x` here means what it says: the config file, the on-disk transcript
format, the window layout and the keymap have all changed without migration and will again. A
minor bump may break any of them.

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

[0.1.0]: https://github.com/jarviisha/onehand/releases/tag/v0.1.0
