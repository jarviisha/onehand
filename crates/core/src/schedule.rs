//! Schedules: work onehand starts by the clock, while it runs.
//!
//! A schedule names a project, what it starts there (a session with a prompt,
//! or a workflow on a brief), the agent, its mode and settings, the tools its
//! agent may use, the labels its issues get, and when, as a cron expression.
//! The app looks once a minute and once at start; what a look comes to is
//! [`decide`]'s, so the rule — fire once per due time, catch a missed one up
//! once or skip it, never fire for a time before the schedule was on — lives
//! here once. All of them are kept in `schedules.toml` in the config
//! directory, each naming its project by path, so one file serves every
//! window and a project no window holds simply waits.

mod cron;

pub use cron::{parse, Cron};

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One schedule, as `schedules.toml` keeps it.
///
/// **Plain values before tables**, which is the only order TOML can write a
/// struct in: `options` and `starts` come last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub project: PathBuf,
    /// Off, it never fires; nothing is lost by switching it off.
    #[serde(default)]
    pub on: bool,
    /// The cron expression the clock reads.
    pub when: String,
    /// The agent its sessions use; none, the first configured one.
    #[serde(default)]
    pub agent: Option<String>,
    /// The agent's own session mode its sessions start in; none, the agent's.
    #[serde(default)]
    pub mode: Option<String>,
    /// The tools its agent may use, by name; none by default.
    #[serde(default)]
    pub tools: Vec<String>,
    /// The labels an issue its agent writes gets.
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub missed: Missed,
    #[serde(default)]
    pub overlap: Overlap,
    /// When it was made or last switched on, in epoch seconds: nothing due
    /// before it fires.
    #[serde(default)]
    pub on_since: u64,
    /// When it last fired, or was skipped, in epoch seconds.
    #[serde(default)]
    pub last_fired: Option<u64>,
    /// What its last look said, when that was a skip or a start that failed.
    #[serde(default)]
    pub said: Option<String>,
    /// The agent's settings (its model and whatever else it offers), by id.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    pub starts: Starts,
}

/// What a schedule starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Starts {
    /// A session sent this prompt; a skill is used by naming it there.
    Prompt { prompt: String },
    /// A workflow, by id, on this brief.
    Workflow {
        workflow: String,
        title: String,
        #[serde(default)]
        body: String,
    },
}

/// What a due time that passed while onehand was closed comes to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Missed {
    /// Run once when onehand opens, however many times it was missed.
    #[default]
    RunOnce,
    /// Wait for the next time.
    Skip,
}

/// What a due time that finds the last start still queued or running comes
/// to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overlap {
    #[default]
    Skip,
    /// Start once the last one has stopped.
    Queue,
}

/// The form's quick choices, each one shape of expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quick {
    EveryHours {
        hours: u8,
        minute: u8,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekdays {
        hour: u8,
        minute: u8,
    },
    /// `weekday` counts from Sunday, 0.
    Weekly {
        weekday: u8,
        hour: u8,
        minute: u8,
    },
}

impl Quick {
    /// The expression it writes.
    pub fn expr(&self) -> String {
        match *self {
            Self::EveryHours { hours, minute } => format!("{minute} */{hours} * * *"),
            Self::Daily { hour, minute } => format!("{minute} {hour} * * *"),
            Self::Weekdays { hour, minute } => format!("{minute} {hour} * * 1-5"),
            Self::Weekly {
                weekday,
                hour,
                minute,
            } => format!("{minute} {hour} * * {weekday}"),
        }
    }

    /// The choice `expr` is, when it is exactly one of the shapes they write.
    pub fn read(expr: &str) -> Option<Self> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        let [minute, hour, "*", "*", weekday] = parts.as_slice() else {
            return None;
        };
        let minute: u8 = minute.parse().ok().filter(|m| *m < 60)?;
        if let Some(hours) = hour.strip_prefix("*/") {
            let hours: u8 = hours.parse().ok().filter(|h| (1..24).contains(h))?;
            return (*weekday == "*").then_some(Self::EveryHours { hours, minute });
        }
        let hour: u8 = hour.parse().ok().filter(|h| *h < 24)?;
        match *weekday {
            "*" => Some(Self::Daily { hour, minute }),
            "1-5" => Some(Self::Weekdays { hour, minute }),
            day => {
                let weekday: u8 = day.parse().ok().filter(|d| *d < 7)?;
                Some(Self::Weekly {
                    weekday,
                    hour,
                    minute,
                })
            }
        }
    }
}

