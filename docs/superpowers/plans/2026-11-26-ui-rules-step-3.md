run: 2026-11-26-ui

# UI rules, step 3: the reviewer. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the author of a visible change drives the view it changed in the headless session, at
two widths and in both palettes, and hands the pictures and the diff to a reviewer that did not
write the change; the rule on verifying says so everywhere it is stated.

**Architecture:** the drive skill's two shell scripts gain a window width (the headless output's
mode, changed live through `swaymsg`), a palette (`appearance` in the session's own
`config.toml`, read at boot, so a palette change restarts the app on its data) and a matrix
command that replays a steps file and takes the four pictures. The reviewer is a section of the
`ui` skill, run as a fresh subagent. AGENTS.md, the drive skill and `docs/rules-and-gotchas.md`
change the rule on verifying. No Rust changes.

**Tech stack:** bash, sway (headless, pixman), `swaymsg`, `grim`, `wtype`, the existing
`vpclick`; Markdown skills.

**Spec:** `/home/jarviisha/development/rust-projects/onehand-ui-rules-lead/spec.md`, sections
*The reviewer*, *Who checks a rule*, *What happens to the documents*, *Skills*, step 3. Not
committed by this job (job a commits it).

## Global constraints

- Code never cites a document: no spec, document or rule id in a script's comments or output.
- No rule ids exist yet. The reviewer's brief names a rule by its document and section today
  (`DESIGN.md`, *Colour and state*) and by id once ids exist, in the same sentence.
- No rule text in `DESIGN.md` or `docs/ui-patterns.md`; no `clippy.toml`, guards or census; no
  tests in `crates/`.
- `checklist.md` stays (step 5 removes it).
- Shared files: in AGENTS.md only the verify rule (lines 223-225) and the `drive` row (line 78)
  change; in `docs/rules-and-gotchas.md` only the verify line (86-87). No reflow of anything
  else.
- The scripts never touch the owner's onehand or its data root: everything stays under
  `$DRIVE_ROOT` (default `/tmp/onehand-drive`), processes stopped by their pid files only.
- Every `.md` in English. Commits `type(scope): summary`, plain body, trailer
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review focus

1. **A palette switch mid-session loses the view.** The config is read at boot, so `palette`
   restarts the app; the matrix therefore replays the steps file after each palette switch rather
   than assuming the view survives. Pinned by Task 2's check (the matrix's four pictures show the
   same page, not the start page, for a view reached by a click).
2. **Coordinates in a steps file are taken at one width.** The matrix runs the steps at the wide
   width and only then narrows the output, so a click recorded on a 1600-wide screenshot lands.
   Pinned by Task 2's check (a steps file with a click).
3. **`restart` and `palette` need the binary again.** Today `restart` falls back to
   `target/debug/onehand`, which on a routed machine does not exist; `up` records the binary it
   was given in `$DRIVE_ROOT/bin` and later commands reuse it. Pinned by Task 1's check
   (`restart` with no argument after `up <path>`).
4. **A palette written twice.** `palette dark` then `palette light` leaves exactly one
   `appearance` line. Pinned by Task 1's check (`grep -c '^appearance' == 1`).
5. **The reviewer marks its own homework.** The brief forbids the author answering as the
   reviewer; where no second agent can be started, the change says it was not reviewed instead.
   Pinned by Task 3's walk-through.

---

### Task 1: width and palette in the drive scripts

**Files:**
- Modify: `.claude/skills/drive/scripts/session.sh`
- Modify: `.claude/skills/drive/scripts/drive.sh`

**Interfaces (produces):**
- `session.sh up [BIN]`: as today, and writes the absolute BIN (or the default) to
  `$DRIVE_ROOT/bin`; resets the output to the wide default (1600x1000).
- `session.sh restart [BIN]`, `session.sh palette light|dark [BIN]`: BIN defaults to the content
  of `$DRIVE_ROOT/bin`, then to `target/debug/onehand`.
- `session.sh palette light|dark`: replaces or inserts the single top-level line
  `appearance = "<light|dark>"` at the head of `$DRIVE_ROOT/home/config.toml`, then stops the
  app, makes the held keyboard afresh and starts the app again. Tasks, conversations and the
  project are kept (unlike `restart`). Refuses any other word with the usage, exit 2.
- `drive.sh size W [H]`: `swaymsg output HEADLESS-1 mode WxH` on the session's own sway socket
  (`$run/sway-ipc.<uid>.<sway pid>.sock`), H defaulting to 1000, then waits 1 s for the window
  to settle. Refuses a non-number, exit 2.
