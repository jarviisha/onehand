# R4 step 4b: the schedule form

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A schedule is made, edited and deleted on the Schedules page, in the form the owner
approved in the mockup: name, project, what it starts (a session with a prompt, or a workflow on a
brief), agent, mode and settings, tools, labels, when (quick choices writing a cron expression, or
an expression), what a missed or overlapping time comes to, and on or off.

**Spec:** `docs/superpowers/specs/2026-10-23-schedules-design.md`; mockup
`.superpowers/brainstorm/*/content/schedules-v1.html` (local). Step 4a: `…-step-4a-page.md`.

## Global Constraints

- The `ui` skill. A page form (`docs/ui-patterns.md`, *Forms*): fields `.tall()`, its submit in
  the page's head as the workflow editor's (*Cancel*, *Save* the one primary, *Delete…* in ⋯), its
  problems in a bar along the page's foot, none on a new form nobody has changed. *New schedule* is
  the list's primary.
- **Reuse, lifted where a second place needs it**: the workflow step's row of choices
  (`pick_row`) becomes `controls::setting_row`, and its foot bar `page_parts::foot_bar`; both
  callers use them. The agent's offer and *Check the agent* as the workflow step's.
- What makes a schedule unsavable is core's (`schedule::problems`), tested.
- Saving and deleting go through the one ordered writer, patching the file by id; the list shows
  the result at once; a failure is a toast. Switched on by the save (new, or off before) sets
  `on_since` to now. Deleting asks first (`Confirm`) and keeps the schedule's tasks.
- Esc closes the form (`CloseScheduleForm`, `SchedulesPage && !Dialog && !Input`), asking first
  while it holds changes, as Cancel does.

## Review Focus

1. A time typed as `9` or `25:00` → a problem naming the field, *Save* refuses.
2. Switching *What it starts* keeps what was typed in the other kind, until saved.
3. An agent whose offer is not known → *Check the agent*; the mode and options are kept as picked.
4. Editing a schedule keeps its `last_fired`, `said` and `on_since` unless switched on by it.
5. Deleting a schedule while a look is writing → the writer orders them; the list follows.

---

### Task 1: Core

- `schedule::problems(s: &Schedule) -> Vec<String>`: *Name it*, *Pick a project*, *Say what to
  ask* (prompt), *Pick a workflow*, *Give the brief a title* (workflow), *When: <why>*.
- `control::tool_names() -> &'static [&'static str]`.
- Tests: `a_blank_schedule_has_every_problem`, `a_whole_schedule_has_none`.

### Task 2: Lifted builders

- `controls::setting_row(id, none_label, offered, current, strict, refused, pick, cx)` and
  `controls::SettingPick` (from `workflows_page/step/settings.rs`); that file calls them.
- `page_parts::foot_bar(icon, base, lines, cx)` (the drawing of `workflows_page/form.rs`
  `message_bar`, which calls it).

### Task 3: The draft and the form

- `schedules_page/draft.rs`: `ScheduleDraft` (inputs and choices; `from(&Schedule)`,
  `blank(project)`, `to_schedule(cx) -> Result<Schedule, String>` with the when parsed,
  `problems(cx)`, `dirty(cx)`).
- `schedules_page/form.rs`: the head and sections (*What it starts*, *The agent*, *Tools*,
  *When*, *Missed and overlapping times*); `schedules_page/when.rs`: the quick choices, their
  fields, the expression, the next three times.
- `schedules_page.rs`: *New schedule*, a row opens it, `open`, `close` (asks when dirty),
  `save`, `delete`; Esc. `schedules::put(all, cx)` sets the clock's list after a write.

### Task 4: Documents, checks, review, PR

- `docs/screens.md` (the form; *New schedule*), `docs/schedules.md` (made in the app),
  `.claude/skills/ui/catalog.md` (`setting_row`, `foot_bar`), CHANGELOG. Checklist in the PR.
- `make fmt`, clippy `-D warnings`, `cargo test --workspace --locked`; Opus review; PR; CI; ask.
