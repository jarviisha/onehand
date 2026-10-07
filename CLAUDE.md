# CLAUDE.md

`onehand` is a Rust desktop GUI (**GPUI** + [gpui-component](https://github.com/longbridge/gpui-component))
that hosts AI coding agents. Window: a left navigation **rail**, a central **agent pane** (a native
chat), a right **Workbench** dock (editor, file tree, Markdown, Neovim, Issues, Plugins) and a bottom
**terminal** dock. A *workspace* groups *projects*; each project runs *sessions*, and **every
session is an ACP agent** ([Agent Client Protocol](https://agentclientprotocol.com)).

## Where the rest lives

Read the relevant one **before** changing the area it covers.
This is the shared guide for all coding agents; `AGENTS.md` routes here without
duplicating the rules. Automatic skill discovery is optional; the reading routes below are not.

How the repository is held together:

- [CONTEXT.md](CONTEXT.md): the glossary; use its terms and never the words it says to avoid.
- [DESIGN.md](DESIGN.md): the **binding** UI overview (layout, transcript, theme rules),
  structure and behaviour only. Before any visible change, also read
  [.claude/skills/design-contract/SKILL.md](.claude/skills/design-contract/SKILL.md).
- [docs/rules-and-gotchas.md](docs/rules-and-gotchas.md): the full reasons behind the short rules
  and gotchas below.
- [docs/known-gaps.md](docs/known-gaps.md): what this build deliberately does not do yet, and why.

The features that span core and app, each as built:

- [docs/workflows.md](docs/workflows.md): workflows, the run engine and its one driver.
- [docs/tasks.md](docs/tasks.md): the Tasks page and the task model behind it.
- [docs/unattended.md](docs/unattended.md): unattended runs end to end.

Everything else (chat pane, Workbench, rail, terminal, persistence, config) keeps its reasons in
the comments beside its code, starting from each module's header; `crates/app/src/lib.rs` and
`crates/core/src/lib.rs` list the modules.

## Authority and scope

- The user's explicit task determines the change. Repository contracts constrain the
  implementation; generic skills and plugin defaults must be adapted to those contracts.
- `CONTEXT.md` owns vocabulary, `DESIGN.md` owns the UI overview, and feature documents own
  their detailed behaviour. Code and tests show what is implemented, not permission to rewrite
  a contract to fit it. Surface a conflict; preserve the contract unless the requested change
  authorizes changing it. Update the owning document alongside an authorized behaviour change.
- `docs/known-gaps.md` records deliberate omissions; listing one does not request implementing it.
  `CHANGELOG.md` is history. `docs/proposals/` holds shared proposals, and `localDocs/` local
  proposals and superseded notes; neither is a current requirement. Consult a proposal only when
  the task explicitly calls for it, and reconcile it with the current contracts before
  implementation. A proposal that is built moves into its owning documents and leaves
  `docs/proposals/` in the same PR.
- Keep a rule in one owning document. Link to it from entrypoints and skills rather than
  copying it or creating a competing design system.

## Skills and plugins

These are applicability rules, not a list of tools every contributor must install.
Availability or installation does not authorize a skill's side effects.

| Tool or workflow | Fit for this repository |
|---|---|
| `design-contract` | Required reading for visible changes; follow the existing GPUI theme and components. |
| Matt Pocock review, debugging, domain modeling | Use for the requested concern; keep the existing glossary and document ownership. Tracker/triage workflows publish to this repository's GitHub issues: a spec gets `ready-for-agent`, never `auto` (the trigger label: an unattended run would claim it). A proposal under `docs/proposals/` keeps its own table of which step has a spec. |
| Matt Pocock `setup-pre-commit`, TypeScript setup | Do not apply npm/Husky/Prettier or TypeScript defaults to this Rust workspace. Use the Makefile and Cargo checks. |
| `ponytail` | Simplify within the requested behaviour. Preserve the module seams, guards, rendering bounds and deliberate exceptions; fewer files is not a reason to bypass them. |
| `ui-ux-pro-max` | General UX guidance only; it has no GPUI stack. Adapt recommendations to `DESIGN.md`, not a generated palette or a second design-system document. |
| Commit/push/PR commands | Use only for the Git operations requested. Run relevant checks before invoking a command-only workflow; it is not a validation step. |

## Commands

```bash
cargo run [-- /path/to/project]  # the app; the arg seeds the workspace's first project
make dev [ROOT=/path]            # a debug build beside the app in use, data root ~/onehand-dev
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
- **CI:** fmt, core tests, tests of every other workspace member (plugins and vendor included),
  and clippy. Dependency-resolving commands use `--locked`, because `Cargo.lock` is the
  only pin for revless `gpui`. A `v*` tag cuts a GitHub pre-release tarball. Nothing goes to
  crates.io.

## Repo layout

| Path | Crate | What |
|---|---|---|
| `crates/app` | `onehand` | GPUI front end + binary (`main.rs` is ~25 lines; logic lives in the lib) |
| `crates/core` | `onehand-core` | GUI-free logic: config, workspace tree, ACP client, chat model, remote bridge, connectors, issues, editor rules, git status, worktrees, the workflow engine, tasks, unattended runs |
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
- **Nothing in core is `pub` unless something outside the crate names it.** First-party
  libraries other than the app carry `#![warn(unreachable_pub)]`. Keeping internal items
  private lets `dead_code` reach them; publicly reachable items can hide unused code.
- **In `crates/app` only `assets` and `shell` are `pub`; keep new modules private.**
- **Shared rules live in core, not per call site** (e.g. `GitStatus::label`, `Chat::apply`,
  `AppConfig::update_in_place`, `Chats::reconcile`).
- **`crates/app/src/guards.rs` checks recurring violations rustc cannot catch.** These source
  scans are partial checks, not proof of every rule. Their coverage and review gaps are in
  [docs/rules-and-gotchas.md](docs/rules-and-gotchas.md#guard-coverage).
  Match our own event enums exhaustively, without wildcard arms or partial `if let` handling.
  Each guard was added after the same mistake appeared in several places. Remove one only
  after a probe shows a lint covers it.

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
  - Some bundled SVGs have a hard-coded stroke (`dash.svg`, `resize-corner.svg`). After bumping
    `gpui-component`, check for `stroke="black"` and `stroke="#`.
- **Code describes; it never cites.** No comment, doc comment or runtime string names a document
  (CLAUDE.md, DESIGN.md, anything under `docs/`, a section or item code). Give
  the reason in the comment's own words. Pointing at code is fine. Documents point at code, never
  the reverse.
- **Every `.md` file is written in English.** Quoted non-English data stays as it is.
- **Never hard-code a colour, radius or size.** Read `cx.theme()`; sizes are rems, because zoom
  overrides the rem base per panel.
- **Reuse gpui-component before building.** Buttons go through `crate::controls::action`, menu
  rows through `controls::menu_item`/`menu_row` (re-exported from `onehand_plugin_host`, where a
  plugin reaches them too), so the pointer cursor is right.
- **Keep rendering bounded**: a named cap per list, and say on screen when it bites.
- **Split a file before it passes about 800 lines of code** (tests not counted). Cut along a
  seam that stands alone: a strip, a dialog, a page section, a parser. It becomes a sibling
  module (`foo.rs` + `foo/bar.rs`, or a file beside it in the same directory). Tests move to
  `foo/tests.rs` first. When a change would push a file past the line, split it in the same PR.
  Never split mid-thought just to hit a number.
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
- **Terminal pitfalls:** before changing `vendor/gpui-terminal/**`, `crates/terminal-ui/**`,
  `crates/app/src/terminal.rs`, `crates/app/src/terminal/**` or
  `plugins/builtin/workbench-neovim/**`, read
  [.claude/rules/terminal.md](.claude/rules/terminal.md). This is required even when the host
  does not load path-scoped rules automatically.