- The sway socket path is computed once, in each script, the way `start_app` does today.

- [ ] **Step 1: Implement** the above. `palette` writes the file with `sed` on a temp file beside
  it and `mv` (a half-written config would boot with defaults).
- [ ] **Step 2: Check** with `shellcheck .claude/skills/drive/scripts/*.sh`. Expected: no new
  warnings against the base (run it on the base first and keep the output).
- [ ] **Step 3: Check in the session** on the binary from the build host (see *Probe*):
  `session.sh up <bin>`; `drive.sh size 720`; `drive.sh shot a` is 720x1000 (`file`);
  `session.sh palette light`; `session.sh palette dark`; `grep -c '^appearance'
  $DRIVE_ROOT/home/config.toml` is 1; `session.sh restart` (no BIN) returns 0;
  `drive.sh alive` says alive; `session.sh down`; `pgrep -x sway` finds nothing.
- [ ] **Step 4: Commit** `feat(drive): the headless session takes a window width and a palette`.

### Task 2: the matrix: one view, two widths, two palettes

**Files:**
- Modify: `.claude/skills/drive/scripts/drive.sh`
- Modify: `.claude/skills/drive/SKILL.md`

**Interfaces:**
- Consumes: Task 1's `size`, `palette`, `$DRIVE_ROOT/bin`.
- Produces: `drive.sh matrix NAME [STEPS]`. For each palette in `light dark`: `session.sh
  palette P`; `size` to the wide width; if STEPS is given, run it with `bash STEPS` (a file of
  `drive.sh` lines, keys preferred over clicks, coordinates taken from a 1600-wide screenshot);
  then for each width, narrowest last: `size W`, `shot NAME-W-P`. Widths come from
  `$DRIVE_WIDTHS`, default `720 1600` (720 is the window's minimum width, `WINDOW_MIN` in
  `crates/app/src/shell/boot/open.rs`). Prints the four paths. Ends with the output back at
  1600 and `alive` checked; a dead app stops the matrix with its log tail, exit 1.
- The drive skill's `SKILL.md` documents `size`, `palette`, `matrix` and a steps file under
  *Drive*, and step 1 *Build* says how a binary comes back where `cargo build` is routed to a
  build host: `<onehand> --build <host> -- cargo build -p onehand -p onehand-host`, landing under
  the project's build host's `into` (`target/from-<host>/target/debug/onehand`); all of the
  build host's artifacts are built or none come back. `<onehand>` is the binary of the running
  onehand, which is not on `PATH`: the `cargo` shim under the config directory's `route-shims/`
  names it in its `exec` line. No path from one machine is written in the skill. Only the steps and the gotchas change in
  this task; the opening and frontmatter are Task 4's.
- A gotcha line: a palette switch restarts the app, so a view is reached again by the steps
  file, never assumed to survive.

- [ ] **Step 1: Implement** `matrix` and the skill text.
- [ ] **Step 2: Check** `shellcheck` as in Task 1.
- [ ] **Step 3: Check in the session** with a steps file that reaches a view by a key and a
  click (open Settings with `Ctrl+,`; then click a section in its list): `drive.sh matrix
  settings steps.sh` writes four PNGs, two 720 and two 1600 wide; reading them shows the same
  Settings section in each, light and dark. Copy them to
  `/home/jarviisha/development/rust-projects/onehand-ui-rules-lead/c/artifacts/build/` for the
  lead. `session.sh down`.
- [ ] **Step 4: Commit** `feat(drive): a matrix pictures one view at two widths in both palettes`.

### Task 3: the reviewer, a section of the `ui` skill

