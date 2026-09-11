# Terminal performance audit — 2026-09-10

Reference: [issue #9](https://github.com/jarviisha/onehand/issues/9), with the shared-terminal changes from PR #10 already present.

Follow-up: [controlled cadence and host-stage profiling](terminal-profiling.md)
separates the remaining CPU costs and repeats the border comparison on 2026-09-11.

## Findings and changes

- **Straight box-drawing strokes used the vector-path pipeline.** Neovim indent guides and picker borders can insert these between text on every row. In the pinned GPUI WGPU backend (`e0931d5`, `gpui_wgpu/src/wgpu_renderer.rs`), every `PrimitiveBatch::Paths` ends the main pass, clears/rasterizes an intermediate attachment, and resumes the main pass. A straight segment now uses a quad with the same centerline, thickness, endpoints and existing junction overlap. Double strokes still use two segments; rounded corners retain their curved paths.
- **A redundant terminal background covered the entire canvas.** The canvas already paints its full bounds, including padding. The outer div no longer paints another full-size background behind it.
- **IME notifications bypassed the keyboard repaint optimization.** Commit now notifies only if it removes a composition, scroll offset or selection. Repeated identical preedit updates and clearing an empty composition no longer request a frame.
- **The host's 2,000-line scrollback limit was never applied.** `TerminalConfig.scrollback` now reaches the Alacritty grid at construction and on configuration changes, including changes made while the alternate screen is active. Previously the grid retained the default 10,000 lines.

The ordinary glyph pipeline still submits the visible grid to GPUI on each painted frame. Row-run shaping and the line-layout cache reduce CPU shaping work, but do not retain terminal pixels on the GPU between window frames. This patch does not introduce damage-based GPU rendering or claim general performance parity with Kitty.

## Measurement method

- AMD Ryzen 7 PRO 4750U, integrated AMD GPU `1002:1636`, amdgpu, Wayland/Hyprland, 60 Hz, scale 1.
- Neovim 0.12.3 (`--clean -n -i NONE`); Kitty 0.47.1 with `--config NONE`; Snacks `ad9ede6a9cddf16cedbd31b8932d6dcdee9b716e` loaded directly from the installed runtime.
- Release builds with repository LTO settings. No compilation runs during accepted measurements.
- Benchmark windows approximately 960 × 1,000 px, pinned on the same monitor. Terminal probe: Liberation Mono, 14 px, 114 × 62 cells. Kitty: same family, 10.5 pt, 120 × 62 cells because cell widths are rounded differently. This is a matched pixel area comparison, not an exact matched grid comparison.
- CPU is host process CPU-seconds divided by elapsed time, relative to one logical core. Child Neovim/rg CPU is excluded. GPU is the delta of `drm-engine-gfx` nanoseconds from `/proc/PID/fdinfo`, deduplicated by DRM device/client ID. It measures engine utilization, not VRAM or total desktop GPU load. GPU frequency is not locked; avoid interpreting small differences as improvements.
- Cursor/scroll/redraw: a generated 20,000-line Lua buffer, 500 updates per phase at a nominal 16 ms interval. These exercise PTY-driven redraws, not actual keyboard repeat delivery or a running IME.
- `PERF_GUIDES=1` adds two box-drawing indent strokes to each generated line to isolate the straight-stroke rendering cost.
- Snacks: real fullscreen Grep, cycling eight queries over this repository at 100 ms intervals for eight seconds; then moving through results for eight seconds. Uses Snacks defaults in a clean Neovim, not the user's entire plugin configuration.
- `seq`: one `seq 1 200000`, followed by eight seconds to observe draining, then five seconds idle. The terminal probe's frame counter should stop once output drains.
- The probe logs cumulative `Window::draw` histograms once per second without invalidating the window. This adds a small amount of idle CPU to the probe only.

Early exploratory runs on another monitor, runs overlapping compilation, and a run whose frame counter stopped when its window became hidden are excluded from final comparisons. No GPU saving is inferred from a hidden window.

## Results

The final before/after/Kitty Snacks runs were sequential, on the same monitor, with unchanged window geometry throughout. The reference is the terminal code already on `main` before this patch. [Raw aggregates and build fingerprints](terminal-performance-results.json) accompany these results.

| Workload | Reference GPU | Patched GPU | Kitty GPU |
| --- | ---: | ---: | ---: |
| Snacks Grep, changing queries | 77.5% | 21.6% | 8.0% |
| Snacks result-list movement | 78.2% | 11.0% | 3.4% |
| Idle after Snacks | 1.35%* | 0.0% | 0.0% |

GPU engine time fell about **72% for Grep** and **86% for result movement**. This is a large improvement, not parity: the patched shared probe still uses about 2.7 times Kitty's GPU utilization during this Grep workload. Query results/preview contents can vary with asynchronous search ordering; the runs use the same queries and repository, not a recorded byte-for-byte PTY replay. GPU clocks are dynamic.

The reference produced 210 window draws over the entire Snacks run; the patched probe produced 452. Host draw-duration p95 fell from 10.9 ms to 7.7 ms; maximum observed draw time, including startup, fell from 110.4 ms to 12.2 ms. These are GPUI draw timings, not presentation latency or GPU timings. Grep CPU rose from 17.9% to 24.5% while more frames were processed; result-list CPU fell from 16.2% to 12.7%. CPU percentage alone would miss the removal of a GPU bottleneck.

The final indent-guide probe used 15.5% GPU for cursor movement and 12.9% for scrolling; Kitty used 11.6% and 13.7%, respectively. The guide reference window was moved and later covered during its run, so that run is excluded from the before/after comparison. Its early high-GPU observation was used to investigate the path pipeline, not to claim a reliable improvement percentage.

Full-host checks, with the actual app chrome present:

| Workload | Terminal grid | Host CPU | Host GPU |
| --- | --- | ---: | ---: |
| Workbench Neovim, Snacks Grep | 48 × 56 | 28.8% | 25.4% |
| Workbench Neovim, result-list movement | 48 × 56 | 12.7% | 10.8% |
| Terminal dock, single `seq` plus drain window | 84 × 12 | 0.26 CPU-seconds / 8.26 s | 0.046 GPU-seconds / 8.26 s |
| Terminal dock, subsequent idle | 84 × 12 | 1.0% | 0.0% |

The full Workbench draw p95 was 8.7 ms. The Terminal dock displayed `200000` followed by `ONEHAND_SEQ_DONE`. Its frame counter stopped at 27 after the initial burst (one later app frame brought it to 28); there was no sustained redraw loop. GPU returned to zero in the subsequent idle interval. These full-host checks have smaller grids than the standalone probe and are not direct Kitty comparisons.

\* The reference idle interval includes outstanding GPU work at the phase boundary. The patched and Kitty idle counters stayed flat. All idle CPU numbers include the probe's once-per-second histogram logging.

The remaining gap to Kitty, real IME integration, keyboard input-to-present latency, and prolonged testing with the user's complete Neovim configuration remain open validation items for issue #9. A row damage cache alone would not eliminate full-scene GPU submission. Further work should distinguish residual curved-path passes, glyph submission and host chrome using a GPU trace rather than infer the next bottleneck from CPU shaping alone.

## Reproducing

Build before starting the measurements:

```sh
cargo build --release --locked -p onehand --example terminal_perf --bin onehand
export PERF_PHASE=/tmp/onehand-terminal.phase
export PERF_WORKLOAD=cursor  # cursor, snacks, or seq
export PERF_SNACKS_RTP="$HOME/.local/share/nvim/lazy/snacks.nvim"
export PERF_GREP_ROOT="$PWD"
export PERF_GUIDES=1  # optional: include indent strokes in the cursor workload
export SHELL="$PWD/scripts/terminal-perf-workload.sh"
target/release/examples/terminal_perf
```

Resize the probe and comparison window to the same pixel area and ensure they remain visible on the same monitor. The probe requires Liberation Mono installed; verify the family rather than allowing a silent font fallback. In a second terminal, use the PID of the host being measured:

```sh
python3 scripts/terminal-perf-sample.py PID --seconds 8
```

`PERF_PHASE` contains the current phase and terminal dimensions. Align samples to each phase; do not include startup or compare a visible window against a covered/off-workspace one. The Lua workload deliberately remains open at the end for inspection.

Kitty, with the same workload environment:

```sh
kitty --config NONE -o font_family='Liberation Mono' -o font_size=10.5 \
  -o cursor_blink_interval=0 -o window_padding_width=0 \
  ./scripts/terminal-perf-workload.sh
```

For the full application, set `PERF_APP=terminal` to mount the actual Terminal dock. Use an isolated `XDG_CONFIG_HOME` containing `onehand/config.toml` with `agents = []` and `[font] monospace = "Liberation Mono"`. Set `PERF_APP=neovim` to mount Workbench Neovim; for automated workloads, place an `nvim` wrapper on a temporary PATH that executes `terminal-perf-workload.sh`, and set `PERF_NVIM` to the absolute path of the real Neovim first. Pass a temporary project directory as the probe's positional argument. This avoids touching the user's running application, editor buffers, and configuration.

## Verification

- 100 `gpui-terminal` unit tests passed, including straight-stroke bounds, repeated IME composition events, and parsing 200,000 lines in 4 KiB chunks with a 2,000-line history limit.
- 19 terminal doc tests passed, 28 ignored; shared terminal UI and Neovim plugin tests/doc tests passed.
- The parser regression also reduces history while the alternate screen is active, returns to the primary screen, and verifies the reduced limit and zero-history behavior.
- Release application and probe built successfully; formatting and strict Clippy for the probe passed.
- Inspected the actual Workbench/Snacks and Terminal dock windows, including the final output sentinel. Compared separate light, heavy, double and rounded border samples before/after. Stroke geometry and connectivity were retained; thin straight strokes appear crisper with quad rasterization, so this is not a claim of pixel-identical antialiasing. Rounded corners keep their original path rendering.
