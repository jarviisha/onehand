# Rounded terminal corners: quad rendering

This follows the PTY scheduling change in [terminal-coalescing.md](terminal-coalescing.md)
and addresses the remaining rounded-border cost identified by
[terminal-profiling.md](terminal-profiling.md). Measurements below isolate this
change against PR #29's previous head, `939605f`.

## Change and appearance

The shared renderer now paints `╭ ╮ ╯ ╰` using a clipped, transparent rounded
outline. GPUI's quad shader computes the curve in the main render pass; it no
longer needs to rasterize a terminal corner through an intermediate path
attachment. The outline extends beyond the cell, with its opposite borders
outside the content mask. Its mask intersects the parent's clip.

The outgoing centerlines, light-stroke width and existing one-logical-pixel
neighbor overlap are retained. Border widths come from the difference between
the snapped stroke edges, matching the straight fill quads at fractional scales.
The interior stays transparent, and the supplied foreground/alpha is retained.
No parser scheduling, frame limit, font shaping, row cache, or GPUI dependency
changes are included in this step.

The bend intentionally changes: the previous quadratic curve had separate
horizontal and vertical extents of 40% of the cell dimensions. The new
centerline is a circular arc with radius 40% of the smaller dimension. Corners
therefore look less elongated in tall cells. This is not pixel-identical
rasterization of the old curve.

The `terminal_corners` example renders old/new pairs at four cell sizes,
including fractional origins. Each row exercises plain backgrounds, alternating
cell backgrounds, translucent strokes, and an additional outer clip. Visual
inspection found connected joins, preserved background colors and clipped
geometry at all three tested scales:

- [Wayland, scale 1](terminal-rounded/gallery-1x.png)
- [XWayland, GPUI scale 1.25](terminal-rounded/gallery-1.25x.png)
- [XWayland, GPUI scale 1.5](terminal-rounded/gallery-1.5x.png)

Fractional scales were exercised through `GPUI_X11_SCALE_FACTOR`, without
changing the user's monitor configuration. The example logs GPUI's actual
scale factor. This checks the shared renderer at fractional scales, not the
Wayland fractional-scale protocol or physical scanout at those scales.
Translucent strokes retain the existing brighter overlaps at neighboring cells.
The reference quadratic renderer exists only in this diagnostic example.

```sh
cargo run --locked -p onehand --example terminal_corners
env -u WAYLAND_DISPLAY GPUI_X11_SCALE_FACTOR=1.25 \
  cargo run --locked -p onehand --example terminal_corners
```

## Measurement method

Both sides use release `terminal_perf` builds with `terminal-profiling` enabled
and event tracing disabled. The baseline is `939605f`; the candidate adds only
the production corner change. No compiler or gallery window runs during a
performance case. Cases run sequentially on eDP-1 at 1920 × 1080, about 60 Hz,
scale 1, with a 960 × 1000 or 1001 window and Liberation Mono 14 px (114 × 62
cells in either case). Each accepted case keeps its geometry throughout.
The host is AMD Ryzen 7 PRO 4750U with integrated AMD graphics, Linux
7.0.12-arch1-1, Neovim 0.12.3, and Rust 1.96.0.

The replay contains eight rounded corners across two panels in a fixed 100 × 50
area. `small` changes one character; `full` rewrites the area. Updates are offered
at absolute 20 or 40 Hz deadlines. The 20 Hz rounded cases run in before/after/
after/before order; the last baseline is retried after a workspace change. Straight borders provide a control. The Snacks cases use
`nvim --clean`, a fixed copied source tree, and Snacks revision
`ad9ede6a9cddf16cedbd31b8932d6dcdee9b716e`, with 10 scripted actions/second.
Repeated searches are asynchronous, so
they provide application-level evidence rather than identical PTY byte streams.

CPU is host process time, with 100% meaning one core; Neovim and search children
are excluded. GPU percentages measure the host's DRM graphics-engine busy time,
not VRAM, whole-system GPU utilization, or individual shader/pass timestamps.
Steady intervals exclude one second at each boundary. GPU clocks remain dynamic;
small differences must not be treated as stable speedups. Draw counts come from
the GPUI window histogram. No keyboard latency measurement is added in this step.

## Results

