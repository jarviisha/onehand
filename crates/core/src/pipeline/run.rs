//! A pipeline run as pure state: where it is in its template, what it has
//! kept, and what happens next. Nothing here touches a process, a file or a
//! clock beyond reading the time for its history; whoever drives the run
//! does what each [`Action`] says and reports back through the method named
//! for what happened.
//!
//! **One place decides every transition.** A turn ending, a command
//! finishing, an approval, a Stop and a resume each go through a method
//! here, so no two callers can disagree about what a Stop means.

use super::facts::{self, Facts, Mark};
use super::prompt::{self, Fill};
use super::template::{Place, StepKind, StepSpec, Template};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What a run is asked to do: a title, a body, and what the person starting
/// it asked of every step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub title: String,
    pub body: String,
    pub instructions: Option<String>,
}

/// Where a run works and with what, as chosen when it started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    /// The project it was started from.
    pub repo: PathBuf,
    /// Where its work is: the project itself, or the worktree made for it.
    pub dir: PathBuf,
    /// The branch made for it, on a worktree.
    pub branch: Option<String>,
    /// The configured agent it runs, by name; the default one when unset.
    pub agent: Option<String>,
    /// The project's check command, for a command step that names none.
    pub check: Option<String>,
}

/// The points in the work a run measures from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// Where the current agent step's work is measured from.
    pub step_from: Option<Mark>,
    /// The commit a command last passed on.
    pub verified_at: Option<String>,
}

/// One move of a run, for its history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// Seconds past the epoch.
    pub at: u64,
    pub from: String,
    pub to: String,
    pub why: String,
}

/// What the driver of a run does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Read the [`Mark`] of the work and report it to [`PipelineRun::measured`].
    Measure,
    /// Send this prompt to the run's session.
    Prompt(String),
    /// Run this command in the work and report to
    /// [`PipelineRun::command_finished`].
    RunCommand(String),
    /// Wait for a person to approve or revise.
    AwaitApproval,
    /// The run is over.
    Finish(Outcome),
    /// Nothing: the report did not fit what the run was waiting for.
    Idle,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every step passed.
    Done,
    Stopped(Stop),
    /// Turns kept failing their gates, or the command kept failing, at this
    /// step.
    Exhausted {
        step: String,
    },
    Failed(String),
}

impl Outcome {
    /// Whether the run may be picked up again from where it was: its agent
    /// stopped or its session went, neither of which is the run's own
    /// outcome, and app shutdown can look like either.
    pub fn resumable(&self) -> bool {
        match self {
            Self::Stopped(Stop::LinkLost | Stop::Closed) => true,
            Self::Stopped(Stop::ByPerson | Stop::TakenOver | Stop::TimedOut)
            | Self::Done
            | Self::Exhausted { .. }
            | Self::Failed(_) => false,
        }
    }

    /// What happened, as a line for the transcript.
    pub fn said(&self) -> String {
        match self {
            Self::Done => "Pipeline done: every step passed".to_string(),
            Self::Stopped(stop) => format!("Pipeline stopped: {}", stop.said()),
            Self::Exhausted { step } => {
                format!("Pipeline stopped: too many misses at the {step} step")
            }
            Self::Failed(why) => format!("Pipeline stopped: {why}"),
        }
    }
}

/// Why a run stopped before its steps were done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// A person pressed Stop.
    ByPerson,
    /// A person put a prompt of their own into the session.
    TakenOver,
    /// The run outlasted its timeout.
    TimedOut,
    /// The agent stopped answering.
    LinkLost,
    /// The session was closed.
    Closed,
}

impl Stop {
    fn said(self) -> &'static str {
        match self {
            Self::ByPerson => "stopped by hand",
            Self::TakenOver => "taken over by hand",
            Self::TimedOut => "it timed out",
            Self::LinkLost => "the agent stopped",
            Self::Closed => "its session was closed",
        }
    }
}

/// What a run is waiting for, which is what decides whether a report fits.
/// Not kept in its file: a resumed run works it out again from its step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Await {
    #[default]
    Nothing,
    /// A mark, then this prompt: the step's own when `None`, or a carry-on
    /// after a change that may have been a person's.
    Mark(Option<String>),
    Turn,
    Command,
    Approval,
    /// The run ended; one that may be resumed waits to be.
    Over {
        resumable: bool,
    },
}

/// How many transitions a run's history keeps, newest last.
const HISTORY_MAX: usize = 200;

/// A run of a pipeline, from its first step until it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: String,
    /// The template as it was when the run began: a later edit does not
    /// reach a run under way.
    pub template: Template,
    pub brief: Brief,
    pub setup: Setup,
    /// The step the run is at.
    pub step: usize,
    /// Turns that failed a gate, and commands that failed, since the run
    /// last reached a step it had not been at.
    pub misses: u32,
    /// The furthest step reached: going back to an earlier one, after a
    /// failed command or a revision, keeps the misses counting.
    furthest: usize,
    pub marks: Marks,
    /// What each step that keeps its answer answered, by step id.
    pub outputs: BTreeMap<String, String>,
    /// What a person asked to change, until the step it went back to passes.
    pub revise: Option<String>,
    /// What the failed command printed, until the step it went back to passes.
    check_output: Option<String>,
    /// Working time spent, in seconds, for a resumed run's clock.
    pub spent_secs: u64,
    pub history: Vec<Transition>,
    #[serde(skip)]
    awaiting: Await,
}

