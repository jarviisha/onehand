# onehand

Native desktop host for AI coding agents over the
[Agent Client Protocol](https://agentclientprotocol.com). Rust + [GPUI](https://github.com/zed-industries/zed),
many concurrent sessions per project root, with a quick editor and terminal built in.

> [!WARNING]
> **Early and unstable. Not ready to depend on.**
>
> This is under active development. There has been no release, no version is
> published, and nothing here is covered by a stability promise — window layout,
> the config file format, the on-disk transcript format and the keymap have all
> changed without migration and will again. Expect rough edges and breakage on
> update. No CI runs on it, and it has only been exercised on Linux.

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

## Requirements

- A recent Rust toolchain — the app crate is edition 2024.
- Linux, X11 or Wayland. Nothing here is Linux-only by design, but the platform
  features are built for it and no other platform has been tried.
- Node, for the default agent: it launches through `npx`. Point the config at a
  different command and this goes away.

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
  itself runs — `Ctrl+Shift+N` opens it on the active project, as the
  Workbench's third mode beside Editor and Files.
- `path:line:col` in agent prose is not clickable; only a tool card's path
  header opens a file.
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
| `plugins/builtin` | compile-time Editor, Files, Neovim and Telegram plugins |
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

Not yet chosen, which means default copyright applies and you do not have
permission to use this. If you want to, open an issue and ask — the intent is to
land on something permissive.

The vendored terminal keeps its upstream licences in `vendor/gpui-terminal/`,
and the checked-in icon carries its own notice in `assets/icons/licenses/`.
