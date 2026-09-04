# Architectural decisions

This file records choices that are still in force and whose reasons cannot be
recovered reliably by reading the code. It is not an implementation guide or a
roadmap: current structure and operational details live in `CLAUDE.md`, known
gaps live there and in `README.md`, and exact dependency revisions live in the
manifests and committed `Cargo.lock`.

Each decision states its context, the choice, and the consequences that future
changes must account for. Superseded decisions should be replaced deliberately,
not allowed to drift away from the code.

## D1 · Use gpui-component's theme with a onehand surface ramp

**Context.** The library palette works well for panels and forms, but a
transcript simultaneously needs a reading ground, an output well, a user bubble
and a floating composer. In dark mode the defaults did not distinguish enough
of those adjacent surfaces.

**Decision.** Keep gpui-component's typography, radii, semantic colours and
remaining palette. At boot, `crate::theme::install` overrides only the surface
and corresponding text layers: `background`, `muted`, `secondary`, `accent`,
`popover` and `border`. Render code reads visual values from `cx.theme()`; it
does not carry literal colours or a second palette.

**Consequences.** The surface ramp has one owner and contrast tests. `DESIGN.md`
and `DESIGN-ANSWER.md` specify structure and behaviour rather than duplicating
theme values. A theme change must preserve the distinctions between adjacent
transcript surfaces.

## D2 · Appearance is user-selectable and follows the system continuously

**Context.** gpui-component supplies light and dark configurations, while the
desktop appearance can arrive late or change after startup. A typo in the same
config file must not prevent the agent list from loading.

**Decision.** `appearance` accepts `system` (the default), `light` or `dark`.
`system` observes desktop changes rather than sampling once. Unknown values fall
back to `system`, and settings writes preserve every unrelated config entry.

**Consequences.** Applying an appearance replaces the whole `ThemeConfig`, so
the resolved monospace family must be restored after every switch and all open
windows must refresh. The embedded terminal keeps its independent ANSI palette.

## D3 · The Workbench editor is a quick editor, not an IDE

**Context.** Syntax highlighting is useful for short edits, but every grammar
adds build time and binary size, and an LSP would introduce a separate lifecycle
and product surface.

**Decision.** Enable gpui-component's tree-sitter support for the deliberately
small grammar set declared in `crates/app/Cargo.toml`. Do not add an LSP to this
editor.

**Consequences.** Removing the tree-sitter feature reduces the editor to plain
text. Adding a grammar requires evidence that the Workbench needs it; adding IDE
semantics requires revisiting this decision rather than quietly growing them.

## D4 · Neovim is a Workbench mode backed by the shared PTY

**Context.** Neovim operates on project files, while the terminal dock represents
login shells. A separate Neovim renderer would duplicate terminal input,
rendering, resize, clipboard and child-process behaviour.

**Decision.** Run Neovim as a PTY program in the Workbench beside Editor and
Files. Spawn it through `onehand_terminal_ui::spawn_pty`; do not embed it through
msgpack RPC or mount it as a terminal tab.

**Consequences.** The mode inherits the shared terminal patches. Its zoom changes
the grid font size, it takes the `Terminal` key context, and merely selecting the
mode does not spawn a process. PTY protocol support and remaining limitations
are documented in `CLAUDE.md`.

## D5 · Prefer the bundled icon registry; check in only missing shapes

**Context.** gpui-component itself references its bundled icon assets, so they
must ship regardless. A parallel app-owned icon set previously duplicated those
assets and drifted in naming and stroke style.

**Decision.** Use `gpui_component::IconName` for every shape it can draw.
`assets/icons/` contains only brand marks and shapes with no usable bundled
equivalent. An oddly named or imperfect approximation does not qualify as an
absence.

**Consequences.** Updating gpui-component requires visually checking the chrome,
because an unresolved icon draws blank rather than failing the build. New local
icons go through `assets/icons/manifest.toml`, `scripts/sync-icons.sh` and the
`icons!` registry, with their licences retained.

## D6 · Plugins are built in and the registry seals at startup

**Context.** The first plugin boundary is for composition and ownership, not an
extension marketplace. Loading third-party Rust dynamic libraries would expose
an unstable ABI and couple extensions to the GUI implementation.

**Decision.** Editor, Files, Neovim and Telegram are separate built-in crates
linked into the binary. `crates/app/src/plugins.rs` registers their contributions
before the first window and then seals the registry. The GUI-free API contains
stable identifiers, descriptors and capabilities. Per-window Workbench hosting
and contribution-specific integration types live in the host; the composition
root attaches the remote-channel factory. Any future external plugin system uses
a process protocol.

**Consequences.** Registration is unavailable after boot and the Rust API stays
`0.x` without a third-party compatibility promise. This boundary must not change
observable Workbench order, configuration, shortcuts or per-root state.

## D7 · Pin GPUI through one git source and the lock file

**Context.** Cargo identifies a git source by URL and revision. gpui-component
depends on the Zed repository without a `rev`; adding one to onehand creates a
second, type-incompatible `gpui` crate even when both commits contain identical
code.

**Decision.** Declare `gpui` and `gpui_platform` from the same Zed git URL with
no `rev`, including in the vendored terminal. Pin gpui-component by revision and
commit `Cargo.lock`, which is the exact GPUI pin.

**Consequences.** GPUI upgrades are deliberate lock-file updates, for example
`cargo update -p gpui --precise <sha>`. Never solve an upgrade by adding a
`gpui` revision to a manifest; the characteristic failure is an “expected
`gpui::…`, found `gpui::…`” type mismatch.

## D8 · Keep gpui-terminal vendored as an auditable patch stack

**Context.** The upstream crate provides a useful PTY and grid renderer but did
not provide the interaction and protocol behaviour onehand needs. Depending on
it directly would lose selection, clipboard, IME, bounded reads and the terminal
semantics required by full-screen programs.

**Decision.** Keep `vendor/gpui-terminal` in the workspace. Preserve the
upstream import as one commit and onehand's patches as the following commit, and
mark local changes with `onehand patch` so the delta remains reviewable.

**Consequences.** First-party formatting and lint commands exclude `vendor/`;
do not run bulk fixers over it. The upstream revision and licence live beside
the vendored crate, while the implemented patch set and protocol details live in
`CLAUDE.md`.