impl PipelineRun {
    /// A run of `template` on `brief`, at its first step.
    pub fn begin(id: String, template: Template, brief: Brief, setup: Setup) -> (Self, Action) {
        let mut run = Self {
            id,
            template,
            brief,
            setup,
            step: 0,
            misses: 0,
            furthest: 0,
            marks: Marks::default(),
            outputs: BTreeMap::new(),
            revise: None,
            check_output: None,
            spent_secs: 0,
            history: Vec::new(),
            awaiting: Await::Nothing,
        };
        let action = run.enter(0, "started");
        (run, action)
    }

    /// The step the run is at.
    pub fn current(&self) -> Option<&StepSpec> {
        self.template.steps.get(self.step)
    }

    /// A person is asked to approve or revise.
    pub fn awaiting_approval(&self) -> bool {
        self.awaiting == Await::Approval
    }

    /// A turn the run sent is what it waits for.
    pub fn awaiting_turn(&self) -> bool {
        self.awaiting == Await::Turn
    }

    /// The run has ended.
    pub fn over(&self) -> bool {
        matches!(self.awaiting, Await::Over { .. })
    }

    /// The mark asked for: the step's prompt, or the carry-on waiting on it.
    pub fn measured(&mut self, mark: Mark) -> Action {
        let Await::Mark(carry) = &self.awaiting else {
            return Action::Idle;
        };
        let carry = carry.clone();
        let text = match carry {
            None => {
                self.marks.step_from = Some(mark);
                self.step_prompt()
            }
            // Again within a step: what the turn left may be a person's, so
            // the work may be as the step found it or as it is now.
            Some(text) => {
                match &mut self.marks.step_from {
                    Some(from) => {
                        from.head = mark.head;
                        for digest in mark.digests {
                            if !from.digests.contains(&digest) {
                                from.digests.push(digest);
                            }
                        }
                    }
                    None => self.marks.step_from = Some(mark),
                }
                text
            }
        };
        self.awaiting = Await::Turn;
        self.log_here("measured the work");
        Action::Prompt(text)
    }

    /// A turn the run sent ended, leaving `facts`, answering `answer`.
    pub fn turn_ended(&mut self, facts: &Facts, answer: &str) -> Action {
        if self.awaiting != Await::Turn {
            return Action::Idle;
        }
        let (Some(step), Some(from)) = (self.current().cloned(), self.marks.step_from.clone())
        else {
            return Action::Idle;
        };
        let StepKind::Agent {
            gates, keep_answer, ..
        } = &step.kind
        else {
            return Action::Idle;
        };
        let failed = gates
            .iter()
            .copied()
            .find(|gate| !facts::holds(*gate, facts, &from, answer));
        let Some(gate) = failed else {
            if *keep_answer {
                self.outputs
                    .insert(step.id.clone(), answer.trim().to_string());
            }
            self.revise = None;
            self.check_output = None;
            return self.enter(self.step + 1, "its gates held");
        };
        let why = format!("missed: {}", gate.label());
        if let Some(action) = self.miss(&why) {
            return action;
        }
        let place = self.template.place;
        let text = prompt::carry_on(gate, facts, place);
        let theirs = place == Place::Checkout
            && matches!(
                gate,
                super::GateKind::CodeUnchanged | super::GateKind::Uncommitted
            );
        if theirs {
            self.awaiting = Await::Mark(Some(text));
            Action::Measure
        } else {
            self.awaiting = Await::Turn;
            Action::Prompt(text)
        }
    }

    /// The step's command finished: `Ok` with the commit it passed on, or
    /// `Err` with how its output ended.
    pub fn command_finished(&mut self, ran: Result<String, String>) -> Action {
        if self.awaiting != Await::Command {
            return Action::Idle;
        }
        match ran {
            Ok(head) => {
                self.marks.verified_at = Some(head);
                self.check_output = None;
                self.enter(self.step + 1, "the command passed")
            }
            Err(out) => {
                if let Some(action) = self.miss("the command failed") {
                    return action;
                }
                let back = match self.current().map(|step| &step.kind) {
                    Some(StepKind::Command { on_fail, .. }) => self.template.index_of(on_fail),
                    _ => None,
                };
                let Some(back) = back else {
                    return self.finish(Outcome::Failed("the command failed".to_string()));
                };
                self.check_output = Some(out);
                self.enter(back, "the command failed")
            }
        }
    }

    /// A person approved: go on.
    pub fn approved(&mut self) -> Action {
        if self.awaiting != Await::Approval {
            return Action::Idle;
        }
        self.enter(self.step + 1, "approved")
    }

