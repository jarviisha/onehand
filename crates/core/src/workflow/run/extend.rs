//! A run out of time: it waits for a person, its visit open and its turn
//! left unjudged, until it is extended, going on where it was, or stopped.
//! What it was given more is kept in its file, so a run picked up again
//! keeps it.

use super::{Action, ApprovalAt, Await, Run};
use std::time::Duration;

impl Run {
    /// The run ran out of time during the step's turn: it waits for a person,
    /// its visit open, the turn left unjudged. Only while it waits on a turn.
    pub(crate) fn ran_out(&mut self, now: u64) -> Action {
        if self.awaiting != Await::Turn {
            return Action::Idle;
        }
        self.awaiting = Await::OutOfTime;
        self.log_here("out of time", now);
        Action::AwaitExtend
    }

    /// The run ran out of time and waits for a person.
    pub fn awaiting_extend(&self) -> bool {
        self.awaiting == Await::OutOfTime
    }

    /// What an extend press drawn from the run now names: the run, and the
    /// open visit that ran out. `None` while it has not run out.
    pub fn out_of_time_at(&self) -> Option<ApprovalAt> {
        if self.awaiting != Await::OutOfTime {
            return None;
        }
        let visit = self.visits.last().filter(|v| v.ended_at.is_none())?;
        Some(ApprovalAt {
            run: self.id.clone(),
            visit: visit.id,
        })
    }

    /// A person gave the run that ran out at `at` `by` more: kept, and the
    /// step carries on where it was, unless the run no longer waits there. At
    /// a limited step whose room it no longer `holds_room`, it asks first.
    pub(crate) fn extended(
        &mut self,
        at: &ApprovalAt,
        by: Duration,
        holds_room: bool,
        now: u64,
    ) -> Action {
        if self.out_of_time_at().as_ref() != Some(at) {
            return Action::Idle;
        }
        let label = self.current().map(|s| s.label.clone()).unwrap_or_default();
        let more = crate::workflow::template::said_duration(by);
        self.extended_secs += by.as_secs();
        self.log_here(&format!("extended by {more}"), now);
        let text = format!(
            "You ran out of time at {label}. You have {}. \
             Carry on with the step from where you were.",
            more_said(by)
        );
        // The room it held went when it came to rest: it asks again first.
        if self.limit_at(self.step).is_some() && !holds_room {
            self.extend_text = Some(text);
            return self.wait_for_room();
        }
        self.awaiting = Await::Turn;
        Action::Prompt(text)
    }
}

/// `by`, as the agent is told what it has more: in minutes, or in seconds
/// when it is not whole minutes.
fn more_said(by: Duration) -> String {
    match by.as_secs() {
        60 => "1 more minute".to_string(),
        secs if secs % 60 == 0 => format!("{} more minutes", secs / 60),
        1 => "1 more second".to_string(),
        secs => format!("{secs} more seconds"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_is_said_in_minutes_or_seconds() {
        let s = Duration::from_secs;
        assert_eq!(more_said(s(2700)), "45 more minutes");
        assert_eq!(more_said(s(60)), "1 more minute");
        assert_eq!(more_said(s(90)), "90 more seconds");
        assert_eq!(more_said(s(7200)), "120 more minutes");
    }
}
