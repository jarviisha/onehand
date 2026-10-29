//! What the agent runs beside its turns, and how close it is to its limits.
//!
//! Neither is transcript: a background item comes and goes by the adapter's
//! own reports, and the meters are read off the latest usage report. Both are
//! cleared when the adapter goes away, since nothing would ever end them.
//!
//! What stays is the end: a line in the transcript saying what ran, how it
//! ended and for how long, so a conversation read back later still shows it.

use super::items::{ChatItem, NoticeLevel};
use super::{ApplyOutcome, Chat, Link, Replay};
use crate::acp::{
    AcpRequest, BackgroundEnd, BackgroundReport, LimitStatus, RateLimit, ToolStatus, Usage,
};
use crate::chat::activity::first_line_trunc;
use std::time::{Duration, Instant};

/// One piece of work the agent started that runs on after its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub started: Instant,
}

/// What the agent does by itself between turns. No adapter says when such a
/// cycle starts, only (Claude) when it ends, so it is read off what arrives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum OwnCycle {
    #[default]
    None,
    /// The last background item ended, and the agent will most likely answer
    /// it: running, so the wait for that answer neither reads idle nor lets a
    /// run judge its step early.
    Expected,
    /// The agent is writing with no turn open.
    Writing,
}

/// A silence this long ends a cycle of an adapter that never says when its
/// own work settles.
const QUIET: Duration = Duration::from_secs(3);

/// A silence this long ends a cycle the adapter should have ended itself, or
/// one expected that never started: its word may never come, and a model can
/// take a while to start answering.
const GRACE: Duration = Duration::from_secs(30);

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

fn status_level(limit: &RateLimit) -> Level {
    match limit.status {
        LimitStatus::Allowed => Level::Calm,
        LimitStatus::Warning => Level::Warning,
        LimitStatus::Rejected => Level::Danger,
    }
}

/// How long a background item's name runs on its line before it is clipped.
const NAME_CHARS: usize = 80;