    /// A person sent the answer back with `note`: the step it approves runs
    /// again.
    pub fn revised(&mut self, note: String) -> Action {
        if self.awaiting != Await::Approval {
            return Action::Idle;
        }
        let back = match self.current().map(|step| &step.kind) {
            Some(StepKind::Approval { of }) => self.template.index_of(of),
            _ => None,
        };
        let Some(back) = back else {
            return Action::Idle;
        };
        self.revise = Some(note);
        self.enter(back, "sent back to be revised")
    }

    /// The run is to stop, for `stop`. **Never judges the turn** that was
    /// under way: a turn cut short could otherwise pass as finished work.
    pub fn stopped(&mut self, stop: Stop) -> Action {
        if self.over() {
            return Action::Idle;
        }
        self.finish(Outcome::Stopped(stop))
    }

    /// The driver could not go on, for `why`.
    pub fn failed(&mut self, why: String) -> Action {
        if self.over() {
            return Action::Idle;
        }
        self.finish(Outcome::Failed(why))
    }

    /// Carry on in a new session, after a restart or after its agent or
    /// session went: the step again, from the mark it kept, so work done
    /// before still counts as done in this step. A run that ended on its own
    /// outcome does not come back.
    pub fn resume(&mut self) -> Action {
        if self.awaiting == (Await::Over { resumable: false }) {
            return Action::Idle;
        }
        self.log_here("resumed");
        match (self.current().map(|step| &step.kind), &self.marks.step_from) {
            (Some(StepKind::Agent { .. }), Some(_)) => {
                self.awaiting = Await::Turn;
                Action::Prompt(self.step_prompt())
            }
            (Some(_), _) => self.start_step(),
            (None, _) => self.finish(Outcome::Done),
        }
    }

    /// Go to the step at `at`, or end done past the last.
    fn enter(&mut self, at: usize, why: &str) -> Action {
        let from = self.step_id();
        let Some(to) = self.template.steps.get(at).map(|step| step.id.clone()) else {
            self.log(from, "done".to_string(), why);
            self.awaiting = Await::Over { resumable: false };
            return Action::Finish(Outcome::Done);
        };
        if at > self.furthest {
            self.furthest = at;
            self.misses = 0;
        }
        self.step = at;
        self.marks.step_from = None;
        self.log(from, to, why);
        self.start_step()
    }

    /// What the step at hand starts with.
    fn start_step(&mut self) -> Action {
        match self.current().map(|step| step.kind.clone()) {
            Some(StepKind::Agent { .. }) => {
                self.awaiting = Await::Mark(None);
                Action::Measure
            }
            Some(StepKind::Command { command, .. }) => {
                match command.or_else(|| self.setup.check.clone()) {
                    Some(command) if !command.trim().is_empty() => {
                        self.awaiting = Await::Command;
                        Action::RunCommand(command)
                    }
                    _ => self.finish(Outcome::Failed(
                        "the project has no check command to run".to_string(),
                    )),
                }
            }
            Some(StepKind::Approval { .. }) => {
                self.awaiting = Await::Approval;
                Action::AwaitApproval
            }
            None => self.finish(Outcome::Done),
        }
    }

    /// Count a miss; past the template's allowance, the run is exhausted.
    fn miss(&mut self, why: &str) -> Option<Action> {
        self.misses += 1;
        self.log_here(why);
        if self.misses <= self.template.misses {
            return None;
        }
        let step = self
            .current()
            .map_or_else(String::new, |step| step.label.clone());
        Some(self.finish(Outcome::Exhausted { step }))
    }

    fn finish(&mut self, outcome: Outcome) -> Action {
        let to = match &outcome {
            Outcome::Done => "done",
            Outcome::Stopped(_) => "stopped",
            Outcome::Exhausted { .. } => "exhausted",
            Outcome::Failed(_) => "failed",
        };
        self.log(self.step_id(), to.to_string(), &outcome.said());
        self.awaiting = Await::Over {
            resumable: outcome.resumable(),
        };
        Action::Finish(outcome)
    }

    /// The prompt that starts the agent step at hand.
    fn step_prompt(&self) -> String {
        let Some(StepSpec {
            id,
            kind: StepKind::Agent { prompt, gates, .. },
            ..
        }) = self.current()
        else {
            return String::new();
        };
        let fill = Fill {
            brief: &self.brief,
            outputs: &self.outputs,
            check_output: self.check_output.as_deref(),
            revise: self.revise.as_deref(),
            revised_answer: self
                .revise
                .as_ref()
                .and_then(|_| self.outputs.get(id))
                .map(String::as_str),
        };
        prompt::step_prompt(prompt, gates, self.template.place, &fill)
    }

    /// The step at hand by id, or `start` before the first move.
    fn step_id(&self) -> String {
        match self.current() {
            Some(step) if !self.history.is_empty() => step.id.clone(),
            _ => "start".to_string(),
        }
    }

    fn log_here(&mut self, why: &str) {
        let here = self.step_id();
        self.log(here.clone(), here, why);
    }

    fn log(&mut self, from: String, to: String, why: &str) {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.history.push(Transition {
            at,
            from,
            to,
            why: why.to_string(),
        });
        if self.history.len() > HISTORY_MAX {
            self.history.remove(0);
        }
    }
}
