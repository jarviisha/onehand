# Drain redraw tails without a parser sleep

This is step 1 of the follow-up work in [PR #29](https://github.com/jarviisha/onehand/pull/29),
after [controlled stage profiling](terminal-profiling.md) found more GPUI draws
than the Neovim workload's nominal update rate. Curved-path rendering and row
data caching are separate work and are not changed here.

## Cause and change

The PTY consumer used to notify after a batch, then sleep for 8 ms before
parsing more output. That paused the parser, not just the repaint requests.
Neovim can write one redraw across several PTY reads. A first part was painted
while the parser slept; the trailing bytes were processed afterwards and
requested another frame.

A before-change diagnostic trace contains this sequence (times are relative
to the first recorded event):

| Time, ms | Event |
| ---: | --- |
| 20034.419 | Parse a 9,420-byte batch and request a repaint |
| 20037.620 | Render the terminal |
| 20043.758 | Parse another 24 bytes and request a repaint |
| 20054.027 | Render the terminal again |

The consumer now drains already queued chunks, parses a bounded batch, requests
a repaint through the existing gate, and yields the foreground executor. It
does not wait on a timer before processing the next batch. This lets trailing
output join the pending frame without adding a delay to the first input echo.

The existing limits remain: 256 queued 4 KiB chunks, at most 64 chunks per
foreground update, then an executor yield. The gate still permits only one
pending output notification until the terminal is rendered. A hidden terminal
does not continually request frames, and an idle terminal waits on the channel.
There is no new polling timer or lower FPS cap.

## Measurements

[Aggregates and binary fingerprints](terminal-coalescing-results.json) retain
12 accepted cases: four diagnostic traces and eight release runs. Two other
echo runs had window movement or visibility changes and are excluded entirely,
including their latency samples. All accepted windows stayed on `eDP-1` at
960 × 1,000 pixels. No compilation ran during accepted measurements.

The diagnostic Neovim run had 63 instances of a large batch being rendered
before a small tail arrived at the parser within 25 ms. The after-change trace
had zero such instances. That trace's redraw phase changed from 23.6 to
19.8 draws/second at a nominal 20 Hz editor cadence.

Release results, with tracing disabled:

| Neovim forced redraw | Before | After |
| --- | ---: | ---: |
| 20 Hz: GPUI draws/second, two runs | 24.46–24.68 | 19.67–19.98 |
| 20 Hz: host CPU, one core | 12.00–12.34% | 8.52–10.38% |
| 20 Hz: graphics-engine utilization | 2.49–2.66% | 1.95–1.96% |
| 40 Hz: GPUI draws/second | 49.11 | 39.95 |
| 40 Hz: host CPU, one core | 25.24% | 18.52% |
| 40 Hz: graphics-engine utilization | 4.42% | 4.04% |

The 40 Hz result shows that the change does not cap output to 20 Hz. Cursor and
ordinary scrolling phases continue at their offered 20/40 Hz; their CPU does
not consistently improve, since they were not producing the same surplus
frames. GPU clocks remain dynamic, and the smaller GPU differences should not
be interpreted as stable savings across machines or workloads.

The isolated `seq 1 200000` checks both processed **1,488,913 PTY bytes**, exactly
the expected CRLF-expanded numbers plus `ONEHAND_SEQ_DONE`. CPU time in the
roughly eight-second burst/drain window was 0.17 seconds before and 0.13 seconds
after; graphics-engine time was 0.0078 and 0.0040 seconds. In both builds, frame
counts stayed flat across all five idle samples and idle GPU deltas were zero.
This checks complete byte processing and draining; the parser unit test checks
the retained final lines and scrollback contents.

The diagnostic echo tests matched all **320/320 inputs** to response
paint/submission events in each build:

| App dispatch to GPUI submission | Before p95 | After p95 |
| --- | ---: | ---: |
| Small echo | 17.93 ms | 17.40 ms |
| Echo with a dense redraw | 25.26 ms | 19.65 ms |

For dense redraw, dispatch-to-final-response-parse p95 fell from 8.87 to
0.56 ms. This directly checks the removed parser wait. The test found no
echo-latency regression, but it is not a physical keyboard-to-screen test and
does not establish an input-latency guarantee under arbitrary sustained output.

This addresses the measured extra frames from the post-notify sleep. It does
not make every application write exactly one frame: output arriving after a
frame was actually rendered can legitimately need another one. The remaining
glyph submission, row assembly and curved-path costs from the prior report
remain open; issue #9 is not closed by this step.

## Diagnostics and latency method

The opt-in profiling build can additionally record a bounded, payload-free
trace with `ONEHAND_TERMINAL_TRACE=1` (runner option `--trace`). It records batch
sizes, notification decisions, render/paint boundaries and input byte counts.
The example also collects GPUI's frame-submission events on the same monotonic
time axis. No PTY contents or actual key values are logged. Old entries are
evicted above 4,096 events between sampler drains, so this is a short diagnostic
trace, not a lossless event recorder. Normal builds compile out this machinery.

`terminal-perf-echo.py` waits for simulated GPUI keystrokes and acknowledges each
through the real PTY with a numbered update. The second phase also writes a
dense screen. The reply number is at the end of the output, so the analyzer
pairs input with the **last** response batch before the next input, then its
next paint and GPUI submission. An earlier partial paint is not counted as an
acknowledgment. Inputs without a matching paint/submission are reported, not
silently converted to zero latency.

This measures app-dispatch-to-submission latency. It excludes physical keyboard
and OS dispatch time, compositor scheduling, scanout, and actual visibility of
individual pixels. The generic GPUI input-latency histogram is insufficient
here: it only retains input events that synchronously invalidated the window,
whereas normal terminal typing waits for asynchronous PTY echo to invalidate it.

The diagnostic before/after runs use the same repository development profile
(workspace opt-level 1, dependencies opt-level 3). CPU/GPU comparisons use
separate release builds and do not enable event tracing. Do not mix those build
profiles when interpreting timings.

## Reproduction

```sh
cargo build --locked -p onehand --example terminal_perf --features terminal-profiling
python3 scripts/terminal-perf-run.py trace-nvim --workload cursor --hz 20 --trace \
  --binary target/debug/examples/terminal_perf
python3 scripts/terminal-perf-run.py trace-echo --workload echo --hz 20 --trace \
  --binary target/debug/examples/terminal_perf
python3 scripts/terminal-perf-trace.py /tmp/onehand-terminal-profiling/trace-echo.log --echo

cargo build --release --locked -p onehand --example terminal_perf --features terminal-profiling
python3 scripts/terminal-perf-run.py release-nvim --workload cursor --hz 20
python3 scripts/terminal-perf-run.py release-seq --workload seq
```

Use `--monitor NAME` to select the Hyprland monitor; the default is `eDP-1`.
Before/after cases must stay visible at the same size and on the same monitor,
and compilation must finish before measurement begins. Runs whose window moved
or became hidden are excluded. The runner controls and closes only its own
window, restoring the previous focus after completion.

## Verification

- All workspace tests passed, including 101 terminal unit tests and 19 terminal
  doc tests (28 ignored).
- New checks cover a queued redraw tail being parsed before requesting the
  frame, lossless draining across the parse-budget boundary, and an executor
  yield that reschedules without a timer. Existing hidden-grid and repaint-gate
  tests remain, as does the 200,000-line parser/scrollback regression.
- Strict first-party Clippy, strict profiling-example Clippy, formatting and
  script syntax checks passed.
- A synthetic trace fixture verifies that a partial paint before the final
  response batch is not counted as an acknowledgment and that an input without
  a submission is reported as unmatched.
