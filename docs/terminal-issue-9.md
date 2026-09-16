# Shared terminal row cache and issue #9 acceptance

This completes the CPU caching work for [issue #9](https://github.com/jarviisha/onehand/issues/9),
on top of the GPU primitive and PTY scheduling changes in
[PR #29](https://github.com/jarviisha/onehand/pull/29). The Terminal dock and
Workbench Neovim continue to use the same `gpui_terminal::TerminalView`.

## Change and invalidation

`TerminalRenderer` retains one visible grid of cell snapshots, background
rectangles, box commands, text runs and GPUI `ShapedLine` values. A paint compares
each current visible row with its snapshot. Unchanged rows reuse all of that
data and make no row `shape_line` calls. Changed rows rebuild their text runs,
retaining the shaped lines for runs whose text and style still match. Their
shaped-run vector is updated in place, avoiding repeated allocation and copying
of GPUI's inline decoration storage. Backgrounds and box commands are rebuilt
only if their own inputs changed.
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

Measurements were taken on 2026-09-16 against `main` at `b394d8e`, after the
dock refactor in PR #30. Both binaries use that application code. The machine
is a Ryzen 7 PRO 4750U with integrated AMD graphics, Linux 7.2.6, Rust 1.96.0
and Neovim 0.12.5. Accepted windows stay at 960 × 1,000 on the same 60 Hz
1920 × 1080 display at scale 1. The isolated grid is 114 × 62; the real dock
is 80 × 10 and Workbench is 45 × 58 in both builds.

[Aggregates, fingerprints and visual checks](terminal-issue-9-results.json)
retain 14 accepted release runs and all 34 visual comparisons. All final runs
passed the geometry/visibility checks. Exploratory runs from earlier candidates,
including a run discarded for a workspace change, are excluded from this report.

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

## Results

The controlled small-update cases reduce host CPU by **15–25%**. Row shaping
calls fall sharply; GPU utilization does not consistently fall, since cached
rows still submit their glyphs and quads.

| Workload | Host CPU before → after | Terminal paint, µs/frame before → after | Row shaping calls/frame before → after |
| --- | ---: | ---: | ---: |
| Replay, small update, 20 Hz | 8.79% → 6.57% | 2,307 → 1,376 | 96 → 1 |
| Clean Neovim, cursor, 20 Hz | 10.49% → 8.95% | 2,771 → 1,914 | 121 → 3 |
| Clean Neovim, cursor, 40 Hz | 20.17% → 16.98% | 2,853 → 2,001 | 121 → 2.94 |
| Clean Neovim, scrolling, 40 Hz | 20.91% → 17.75% | 3,068 → 2,479 | 121 → 121 |
| Clean Neovim, forced redraw, 20 Hz | 11.16% → 10.48% | 2,914 → 2,507 | 121 → 121 |

The replay reuses 98.4% of row lookups, and the clean cursor phase reuses about
95.2%. The latter changes cursor-line highlighting as well as cursor position;
the remaining row-shaping calls are for changed text/styles. Unchanged cells
with a cursor-only position change need no row shaping. The measured draw rate
stays near the offered 20/40 Hz (approximately 20 and 39 in the cursor cases),
rather than saving work by capping the rate. Scrolling still invalidates most
rows; in-place run storage and independent background/box reuse reduce some
of that construction cost even when the text must be shaped again.

Full-application results with the installed user configuration:

| Workbench workload | Host CPU before | Host CPU after |
| --- | ---: | ---: |
| Cursor movement, 20 Hz | 14.04% | 13.49% |
| Scrolling, 20 Hz | 13.76% | 12.65% |
| Forced redraw, 20 Hz | 12.30% | 13.75% |
| Snacks Grep, first pair | 18.14% | 15.42% |
| Snacks Grep, reverse-order pair | 23.60% | 21.34% |
| Snacks list movement, first pair | 13.86% | 14.10% |
| Snacks list movement, reverse-order pair | 12.85% | 14.45% |

These results are mixed: ordinary Workbench movement improves modestly, while
forced redraw and Snacks list movement consume more host CPU in these samples.
Relative line numbers change most visible rows on every movement; retaining
stable runs reduces cursor-phase row shaping from **116 to 54 calls/frame**,
but does not remove the rest of the full-window work. The controlled clean
Neovim saving is not a promise of the same saving under every user configuration.

Snacks receives the same scripted 10 actions/second, but its asynchronous
search/preview output generates different frame counts: Grep ranges from
22.1 to 38.9 draws/second across these runs. Thus its CPU/GPU deltas are workload
observations, not an isolated per-frame saving or a general performance guarantee.
All scripted query/list phases finish. All candidate idle phases have flat
frame counts and zero graphics-engine deltas; one baseline Snacks run has one
late idle frame. The sampling timer itself remains diagnostic-only.

In the real Terminal dock, both builds process exactly **1,488,913 PTY bytes**
for `seq 1 200000` plus its completion marker. Across the approximately
eight-second burst/drain window, host CPU time is **0.21 → 0.20 seconds** and
graphics-engine time is **4.26 → 2.34 ms**. Both builds have five consecutive idle
samples at seven cumulative frames, with zero idle graphics-engine time. This
checks complete draining without a sustained render load; the parser regression
independently checks retained final lines and scrollback contents. It does not
measure physical keyboard-to-screen latency or establish parity with Kitty.

The original acceptance cases are now exercised in both actual application
hosts, alongside damage/cache correctness and the existing lossless/coalescing
regressions. GPUI still submits the full scene, and reducing that remaining GPU
cost would require further renderer work beyond this CPU cache.

## Visual correctness

The `terminal_cache` example paints a retained renderer beside a fresh renderer
using the same terminal state. Its 17 sequential cases cover cursor motion,
selection, focus/preedit, changed text/styles, OSC colours, application palette,
zoom/line height, font family, resize, scrollback, alternate-screen entry/exit,
and changed-run reuse/relocation/style invalidation.
The reference exercises the same rendering algorithm with caching reset each
paint, so this checks stale-cache errors rather than unrelated font coverage.

The automated comparison checks the two interior regions with a tolerance of
one value per 8-bit RGB channel, allowing position-dependent rounding. A larger
difference fails. Native Wayland at 1× and XWayland at GPUI scale 1.25 are tested
without changing monitor settings. Separate cache unit tests cover scale-key
invalidation; the visual runs use a fixed scale for all transitions.

All 34 comparisons passed with an observed maximum RGB difference of zero.
Example captures (retained on the left, fresh on the right):
[initial grid at 1×](terminal-cache/visual-wayland-00.png),
[font change at 1.25×](terminal-cache/visual-x11-09.png), and
[relocated stable run](terminal-cache/visual-x11-15.png).

## Automated verification

- All workspace tests passed with terminal profiling enabled, including 109
  terminal unit tests, 92 app tests and 19 terminal doc tests (28 ignored).
- Seven new cache tests cover unchanged/changed rows, cell styles and copy-on-write
  extras, independent background/box invalidation, rendering-key changes,
  viewport replacement/shrink, shared renderer clones and run reuse within
  changed rows (including relocation and rejection of changed text/style).
- Existing repaint-gate, bounded lossless draining, foreground-yield and
  200,000-line parser/scrollback regressions remain passing.
- Strict first-party Clippy, strict profiling-example Clippy, first-party
  formatting and Python/shell syntax checks passed.

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
