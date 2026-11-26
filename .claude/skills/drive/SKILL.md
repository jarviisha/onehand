---
name: drive
description: Launch onehand in a headless sway session of its own and drive it (click, type, resize, switch palette, screenshot) to picture a visible change before calling it finished, or to test end to end when the owner asks.
---

# Drive

Run the real app on a Wayland compositor of its own: one headless output of 1600x1000, a data
root under `/tmp`, and a pointer and keyboard the agent controls. The person's own onehand and
desktop are never touched. Use it to picture the view a visible change touched before calling
the change finished (the `ui` skill says how, and who reviews the pictures), or when the owner
asks you to test in the app.

The scripts are in `scripts/` beside this file; run them from anywhere. `DRIVE_ROOT` (default
`/tmp/onehand-drive`) holds everything: logs, pids, screenshots, the data root and the project.

## Steps

1. **Build** what is to be tested: `cargo build` (the app on the branch's code). Done when it
   builds. A `--release` binary works too; pass its path to `up`.
   - Where `cargo build` is routed to a build host, it brings no binary back. Build through the
     project's build host instead: `<onehand> --build <host> -- cargo build -p onehand -p
     onehand-host`. All of the build host's artifacts are built or none come back, and they
     land under its `into` (`target/from-<host>/target/debug/onehand`); pass that path to `up`.
   - `<onehand>` is the binary of the running onehand, which is not on `PATH`: the `cargo` shim
     in `route-shims/` under the config directory names it on its `exec` line.
2. **Start**: `scripts/session.sh up [BIN]`. It does five things:
   - takes the binary: a BIN given to `up`, `restart` or `palette` is kept for the commands
     after it until `down` forgets it; with none given and none kept, the repo's
     `target/debug/onehand`. A missing one is said by its path before anything starts;
   - starts at 1600x1000;
   - seeds the data root on the first run: a git project `proj` whose check command is `true`,
     and two agents, *Mock Work* and *Mock UI*;
   - builds the virtual pointer;
   - starts sway;
   - holds a virtual keyboard for the whole session, so the window counts as active, made
     afresh on every `up`, since an app started on a held one can panic;
   - starts the app and waits for its window.

   Done when it returns 0. A non-zero exit prints the app's or sway's log tail. `restart` and
   `palette` refuse when the session's sway is not running.
3. **Drive**, one action at a time, and look after each:
   - `scripts/drive.sh shot NAME` writes `$DRIVE_ROOT/NAME.png`. **Read the screenshot** before
     the next action, and take every coordinate from what is on it.
   - `click X Y`, `move X Y` and `scroll X Y N` use output pixels. The pointer is gone once each
     returns, so nothing stays hovered.
   - `hover X Y NAME [click]` rests the pointer there, pressing first with `click`, and writes the
     screenshot while it rests: for a hover state or a tooltip, or one that should not show after
     a press.
   - `type TEXT` types into the field that has focus; click the field first.
   - `key -k Escape` passes `wtype` arguments.
   - `drive.sh alive` after any action that could crash. If the app is gone it prints the panic
     and backtrace and exits 1.
   - `drive.sh size W [H]` sets the window's size in output pixels (H defaults to 1000), live.
     The narrowest the window goes is 720 wide (`WINDOW_MIN`, `crates/app/src/shell/boot/open.rs`);
     a narrower width or a zero height is refused.
   - `session.sh palette light|dark` draws the app light or dark. The choice is read at boot, so
     the app starts again, its data kept and its view lost.
   - `drive.sh matrix NAME [STEPS]` pictures one view in both palettes and at each width of
     `$DRIVE_WIDTHS` (default `720 1600`), narrowest last: `NAME-<width>-<palette>.png`. After
     each palette it reaches the view again by running STEPS at 1600 wide, then narrows the
     window. It runs light, then dark, and leaves the app dark at 1600 wide. STEPS is a bash
     file of `$DRIVE …` lines, keys before clicks, its coordinates read off a 1600-wide
     screenshot. A step that fails stops the matrix, saying its line, and so does the app
     dying, with its log's tail:

     ```bash
     $DRIVE key -M ctrl -k comma -m ctrl   # Settings
     sleep 1
     $DRIVE click 413 242                  # its Agents section
     sleep 1
     ```

   Done when every behaviour the change touches has been seen working on a screenshot, or a
   failure has been captured with its log.
4. **Start over** without restarting sway: `scripts/session.sh restart`. It wipes tasks and
   conversations, resets the project (its worktrees, branches and step marks included) and the
   workspace to `proj` alone, makes the held keyboard afresh, and starts the app again.
5. **Stop**: `scripts/session.sh down`. Done when no sway, held keyboard or debug onehand of
   this session is left (`pgrep -x sway` finds nothing).
6. **Report** what was driven, each result with the screenshot that shows it, and anything
   found. A crash found while driving is a finding whether or not the change under test caused
   it; say which.

## The agents

- **Mock Work** (`scripts/mock_work_agent.js`) is for workflow runs, and is the one to pick in a
  launcher.
  - Each prompt streams for about four seconds, which leaves time to press Stop mid-turn.
  - An implement step's prompt (*This step is the change*) appends to `README.md`, so the
    *code changed* gates pass, and commits it when the prompt asks for a commit (*commit your
    work*, a branch run), so the *committed* gate passes too.
  - A plan step's prompt changes nothing.
  - It parks no cards and returns no errors, so a run goes from its first step to its last.
- **Mock UI** (`crates/core/examples/mock_ui_agent.js`) is for looking at the chrome.
  - Every prompt parks a permission card and a question card.
  - Every other turn ends on an error.
  - So a workflow run on it parks or fails, by design.

## Gotchas

- **Stop the app by its pid file and nothing else.** The person's own onehand runs under the
  same process name, so `pkill onehand` or `pkill -f` on a path would end it. `session.sh`
  only ever kills the pids it wrote.
- **SIGTERM may not end the app.** `session.sh` waits, then sends SIGKILL to that pid. If the
  log says *onehand is already running on …*, the previous instance still holds the data root's
  lock: run `session.sh restart` again.
- **There is no pointer without `vpclick`.** The headless seat has no pointer device, so
  `swaymsg seat … cursor` moves nothing a client sees. The C tool creates a wlr virtual pointer
  instead.
- **A window that is never active stops toasts hiding.** GPUI on Wayland counts a window
  active only while it holds keyboard focus, and the headless seat's only keyboard is the one
  `session.sh` holds. If toasts pile up, or the first key of a `wtype` is lost, check that
  `keyboard.pid` is alive; `session.sh restart` starts it again. Read the screenshot when the
  exact text matters.
- **Seconds pass between tool calls.** Reading a screenshot takes time the app keeps running in,
  so an action that has to land within a window (Stop on a short command, a press mid-turn) goes
  in the same shell command as the one that started it, with `sleep` between.
- **Layouts move.** A dialog grows a line (a warning, a hint) and its buttons shift down, so
  take coordinates from the latest screenshot, not from an earlier run.
- **A palette switch loses the view.** The app starts again on it, so a view is reached anew by
  the steps file, never assumed to survive; `matrix` does this for you.
- `MESA-EGL: warning … failed to get driver name` in `app.log` is the pixman renderer's noise.
