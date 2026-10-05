---
paths:
  - "vendor/gpui-terminal/**"
  - "crates/terminal-ui/**"
  - "crates/app/src/terminal.rs"
  - "plugins/builtin/workbench-neovim/**"
---

# Terminal grid pitfalls

What goes wrong in the terminal grid and the code around it. Full reasons:
`docs/rules-and-gotchas.md` (Gotchas).

- `vendor/gpui-terminal` is upstream `zortax/gpui-terminal@51f0292` plus our patches, each marked
  `onehand patch`. Never run `cargo fmt` / `clippy --fix` over it — use `make fmt` / `make lint`.
- Never `use super::*` in a test module there: gpui's glob import shadows `#[test]` and rustc dies
  with "recursion limit reached while expanding `#[test]`". Import the needed items by name.
- `gpui` is a revless git dependency everywhere, vendor included; adding a rev yields a second
  `gpui` and "expected gpui::App, found App".
- Per-cell paint cost is multiplied by the screen on every redraw: paint per run
  (`render::split_row_runs`, forced cell width, ligatures off, double-width cells alone) and keep
  `render::RowCache`. Measure before assuming shaping is the cost.
- A terminal repaint redraws the whole window. The reader drains with `view::take_batch`, asks
  once through `view::RepaintGate`, yields (`view::YieldOnce`) and never sleeps; parse batches are
  bounded by `view::PARSE_BATCH_CHUNKS`.
- A keystroke is not a repaint: only ask when `view::typing_changes_the_view`; pasting asks none.
- `TerminalRenderer::measure_cell` runs on a clone inside paint — write the result back to the
  view, or every click lands on the wrong cell (mostly vertically).
- Box-drawing strokes stay in the quad pass; no `PathBuilder` for rounded corners.
- The grid is told its surface (`terminal_palette` takes it) and is handed the resolved mono
  family by `spawn_pty`; the vendored default `monospace` resolves to nothing.
- A tab whose child exits is reaped via `Window::defer` inside `spawn_pty`, as a sweep, moving
  focus only if focus was inside the panel.
