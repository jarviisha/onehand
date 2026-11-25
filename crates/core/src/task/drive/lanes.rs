//! A run's lanes: the sessions it works its steps in. An agent step that
//! names its agent works in a session of its own, kept for every visit of
//! that step, and so does a review, whether it names one or not, so the
//! work is never judged in the session that wrote it; every other step
//! shares the run's own. One lane is worked at
//! a time, so what is kept of a session — the prompts sent, the agent seen
//! up, the settings put in force — is kept per lane, while the turn's
//! answer, the clock and the misses are the run's.
//!
//! **Only a prompt moves the run to another lane.** A command, an approval
//! or a forge step is done where the run already is. The prompt waits in
//! its lane until that lane's session is open and the run's calls come with
//! it; a lane whose session went while the run was elsewhere opens a new
//! one on its next visit and says so, since what it knew is gone.

use super::settings::Picking;
use super::{Asked, Effect, Input, RunDrive};
use crate::chat::Chat;
use crate::preflight::{found_late, About};
use crate::workflow::{Run, Settings, StepSpec, Stop};
use crate::Now;

/// Which session a step works in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// The run's own session, shared by every step naming no agent.
    Run,
    /// The session of the step of this id: an agent step naming its agent,
    /// or a review.
    Step(String),
}

impl Lane {
    /// The lane `step` works in.
    fn of(step: Option<&StepSpec>) -> Self {
        match step {
            Some(step) if step.agent().is_some() || step.is_review() => Self::Step(step.id.clone()),
            Some(_) | None => Self::Run,
        }
    }

    /// The lane the step `run` is at works in: the one its first session is.
    pub fn at(run: &Run) -> Self {
        Self::of(run.current())
    }

    /// What the lane's session is called: the task's title for the run's
    /// own, and the title with the step's label for a step's.
    pub(super) fn name(&self, run: &Run) -> String {
        let title = &run.brief.title;
        match self {
            Self::Run => title.clone(),
            Self::Step(id) => match run.template.steps.iter().find(|step| &step.id == id) {
                Some(step) => format!("{title} · {}", step.label),
                None => title.clone(),
            },
        }
    }

    /// The agent the lane's session runs: the step's, or the run's, which is
    /// empty when its setup names none.
    pub fn agent(&self, run: &Run) -> String {
        let step = match self {
            Self::Run => None,
            Self::Step(id) => run.template.steps.iter().find(|step| &step.id == id),
        };
        step.and_then(StepSpec::agent)
            .map(str::to_string)
            .or_else(|| run.setup.agent.clone())
            .unwrap_or_default()
    }
}

/// What a run keeps of one lane's session.
#[derive(Debug, Default)]
pub(super) struct LaneState {
    /// The lane has a session open.
    pub(super) session: bool,
    /// The lane's session went while the run was elsewhere: the next one
    /// opened says so.
    lost: bool,
    /// How many prompts the run has sent the session: any more, and a person
    /// is driving.
    pub(super) sent: usize,
    /// A prompt waiting for the agent to come up, or for the lane's session.
    pub(super) pending: Option<String>,
    /// The agent has been seen up in this session, and the lane's mode set.
    pub(super) up: bool,
    /// What a step's lane had in force when its agent came up, which a step
    /// not naming a setting goes back to. The run's own lane keeps its in
    /// the run, so a Resume finds it; a step's is read again from its new
    /// session.
    pub(super) baseline: Option<Settings>,
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

impl LaneState {
    /// A lane starting on a session, or on none.
    pub(super) fn new(session: bool) -> Self {
        Self {
            session,
            ..Self::default()
        }
    }
}

impl RunDrive {
    /// The lane the run works in now: the one whose session its events,
    /// its Stop and its drain are of.
    pub fn current_lane(&self) -> &Lane {
        &self.lane
    }

    /// The lanes that have a session, the current one first.
    #[cfg(test)]
    pub(crate) fn lanes(&self) -> impl Iterator<Item = &Lane> {
        let here = self.here.session.then_some(&self.lane);
        let away = self.away.iter().filter(|(_, state)| state.session);
        here.into_iter().chain(away.map(|(lane, _)| lane))
    }

