# Shared terminal row cache and issue #9 acceptance

This completes the CPU caching work for [issue #9](https://github.com/jarviisha/onehand/issues/9),
on top of the GPU primitive and PTY scheduling changes in
[PR #29](https://github.com/jarviisha/onehand/pull/29). The Terminal dock and
Workbench Neovim continue to use the same `gpui_terminal::TerminalView`.

## Change and invalidation

`TerminalRenderer` retains one visible grid of cell snapshots, background
rectangles, box commands, text runs and GPUI `ShapedLine` values. A paint compares
each current visible row with its snapshot. Unchanged rows reuse all of that
data and make no row `shape_line` calls. Changed rows rebuild their text runs;
backgrounds and box commands are rebuilt only if their own inputs changed.
For example, changing a relative line number does not rebuild the same borders.
Cell snapshots reuse their allocation when updated.

This uses full cell equality instead of consuming `Term::damage()`: it also
observes edits through the public mutable-grid API, and does not depend on
another caller leaving the damage flags intact. The comparison is still
O(visible cells); it removes repeated construction and shaping lookups, not
every visit to the grid. Whole-screen scrolling can still invalidate most rows.

Renderer clones share the cache. The key includes dimensions, font family and
size, cell metrics, line-height multiplier, window scale, the application
palette and every terminal OSC colour override. A different window text system
also resets it. Scrollback and alternate-screen transitions compare the new
visible cells in each row slot. Shrinking the viewport drops old row contents;
there is no history of off-screen layouts.

Selection, preedit and cursor remain live overlays. Cached geometry is relative
to columns and rows, with the current origin applied at paint time. GPUI still
receives all visible primitives on each frame; this is not a retained GPU
surface or partial-window presentation change. There is no new output timer,
frame-rate cap, or change to parsing, backpressure or PTY input.

## Measurement method

Release builds use the locked dependencies and opt-in `terminal-profiling`
feature, with event tracing disabled. CPU is the onehand host process, where
100% means one logical CPU; Neovim and search subprocesses are excluded. GPU
values are per-process DRM graphics-engine busy time, not VRAM or whole-system
GPU utilization. Background/box timing includes cached command submission plus
construction on misses; row shaping counts exclude the separate cursor pass.

The runner retains geometry and visibility checks, raw samples, stage counters
and binary SHA-256 values. Active phase averages omit one second at each end;
the bounded `seq` burst is assessed over its whole drain window. Compiler and
visual-comparison processes do not run during accepted performance cases.
GPU clocks are dynamic; small differences are not a stable GPU-saving claim.

Full-application cases mount the actual Terminal dock or Workbench through the
application's public shell methods. Each has an isolated onehand config and
project fixture, with no agent sessions. Workbench cases load the installed
user Neovim init/plugins, including relative line numbers and the existing
Snacks setup. The benchmark still standardizes cursor blinking, test-buffer
contents, syntax and selected display options; it is a repeatable workload
under that configuration, not an untouched interactive editing session.

## Visual correctness

The `terminal_cache` example paints a retained renderer beside a fresh renderer
using the same terminal state. Its 14 sequential cases cover cursor motion,
selection, focus/preedit, changed text/styles, OSC colours, application palette,
zoom/line height, font family, resize, scrollback and alternate-screen entry/exit.
The reference exercises the same rendering algorithm with caching reset each
paint, so this checks stale-cache errors rather than unrelated font coverage.

The automated comparison checks the two interior regions with a tolerance of
one value per 8-bit RGB channel, allowing position-dependent rounding. A larger
difference fails. Native Wayland at 1× and XWayland at GPUI scale 1.25 are tested
without changing monitor settings. Separate cache unit tests cover scale-key
invalidation; the visual runs use a fixed scale for all transitions.

## Reproduction

Build first, then run each GUI benchmark sequentially:

```sh
cargo build --locked --release -p onehand --example terminal_perf --features terminal-profiling
cargo build --locked -p onehand --example terminal_cache

python3 scripts/terminal-perf-run.py rows-replay --workload replay --border rounded
python3 scripts/terminal-perf-run.py rows-nvim --workload cursor --hz 20
python3 scripts/terminal-perf-run.py rows-nvim40 --workload cursor --hz 40

mkdir -p /tmp/onehand-acceptance-project
cp -r crates/app/src /tmp/onehand-acceptance-project/src
python3 scripts/terminal-perf-run.py app-seq --app terminal --workload seq \
  --project /tmp/onehand-acceptance-project
python3 scripts/terminal-perf-run.py app-nvim --app neovim --workload cursor \
  --project /tmp/onehand-acceptance-project --user-nvim-config
PERF_SNACKS_RTP="$HOME/.local/share/nvim/lazy/snacks.nvim" \
PERF_GREP_ROOT=/tmp/onehand-acceptance-project \
python3 scripts/terminal-perf-run.py app-snacks --app neovim --workload snacks \
  --project /tmp/onehand-acceptance-project --user-nvim-config

python3 scripts/terminal-cache-compare.py rows-wayland
python3 scripts/terminal-cache-compare.py rows-x11 --x11-scale 1.25
```

Hyprland, `/proc` DRM access and the installed fonts are required; visual
comparisons additionally require `grim` and Pillow. Use `--monitor NAME` to
select the display. Both runners control only their own window and restore
the previous focus. Use a fresh case name for every run. Invalid runs must be
retained and excluded, not mixed into the before/after aggregates.