**Files:**
- Modify: `.claude/skills/ui/SKILL.md` (a new section *The reviewer* before *Finishing*;
  *Finishing*'s implementation path)

**What the section says:**
- **When.** Every change a person can see, before it is called finished. Skipped, and the skip
  said in the report and the PR body in one line (*Not pictured: words only* / *Not pictured:
  nothing a screenshot shows*), for a change to words alone or one no screenshot can show (a key
  binding, focus order, a refusal's tooltip text). A contract-only change takes the contract
  path and no reviewer.
- **The author first.** Builds the app, drives the changed view with the drive skill's
  `matrix` (both palettes; the narrowest width the view's region allows and a wide one, a dock
  dragged to its minimum by the steps file where the window's minimum is not the region's), and
  pictures the states the change touches (a long name, an empty list, a hover) with extra steps
  files, reading each picture itself first.
- **The reviewer is fresh.** An agent with no part in the change, started for this alone: a
  subagent, or a second session where the host has no subagents (in Claude Code, for example,
  the Agent tool with `general-purpose`). Never the author answering for it. Where no second
  agent can be started, the PR says *Not reviewed: no second agent* and the owner's look stands
  alone.
- **Its brief** (a block to paste, filled in): the diff (`git diff <base>...HEAD -- crates
  .claude docs DESIGN.md`), the picture paths with the width, palette and state of each, and the
  rules the change touches, each named by its document and section today and by its id once
  rules carry ids, with the instruction to read them. It is told the author's intent in one line
  and nothing of the author's own judgment.
- **Its answer:** one line per rule given: *holds*, *broken* (the picture and where on it, the
  file and line in the diff) or *cannot see* (why: a hover not pictured, a zoom, real text the
  view did not hold); then *What I could not see*, the states and widths no picture shows; then
  anything else a picture shows wrong that no given rule names.
- **Its findings** are fixed in the change, the affected pictures taken again and given to the
  same reviewer, or listed in the PR body as open. The PR body says in words what was pictured
  (the view, the widths, the palettes, the states) and what the reviewer found, fixed or left
  open; it gives no path, since a picture under `/tmp` means nothing to a later reader. Nothing
  is dropped silently.

**Finishing while `checklist.md` still exists** (until step 5 removes it): the implementation
path becomes 1. answer the checklist, as today; 2. run the checks, as today; 3. *picture and
review* (the new section), whose answer replaces the author's own word on the items a picture
can show, checklist items 5, 8, 9 and 17, which then cite the reviewer's lines instead of a
self-judgment; 4. carry the documents, as today; 5. report: the old "claim no visual
verification" becomes "report what was pictured and what the reviewer found; the owner's look
in `make dev` or `make showcase` stays, for taste, and is handed over as before". `checklist.md`
itself is not edited.

- [ ] **Step 1: Write** the section and the *Finishing* change.
- [ ] **Step 2: Check:** frontmatter (`name: ui`, `description` set) and `git diff --check`;
  `grep -n "claim no visual verification" .claude/skills/ui/SKILL.md` finds nothing; every
  relative link in the file resolves (`grep -o '](\.[^)]*)'`, `test -e` on each).
- [ ] **Step 3: Walk-through** of three cases through the text, written in the commit body: a
  label renamed (skips, says so); a new row in the rail (pictured at 720 and 1600, reviewed); a
  machine with no subagent (says not reviewed).
- [ ] **Step 4: Commit** `docs(ui): a visible change ends with a review its author does not write`.

### Task 4: the rule on verifying, everywhere it is stated

**Files:**
- Modify: `AGENTS.md:78` (the `drive` row), `AGENTS.md:223-225` (the rule)
- Modify: `.claude/skills/drive/SKILL.md` (frontmatter `description`; drop
  `disable-model-invocation: true`; the opening paragraph)
- Modify: `docs/rules-and-gotchas.md:86-87`

**The wording:**
- AGENTS.md rule: **Drive what you changed, never the owner's app.** The agent does not open the
  owner's app or desktop, and does drive what it changed in the headless session (`/drive`,
  [.claude/skills/drive/](.claude/skills/drive/)) before it says a visible change is finished;
  the `ui` skill says how, and who reviews the pictures.
- AGENTS.md row `drive`: Before a visible change is called finished, to picture the view it
  changed; or when the owner asks to test in the app. It runs onehand in a headless sway session
  of its own and drives it end to end. Unrelated to a *drive*, the glossary's term.
- Drive skill `description`: Launch onehand in a headless sway session of its own and drive it
  (click, type, resize, switch palette, screenshot): to picture a visible change before calling
  it finished, or to test end to end when the owner asks. The opening drops "Use it only when
  the owner asks... the user looks" for the same two uses and "The person's own onehand and
  desktop are never touched" stays.
- rules-and-gotchas line: **Drive what you changed, never the owner's app.** Opening the owner's
  running onehand or desktop could change their data or interrupt their work; the headless
  session has a data root of its own under `/tmp`, so the agent builds, runs the checks, then
  drives the changed view there and has the pictures reviewed before it says the change is
  finished. The owner still looks, for taste.

- [ ] **Step 1: Edit** the three files, touching only the lines named.
- [ ] **Step 2: Check:** `grep -rn "self-verify\|user looks\|only when the owner asks"
  --include='*.md' . | grep -v 'docs/superpowers\|localDocs\|CHANGELOG'` finds nothing;
  `grep -n disable-model-invocation .claude/skills/drive/SKILL.md` finds nothing;
  `git diff --stat` shows AGENTS.md and rules-and-gotchas.md with only the hunks named.
- [ ] **Step 3: Commit** `docs(agents): the agent drives what it changed before calling it finished`.

### Task 5: close the build

- [ ] Run `/home/jarviisha/development/rust-projects/onehand-ui-rules-lead/check.sh` on the
  committed HEAD (no Rust changed, so it holds the base's result unless the base moved).
- [ ] Push `feat/ui-reviewer` (`git push -u origin feat/ui-reviewer`, never force). No PR until a
  decision says so. The PR body will list the reviewer's own first use as a run of the new
  procedure on nothing visible: this change skips it (*Not pictured: nothing a screenshot
  shows*).

The build's first commit copies this plan, as approved and unchanged, to
`docs/superpowers/plans/2026-11-26-ui-rules-step-3.md`.

## Builder's calls

- **Width through the output, not the window.** The window is tiled and fills the headless
  output, so the output's mode is the window's size; `swaymsg output HEADLESS-1 mode WxH` takes
  effect live, no restart (probe). A floating window with a set size was not needed.
- **Narrow = 720 by default**, the window's minimum (`WINDOW_MIN`). A view in a dock whose
  region's minimum is narrower than what a 720 window gives it is reached by dragging the dock in
  the steps file; the reviewer section says so. `$DRIVE_WIDTHS` overrides.
- **Palette through the config, with a restart.** `appearance` is read once at boot
  (`crates/app/src/shell/boot.rs`, `AppConfig::load_resolved`); the window's appearance observer
  only follows the desktop when the choice is *System*, and the headless session has no desktop
  setting to flip. Driving the Settings page's appearance control instead would need
  coordinates. With no `appearance` the headless app draws dark (probe).
- **No zoom axis** in the matrix: the spec's reviewer asks for two widths and two palettes; zoom
  is the strain tests' (step 2). A steps file can press the zoom keys when a change needs it.
- **The reviewer is a section, not a skill or an agent definition** (spec, *Skills*); no
  `.claude/agents/` file.
- **Pictures are not committed.** They stay under `$DRIVE_ROOT`; the PR body says in words what
  was pictured (the view, the widths, the palettes, the states) and what the reviewer found,
  fixed or left open, with no path.
- **`checklist.md` untouched;** *Finishing* says which items the reviewer's lines answer.

## Probe (done in turn 1)

- **Binary:** `cargo build` here is routed to 100-52 and brings nothing back. The project's
  build host `100-52` names two artifacts, all or none:
  `/data/onehand-target/release/onehand --build 100-52 -- cargo build -p onehand --locked`
  failed with *target/debug/onehand-host was not built on 100-52*;
  `... cargo build -p onehand -p onehand-host --locked` built in 1m56s and said *brought back
  target/from-100-52/target/debug/onehand, target/from-100-52/target/debug/onehand-host*
  (ignored by git; the worktree stays clean). No local build.
- **Session:** `session.sh up <worktree>/target/from-100-52/target/debug/onehand` returned 0,
  `alive`.
- **Width:** `swaymsg -s <session socket> output HEADLESS-1 mode 720x1000` answered success; the
  output reported 720x1000 and `grim` wrote a 720x1000 picture with the app laid out at that
  width (rail beside the project page). Back to 1600x1000 the same way.
- **Palette:** `appearance = "light"` added at the head of `$DRIVE_ROOT/home/config.toml`, then
  `session.sh restart <bin>`: the app came up light. Without the line it is dark.
- **Pictures** (the project page of `proj`), under `c/artifacts/probe/` (PNGs, so they cannot
  carry the run line):
  - `project-1600-dark.png` 99091c726d44f944bab627706ad4d359bcc1e3d3f7d76f3456ccfc63d23b9591
  - `project-720-dark.png` 377d2db6ed10d8311e5d59b1803cc51aa3f8090b6c0e19945f95217fd80cdbd7
  - `project-1600-light.png` b2d3bb5cfcc3c7c3e07de6ebb89038806ed027e4073be7a0c039654f84acfc70
  - `project-720-light.png` 2046f9d65cab976f2d8a1688a44f8d9a3e7aca4b8b311c1e7d9b4f8279d4361e
- `session.sh down` left no sway (`pgrep -x sway` empty). The app log held nothing but the
  renderer's MESA noise.
