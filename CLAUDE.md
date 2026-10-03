# CLAUDE.md

`onehand` is a Rust desktop GUI (**GPUI** + [gpui-component](https://github.com/longbridge/gpui-component))
that hosts AI coding agents. Window: a left navigation **rail**, a central **agent pane** (a native
chat), a right **Workbench** dock (editor, file tree, Markdown, Neovim, Issues, Plugins) and a bottom
**terminal** dock. A *workspace* groups *project roots*; each root runs *sessions*, and **every
session is an ACP agent** ([Agent Client Protocol](https://agentclientprotocol.com)).

## Where the rest lives

Read the relevant one **before** changing the area it covers:

- [docs/architecture.md](docs/architecture.md): the full account of every subsystem and why it is
  shaped that way (ACP client, remote bridge, unattended runs, chat pane, Workbench, terminal,
  window shell, keyboard, persistence, config) plus the mock agent, CI and releases.
- [docs/known-gaps.md](docs/known-gaps.md): what this build deliberately does not do yet, and why.
- [docs/rules-and-gotchas.md](docs/rules-and-gotchas.md): the full reasons behind the short rules
  and gotchas below.
- [DECISIONS.md](DECISIONS.md): decisions D1–D8 that code cannot explain (theme, appearance,
  editor/Neovim scope, icons, plugins, GPUI source identity, vendored terminal).
- [DESIGN.md](DESIGN.md) (whole-app UI) and [DESIGN-ANSWER.md](DESIGN-ANSWER.md) (transcript):
  **binding** UI contracts, structure and behaviour only. The `design-contract` skill triggers on
  any visible change.
- [docs/unattended.md](docs/unattended.md): unattended runs end to end.
- [docs/pipelines.md](docs/pipelines.md): pipeline templates, the run engine and its one driver.
- [docs/tasks.md](docs/tasks.md): the Tasks page and the task model behind it, as designed.
- [CONTEXT.md](CONTEXT.md): the glossary; use its terms and never the words it says to avoid.
- [docs/roadmap.md](docs/roadmap.md): the milestones from the pipeline engine to Tasks and
  issue/PR integration, and the decisions already taken for them.

## Commands

```bash
cargo run [-- /path/to/project]  # the app; the arg seeds the workspace's project root
cargo check                      # fast type-check
cargo test                       # everything; `cargo test <substring>` for one test
cargo test -p onehand-core       # core only (fast, no GUI)
make fmt                         # NOT bare `cargo fmt`
make lint                        # fmt check + clippy (warnings allowed locally)
make clippy CLIPPY_EXTRA="-- -D warnings"  # what CI runs: warnings denied
cargo build --release            # LTO; target/release/onehand

# Headless ACP smoke test; ACP_CMD swaps the adapter
cargo run -p onehand-core --example acp_smoke
ACP_CMD="node crates/core/examples/mock_terminal_agent.js" cargo run -p onehand-core --example acp_smoke go
ACP_CMD="node crates/core/examples/mock_ask_agent.js" cargo run -p onehand-core --example acp_smoke go
```

- **Why the Makefile:** `vendor/` is a workspace member, and bare cargo fmt/clippy would rewrite
  it. Its diff against upstream must stay exactly our patches.
- **The app's chrome without an API key:** add this agent in Settings ▸ Agents or `onehand.toml`.
  One prompt plays a whole transcript over about 12s: every block kind, a parked permission and
  question cards. Every other turn ends on a JSON-RPC error on purpose. `fast` in the prompt
  lands it at once.

  ```toml
  [[agents]]
  name = "Mock UI"
  command = "node"
  args = ["crates/core/examples/mock_ui_agent.js"]
  ```
- **Tests:** unit tests live in `#[cfg(test)]` modules, inline or in a file of their own beside
  the module (`foo.rs` + `foo/tests.rs`); there is still no `tests/` directory.
- **CI:** fmt, core tests, app tests and clippy, all `--locked`, because `Cargo.lock` is the
  only pin for revless `gpui`. A `v*` tag cuts a GitHub pre-release tarball. Nothing goes to
  crates.io.

## Repo layout

| Path | Crate | What |
|---|---|---|
| `crates/app` | `onehand` | GPUI front end + binary (`main.rs` is ~15 lines; logic lives in the lib) |
| `crates/core` | `onehand-core` | GUI-free logic: config, workspace tree, ACP client, chat model, remote bridge, connectors, issues, editor rules, git status, worktrees |
| `crates/plugin-api` | `onehand-plugin-api` | GUI-free plugin IDs and descriptors |
| `crates/plugin-host` | `onehand-plugin-host` | `WorkbenchMode` + `Request`, remote-channel factory, and what a plugin draws as the app does: the button wrapper, menu rows and `menu_below`, the segmented `switch`, status ink and dock surface |
| `crates/terminal-ui` | `onehand-terminal-ui` | PTY/grid ownership shared by the terminal dock and Neovim |
| `plugins/builtin/*` | built-in plugins | Editor, Files, Markdown, Neovim, Issues, Plugins, Telegram, GitHub; composed in `crates/app/src/plugins.rs` |
| `vendor/gpui-terminal` | `gpui-terminal` | vendored terminal grid + our `onehand patch` interaction layer |

## Invariants

- **Core is GUI-free:** `cargo tree -p onehand-core -i gpui` must error with "did not match any
  packages". Use `-i`, not `| grep gpui`: the checkout path contains `gpui`.
- **Core dictates no async runtime.** Blocking functions, thin async wrappers. GPUI runs on smol
  with no tokio reactor; tokio I/O on the UI executor panics. In the app, `acp.rs` and `remote.rs`
  drive their tokio side on runtimes of their own and cross to GPUI over a `futures` channel.
- **Nothing in core is `pub` unless something outside the crate names it.** Every first-party
  library carries `#![warn(unreachable_pub)]`. `dead_code` stops at a `pub` item, which is how dead
  code hid before.
- **In `crates/app` only `assets` and `shell` are `pub`; keep new modules private.**
- **Shared rules live in core, not per call site** (e.g. `GitStatus::label`, `Chat::apply`,
  `AppConfig::update_in_place`, `Chats::reconcile`).
- **`crates/app/src/guards.rs` tests hold rules rustc cannot.** Among them:
  - no glyph used as an icon;
  - every button goes through the app's wrapper;
  - our own event enums matched exhaustively (never `matches!`);
  - no field assigned and never read;
  - code never cites a document;
  - every `.md` file is in English.

  Each guard was added after the same mistake appeared in
  several places. Remove one only after a probe shows a lint covers it.

## GPUI model

- State lives in entities (`Entity<T>`), which mutate via `cx.update`/`cx.listener` and publish
  with `cx.notify()`. A panel asks its owner for things by emitting an event (`EventEmitter` +
  `cx.subscribe`). For example, the chat emits `ChatPaneEvent::OpenFile` and the shell decides
  where it goes.
- An entity renders where it is mounted, once. Updating an entity that is already being updated
  panics. Defer (`cx.defer` / `Window::defer`) when reaching into a parent or another window's
  shell.
- Async work runs as `cx.spawn` plus `cx.background_executor()`, and results come back through
  `entity.update`. Native dialogs, directory scans and file reads never run inline in a render or
  action handler.
- Data model: `Shared` (the process-wide global: agent menu, windows, recents, keymap, ACP
  runtime, remote bridge, unattended runs, among others) → `Shell` per window →
  `Workspace { roots }` → `ProjectRoot { sessions }` → `Session { spec, uid }`. Sessions connect
  lazily, on first show.

## Rules

- **Icons:** every icon is an SVG from `gpui_component::IconName`, never a Unicode or emoji glyph.
  `crate::icons` holds only what that enum cannot draw. Add one via `assets/icons/manifest.toml` +
  `scripts/sync-icons.sh` + the `icons!` macro, and never hand-edit a synced SVG.
  - Some library icons are renamed: `close` is Lucide's `x`.
  - An icon that fails to resolve draws nothing rather than failing the build.
  - Some bundled SVGs have a hard-coded stroke (`dash.svg`). After bumping `gpui-component`, grep
    for `stroke="black"`.
- **Code describes; it never cites.** No comment, doc comment or runtime string names a document
  (CLAUDE.md, DESIGN*.md, DECISIONS.md, anything under `docs/`, a section or item code). Give
  the reason in the comment's own words. Pointing at code is fine. Documents point at code, never
  the reverse.
- **Every `.md` file is written in English.** Quoted non-English data stays as it is.
- **Never hard-code a colour, radius or size.** Read `cx.theme()`; sizes are rems, because zoom
  overrides the rem base per panel.
- **Reuse gpui-component before building.** Buttons go through `crate::controls::action`, menu
  rows through `controls::menu_item`/`menu_row` (re-exported from `onehand_plugin_host`, where a
  plugin reaches them too), so the pointer cursor is right.
- **Keep rendering bounded**: a named cap per list, and say on screen when it bites.
- **Don't self-verify UI by launching or screenshotting.** Build, test, stop; the user looks.

## Load-bearing specifics

- **Key contexts:**
  - Window commands are bound `Shell && !Dialog`.
  - `Ctrl+S` is bound `Shell && !Terminal && !Dialog`, so a PTY keeps it.
  - Anything mounting a live grid, the Neovim mode included, must take the `Terminal` context,
    or the editor's save fires over `:w`.
  - The terminal toggle is plain `` Ctrl+` ``, because shift+backtick cannot be typed.
  - Commands live in `crates/app/src/keymap.rs`, and overrides persist under `[keymap]`.
- **Overlay layers:** `Shell::render` mounts `Root::render_{sheet,dialog,notification}_layer`
  itself. Without them every dialog is dead.
- **Unattended teardown:** a run's project is dropped with `Shell::forget_root`, never
  `remove_root`, which re-shows the active session and steals the caret.
- **Transcripts:** `items.jsonl` is append-only and written at every turn end. A rename writes
  metadata only (`Chat::flush_meta`).
- **Telegram token:** never in `onehand.toml`, which settings rewrite whole. It comes from
  `$ONEHAND_TELEGRAM_TOKEN` (renamed by `token_env`) or `<config_dir>/onehand/telegram.token`. A chat not on
  `allowed_chats` gets no reply at all.

## Gotchas (one line each; full reasons in docs/rules-and-gotchas.md)

- **Key bindings beat `on_key_down`**, matched against the focus context stack first. A binding
  can silently steal a key a PTY needs. `!Ctx` means "nowhere in the stack".
- **`with_rem_size` goes in all three element phases** (layout, prepaint, paint).
- **Font families fail silently.** Pick from `all_font_names()` (`use_installed_mono` (`shell/boot.rs`),
  `config::resolve_monospace`); never assume a name resolves.
- **`mx_auto` does nothing in a `gpui::list` row**, which is its own layout root. Centre with
  `h_flex().justify_center()` around a `max_w` child.
- **Bare dock panels track their own focus.** Every panel here is a bare `DockItem::Panel`, so
  its `render` must call `track_focus`, or `contains_focused` lies and panel keys hit the wrong
  panel.
- **Closing a panel that holds focus kills the whole keymap.** Call `ChatPane::reclaim_focus`
  *before* the panel leaves the frame.
- **Zoom factors snap to the step**, or float drift leaves `Ctrl+0` the only way back to 100%.
- **`gpui` carries no rev anywhere** (vendor included). `gpui-component` is pinned by rev.
  `Cargo.lock` pins both.
- **A Vietnamese IME can swallow a typed `/`.** Keep the composer's `+` menu entries that
  insert `@` and `/` from code (`Composer::insert_trigger`).
- **Terminal pitfalls** (paint cost, repaint gate, cell measurement, vendor tests) load
  automatically from `.claude/rules/terminal.md` when terminal files are read.
