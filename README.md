# onehand

Native desktop host for AI coding agents over the
[Agent Client Protocol](https://agentclientprotocol.com). Rust + [GPUI](https://github.com/zed-industries/zed),
many concurrent sessions per project root, with a quick editor and terminal built in.

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

Work is a tree. A *workspace* groups one or more *project roots*, and each root
runs one or more *sessions* — a session being one agent bound to that root. A
root can hold several at once, and the left rail lists them by conversation
rather than by agent, so switching is one click.

Every session speaks ACP, so the agent is whatever you point it at. Claude Code
is the default; anything that implements the protocol should work. Commands the
agent runs come back over ACP's terminal extension and render inline in the
transcript.

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
cargo run                       # the positional argument seeds the project root
cargo run -- /path/to/project
cargo build --release           # binary at target/release/onehand

make desktop                    # install the desktop entry + icon (Linux)
```

There is a headless smoke test that connects to an agent, sends a prompt and
prints the reply, with no window involved:

```bash
cargo run -p onehand-core --example acp_smoke
```

## Known gaps

Listed because a missing feature nobody wrote down reads as a bug in the ones
that exist:

- No command palette.
- The terminal has no `APP_KEYPAD` mode and its cursor does not blink. Neovim
  itself runs — `Ctrl+Shift+N` opens it on the active project, as one of the
  Workbench's modes beside Editor, Files and Markdown.
- `path:line:col` in agent prose is not clickable; only a tool card's path
  header opens a file.
- The remote bridge does not stream the transcript. A finished turn carries the
  end of the agent's last answer and nothing else — no tool cards, no diffs,
  nothing mid-turn.
- Telegram is the only remote channel. The layer underneath it is general, but
  nothing else implements it.
- The bundled icon set covers less than the app wants, so some glyphs are
  approximations.

## Layout

| Path | What |
|---|---|
| `crates/app` | the GPUI front end and the binary |
| `crates/core` | GUI-free logic: config, the workspace tree, ACP, the chat model |
| `crates/plugin-api` | GUI-free plugin IDs, descriptors and capabilities |
| `crates/plugin-host` | startup registry and typed contribution contracts |
| `crates/terminal-ui` | shared PTY/grid ownership for Terminal and Neovim |
| `plugins/builtin` | compile-time Editor, Files, Markdown, Neovim and Telegram plugins |
| `vendor/gpui-terminal` | a vendored terminal grid plus the interaction layer upstream never had |

`crates/core` has no dependency on any UI framework, deliberately: it is the
half that survived one front-end rewrite.

Plugins in this milestone are built into the same binary. Registration happens
once before the first window is created; there is no plugin process, dynamic
loading, IPC, marketplace or plugin-management screen.

Deeper notes live beside the code — `CLAUDE.md` for how the app is put together,
`DECISIONS.md` for the choices reading the code will not explain, and `DESIGN.md`
with `DESIGN-ANSWER.md` for the UI contracts.

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
