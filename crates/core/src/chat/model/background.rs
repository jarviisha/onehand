//! What the agent runs beside its turns, and how close it is to its limits.
//!
//! Neither is transcript: a background item comes and goes by the adapter's
//! own reports, and the meters are read off the latest usage report. Both are
//! cleared when the adapter goes away, since nothing would ever end them.

use super::items::ChatItem;
use super::{Chat, Replay};
use crate::acp::{AcpRequest, BackgroundReport, LimitStatus, RateLimit, ToolStatus, Usage};
use std::time::Instant;

/// One piece of work the agent started that runs on after its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub started: Instant,
}

/// How near a meter is to its cap, worst last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Calm,
    Warning,
    Danger,
}

/// A meter as drawn: its name, how full it is, and the numbers behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meter {
    pub label: String,
    /// `None` when no report said how full it is; the level still follows
    /// what the adapter's status says.
    pub percent: Option<u8>,
    pub level: Level,
    pub tip: String,
}

/// From here on a meter warns.
const WARN_AT: u8 = 80;

fn level_of(percent: u8) -> Level {
    if percent >= WARN_AT {
        Level::Warning
    } else {
        Level::Calm
    }
}

/// `84k` for thousands, rounded down; the plain number under a thousand.
fn tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{}k", n / 1000)
    }
}

/// A limit's name in a sentence.
fn limit_name(kind: Option<&str>) -> String {
    match kind {
        Some("five_hour") => "5-hour limit".into(),
        Some("seven_day") => "Weekly limit".into(),
        Some("seven_day_opus") => "Weekly Opus limit".into(),
        Some("seven_day_sonnet") => "Weekly Sonnet limit".into(),
        Some(other) => other.to_string(),
        None => "Limit".into(),
    }
}

/// A limit's name on the meter, where there is room for a word or two.
fn limit_label(kind: Option<&str>) -> &'static str {
    match kind {
        Some("five_hour") => "5h limit",
        Some(k) if k.starts_with("seven_day") => "Weekly limit",
        _ => "Limit",
    }
}

fn limit_percent(limit: &RateLimit) -> Option<u8> {
    limit
        .utilization
        .map(|u| (u * 100.0).round().clamp(0.0, 100.0) as u8)
}

fn limit_line(limit: &RateLimit) -> String {
    let mut line = limit_name(limit.kind.as_deref());
    if let Some(percent) = limit_percent(limit) {
        line.push_str(&format!(" {percent}%"));
    }
    let reset = limit
        .resets_at
        .and_then(|secs| chrono::DateTime::from_timestamp(secs as i64, 0))
        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string());
    if let Some(reset) = reset {
        line.push_str(&format!(", resets {reset}"));
    }
    line
}

impl Chat {
    /// The agent's live background work, oldest first.
    pub fn background(&self) -> &[BackgroundItem] {
        &self.background
    }

    /// Ask the adapter to stop one background item. The list follows the
    /// adapter's report that it ended, not this request.
    pub fn stop_background(&mut self, id: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::StopBackground { id: id.to_string() });
        }
    }

    /// How full the agent's context is, once it has said.
    pub fn context_meter(&self) -> Option<Meter> {
        let (used, size) = self.context?;
        if size == 0 {
            return None;
        }
        let percent = (used.saturating_mul(100) / size).min(100) as u8;
        Some(Meter {
            label: "Context".into(),
            percent: Some(percent),
            level: level_of(percent),
            tip: format!("{} of {} tokens", tokens(used), tokens(size)),
        })
    }

    /// The fullest limit reported, with every one seen in its tip.
    pub fn limit_meter(&self) -> Option<Meter> {
        let shown = self
            .limits
            .iter()
            .max_by_key(|limit| limit_percent(limit))?;
        let percent = limit_percent(shown);
        let status = match shown.status {
            LimitStatus::Allowed => Level::Calm,
            LimitStatus::Warning => Level::Warning,
            LimitStatus::Rejected => Level::Danger,
        };
        Some(Meter {
            label: limit_label(shown.kind.as_deref()).into(),
            percent,
            level: percent.map_or(Level::Calm, level_of).max(status),
            tip: self
                .limits
                .iter()
                .map(limit_line)
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }

    /// Whether the transcript holds something not yet on disk that is safe to
    /// write now, between turns: no turn running, no step still in flight,
    /// and no replay half-delivered. A line written early is frozen on disk,
    /// so anything still moving waits.
    ///
    /// Not while a rewrite is pending either: that is a resume settling, and
    /// the file being replaced is not new work between turns.
    pub fn unsaved_between_turns(&self) -> bool {
        !self.busy
            && !self.rewrite
            && !matches!(self.replay, Replay::Partial(_))
            && self.persisted < self.history.len() + self.items.len()
            && !self.items.iter().any(|item| {
                matches!(item, ChatItem::Tool(tool)
                    if matches!(tool.call.status, ToolStatus::Pending | ToolStatus::InProgress))
            })
    }

    pub(super) fn apply_background(&mut self, report: BackgroundReport) {
        match report {
            BackgroundReport::Spawned { id, name, kind } => {
                if self.background.iter().any(|item| item.id == id) {
                    return;
                }
                let kind = if kind.is_empty() { "task".into() } else { kind };
                let name = if name.is_empty() { kind.clone() } else { name };
                self.background.push(BackgroundItem {
                    id,
                    name,
                    kind,
                    started: Instant::now(),
                });
            }
            BackgroundReport::Ended { id } => self.background.retain(|item| item.id != id),
        }
    }

    pub(super) fn apply_usage(&mut self, usage: Usage) {
        self.context = Some((usage.used, usage.size));
        if let Some(limit) = usage.limit {
            match self.limits.iter_mut().find(|seen| seen.kind == limit.kind) {
                Some(seen) => *seen = limit,
                None => self.limits.push(limit),
            }
        }
    }

    /// Forget the background work and the meters: the adapter that reported
    /// them is gone and will never end them.
    pub(super) fn forget_activity(&mut self) {
        self.background.clear();
        self.context = None;
        self.limits.clear();
    }
}

#[cfg(test)]
mod tests;
