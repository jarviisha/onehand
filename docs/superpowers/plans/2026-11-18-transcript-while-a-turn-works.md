# The Transcript While a Turn Works: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While a turn works, the transcript says each fact once, in the tense that is true, and marks what is alive by a shimmer over its words instead of a spinner.

**Architecture:** Core decides the words: a step's verb for its status, a cluster's two sentences (what runs, what has settled), a thought's label, a path's short form. The app draws them: one `working_text` builder for the shimmer, and every place that drew a spinner keeps its mark and shimmers its words.

**Tech Stack:** Rust, GPUI (`gpui-pre`, pinned by the workspace), gpui-component. No new dependency.

**Spec:** `docs/superpowers/specs/2026-11-18-transcript-working-state-design.md`, section 1 only. Sections 2 to 4 are later PRs and are out of scope here. The mock-up `localDocs/2026-11-18-transcript-prototype/index.html` (states 0 to 3) shows the target; where they disagree the spec is right.

## Global Constraints

- Read `AGENTS.md`, then `DESIGN.md` *Transcript*, `.claude/skills/ui/SKILL.md` and its `checklist.md` before a UI task; answer the checklist before the last task ends.
- Rules and wording live in core; the app matches on core's enums, never on a label core shows.
- No comment, doc comment or string names a document. No other product is named anywhere.
- Never hard-code a colour, radius or size: `cx.theme()`, `crate::theme::meta_ink`, rems.
- The shimmer carries no accent: base in the middle ink (`crate::theme::meta_ink(cx)`), band in `cx.theme().foreground`. One pass takes about two seconds, then rests; total period 2.2s.
- No rotating spinner remains under `crates/app/src/chat/transcript/` or in the running line (`crates/app/src/chat/pane/runs.rs`). Spinners elsewhere stay.
- A step's own clock shows at its row's end once it has run ten seconds.
- A file passes about 800 lines of code: split it in this PR along a seam.
- Checks: `cargo test -p onehand-core --locked`, `cargo test -p onehand --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`. Never bare `cargo fmt`.
- Do not launch or screenshot the app. Build, test, stop.
- Commits on `style/transcript-working-state`: `type(scope): summary`, lowercase after the colon.

## Review Focus

- An open cluster whose only step is still running has nothing settled to say: its line reads *Working* in the line's own ink, never an empty line. (Task 2)
- A step with an empty subject (a tool with no title) still shimmers its verb alone and leaves no double space. (Task 1)
- Text with multi-byte characters (Vietnamese, CJK, an emoji in a command): the band moves by character boundary and never panics on a byte index. (Task 3)
- A line truncated to its box: the band sweeps the drawn part and the row is not laid out again each frame; a long turn with five running rows stays smooth. (Task 3, by hand in `make dev`)
- A step saved before start stamps were kept, or restored from an archive, has no clock: no *0s*, no panic. (Task 1)

---

### Task 1: Core says a step's verb for its status, a thought's label and a path's short form

**Files:**
- Modify: `crates/core/src/chat/activity.rs` (`Presentation`, `classify`, `short_path`)
- Modify: `crates/core/src/chat/model/items.rs` (`ToolItem`, `Thought`)
- Test: the `#[cfg(test)]` module of each

