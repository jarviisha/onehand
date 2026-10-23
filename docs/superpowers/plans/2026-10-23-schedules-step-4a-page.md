# R4 step 4a: the Schedules page, the project page's box, and switching one on or off

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The schedules in `schedules.toml` are seen and switched on or off in the app: a
*Schedules* page on the rail, filtered by project as Tasks is, and a *Schedules* box on each
project's page leading to it. Making and editing one is 4b.

**Spec:** `docs/superpowers/specs/2026-10-23-schedules-design.md` (*Where*, the mockup's option C).
Builder's call: step 4 is two PRs; 4a the page, 4b the form.

## Global Constraints

- The `ui` skill and its checklist. A column page (`docs/ui-patterns.md`, *Column pages*) at
  `PAGE_PROSE`, its head `page_head`, its filter a `.short()` picker as the Tasks page's, its list
  rows in one `page_card` box, `ruled`, capped at `LIST_SHOWN` with *N more*.
- No primary on the page until 4b brings *New schedule*.
- A row's switch is the library `Switch`, `.small()`, in the `flex_none().cursor_pointer()`
  holder the unattended switches use. Switching on sets `on_since` to now (nothing before it is
  caught up); the write patches the file as it is, off the UI thread; a failed write puts it back
  and says so once.
- Words in CONTEXT.md's terms; no colour, radius or size hard-coded; a new key is a keymap command.

## Review Focus

1. A schedule whose expression does not parse → its row says so in the warning ink, no next time.
2. A row's `said` (a skip, a start that failed) → in its line, the warning ink for a failure.
3. A filter on a project that leaves the window → back to every project (as Tasks).
4. No schedule at all → the empty line says where they are kept.
5. Switching one on while the clock is mid-look → the clock's write patches `last_fired`/`said`
   only, so the switch stands.

---

### Task 1: Core words

- `schedule::when_said(expr: &str) -> String`: a quick choice in words (*every 3 hours at :05*,
  *every day at 09:00*, *weekdays at 09:00*, *Mondays at 09:00*), else the expression; one that
  does not parse → `"<expr> does not parse: <why>"`.
- `schedule::next_said(t: u64, now: u64, tz) -> String`: *today 09:00*, *tomorrow 09:00*, a
  weekday name within the week (*Mon 09:00*), else *Oct 30 09:00*.
- Tests: `quick_choices_read_in_words`, `a_bad_expression_says_why`, `the_next_time_reads_near_or_far`.

### Task 2: The clock keeps the list; switching on and off

- `schedules.rs`: `Clock.all: Vec<Schedule>`, refreshed by every look (and windows refreshed when
  it changed); `pub(crate) fn listed(cx) -> &[Schedule]`; `pub(crate) fn set_on(id, on, window, cx)`.

### Task 3: The page, the rail, the key, the project box

- `crates/app/src/schedules_page.rs`: `SchedulesPage` entity (`projects`, `filter`), the page.
- `chat/pane.rs` `Page::Schedules(AnyView)` and every match; `show_schedules`,
  `showing_schedules`; `ChatPaneEvent::ShowSchedules(Option<PathBuf>)`.
- Shell: `schedules_page` field, `show_schedules(filter, window, cx)`, `schedules_shown`,
  projects told as the Workflows page's are.
- Rail row after Workflows: `IconName::Calendar`, *Every schedule, and when each runs next*.
- `keymap`: `ShowSchedules`, `show_schedules`.
- Project page: `schedules_box` after Tasks: its schedules' names and next times, each row
  opening the page filtered to the project; *No schedule here* when none.

### Task 4: Documents, checks, review, PR

- `docs/screens.md` (the page, the box), `docs/schedules.md` (the page replaces "not built yet"),
  CHANGELOG. The checklist answered in the PR.
- `make fmt`, clippy `-D warnings`, `cargo test --workspace --locked`; Opus review; PR; CI; ask.
