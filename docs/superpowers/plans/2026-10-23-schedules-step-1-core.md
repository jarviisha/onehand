# R4 step 1: core `schedule`, the cron, the due rule and `schedules.toml`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core knows what a schedule is, reads and writes `schedules.toml`, parses and matches a
five-field cron expression, turns the form's quick choices into expressions and back, and decides
what a schedule does at a given moment. No behaviour change in the app.

**Architecture:** One core module `schedule` (`schedule.rs` + `schedule/cron.rs` +
`schedule/tests.rs`), pure apart from the blocking file read and write. Times are epoch seconds
in and out; the local calendar comes from a `chrono::TimeZone` passed in, `chrono::Local` in the
app and fixed offsets in tests.

**Tech Stack:** Rust, serde + toml (already), chrono 0.4 (already in `Cargo.lock` and a direct
dependency of `onehand-issues`; added to core).

**Spec:** `docs/superpowers/specs/2026-10-23-schedules-design.md` (Goal 1, *What a schedule is*,
Decisions 1, 2, 2a, 4, 5).

## Global Constraints

- Five fields: minute (0–59), hour (0–23), day of month (1–31), month (1–12), day of week (0–7,
  0 and 7 Sunday); `*`, numbers, `a-b`, lists `a,b`, steps `*/n` and `a-b/n`. Day of month and
  day of week both restricted → either matches (cron's rule).
- The machine's own time zone only. Missed many times → once; the due rule never fires twice for
  one due time; nothing due before the schedule was made or switched on.
- Defaults: missed → run once; overlap → skip; tools, labels, options → none.
- Core stays GUI-free and runtime-free; nothing is `pub` unless the app names it.
- `chrono` is added to core with the version the workspace already locks; `cargo ... --locked`
  must pass with the lock committed.

## Review Focus

1. `0 9 * * *` checked at 09:00:30 the day the schedule was switched on at 08:59 → due once, then
   not again at 09:00:59. → test `fires_once_for_one_due_time`.
2. A schedule last fired a week ago, every day at 9 → one start, marked missed when the due time is
   before onehand opened. → test `a_week_missed_is_one_start`.
3. `0 9 31 2 *` (never matches) → no hang. → test `an_expression_that_never_matches_is_bounded`.
4. `0 9 1 * 1` → the 1st of a month *or* a Monday (cron's either rule). → test
   `day_of_month_and_weekday_restricted_match_either`.
5. A hand-edited `schedules.toml` with an unknown `starts` kind → refused with the file named, not
   a panic. → test `a_file_that_does_not_parse_says_where`.

---

### Task 1: The cron

**Files:** Create `crates/core/src/schedule/cron.rs`.

**Produces:**
- `pub struct Cron` (fields private: one bit set per field, and whether day of month / day of week
  were `*`).
- `pub fn parse(expr: &str) -> Result<Cron, String>` — errors name the field and the bad part,
  e.g. `"hour: 24 is past 23"`, `"5 fields expected, found 4"`.
- `pub(crate) fn matches(&self, t: &impl chrono::Datelike + chrono::Timelike) -> bool`.

Tests (in `schedule/tests.rs`): `every_field_form_parses` (`*/15 9-17 * * 1-5`, `0,30 * * * *`,
`0 0 1 1 *`, `0 9 * * 7` = Sunday); `a_bad_field_is_named` (`60 * * * *` → contains `minute`,
`* * * *` → contains `5 fields`, `*/0 * * * *` → contains `step`);
`day_of_month_and_weekday_restricted_match_either`.

### Task 2: The schedule, its quick choices and the due rule

**Files:** Create `crates/core/src/schedule.rs`; modify `crates/core/src/lib.rs` (`pub mod schedule;`),
`crates/core/Cargo.toml` (`chrono = "0.4"`).

**Produces:**
- `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Schedule { id: String,
  name: String, project: PathBuf, on: bool, starts: Starts, agent: Option<String>,
  mode: Option<String>, options: BTreeMap<String, String>, tools: Vec<String>,
  labels: Vec<String>, when: String, missed: Missed, overlap: Overlap, on_since: u64,
  last_fired: Option<u64>, said: Option<String> }` — fields `pub`; serde defaults for every
  optional one.
- `#[serde(tag = "kind", rename_all = "snake_case")] pub enum Starts { Prompt { prompt: String },
  Workflow { workflow: String, title: String, body: String } }`.
- `pub enum Missed { RunOnce, Skip }` (default `RunOnce`), `pub enum Overlap { Skip, Queue }`
  (default `Skip`), both `snake_case` in the file.
- `pub enum Quick { EveryHours { hours: u8, minute: u8 }, Daily { hour: u8, minute: u8 },
  Weekdays { hour: u8, minute: u8 }, Weekly { weekday: u8, hour: u8, minute: u8 } }`;
  `pub fn expr(&self) -> String` (`0 */3 * * *`… exact: `"{minute} */{hours} * * *"`,
  `"{minute} {hour} * * *"`, `"{minute} {hour} * * 1-5"`, `"{minute} {hour} * * {weekday}"`);
  `pub fn read(expr: &str) -> Option<Quick>` the inverse for exactly those shapes.
- `pub enum Act { Wait, Start { missed: bool }, Skip(String), Queue }` and
  `pub fn decide(s: &Schedule, now: u64, opened_at: u64, busy: bool, tz: &impl TimeZone) -> Act`:
  - off, or `when` that does not parse → `Wait` (the form refuses the latter; a hand edit waits);
  - the latest due minute in `(max(last_fired, on_since), now]`, searched backwards a minute at a
    time, at most 366 days → none → `Wait`;
  - `missed` = that minute is before `opened_at`; `Missed::Skip` and missed →
    `Skip("missed while onehand was closed")`;
  - `busy` (its last start still queued or running): `Overlap::Skip` →
    `Skip("the last run was still going")`, `Overlap::Queue` → `Queue`;
  - else `Start { missed }`.
  The app records `last_fired = now` after acting on anything but `Wait`.
- `pub fn next_times(expr: &str, after: u64, count: usize, tz: &impl TimeZone) -> Vec<u64>` —
  the form's preview, forward, the same 366-day bound.

Tests: `quick_choices_write_and_read_back` (each variant round-trips; `read("0 9 1 * *")` is `None`);
`fires_once_for_one_due_time`; `a_week_missed_is_one_start` (`Start { missed: true }`);
`a_missed_time_set_to_skip_is_skipped`; `nothing_before_it_was_switched_on`;
`a_busy_schedule_skips_or_queues_as_set`; `an_expression_that_never_matches_is_bounded`
(returns `Wait` and `next_times` empty, under a second); `next_times_lists_the_coming_runs`
(`0 9 * * *` after 08:00 → today 09:00, tomorrow 09:00). All with `FixedOffset::east_opt(7*3600)`.

### Task 3: `schedules.toml`

**Files:** `crates/core/src/schedule.rs`; `crates/core/src/config.rs` (nothing new: `write_atomic`
is already `pub(crate)`).

**Produces:**
- `pub fn path() -> PathBuf` — `config::config_dir().join("schedules.toml")`.
- `pub fn load_blocking(path: &Path) -> Result<Vec<Schedule>, String>` — missing file → empty;
  a file that does not parse → `Err("<path> won't parse: …")`.
- `pub fn save_blocking(path: &Path, all: &[Schedule]) -> Result<(), String>` — `[[schedule]]`
  tables, through `write_atomic`.
- `pub fn new_id() -> String` — as `task::new_id` makes one.

Tests: `schedules_round_trip_through_the_file`; `a_missing_file_is_no_schedules`;
`a_file_that_does_not_parse_says_where`; `defaults_fill_what_the_file_leaves_out` (a table with
only `id`, `name`, `project`, `when`, `[starts]` loads with `on = false`, `Missed::RunOnce`,
`Overlap::Skip`, empty tools).

### Task 4: Checks and PR

- `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test --workspace --locked`,
  `cargo tree -p onehand-core -i gpui` (no match). Opus review; PR (the spec rides along); CI;
  ask before merging. No user-facing document yet: `docs/schedules.md` comes with step 3, when a
  schedule first does something.
