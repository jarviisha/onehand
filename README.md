# onehand

Native desktop host for AI coding agents over the
[Agent Client Protocol](https://agentclientprotocol.com). Rust + [GPUI](https://github.com/zed-industries/zed),
many concurrent sessions per project, with a quick editor and terminal built in.

> [!WARNING]
> **Early and unstable. Not ready to depend on.**
>
> `v0.1.0` is a pre-release, and nothing here is covered by a stability promise —
> window layout, the config file format, the on-disk transcript format and the
> keymap have all changed without migration and will again. Expect rough edges
> and breakage on update. It has only been exercised on Linux.

## What it is

Most editors treat an agent as a panel bolted onto the side. onehand inverts
that: the conversation **is** the window, and the editor and the terminal are
what open when you need them.

Work is a tree. A *workspace* groups one or more *projects*, and each project
runs one or more *sessions* — a session being one agent bound to that project. A
project can hold several at once, and the left rail lists them by conversation
rather than by agent, so switching is one click.

Every session speaks ACP, so the agent is whatever you point it at. Claude Code
is the default; anything that implements the protocol should work. Commands the
agent runs come back over ACP's terminal extension and render inline in the
transcript.

Beside the conversation:

- **Workbench**, a dock on the right with an editor and file tree, a Markdown
  reader, Neovim, the project's Issues and a Plugins mode for Claude Code's
  plugins.
- **Workflows**: a brief taken through named steps (plan, approve, implement,
  run the check, push, open a pull request, wait for its status checks), each
  judged by what git and the transcript show rather than by what the agent says.
- **Tasks**: one page for what is running, what needs you and what finished,
  with each run's steps, output and diffs, and Resume and Retry.
- **Unattended runs**: an issue carrying a label, on GitHub or kept in onehand,
  worked in a worktree and session of its own while nobody watches, and told how
  it went.
- **A Telegram bridge** for following and answering sessions away from the
  machine.

## Install

Prebuilt Linux x86_64 tarballs are on the
[releases page](https://github.com/jarviisha/onehand/releases). Unpack one and
run the installer beside the binary — it registers the desktop entry and the
icon, which is what gives the window a name and a picture:

```bash
tar xf onehand-v0.1.0-linux-x86_64.tar.gz
cd onehand-v0.1.0-linux-x86_64
./install-desktop.sh          # desktop entry + icon, into ~/.local/share
./onehand /path/to/project
```

The entry points at wherever the folder was unpacked, so unpack it somewhere it
can stay and re-run the installer if it moves.

## Requirements

To run a release build:

- Linux, X11 or Wayland. Nothing here is Linux-only by design, but the platform
  features are built for it and no other platform has been tried.
- The shared libraries a desktop app links: fontconfig, freetype, libxkbcommon,
  and X11 or Wayland. On Debian and Ubuntu those are `libfontconfig1`,
  `libfreetype6`, `libxkbcommon0`, `libxkbcommon-x11-0`, `libasound2`.
- Node, for the default agent: it launches through `npx`. Point the config at a
  different command and this goes away.

To build it, additionally:

- A recent Rust toolchain — the app crate is edition 2024.
- The `-dev` half of the libraries above, plus `pkg-config`, `cmake` and
  `clang`. The CI workflow's package list is the tested one.

## Build and run

```bash
cargo run                       # the positional argument seeds the first project
cargo run -- /path/to/project
cargo build --release           # binary at target/release/onehand

make desktop                    # install the desktop entry + icon (Linux)
```

There is a headless smoke test that connects to an agent, sends a prompt and
prints the reply, with no window involved:

```bash
cargo run -p onehand-core --example acp_smoke
```

## Keyboard shortcuts

Open **Settings → Shortcuts** (or press `Ctrl+,`) to edit app shortcuts. Changes
apply immediately to every window and persist in the resolved app config.
Choose **Edit**, enter a combination such as `ctrl-shift-j`, then **Save**.
Separate alternative shortcuts with spaces; leave the field empty to unassign.
**Reset** restores that command's defaults. Invalid or conflicting shortcuts are
rejected before saving; terminal copy/paste and Tab routing remain fixed.

- `Ctrl+Shift+J` hides Workbench immediately, regardless of focus, or reopens its
  previous mode. It preserves buffers and running processes.
- `Ctrl+Shift+E / M / N` open and focus Editor / Markdown / Neovim. Repeating a
  mode shortcut keeps that mode visible.
- ``Ctrl+` `` shows or hides the terminal.
- `Ctrl+Shift+O` starts a new session on the current project with the default
  agent.
- `Ctrl+Shift+R` restarts the agent; `Ctrl+Shift+W` closes the session. During a
  running turn, confirmation requires releasing and pressing the shortcut again.

Overrides use stable command IDs in the existing TOML config. Omitted commands
keep their defaults; an empty array unassigns a command:

```toml
[keymap]
toggle_workbench = ["alt-j"]
restart = []
```

Contexts remain attached to commands: saving stays outside the terminal, and
completion stays in the composer.

## Known gaps

No command palette, no `APP_KEYPAD` in the terminal, `path:line:col` in agent
prose is not clickable, and Telegram is the only remote channel. The full list,
with the reason for each, is [docs/known-gaps.md](docs/known-gaps.md).

## Layout

| Path | What |
|---|---|
| `crates/app` | the GPUI front end and the binary |
| `crates/core` | GUI-free logic: config, the workspace tree, ACP, the chat model, workflows, tasks, unattended runs |
| `crates/plugin-api` | GUI-free plugin IDs and descriptors |
| `crates/plugin-host` | what a plugin is handed to draw and talk to the app as the app does |
| `crates/terminal-ui` | shared PTY/grid ownership for Terminal and Neovim |
| `plugins/builtin` | Editor, Files, Markdown, Neovim, Issues, Plugins, Telegram and the GitHub connector |
| `vendor/gpui-terminal` | a vendored terminal grid plus the interaction layer upstream never had |

`crates/core` has no dependency on any UI framework, deliberately: it is the
half that survived one front-end rewrite.

Plugins are built into the same binary and composed at compile time; there is
no plugin process, dynamic loading, IPC or marketplace for onehand's own
plugins.

## Documentation

- [CONTEXT.md](CONTEXT.md): the glossary.
- [DESIGN.md](DESIGN.md): the UI as built.
- [docs/](docs/): workflows, tasks and unattended runs end to end, the known
  gaps, and the reasons behind the repository's rules.
- [CLAUDE.md](CLAUDE.md): working in the repository.
- [CHANGELOG.md](CHANGELOG.md): what changed in each release.

## Licence

Dual licensed under either

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option. This is the Rust ecosystem's usual pair, and it is what the
vendored terminal already carried.

Unless you state otherwise, any contribution you deliberately submit for
inclusion in this work is licensed the same way, with no additional terms.

The vendored terminal keeps its upstream licences in `vendor/gpui-terminal/`,
and the checked-in icons carry their own notices in `assets/icons/licenses/`.