**Interfaces:**
- Produces:
  - `Presentation::verb(&self, status: ToolStatus) -> &'static str`: `"Waiting"` for `Pending`; the present form for `InProgress`; `self.action` otherwise.
  - Present forms, one per past verb `classify` and `execute_presentation` can return: `Inspected`→`Reading`, `Searched`→`Searching`, `Fetched`→`Fetching`, `Created`→`Creating`, `Edited`→`Editing`, `Deleted`→`Deleting`, `Moved`→`Moving`, `Reasoned`→`Thinking`, `Used tool`→`Using tool`, `Ran`→`Running`, `Ran tests`→`Running tests`, and each other verb `execute_presentation` returns→its `-ing` form. Kept beside `action` as a field `doing: &'static str` set where `action` is set, so adding a verb without its present form does not compile.
  - `activity::display_path(root: &Path, value: &str) -> String`: `value` relative to `root` when under it; else with the home directory (`dirs::home_dir()`) replaced by `~`; else unchanged. A value holding whitespace (a command) comes back unchanged.
  - `ToolItem::running_secs(&self) -> Option<u64>`: seconds since `started` while the status is `InProgress`; `None` with no start stamp or any other status.
  - `Thought::label(&self) -> String`: `"Thinking…"` while running; `"Thought"` when `elapsed_secs` is `None` or `Some(0)`; `"Thought for {duration}"` otherwise, with `crate::duration`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_step_reads_in_the_tense_its_status_is() {
    let shown = presentation(&tool(ToolKind::Execute, "cargo check"));
    assert_eq!(shown.verb(ToolStatus::Pending), "Waiting");
    assert_eq!(shown.verb(ToolStatus::InProgress), "Running");
    assert_eq!(shown.verb(ToolStatus::Completed), "Ran");
    assert_eq!(shown.verb(ToolStatus::Failed), "Ran");
    let read = presentation(&tool(ToolKind::Read, "Read src/a.rs"));
    assert_eq!(read.verb(ToolStatus::InProgress), "Reading");
}

#[test]
fn a_path_is_shortened_one_way() {
    let root = Path::new("/work/app");
    assert_eq!(display_path(root, "/work/app/src/a.rs"), "src/a.rs");
    assert_eq!(display_path(root, "/work/app"), "/work/app");
    assert_eq!(display_path(root, "cargo test -p a/b"), "cargo test -p a/b");
    let home = dirs::home_dir().unwrap();
    let outside = home.join("notes/x.md");
    assert_eq!(display_path(root, outside.to_str().unwrap()), "~/notes/x.md");
}

#[test]
fn a_thought_under_a_second_has_no_duration() {
    assert_eq!(thought(None, true).label(), "Thinking…");
    assert_eq!(thought(Some(0), false).label(), "Thought");
    assert_eq!(thought(Some(42), false).label(), "Thought for 42s");
    assert_eq!(thought(None, false).label(), "Thought");
}

#[test]
fn only_a_step_in_progress_has_a_clock() {
    assert_eq!(step(ToolStatus::Pending).running_secs(), None);
    assert_eq!(step(ToolStatus::Completed).running_secs(), None);
    assert_eq!(unstamped(ToolStatus::InProgress).running_secs(), None);
}
```

Build `tool`, `thought`, `step`, `unstamped` from the helpers the two test modules already have.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p onehand-core --locked tense_its_status shortened_one_way under_a_second in_progress_has_a_clock`
Expected: compile errors for the four missing items.

- [ ] **Step 3: Implement the five items above.** `display_path` replaces the body the app keeps as `path_for_display`; nothing here is `pub` beyond what the app names.

- [ ] **Step 4: Run** `cargo test -p onehand-core --locked`. Expected: PASS.

- [ ] **Step 5: Commit** `refactor(chat): let core say a step's verb for its status, a thought's label and a path's short form`

### Task 2: A cluster's two sentences

**Files:**
- Modify: `crates/core/src/chat/steps.rs` (`ClusterSummary`, `running_part`, `cluster_summary`)
- Modify: every caller of `cluster_summary` (the compiler lists them; `crates/app/src/chat/viewport.rs` is one)
- Test: `crates/core/src/chat/steps/tests.rs`

