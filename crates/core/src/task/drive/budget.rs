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
    /// `None`: no limit, so nothing to run out of.
    limit: Option<Duration>,
    spent: Duration,
    /// When the current stretch of work began; `None` while waiting.
    since: Option<Instant>,
}

impl Budget {
    /// A budget of `limit`, counting from `now` when `running`, else from
    /// its first [`Self::resume`].
    pub(crate) fn start(limit: Option<Duration>, now: Instant, running: bool) -> Self {
        Self {
            limit,
            spent: Duration::ZERO,
            since: running.then_some(now),
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

    /// Add `by` to the limit; a budget with no limit stays without one.
    pub(crate) fn extend(&mut self, by: Duration) {
        self.limit = self.limit.map(|limit| limit + by);
    }

    /// What has been spent by `now`.
    pub(crate) fn spent(&self, now: Instant) -> Duration {
        let running = self
            .since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        self.spent + running
    }

    /// What is left of the limit at `now`; `None` with no limit.
    pub(crate) fn left(&self, now: Instant) -> Option<Duration> {
        self.limit
            .map(|limit| limit.saturating_sub(self.spent(now)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_on_a_person_spends_none_of_the_budget() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut budget = Budget::start(Some(s(100)), t0, true);
        assert_eq!(budget.left(t0 + s(30)), Some(s(70)));
        budget.pause(t0 + s(30));
        budget.pause(t0 + s(40));
        assert_eq!(budget.left(t0 + s(500)), Some(s(70)));
        budget.resume(t0 + s(500));
        budget.resume(t0 + s(510));
        assert_eq!(budget.left(t0 + s(520)), Some(s(50)));
        assert_eq!(budget.spent(t0 + s(520)), s(50));
        assert_eq!(budget.left(t0 + s(9999)), Some(Duration::ZERO));
    }

    /// A budget with no limit has nothing left to run out of, and still
    /// counts what it spent; one started waiting counts nothing until it
    /// goes on.
    #[test]
    fn no_limit_never_runs_out_and_still_counts() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut budget = Budget::start(None, t0, false);
        assert_eq!(budget.spent(t0 + s(60)), Duration::ZERO);
        budget.resume(t0 + s(60));
        assert_eq!(budget.left(t0 + s(10_000_000)), None);
        assert_eq!(budget.spent(t0 + s(100)), s(40));
    }
}
