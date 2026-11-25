//! A step with a limit: how many runs of the workflow, in one repository,
//! may be at it at once. A run arriving at one that is full waits before it,
//! its last visit closed and none open, until its drive is told it has room.
//! Who has room is not the run's to know: its drive asks, and the tasks
//! decide.

use super::{Action, Await, Run};
use crate::workflow::StepKind;

impl Run {
    /// How many runs may be at the step at `at` at once: its snapshot's.
    pub(crate) fn limit_at(&self, at: usize) -> Option<u32> {
        self.template.limit_at(at)
    }

    /// Whether arriving at the step at `at` waits for room: it is limited,
    /// and the run is not already there with room, which a retry of the same
    /// step keeps.
    pub(super) fn waits_for_room_at(&self, at: usize) -> bool {
        let holds = self.begun() && self.step == at && self.awaiting != Await::Room;
        self.limit_at(at).is_some() && !holds
    }

    /// Wait for room at the step the run is at, with no visit open.
    pub(super) fn wait_for_room(&mut self) -> Action {
        self.awaiting = Await::Room;
        Action::AwaitRoom
    }

    /// The run waits for room at the step it is at.
    pub fn awaiting_room(&self) -> bool {
        self.awaiting == Await::Room
    }

    /// The run was given room at the step it waits before: open the step's
    /// visit and start it, or, after a resume, take it up where it was.
    pub(crate) fn room_given(&mut self, now: u64) -> Action {
        if self.awaiting != Await::Room {
            return Action::Idle;
        }
        self.awaiting = Await::Nothing;
        self.log_here("given room", now);
        self.take_up(now)
    }

    /// Open a visit of the step the run is at and go on with it: an agent or
    /// review step that kept where it started is prompted again from there,
    /// so work done before still counts; any other step starts.
    pub(super) fn take_up(&mut self, now: u64) -> Action {
        if let Some(step) = self.current().map(|step| step.id.clone()) {
            self.open_visit(step, now);
        }
        match (self.current().map(|step| &step.kind), &self.marks.step_from) {
            (Some(StepKind::Agent { .. } | StepKind::Review { .. }), Some(_)) => {
                self.awaiting = Await::Turn;
                Action::Prompt(self.step_prompt())
            }
            (Some(_), _) => self.start_step(now),
            (None, _) => self.finish(super::Outcome::Done, now),
        }
    }
}