**Interfaces:**
- Consumes: `Presentation::verb`, `activity::display_path` (Task 1).
- Produces:
  - `cluster_summary(members: &[&ChatItem], root: &Path) -> ClusterSummary`: the running part's verb comes from `Presentation::verb` and its subject from `display_path(root, ..)`, so the app's row and this line agree. `short_path` and the verb table inside `running_part` go if nothing else uses them.
  - `ClusterSummary::doing(&self) -> Option<String>`: the running part alone, e.g. `"Running cargo test"`.
  - `ClusterSummary::settled(&self) -> String`: the done parts alone, first letter upper case, e.g. `"Edited 1 file, ran 1 command"`; `"Working"` when there are none and something runs; empty when nothing runs and nothing settled.
  - `ClusterSummary::waiting: usize`, the members still `Pending`, beside the existing `running`.
  - `plain()` unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_cluster_says_what_runs_and_what_settled_apart() {
    let items = [edited("src/a.rs"), ran("cargo fmt"), running("cargo test")];
    let summary = cluster_summary(&refs(&items), Path::new("/work/app"));
    assert_eq!(summary.doing().as_deref(), Some("Running cargo test"));
    assert_eq!(summary.settled(), "Edited 1 file, ran 1 command");
}

#[test]
fn a_cluster_with_nothing_settled_is_working() {
    let items = [running("cargo test")];
    let summary = cluster_summary(&refs(&items), Path::new("/work/app"));
    assert_eq!(summary.settled(), "Working");
    let done = [ran("cargo fmt")];
    assert_eq!(cluster_summary(&refs(&done), Path::new("/work/app")).doing(), None);
}

