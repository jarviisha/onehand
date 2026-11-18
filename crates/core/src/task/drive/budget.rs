//! A run's timeout, counted only while it works.

use std::time::{Duration, Instant};

/// How much of its timeout a run has left, counting only the time it spent
/// working.
///
/// **Time spent waiting on a person does not count.** A run that parked a
/// question is standing still because somebody has not answered yet, and a
/// timeout that ran through the wait would cancel the run for a slow reply —
/// which is the one failure the wait exists to avoid. The clock is what the
/// timeout bounds: an agent that works without finishing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    limit: Duration,
    spent: Duration,
    /// When the current stretch of work began; `None` while waiting.
    since: Option<Instant>,
}

impl Budget {
    /// A budget of `limit`, running from `now`.
    pub(crate) fn start(limit: Duration, now: Instant) -> Self {
        Self {
            limit,
            spent: Duration::ZERO,
            since: Some(now),
        }
    }

    /// Stop counting: the run is waiting on a person. Pausing twice is once.
    pub(crate) fn pause(&mut self, now: Instant) {
        if let Some(since) = self.since.take() {
            self.spent += now.saturating_duration_since(since);
        }
    }

    /// Count again from `now`. Resuming a running budget changes nothing.
    pub(crate) fn resume(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    /// The whole timeout, however much of it is spent.
    pub(crate) fn limit(&self) -> Duration {
        self.limit
    }

    /// What is left of the limit at `now`.
    pub(crate) fn left(&self, now: Instant) -> Duration {
        let running = self
            .since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        self.limit.saturating_sub(self.spent + running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_on_a_person_spends_none_of_the_budget() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut budget = Budget::start(s(100), t0);
        assert_eq!(budget.left(t0 + s(30)), s(70));
        budget.pause(t0 + s(30));
        budget.pause(t0 + s(40));
        assert_eq!(budget.left(t0 + s(500)), s(70));
        budget.resume(t0 + s(500));
        budget.resume(t0 + s(510));
        assert_eq!(budget.left(t0 + s(520)), s(50));
        assert_eq!(budget.left(t0 + s(9999)), Duration::ZERO);
    }
}