    /// Whether [`Input::LaneAppended`] could move the run, read without
    /// moving it: a person prompted `lane`'s session, `chat`, which is not
    /// the current one's.
    pub(crate) fn heeds_lane_appended(&self, lane: &Lane, chat: &Chat) -> bool {
        self.away
            .get(lane)
            .is_some_and(|state| state.session && chat.prompted_beyond(state.sent))
    }

    /// The step's prompt `text`, given back when it goes in the current
    /// lane. Otherwise its lane becomes the current one and holds it, and
    /// the lane's session is asked for, or, open already, told of once this
    /// call is over, when the calls come with it.
    pub(super) fn enter_lane(&mut self, text: String, fx: &mut Vec<Effect>) -> Option<String> {
        let lane = Lane::at(&self.run);
        if lane == self.lane {
            return Some(text);
        }
        let state = self.away.remove(&lane).unwrap_or_default();
        let left = std::mem::replace(&mut self.here, state);
        let was = std::mem::replace(&mut self.lane, lane.clone());
        self.away.insert(was, left);
        self.here.pending = Some(text);
        match self.here.session {
            true => fx.push(Effect::Defer(Input::LaneOpened {
                lane,
                result: Ok(()),
            })),
            false => fx.push(Effect::OpenSession {
                agent: lane.agent(&self.run),
                name: lane.name(&self.run),
                lane,
            }),
        }
        None
    }

    /// `lane`'s session is open, with `chat`, or did not start: its prompt
    /// goes, or the run fails at the step as the preflight would have
    /// stopped it. A lane the run has left since is not its to hear of.
    pub(super) fn opened(
        &mut self,
        lane: Lane,
        result: Result<(), String>,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        if lane != self.lane {
            return;
        }
        let label = self
            .run
            .current()
            .map(|step| step.label.clone())
            .unwrap_or_default();
        if result.is_err() {
            let why = match lane.agent(&self.run) {
                agent if agent.is_empty() => format!("The agent of {label} did not start."),
                agent => format!("The agent of {label}, {agent}, did not start."),
            };
            return self.advance(chat, now, fx, move |run| {
                run.failed(why, found_late(About::Agent), now.secs)
            });
        }
        let Some(chat) = chat else {
            return;
        };
        if !std::mem::replace(&mut self.here.session, true) {
            self.asked.push(Asked::Name(lane.name(&self.run)));
        }
        // The step was said in the lane the run left; say it where it works.
        if self.here.pending.is_some() {
            self.asked.extend(self.step_said().map(Asked::Note));
        }
        if std::mem::take(&mut self.here.lost) {
            self.asked.push(Asked::Note(format!(
                "The session of {label} was gone; a new one was opened."
            )));
        }
        if let Some(text) = self.here.pending.take() {
            self.send(text, chat, now, fx);
        }
    }

    /// Something landed in `lane`'s session, `chat`: the current lane's is
    /// heard as ever, and a person's prompt in another's takes the run over.
    pub(super) fn appended_in(
        &mut self,
        lane: Lane,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        if lane == self.lane {
            return self.on_event(Input::Appended, chat, now, fx);
        }
        if let Some(chat) = chat.filter(|chat| self.heeds_lane_appended(&lane, chat)) {
            self.end(Stop::TakenOver, Some(chat), now, fx);
        }
    }

    /// `lane`'s session went. Another lane's is forgotten, and a new one
    /// opened on its next visit; the current lane's ends the run as its
    /// session closing does.
    pub(super) fn gone(&mut self, lane: Lane, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        if lane == self.lane {
            if self.here.session {
                self.end(Stop::Closed, chat, now, fx);
            }
            return;
        }
        if let Some(state) = self.away.get_mut(&lane).filter(|state| state.session) {
            *state = LaneState {
                lost: true,
                ..LaneState::default()
            };
        }
    }
}