#[test]
fn a_step_with_no_subject_leaves_no_gap() {
    let items = [running("")];
    let summary = cluster_summary(&refs(&items), Path::new("/work/app"));
    assert_eq!(summary.doing().as_deref(), Some("Running"));
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core --locked says_what_runs nothing_settled no_subject`. Expected: FAIL to compile.
- [ ] **Step 3: Implement**, and pass `&chat.root` at each caller.
- [ ] **Step 4: Run** `cargo test -p onehand-core --locked && cargo test -p onehand --locked`. Expected: PASS (existing summary tests updated only where a subject's short form changed from two segments to root-relative).
- [ ] **Step 5: Commit** `refactor(chat): give a cluster one sentence for what runs and one for what settled`

### Task 3: The shimmer builder

**Files:**
- Create: `crates/app/src/chat/transcript/parts/working.rs` (declared in `parts.rs` beside `mod activity;`)
- Modify: `.claude/skills/ui/catalog.md` (one row for the builder)
- Test: `#[cfg(test)]` in `working.rs`

**Interfaces:**
- Produces:
  - `pub(in crate::chat) fn working_text(id: impl Into<gpui::ElementId>, runs: Vec<WorkingRun>, cx: &App) -> gpui::AnyElement`
  - `pub(in crate::chat) struct WorkingRun { pub text: SharedString, pub mono: bool }`: a line is drawn as one text made of runs, so a verb in the UI family and a command in mono share one sweep.
  - `fn band(chars: usize, t: f32) -> Option<std::ops::Range<usize>>`: the characters lit at `t` in `0.0..1.0` of the period. The pass takes the first three quarters of the period and the rest is the pause; the band is `BAND` (4) characters wide and enters from before the first character and leaves past the last.

- [ ] **Step 1: Read the pinned GPUI before writing.** In the `gpui-pre` source under `~/.cargo/registry/src/`, read `with_animation` / `Animation` (repeat, duration) and how `StyledText` takes runs or highlights and truncates. Do not write this from memory: the API of this pin is what counts. Pick the way that only repaints.

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn the_band_crosses_the_text_then_rests() {
    assert_eq!(band(20, 0.0), None, "before the first character");
    let mid = band(20, 0.375).unwrap();
    assert_eq!(mid.len(), BAND);
    assert!(mid.start > 0 && mid.end < 20);
    assert_eq!(band(20, 0.8), None, "resting");
    assert_eq!(band(0, 0.3), None, "nothing to light");
    assert!(band(2, 0.375).unwrap().end <= 2, "never past the end");
}

#[test]
fn the_band_is_cut_on_characters() {
    let text = "Đang chạy 日本語 🙂 xong";
    for step in 0..100 {
        if let Some(lit) = band(text.chars().count(), step as f32 / 100.) {
            let _ = byte_range(text, lit); // must not panic
        }
    }
}
```

`byte_range(text: &str, chars: Range<usize>) -> Range<usize>` maps a character range to byte offsets on boundaries.

- [ ] **Step 3: Run** `cargo test -p onehand --locked the_band`. Expected: FAIL to compile.
- [ ] **Step 4: Implement** `band`, `byte_range` and `working_text`: base ink `crate::theme::meta_ink(cx)`, band ink `cx.theme().foreground` with one softer character each side (the two inks mixed), period 2200ms, repeating, single line, truncating as its neighbours do.
- [ ] **Step 5: Run** `cargo test -p onehand --locked the_band`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(chat): a builder that sweeps a band of brighter ink across a line's words`

### Task 4: Rows, fold lines and the plan drop their spinners

**Files:**
- Modify: `crates/app/src/chat/transcript/parts.rs` (`fold_line`), `parts/activity.rs` (`RowMark::draw`, `ActivityRow`, `activity_row`), `tool.rs` (`step_row`, `path_for_display` removed for core's), `strip.rs` (`cluster_line`), `prose.rs` (`thought`), `subagent.rs` (`head`, the thought row), `plan.rs`
- Test: `crates/app/src/chat/transcript/tests.rs`

**Interfaces:**
- Consumes: `working_text`, `WorkingRun` (Task 3); `Presentation::verb`, `ToolItem::running_secs`, `Thought::label`, `display_path` (Task 1); `ClusterSummary::{doing, settled}` (Task 2).
- Produces:
  - `fold_line(id, open: bool, cx) -> Stateful<Div>`: the `running` parameter goes; the slot always holds the chevron.
  - `RowMark::draw` returns the kind's icon for every state.
  - `ActivityRow` keeps its mark; `activity_row` draws a `RowMark::Running` row's verb and object through `working_text`, and a `RowMark::Waiting` row's in `muted_foreground`.

- [ ] **Step 1: Write the failing test** in `transcript/tests.rs`, on the words a line is given (the drawing is the owner's to look at). `cluster_words(summary: &ClusterSummary, open: bool) -> ClusterWords` is the pure half of `cluster_line`; `ClusterWords { shimmer: Option<String>, plain: String }`.

```rust
#[test]
fn a_cluster_line_says_what_runs_only_while_folded() {
    let summary = ClusterSummary {
        running: Some(part("Running", " cargo test")),
        done: vec![part("edited", " 1 file")],
        ..Default::default()
    };
    let open = cluster_words(&summary, true);
    assert_eq!((open.shimmer, open.plain.as_str()), (None, "Edited 1 file"));
    let folded = cluster_words(&summary, false);
    assert_eq!(folded.shimmer.as_deref(), Some("Running cargo test"));
    assert_eq!(folded.plain, "edited 1 file");

    let alone = ClusterSummary { running: Some(part("Running", " cargo test")), ..Default::default() };
    assert_eq!(cluster_words(&alone, true).plain, "Working");
    assert_eq!(cluster_words(&alone, false).plain, "", "nothing settled to add");
}
```

- [ ] **Step 2: Run it.** Expected: FAIL.
- [ ] **Step 3: Implement, one site at a time, building after each:**
  - `step_row`: verb from `presented.verb(t.call.status)`; object path through `display_path`; at the row's end, before the counts, `onehand_core::duration(secs)` in mono and `muted_foreground` when `t.running_secs()` is ten or more.
  - `cluster_line`: open and running, `summary.settled()` in the line's ink; folded and running, `working_text` of `summary.doing()`, then `·` and the done parts (`summary.settled()` lower-cased at its first letter, omitted when it is `Working`); nothing running, `summary.plain()` as today. The `N failed`, the counts and the duration are unchanged.
  - `thought`: `th.label()`; while running it goes through `working_text`. The accent ink goes.
  - `subagent.rs`: the head's name through `working_text` while the card works; the thought row uses `Thought::label`.
  - `plan.rs`: an item in progress while the turn is busy takes the pending item's icon and its words go through `working_text`; the `Spinner` import goes.
- [ ] **Step 4: Run** `cargo test -p onehand --locked`. Expected: PASS. Then `grep -rn "Spinner" crates/app/src/chat/transcript/`. Expected: no match.
- [ ] **Step 5: Commit** `style(chat): keep every mark in its slot and shimmer the words of what runs`

### Task 5: The running line

**Files:**
- Modify: `crates/app/src/chat/pane/runs.rs` (`working_strip`, `working_word` and its tests)
- Test: the tests beside `working_word`

**Interfaces:**
- Consumes: `working_text` (Task 3).
- Produces: `fn running_words(running: usize, waiting: usize) -> Option<String>`: `None` for `(0, 0)`; `"1 running"`; `"2 running, 1 waiting"`; `"1 waiting"`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_line_counts_what_runs_and_what_waits() {
    assert_eq!(running_words(0, 0), None);
    assert_eq!(running_words(1, 0).as_deref(), Some("1 running"));
    assert_eq!(running_words(2, 1).as_deref(), Some("2 running, 1 waiting"));
    assert_eq!(running_words(0, 1).as_deref(), Some("1 waiting"));
}
```

- [ ] **Step 2: Run it.** Expected: FAIL to compile.
- [ ] **Step 3: Implement.** `working_strip` draws, left to right: an empty mark slot as wide as a row's (so the clock stands on the rows' text axis), the clock in its reserved width, `·`, `running_words` counting `InProgress` and `Pending` apart, then `chat.activity_status()` through `working_text` when it is `Some`. Everything in `crate::theme::meta_ink(cx)`; no `Spinner`, no accent. `working_word` and its tests are deleted: a live thought and a running step are no longer repeated here.
- [ ] **Step 4: Run** `cargo test -p onehand --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `style(chat): the running line keeps the clock and the count, and repeats no step`

### Task 6: The contract, the gap, the checklist

**Files:**
- Modify: `DESIGN.md` (*Transcript*), `docs/known-gaps.md`, `CHANGELOG.md`, `.claude/skills/ui/catalog.md` if Task 3 left it

- [ ] **Step 1: `DESIGN.md`, *Transcript*.** Rewrite, in the document's own voice, the sentences that are no longer true: the running line ("a spinner, its clock, *N running*, what the agent says it is doing") becomes the clock, *N running* and *M waiting*, and what the agent says it is doing, with no mark; "leads with an `.xsmall()` mark in one slot (a spinner while it runs, so nothing moves as it settles)" becomes the kind's mark in every state; add one sentence for the shimmer (words of what runs, middle ink, a band of the foreground ink, no accent) and one for a step's own clock past ten seconds and a waiting step's muted row.
- [ ] **Step 2: `docs/known-gaps.md`.** Add: no setting turns the transcript's motion off, and why (the app has no motion setting yet).
- [ ] **Step 3: `CHANGELOG.md`**, under *Unreleased*, *Changed*: one entry written for a person using the app.
- [ ] **Step 4: Answer `.claude/skills/ui/checklist.md`** item by item in the PR body's draft; fix what answers no.
- [ ] **Step 5: Run every check** in Global Constraints. Expected: all pass, `git diff --check` clean.
- [ ] **Step 6: Commit** `docs(design): the transcript's working state, as built`

## Handing over

The PR's title: `style(chat): say what runs once, in the present tense, by its words`. Its body, plain text, ends by asking the owner to look in `make dev` or `make showcase` at: a running row, a folded running cluster, a live thought, a plan with an item in hand, the running line, both themes, a narrow pane, and zoom. No visual check is claimed.