/// The transcript's line for a background item that is over: `ended` says
/// how, `level` how loudly.
fn ended_line(item: &BackgroundItem, ended: &str, level: NoticeLevel) -> ChatItem {
    let took = crate::duration(item.started.elapsed().as_secs());
    let mut text = format!("Background work {ended} after {took}");
    // A name that only repeats the kind (the fallback) is no name.
    if item.name != item.kind {
        text.push_str(": ");
        text.push_str(&first_line_trunc(&item.name, NAME_CHARS));
    }
    ChatItem::failure(text, level, None)
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
            self.stop_asked.push(id.to_string());
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
            tip: format!("Context: {} of {} tokens", tokens(used), tokens(size)),
        })
    }

    /// The worst limit reported, then the fullest, with every one seen in
    /// its tip. A blocked limit that gives no figure still wins over a calm
    /// one that does.
    pub fn limit_meter(&self) -> Option<Meter> {
        let shown = self
            .limits
            .iter()
            .max_by_key(|limit| (status_level(limit), limit_percent(limit)))?;
        let percent = limit_percent(shown);
        let status = status_level(shown);
        // A limit with no figure and nothing wrong has nothing to say: its
        // name alone under the composer reads as a meter that failed to draw.
        if percent.is_none() && status == Level::Calm {
            return None;
        }
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

    /// Whether the agent is running: a turn is open, work or a subagent it
    /// started runs on after its turn, or it is at work by itself.
    pub fn running(&self) -> bool {
        self.running_since(None)
    }

    /// [`Self::running`], counting only background work started at `since` or
    /// later; all of it with no `since`.
    pub fn running_since(&self, since: Option<Instant>) -> bool {
        let fresh = |item: &BackgroundItem| since.is_none_or(|since| item.started >= since);
        self.busy
            || self.background.iter().any(fresh)
            || self.subagents_working() > 0
            || self.own_cycle != OwnCycle::None
    }

    /// Whether work or a subagent the agent started runs beside its turns.
    pub(super) fn works_aside(&self) -> bool {
        !self.background.is_empty() || self.subagents_working() > 0
    }

    /// How long a silence ends the cycle the agent is running by itself, if
    /// it is thought to run one; the front end calls
    /// [`Self::quiet_spell`] once it has passed.
    pub fn own_cycle_wait(&self) -> Option<Duration> {
        match self.own_cycle {
            OwnCycle::None => None,
            OwnCycle::Writing if !self.cycles_marked => Some(QUIET),
            OwnCycle::Writing | OwnCycle::Expected => Some(GRACE),
        }
    }

    /// The agent has been silent for [`Self::own_cycle_wait`]: its own cycle
    /// is taken to be over, unless a step or a thought it left in flight says
    /// it is not. Whether that changed anything.
    pub fn quiet_spell(&mut self) -> bool {
        let over = match self.own_cycle {
            OwnCycle::None => false,
            OwnCycle::Expected => true,
            OwnCycle::Writing => !self.in_flight(),
        };
        if over {
            self.own_cycle = OwnCycle::None;
        }
        over
    }

    /// Follow the agent's own cycle through the event just applied: it
    /// `wrote`, work beside the turns ran before it (`aside_was`), and it
    /// `closed` a turn or a cycle.
    pub(super) fn follow_own_cycle(&mut self, wrote: bool, aside_was: bool, closed: bool) {
        let replaying = matches!(self.replay, Replay::Partial(_)) || self.rewrite;
        self.own_cycle = if closed || self.busy || self.link != Link::Connected || replaying {
            // A turn's own running covers what it writes, and nothing comes
            // from an adapter that is gone, or still replaying a resume.
            OwnCycle::None
        } else if wrote {
            OwnCycle::Writing
        } else if aside_was && !self.works_aside() && self.own_cycle == OwnCycle::None {
            // The last of it ended: the agent answers that by itself.
            OwnCycle::Expected
        } else {
            self.own_cycle
        };
    }

    /// A step or a thought still moving.
    fn in_flight(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(item, ChatItem::Tool(tool)
                if matches!(tool.call.status, ToolStatus::Pending | ToolStatus::InProgress))
                || matches!(item, ChatItem::Thought(thought) if thought.is_running())
        })
    }

    /// Whether the transcript holds something not yet on disk that is safe to
    /// write now, between turns: the adapter still live, no turn running, no
    /// step or thought still in flight, and no replay half-delivered. A line
    /// written early is frozen on disk, so anything still moving waits.
    ///
    /// Not while a rewrite is pending either: that is a resume settling, and
    /// the file being replaced is not new work between turns. Nor once the
    /// adapter is gone: its failure line is the turn's end to say, not work.
    pub fn unsaved_between_turns(&self) -> bool {
        self.link == Link::Connected
            && !self.busy
            && !self.rewrite
            && !matches!(self.replay, Replay::Partial(_))
            && self.persisted < self.saveable_len()
            && !self.in_flight()
    }

    /// Whether the event just applied, with `outcome`, is the agent writing
    /// between turns and so starts (or restarts) the wait for a quiet spell
    /// before it is saved.
    ///
    /// Only for an adapter that never says its own work has settled; one that
    /// does is saved at that word. A turn's end is saved by the turn, and a
    /// report that changed no transcript, or a line onehand wrote itself, is
    /// not the agent writing.
    pub fn starts_quiet_save(&self, outcome: &ApplyOutcome) -> bool {
        outcome.transcript_changed
            && !outcome.turn_ended
            && !self.busy
            && !self.cycles_marked
            && self.link == Link::Connected
    }

    /// Follow one background report: the transcript's line when an item
    /// ended, and whether it was an item a person asked to stop, so nothing
    /// the agent will answer.
    pub(super) fn apply_background(
        &mut self,
        report: BackgroundReport,
    ) -> (Option<ChatItem>, bool) {
        match report {
            BackgroundReport::Spawned { id, name, kind } => {
                if self.background.iter().any(|item| item.id == id) {
                    return (None, false);
                }
                let kind = if kind.is_empty() { "task".into() } else { kind };
                let name = if name.is_empty() { kind.clone() } else { name };
                self.background.push(BackgroundItem {
                    id,
                    name,
                    kind,
                    started: Instant::now(),
                });
                (None, false)
            }
            BackgroundReport::Ended { id, end } => {
                let asked = self.stop_asked.contains(&id);
                self.stop_asked.retain(|asked| *asked != id);
                let Some(at) = self.background.iter().position(|item| item.id == id) else {
                    return (None, asked);
                };
                let item = self.background.remove(at);
                let line = match end {
                    BackgroundEnd::Completed => ended_line(&item, "finished", NoticeLevel::Info),
                    BackgroundEnd::Failed => ended_line(&item, "failed", NoticeLevel::Error),
                    BackgroundEnd::Stopped => ended_line(&item, "stopped", NoticeLevel::Info),
                };
                (Some(line), asked)
            }
        }
    }

    pub(super) fn apply_usage(&mut self, usage: Usage) {
        self.cycles_marked |= usage.marks_cycles;
        self.context = Some((usage.used, usage.size));
        if let Some(limit) = usage.limit {
            match self.limits.iter_mut().find(|seen| seen.kind == limit.kind) {
                Some(seen) => *seen = limit,
                None => self.limits.push(limit),
            }
        }
    }

    /// Forget the background work and the meters, and interrupt the working
    /// subagents: the adapter that reported them is gone and will never end
    /// them. Each item still running says so in the transcript.
    pub(super) fn forget_activity(&mut self) {
        self.interrupt_subagents();
        if !self.background.is_empty() {
            for item in std::mem::take(&mut self.background) {
                let line = ended_line(&item, "interrupted", NoticeLevel::Info);
                self.items.push(line);
            }
            self.touch();
        }
        self.stop_asked.clear();
        self.context = None;
        self.limits.clear();
    }
}

#[cfg(test)]
mod tests;