/// What a look at a schedule comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Nothing is due.
    Wait,
    /// Start it now; `missed` when the time it is for passed while onehand
    /// was closed.
    Start { missed: bool },
    /// Do not start it, and say why on its row.
    Skip(String),
    /// Start it once its last start has stopped.
    Queue,
}

/// How far back a look searches for a due time, and forward for the next:
/// a year, so an expression that never matches costs a bounded look.
const SEARCH_MINUTES: u64 = 366 * 24 * 60;

/// What schedule `s` does at `now`, onehand having opened at `opened_at`,
/// `busy` while its last start is still queued or running, in time zone
/// `tz`. The caller moves `last_fired` to `now` after anything but
/// [`Act::Wait`], which is what makes one due time fire once.
pub fn decide(s: &Schedule, now: u64, opened_at: u64, busy: bool, tz: &impl TimeZone) -> Act {
    let Some(cron) = s.on.then(|| parse(&s.when).ok()).flatten() else {
        return Act::Wait;
    };
    let after = s.last_fired.unwrap_or(0).max(s.on_since);
    let Some(due) = latest_due(&cron, after, now, tz) else {
        return Act::Wait;
    };
    let missed = due < minute_of(opened_at);
    if missed && s.missed == Missed::Skip {
        return Act::Skip("missed while onehand was closed".to_string());
    }
    match (busy, s.overlap) {
        (true, Overlap::Skip) => Act::Skip("the last run was still going".to_string()),
        (true, Overlap::Queue) => Act::Queue,
        (false, _) => Act::Start { missed },
    }
}

/// The next `count` times `expr` names after `after`, for the form to show.
pub fn next_times(expr: &str, after: u64, count: usize, tz: &impl TimeZone) -> Vec<u64> {
    let Ok(cron) = parse(expr) else {
        return Vec::new();
    };
    (1..=SEARCH_MINUTES)
        .map(|i| minute_of(after) + i * 60)
        .filter(|t| names(&cron, *t, tz))
        .take(count)
        .collect()
}

/// The latest minute in `(after, now]` that `cron` names.
fn latest_due(cron: &Cron, after: u64, now: u64, tz: &impl TimeZone) -> Option<u64> {
    (0..SEARCH_MINUTES)
        .map(|i| minute_of(now).checked_sub(i * 60))
        .take_while(|t| t.is_some_and(|t| t > after))
        .flatten()
        .find(|t| names(cron, *t, tz))
}

fn names(cron: &Cron, t: u64, tz: &impl TimeZone) -> bool {
    tz.timestamp_opt(t as i64, 0)
        .single()
        .is_some_and(|local| cron.matches(&local))
}

fn minute_of(t: u64) -> u64 {
    t - t % 60
}

/// Where the schedules are kept.
pub fn path() -> PathBuf {
    crate::config::config_dir().join("schedules.toml")
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    schedule: Vec<Schedule>,
}

#[derive(Serialize)]
struct Out<'a> {
    schedule: &'a [Schedule],
}

/// The schedules in `path`; none when there is no file. Blocking.
pub fn load_blocking(path: &Path) -> Result<Vec<Schedule>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
    };
    toml::from_str::<File>(&text)
        .map(|file| file.schedule)
        .map_err(|e| format!("{} won't parse: {e}", path.display()))
}

/// Write `all` to `path`, whole, in one rename. Blocking.
pub fn save_blocking(path: &Path, all: &[Schedule]) -> Result<(), String> {
    let text = toml::to_string(&Out { schedule: all }).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::config::write_atomic(path, &text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
