//! A run's lanes: the sessions it works its steps in. An agent step that
//! names its agent works in a session of its own, kept for every visit of
//! that step; every other step shares the run's own. One lane is worked at
//! a time, so what is kept of a session — the prompts sent, the agent seen
//! up, the settings put in force — is kept per lane, and the turn's answer
//! and the clock are the run's.

use super::settings::Picking;
use super::RunDrive;
use crate::workflow::{Run, StepSpec};

/// Which session a step works in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// The run's own session, shared by every step naming no agent.
    Run,
    /// The session of the agent step of this id, which names its agent.
    Step(String),
}

impl Lane {
    /// The lane `step` works in.
    fn of(step: Option<&StepSpec>) -> Self {
        match step {
            Some(step) if step.agent().is_some() => Self::Step(step.id.clone()),
            Some(_) | None => Self::Run,
        }
    }

    /// The lane the step `run` is at works in: the one its first session is.
    pub fn at(run: &Run) -> Self {
        Self::of(run.current())
    }
}

/// What a run keeps of one lane's session.
#[derive(Debug, Default)]
pub(super) struct LaneState {
    /// How many prompts the run has sent the session: any more, and a person
    /// is driving.
    pub(super) sent: usize,
    /// A prompt waiting for the agent to come up.
    pub(super) pending: Option<String>,
    /// The agent has been seen up in this session, and the lane's mode set.
    pub(super) up: bool,
    /// The visit whose settings were last put in force.
    pub(super) settled: Option<u32>,
    /// The open visit's settings being put in force, holding its prompt.
    pub(super) picking: Option<Picking>,
    /// The run's own mode, picked when the agent came up and not yet
    /// answered: a refusal of it is the run's mode refused, not a step's.
    pub(super) mode_out: Option<String>,
    /// The agent parked a card nobody has answered yet.
    pub(super) card: bool,
}

impl RunDrive {
    /// The lane the run works in now: the one whose session its events,
    /// its Stop and its drain are of.
    pub fn current_lane(&self) -> &Lane {
        &self.lane
    }

    /// The lanes that have a session, the current one first: what closing
    /// the run closes.
    pub fn lanes(&self) -> impl Iterator<Item = &Lane> {
        let here = (!self.sessionless).then_some(&self.lane);
        here.into_iter().chain(self.away.keys())
    }
}
