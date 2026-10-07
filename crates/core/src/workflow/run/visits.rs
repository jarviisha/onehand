//! A run's step visits, and the marks pinned at their boundaries.

use super::{now, CommandResult, Run, Visit};

impl Run {
    /// Every stay at a step, oldest first.
    pub fn visits(&self) -> &[Visit] {
        &self.visits
    }

    /// The last mark pinned of the work: where the run's last visit left
    /// it, or where that visit found it when the run was cut off before the
    /// visit ended.
    pub fn last_mark(&self) -> Option<&str> {
        let visit = self.visits.last()?;
        visit.end.as_deref().or(visit.start.as_deref())
    }

    /// Every point a mark is pinned at, in order: each visit's start, then
    /// its end once it has one. Only ever grows at its end, so a count of
    /// how many were pinned stays true.
    pub fn boundaries(&self) -> Vec<(u32, bool)> {
        self.visits
            .iter()
            .flat_map(|visit| {
                std::iter::once((visit.id, false)).chain(visit.ended_at.map(|_| (visit.id, true)))
            })
            .collect()
    }

    /// How many boundaries count as pinned: every one up to the last whose
    /// mark landed. One after it, a mark that failed or that the app quit
    /// before it landed, is pinned again by the next driver.
    pub fn pinned_count(&self) -> usize {
        let landed: Vec<bool> = self
            .visits
            .iter()
            .flat_map(|visit| {
                std::iter::once(visit.start.is_some())
                    .chain(visit.ended_at.map(|_| visit.end.is_some()))
            })
            .collect();
        landed.iter().rposition(|&at| at).map_or(0, |at| at + 1)
    }

    /// `commit` is the work at every boundary from the `from`th on: one
    /// commit serves a visit's end and the next one's start.
    pub fn pinned(&mut self, commit: &str, from: usize) {
        let mut at = 0;
        for visit in &mut self.visits {
            for end in [false, true] {
                if end && visit.ended_at.is_none() {
                    continue;
                }
                if at >= from {
                    let slot = if end {
                        &mut visit.end
                    } else {
                        &mut visit.start
                    };
                    *slot = Some(commit.to_string());
                }
                at += 1;
            }
        }
    }

    /// Start a visit of `step`.
    pub(super) fn open_visit(&mut self, step: String) {
        let id = self.visits.last().map_or(1, |visit| visit.id + 1);
        self.visits.push(Visit {
            id,
            step,
            started_at: now(),
            ended_at: None,
            start: None,
            end: None,
            output: None,
            why: None,
            command: None,
        });
    }

    /// End the open visit, if there is one, for `why`.
    pub(super) fn close_visit(&mut self, why: &str) {
        if let Some(visit) = self.visits.last_mut().filter(|v| v.ended_at.is_none()) {
            visit.ended_at = Some(now());
            visit.why = Some(why.to_string());
        }
    }

    /// How the open visit's command came out.
    pub(super) fn visit_command(&mut self, ran: CommandResult) {
        if let Some(visit) = self.visits.last_mut().filter(|v| v.ended_at.is_none()) {
            visit.command = Some(ran);
        }
    }

    /// What the open visit answered or printed.
    pub(super) fn visit_output(&mut self, output: String) {
        if let Some(visit) = self.visits.last_mut().filter(|v| v.ended_at.is_none()) {
            visit.output = Some(output);
        }
    }
}
