# Controlled terminal profiling — 2026-09-11

Follow-up to [the initial audit](terminal-performance.md) and
[issue #9](https://github.com/jarviisha/onehand/issues/9). The initial Snacks
comparison processed different numbers of window frames, so CPU percentages
alone could not identify the residual cost per update.

## Results and interpretation

Thirteen completed cases are retained in [the measured aggregates](terminal-profiling-results.json),
including repeats, an uninstrumented probe control, and actual clean Neovim.
Every replay comparison with the same variant/rate has matching offered update
counts, byte counts and payload hashes. Replay deadline lateness stayed below
2.4 ms. The first attempted case stopped during warmup because of a script
log-parser error; it produced no accepted measurement.

**Small updates still pay almost the entire terminal render cost.** With straight
borders at 20 Hz, changing one character cost 2.68–2.81 ms per GPUI window draw;
rewriting the complete 100 × 50 area cost 2.66–2.94 ms. The parser cost per
draw rose from about 0.005 ms to 0.114–0.116 ms, while the renderer continued to
walk and submit the visible grid in either case.

| Offered workload | Probe CPU, one core | Kitty CPU, one core |
| --- | ---: | ---: |
| One character, 20 Hz, two runs | 8.66–8.81% | 1.71–1.85% |
| Full area, 20 Hz, two runs | 9.08–9.71% | 7.13–7.30% |
| One character, 40 Hz | 17.78% | 3.75% |
| Full area, 40 Hz | 18.91% | 15.36% |
| Neovim cursor movement, nominal 20 Hz | 10.50% | 2.02% |
| Neovim scrolling, nominal 20 Hz | 10.67% | 1.86% |
| Neovim forced redraw, nominal 20 Hz | 15.23% | 8.60% |

Doubling replay cadence approximately doubled CPU consumption. That is
consistent with a continuing per-frame cost, not a CPU throughput ceiling. The
uninstrumented control used 8.35% CPU for small updates and 9.38% for full
updates; the extra stage counters do not explain the large small-update gap.
This control is the previous release probe with the same production terminal
renderer, rather than a same-revision instrumentation A/B build, so it does not
establish an exact instrumentation overhead percentage.

**The existing shaping cache does not remove glyph submission or row assembly.**
Representative elapsed milliseconds per window draw:

| Stage | Replay one character, 20 Hz | Neovim cursor, 20 Hz |
| --- | ---: | ---: |
| Parse | 0.005 | 0.023 |
| Backgrounds and selection | 0.423 | 0.500 |
| Box-drawing scan/submission | 0.281 | 0.152 |
| Split styled runs | 0.301 | 0.408 |
| Prepare/shape runs, including cache hits | 0.199 | 0.403 |
| Submit glyphs | 0.975 | 1.316 |
| Whole terminal paint, including the above render stages | 2.224 | 2.833 |
| Whole GPUI window draw, including terminal paint | 2.811 | 3.670 |

Glyph submission accounts for about 44% of terminal paint in the replay and
46% in Neovim; shaping accounts for about 9% and 14%, respectively. These are
shares of instrumented **terminal paint elapsed time**, not of total process
CPU. Background/run assembly also remains measurable. A cache aimed only at
`shape_line` misses most of this cost. Row-data caching may now be worth
investigating for CPU savings, with full invalidation coverage; it would still
not retain terminal pixels on the GPU.

**Rounded paths remain expensive, but GPU percentages are highly variable.**
For the 20 Hz replay, two straight-border runs used 1.55–2.78% graphics-engine
time during small updates and 1.78–2.01% during full updates. Two rounded-border
runs used 3.73–10.54% and 3.93–11.14%, respectively, with similar host draw times.
The changed geometry is only eight corner cells. This supports further work on
the residual curved-path pipeline, whose intermediate render passes are visible
in the pinned GPUI backend source. It does not establish a stable sixfold cost:
the second rounded run was much lower than the first.

Kitty also varied: its straight-border runs used 2.10–3.81% and 2.72–4.18% GPU;
its rounded run used 1.92% and 1.84%. Later runs sampled AMD clock values ranging
from 200 to 1,600 MHz, with 400 MHz medians. Those intermittent DPM readings
cannot normalize individual GPU submissions. This simple replay does not show
a universal GPU disadvantage for GPUI, nor does it overturn the original
Snacks result: its workload, cadence and scene are different. No GPU timestamp
query or per-pass hardware trace was captured; source inspection and controlled
border variants provide attribution evidence, not GPU timings for each stage.

**An editor update is not necessarily one painted frame.** Replay small updates
produced about 20 or 40 draws/second as intended. The full 40 Hz replay produced
about 42 draws/second. In Neovim, cursor/scroll phases produced about 20, but
forced `redraw!` produced about 28.4 despite the 20 Hz workload timer. This is
consistent with output arriving in multiple repaint batches, but the trace does
not identify each notification's cause. It warrants inspecting batch/frame
scheduling before treating application update frequency as presentation FPS.

Idle graphics-engine counters stayed flat in all completed cases. The probe's
sampling timer remained active, so idle process CPU is not a production idle
benchmark.

These findings do **not** establish a hard GPUI ceiling. They narrow the next
work to glyph/row submission, curved-path rendering, and excess draws during
multipart updates. They do not justify replacing GPUI, hiding the cost by
lowering the frame rate, or claiming issue #9 is resolved. A retained GPU surface
or a different primitive strategy would need its own correctness and latency
validation; actual key-to-present latency, full user Neovim configuration and
per-pass GPU timestamps remain unmeasured here.

## Instrumentation

`gpui-terminal` has an opt-in `profiling` feature, enabled for the example through
`onehand/terminal-profiling`. Normal builds contain none of these spans or
counters. The probe samples cumulative counters on the foreground thread once
per second without requesting a frame:

| Stage | Scope |
| --- | --- |
| `parse` | Lock the grid and feed each PTY chunk to Alacritty; units are bytes |
| `paint` | Entire terminal renderer, including the stages below |
| `backgrounds` | Scan, merge, and submit row backgrounds and selection |
| `boxes` | Scan and submit box-drawing strokes |
| `runs` | Split cells into styled text runs |
| `shape` | Prepare text/font descriptions and call GPUI `shape_line`, including cache hits |
| `glyphs` | Call `ShapedLine::paint`, including glyph lookup and scene submission |
| `cursor` | Draw the terminal cursor |

These are **elapsed host times**, not on-CPU samples or GPU timestamps. Render
stages nest within `paint`; adding `paint` to its children double-counts them.
GPUI's cumulative `Window::draw` histogram also includes layout and scene
construction, but excludes the later platform `draw`/presentation call. Its
summed duration is reconstructed from histogram mean × count, with histogram
precision. Neither measurement is input-to-screen latency.

The external sampler independently reads process CPU ticks and deduplicated
Linux DRM engine nanoseconds. Child processes are excluded. Whole-phase
aggregates retain boundary work; `steady` excludes one second at each end.
Stage comparisons use only probe snapshots inside that steady interval.

## Controlled workloads

`terminal-perf-replay.py` emits a fixed 100 × 50 cell area, with two panels and
no blinking cursor. It does not read terminal replies or run a search. Its two
eight-second phases are:

- `small`: overwrite one character at an absolute 20 or 40 Hz deadline.
- `full`: rewrite the complete area and that character at the same deadline.

The text, straight-border and rounded-border variants change only the panel
border characters. Rounded panels contain eight curved corners in total.
The `.events.json` records update counts, bytes, SHA-256 of offered payloads,
elapsed time and worst scheduling lateness. Equal payload hashes establish
equal offered work for the same variant; they do **not** establish presentation
of every intermediate update. GPUI draw counts provide a separate check on
frame coalescing. No equivalent Kitty presentation counter is collected.

The Neovim cursor/scroll/redraw workload also accepts `PERF_INTERVAL_MS` and
`PERF_TICKS`; defaults preserve the first audit's 16 ms / 500 update workload.
The runner selects 50 ms / 160 updates or 25 ms / 320 updates, so each phase has
the same nominal duration. This checks whether replay findings also appear in
an actual editor.

The Hyprland runner opens only its own window, places it at 960 × 1,000 pixels
on the laptop monitor, checks geometry during the run, and restores focus when
finished. All comparisons must run sequentially after compilation ends.
Dynamic GPU clocks, desktop activity, compositor occlusion and different cell
width rounding remain limitations. The fixed replay content fits both grids;
the full terminal grids themselves are not identical.

## Reproduction

```sh
cargo build --release --locked -p onehand --example terminal_perf --features terminal-profiling
python3 scripts/terminal-perf-run.py probe-straight-20 --hz 20
python3 scripts/terminal-perf-run.py kitty-straight-20 --host kitty --hz 20
python3 scripts/terminal-perf-run.py probe-rounded-20 --hz 20 --border rounded
python3 scripts/terminal-perf-run.py probe-straight-40 --hz 40
python3 scripts/terminal-perf-run.py probe-nvim-20 --workload cursor --hz 20
python3 scripts/terminal-perf-run.py kitty-nvim-20 --host kitty --workload cursor --hz 20
```

Each case needs a fresh name. Aggregates, raw CPU/DRM samples, geometry checks,
probe stage snapshots and application logs are retained under
`/tmp/onehand-terminal-profiling` by default. The runner needs access to the
desktop session and `/proc/PID/fdinfo`; it cannot measure native GPU behavior
inside a headless sandbox. The underlying workload and sampler can also be run
manually on other compositors using the first audit's instructions.