All ten accepted cases retained constant geometry/visibility and monotonic CPU
and GPU counters. Within each replay variant, update counts, offered byte counts
and payload hashes match across baseline and candidate. The rejected
`before-rounded20-b` changed workspace and is excluded entirely; its retry is
`before-rounded20-c`. Full aggregates, binary hashes, stage timings and exclusions
are retained in [terminal-rounded-results.json](terminal-rounded-results.json).

Each entry is **before → after**. Ranges are the two 20 Hz rounded runs; other
rows are one pair. GPU/CPU values are percentages; draws are per second.

| Workload | GPU gfx % | Host CPU % | GPUI draws/s |
| --- | ---: | ---: | ---: |
| Rounded 20 Hz, small | 3.40–3.50 → 1.44–2.15 | 9.16–9.18 → 8.35–8.66 | 19.83–20.07 → 19.96–19.98 |
| Rounded 20 Hz, full | 3.70–3.73 → 1.60–1.99 | 9.19–9.49 → 8.57–9.36 | 19.92–20.07 → 19.97–20.04 |
| Rounded 40 Hz, small | 7.09 → 3.75 | 16.97 → 15.98 | 40.27 → 40.01 |
| Rounded 40 Hz, full | 7.04 → 2.98 | 18.82 → 17.84 | 40.35 → 40.22 |
| Straight 20 Hz, small | 1.99 → 1.33 | 9.44 → 9.55 | 19.96 → 20.26 |
| Straight 20 Hz, full | 1.84 → 2.07 | 8.31 → 8.80 | 19.94 → 20.17 |
| Snacks grep | 8.03 → 4.82 | 23.94 → 23.07 | 36.58 → 37.23 |
| Snacks list movement | 3.78 → 2.01 | 13.03 → 12.69 | 18.17 → 16.50 |

The controlled rounded replays use less GPU busy time while keeping their
20/40 Hz draw cadence. Rounded-border GPU costs now fall near the straight-border
control in these runs. Straight-border results also vary, although that drawing
code is unchanged: this is why the measured percentage reductions are not a
universal hardware speedup. Clock snapshots even contain occasional implausible
readings (1 and 6784 MHz); they remain diagnostic data and are not used to
normalize engine busy time.

Snacks supports the same direction under a real Neovim picker, but its frame
counts differ between runs. In particular, list movement draws 18.17 → 16.50
frames/s, so the entire GPU reduction there cannot be attributed to a cheaper
corner per frame. The identical-byte replay is the stronger controlled evidence.

CPU changes are modest. This does not solve row assembly or glyph submission,
and terminal paint time does not improve uniformly: Snacks list movement takes
4.18 → 4.71 ms per draw in these samples. Idle GPU counters stay flat in every
accepted case. The probe's sampling timer is still present, so its idle CPU is
not a production idle measurement. Kitty parity and the complete user's Neovim
configuration are not measured here; issue #9 remains open.

## Validation and reproduction

- `cargo test --locked --workspace --features onehand/terminal-profiling`: passed,
  including 102 terminal unit tests and 19 terminal doctests (28 ignored).
- `make clippy CARGO='cargo --locked' CLIPPY_EXTRA='-- -D warnings'`: passed.
- `make fmt-check` and `git diff --check`: passed.
- Release profiling probe and the `terminal_corners` visual example: built.
- Geometry regression: all four orientations, fractional origins and multiple
  cell sizes preserve outgoing stroke edges, hide the remote borders and place
  the curve tangents inside the cell.

Keep separate copies of the baseline and candidate binaries, then run the same
case sequentially with each. The runner refuses to overwrite existing results:

```sh
cargo build --release --locked -p onehand --example terminal_perf \
  --features terminal-profiling
python3 scripts/terminal-perf-run.py rounded20 \
  --binary /path/to/saved/terminal_perf --border rounded --hz 20
python3 scripts/terminal-perf-run.py rounded40 \
  --binary /path/to/saved/terminal_perf --border rounded --hz 40
PERF_SNACKS_RTP=/path/to/snacks.nvim PERF_GREP_ROOT=/path/to/fixed/source \
  python3 scripts/terminal-perf-run.py snacks \
  --binary /path/to/saved/terminal_perf --workload snacks
```

The accepted measurements and raw sampler/frame logs were collected under
`/tmp/onehand-terminal-rounded` on 2026-09-15. The committed JSON retains all
accepted aggregates; the three committed gallery images retain the visual
comparison independently of temporary files.
