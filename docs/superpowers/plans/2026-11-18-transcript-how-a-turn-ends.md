# How a Turn Ends: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A turn's ending says what happened: stopped by a cancel, stopped at a limit or refused, or failed; and a failure the turn moved past stops shouting.

**Architecture:** Core gains a step outcome (`ToolStatus::Stopped`) and two notice levels (`NoticeLevel::Stopped`, `NoticeLevel::CutShort(CutShort)`), each saved; it words them and decides when *Continue* is offered. The app draws what core says: a muted *stopped*, a neutral card with *Continue*, the closing line.

**Tech Stack:** Rust, GPUI, gpui-component. No new dependency.

**Spec:** `docs/superpowers/specs/2026-11-18-transcript-working-state-design.md`, section 2, and *Decided: a turn cut short stays a failed turn*.

## Global Constraints

- Builds on PR #357 (branch `style/transcript-working-state`); this branch is stacked on it.
- A turn cut short (output limit, request limit, refusal) stays a failed turn to the model: `Chat::failed`, the rail, runs and leads behave as today. Only the transcript's words and ink change.
- Words live in core; the app matches on core's enums, never on its labels. Match our own enums exhaustively.
- No document is named in code. No hard-coded colour or size.
- Checks: `cargo test -p onehand-core --locked`, `cargo test -p onehand --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

- An archive written before this change (levels `""`, `error`, `failed`; statuses without `stopped`) reads back as before. (Task 1)
- A cancel answered by an adapter with an error still reads *Stopped*, not failed (the existing `self.cancelled` or-ing). (Task 1)
- A turn with both a cancel notice from the adapter and the turn's own end gets one *Stopped* divider, not two. (Task 1)
- *Continue* is refused while a turn runs, after a prompt has followed, and for a refusal. (Task 2)
- A lost agent still fails its running steps; only a cancel stops them. (Task 1)

---

### Task 1: Core says how a step and a turn ended

**Files:** `crates/core/src/acp/types.rs` (`ToolStatus`), `crates/core/src/chat/model/items.rs` (`NoticeLevel`, `CutShort`, `TurnAnswer`), `crates/core/src/chat/model/apply.rs` (`settle_steps`, turn end, `cut_short`, the interrupted notice), `crates/core/src/chat/model/turn.rs` (`settle_running_steps`, `turn_answer`, `standing_failure`), `crates/core/src/chat/model/subagent.rs` (`close`), `crates/core/src/chat/steps.rs` (`ClusterSummary::stopped`, `run_outcome`, `turn_changes`), `crates/core/src/chat/store/lines.rs`; tests beside each.

**Produces:**
- `ToolStatus::Stopped`: saved as `stopped`; `is_running()` false; never sent by an agent.
- `enum CutShort { OutputLimit, RequestLimit, Refused }` with `title()`, `sentence()`, `continues()` (true for the two limits).
- `NoticeLevel::Stopped` (a divider; saved `stopped`) and `NoticeLevel::CutShort(CutShort)` (saved `output-limit`, `request-limit`, `refused`); `NoticeLevel::fails_turn()` true for `Failed` and `CutShort(_)`.
- `enum TurnEnding { Finished, Failed, Stopped }`; `TurnAnswer::ending` replaces `failed`; `closing_line()` says *Processed in* / *Failed after* / *Stopped after*.
- `ClusterSummary::stopped: usize`.

Steps: tests first (a cancelled turn stops its running steps and leaves one *Stopped* notice; a lost link fails them; a cancelled subagent stops its steps; each stop reason gives its `CutShort` and the turn is still `failed()`; the closing line for each ending; every new level and status round-trips through `items.jsonl`; a cluster counts stopped steps apart from failed ones), watch them fail, implement, run core's tests, commit.

### Task 2: Core offers *Continue*

**Files:** `crates/core/src/chat/model/turn.rs`, `crates/core/src/chat/drive.rs`.

**Produces:** `Chat::stopped_at_limit() -> bool` (the standing failure is a `CutShort` that `continues()`); `SessionDrive::continue_after_limit(now) -> (Intake, Vec<Effect>)`, sending `CONTINUE_PROMPT` as a person's prompt, refused when `stopped_at_limit()` is false or a turn runs.

Steps: tests first (refused while busy, after a prompt, for a refusal; sent otherwise), implement, run, commit.

### Task 3: The transcript draws it

**Files:** `crates/app/src/chat/transcript.rs` (notice shapes, the cut-short card, *Continue*), `crates/app/src/chat/session.rs` (`continue_after_limit`), `crates/app/src/chat/transcript/tool.rs` (`step_row(.., past)`), `parts/activity.rs` (`RowMark::Stopped`), `strip.rs` (`N stopped`), `subagent.rs`, `prose.rs` (closing line).

- A stopped step: *stopped* at its row's end in the muted ink. A failed step: *exit N* or *failed* in the danger ink while its turn runs, the muted ink once it has ended.
- A cluster line: *· N stopped* in its own ink after any *N failed*.
- A cut-short notice: the neutral card, flat, the info mark in the muted ink, core's title over core's sentence in the body ink, and *Continue* (`secondary`, `short`) while `stopped_at_limit()` and this is the standing notice.
- A *Stopped* notice: the divider.

Steps: run the app's tests, clippy, commit.

### Task 4: The contract

`DESIGN.md` *Transcript* (errors, the closing line, *Calm once it is past*), `docs/known-gaps.md`, `CHANGELOG.md`. Run every check; commit.
