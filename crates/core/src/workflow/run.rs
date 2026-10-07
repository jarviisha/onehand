//! A run as pure state: where it is in its template, what it has
//! kept, and what happens next. Nothing here touches a process, a file or a
//! clock beyond reading the time for its history; whoever drives the run
//! does what each [`Action`] says and reports back through the method named
//! for what happened.
//!
//! **One place decides every transition.** A turn ending, a command
//! finishing, an approval, a Stop and a resume each go through a method
//! here, so no two callers can disagree about what a Stop means.

use super::facts::{self, CommandResult, Facts, Mark};
use super::prompt::{self, Fill};
use super::status_checks::Seen;
use super::template::{Place, StepKind, StepSpec, Template};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

mod retry;
mod visits;

pub use retry::{Changed, Field, Now, StartWhy, WithCurrent};

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
    /// The session mode the agent is put in before the first prompt, by the
    /// adapter's own id; left as the agent starts when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The connector the branch goes to, by name; `None` where no forge
    /// serves the project, and then a step on the forge passes at once, the
    /// branch being the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<String>,
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
    /// Read the [`Mark`] of the work and report it to [`Run::measured`].
    Measure,
    /// Send this prompt to the run's session.
    Prompt(String),
    /// Run this command in the work and report to
    /// [`Run::command_finished`].
    RunCommand(String),
    /// Wait for a person to approve or revise.
    AwaitApproval,
    /// Push this commit as the run's branch and report to
    /// [`Run::forge_done`].
    Push(String),
    /// Open a draft pull request from the run's branch, or take the one open
    /// there, and report to [`Run::forge_done`].
    OpenPullRequest,
    /// Watch the pull request's status checks on the commit `pushed`, at
    /// most `wait`, and report each look to [`Run::status_checks_seen`].
    AwaitStatusChecks {
        wait: std::time::Duration,
        pushed: Option<String>,
    },
    /// The run is over.
    Finish(Outcome),
    /// Nothing: the report did not fit what the run was waiting for.
    Idle,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Whether a person should look at how the run ended: it did not get
    /// there, and nobody chose that.
    pub fn needs_attention(&self) -> bool {
        match self {
            Self::Exhausted { .. }
            | Self::Failed(_)
            | Self::Stopped(Stop::TimedOut | Stop::LinkLost | Stop::Closed) => true,
            Self::Done | Self::Stopped(Stop::ByPerson | Stop::TakenOver) => false,
        }
    }

    /// What happened, as a line for the transcript.
    pub fn said(&self) -> String {
        match self {
            Self::Done => "Workflow done: every step passed".to_string(),
            Self::Stopped(stop) => format!("Workflow stopped: {}", stop.said()),
            Self::Exhausted { step } => {
                format!("Workflow stopped: too many misses at the {step} step")
            }
            Self::Failed(why) => format!("Workflow stopped: {why}"),
        }
    }
}

/// What kind of thing a failed run failed on, which decides the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    /// What its preflight would have blocked, found once it ran: the agent
    /// no longer configured, a mode not offered, no check command, a workflow
    /// that no longer validates. Changing the configuration is the way out.
    Configuration,
    /// A forge step, or the forge it asked, failed.
    Forge,
    Other,
}

/// The pull request a run's pull request step opened, or took up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrOpened {
    pub number: u64,
    pub url: String,
}

/// Why a run stopped before its steps were done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub(crate) fn said(self) -> &'static str {
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
    Forge,
    StatusChecks,
}

/// One stay at a step: going back to a step is a new visit, never a rewrite
/// of the last one, so a step visited twice keeps both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    /// Its number in the run, from 1.
    pub id: u32,
    /// The step's id.
    pub step: String,
    /// Seconds past the epoch.
    pub started_at: u64,
    /// `None` while the visit is the run's open one.
    pub ended_at: Option<u64>,
    /// The commit pinned for the work as the visit found it, and as it left
    /// it; `None` until pinned, or when pinning failed.
    pub start: Option<String>,
    pub end: Option<String>,
    /// The answer it kept, or how its failed command's output ended.
    pub output: Option<String>,
    /// Why it ended, as its history says it.
    pub why: Option<String>,
    /// How its command came out, on a command step's visit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandResult>,
}

/// What a person's approval or revision was drawn from: the run, and the
/// approval step's visit they read. Visit ids count from 1 in every run, so
/// the run is named too: a press from a view of a run a retry replaced names
/// a visit of the same number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAt {
    pub run: String,
    pub visit: u32,
}

/// Why a command step's visit ended when its command passed, and when it
/// failed: what tells, in a run from before a visit kept its command's
/// result, a command that came out from one cut off while it ran.
pub(crate) const COMMAND_PASSED: &str = "the command passed";
pub(crate) const COMMAND_FAILED: &str = "the command failed";

/// Why a forge step's visit ended when the forge did what it asked: on a
/// pull request step, what tells a run from before the pull request was
/// kept that it opened one.
pub(crate) const FORGE_DONE: &str = "done on the forge";

/// How many transitions a run's history keeps, newest last.
const HISTORY_MAX: usize = 200;

/// A run of a workflow, from its first step until it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
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
    /// Every stay at a step, oldest first. Not capped: each one ends on a
    /// miss or a person, and the answers they keep are capped already.
    #[serde(default)]
    pub(crate) visits: Vec<Visit>,
    /// How the run ended; `None` while it has not, which after a restart
    /// means it was cut off.
    #[serde(default)]
    pub outcome: Option<Outcome>,
    /// What a failed run failed on. Beside the outcome rather than in it, so
    /// a file from before it reads; absent reads as [`Failure::Other`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// The pull request its pull request step opened or took up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PrOpened>,
    #[serde(skip)]
    awaiting: Await,
}

impl Run {
    /// A run of `template` on `brief`, not started: [`Run::resume`] starts
    /// it, so a run that waited for its place and one picked up again take
    /// the same way in.
    pub(crate) fn new(id: String, template: Template, brief: Brief, setup: Setup) -> Self {
        Self {
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
            visits: Vec::new(),
            outcome: None,
            failure: None,
            pull_request: None,
            awaiting: Await::Nothing,
        }
    }

    /// What the run failed on, once it ended failed; a run from before the
    /// kind was kept failed on something other.
    pub(crate) fn failed_on(&self) -> Option<Failure> {
        match self.outcome {
            Some(Outcome::Failed(_)) => Some(self.failure.unwrap_or(Failure::Other)),
            Some(Outcome::Done | Outcome::Stopped(_) | Outcome::Exhausted { .. }) | None => None,
        }
    }

    /// The step the run is at.
    pub fn current(&self) -> Option<&StepSpec> {
        self.template.steps.get(self.step)
    }

    /// A person is asked to approve or revise.
    pub fn awaiting_approval(&self) -> bool {
        self.awaiting == Await::Approval
    }

    /// What a person is asked to approve, while they are: the step that
    /// answered, and the answer it kept. Kept in the run, not read off a
    /// transcript, so a run resumed in a new session still shows it.
    pub fn under_review(&self) -> Option<(&StepSpec, &str)> {
        if !self.awaiting_approval() {
            return None;
        }
        let Some(StepKind::Approval { of }) = self.current().map(|step| &step.kind) else {
            return None;
        };
        let step = self.template.steps.iter().find(|step| &step.id == of)?;
        Some((step, self.outputs.get(of).map_or("", String::as_str)))
    }

    /// The pull request's status checks are what it waits for.
    pub fn awaiting_status_checks(&self) -> bool {
        self.awaiting == Await::StatusChecks
    }

    /// A turn the run sent is what it waits for.
    pub fn awaiting_turn(&self) -> bool {
        self.awaiting == Await::Turn
    }

    /// The run has taken a step: it was started, not only made.
    pub fn begun(&self) -> bool {
        !self.history.is_empty()
    }

    /// The run has ended.
    pub fn over(&self) -> bool {
        self.outcome.is_some()
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
                let answer = answer.trim().to_string();
                self.visit_output(answer.clone());
                self.outputs.insert(step.id.clone(), answer);
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

    /// The step's command finished as `ran` says, kept on its visit.
    /// Whether it passed is its exit status alone.
    pub fn command_finished(&mut self, ran: CommandResult) -> Action {
        if self.awaiting != Await::Command {
            return Action::Idle;
        }
        self.visit_command(ran.clone());
        match ran.passed {
            true => {
                self.marks.verified_at = ran.commit;
                self.check_output = None;
                self.enter(self.step + 1, COMMAND_PASSED)
            }
            false => self.send_back(ran.tail, COMMAND_FAILED),
        }
    }

    /// The push or the pull request the step asked for is done, with the
    /// pull request it opened or took up, or failed for the reason given. A
    /// failure ends the run: a retry starts at the step again.
    pub fn forge_done(&mut self, done: Result<Option<PrOpened>, String>) -> Action {
        if self.awaiting != Await::Forge {
            return Action::Idle;
        }
        match done {
            Ok(opened) => {
                if opened.is_some() {
                    self.pull_request = opened;
                }
                self.enter(self.step + 1, FORGE_DONE)
            }
            Err(why) => self.fail(why, Failure::Forge),
        }
    }

    /// What a look at the pull request's status checks found.
    pub fn status_checks_seen(&mut self, seen: Seen) -> Action {
        if self.awaiting != Await::StatusChecks {
            return Action::Idle;
        }
        match seen {
            Seen::Pending => Action::Idle,
            Seen::Passed => {
                self.check_output = None;
                self.enter(self.step + 1, "its status checks passed")
            }
            // Past every step: whatever was left is moot once it landed.
            Seen::Merged => self.enter(self.template.steps.len(), "its pull request was merged"),
            Seen::Fail(why) => self.fail(why, Failure::Forge),
            Seen::Repair(out) => self.send_back(out, "its status checks failed"),
        }
    }

    /// What the step checked failed, printing `out`: a miss, and back to the
    /// step it sends back to, carrying `out`.
    fn send_back(&mut self, out: String, why: &str) -> Action {
        self.visit_output(out.clone());
        if let Some(action) = self.miss(why) {
            return action;
        }
        let back = self
            .current()
            .and_then(|step| step.kind.sends_back_to())
            .and_then(|id| self.template.index_of(id));
        let Some(back) = back else {
            return self.fail(why.to_string(), Failure::Other);
        };
        self.check_output = Some(out);
        self.enter(back, why)
    }

    /// What an approval press drawn from the run now names: the run, and the
    /// approval step's open visit. `None` while nothing waits for approval.
    pub fn approval_at(&self) -> Option<ApprovalAt> {
        if self.awaiting != Await::Approval {
            return None;
        }
        let visit = self.visits.last().filter(|v| v.ended_at.is_none())?;
        Some(ApprovalAt {
            run: self.id.clone(),
            visit: visit.id,
        })
    }

    /// A person approved what they read at `at`: go on, unless the run no
    /// longer waits there.
    pub fn approved(&mut self, at: &ApprovalAt) -> Action {
        if self.approval_at().as_ref() != Some(at) {
            return Action::Idle;
        }
        self.enter(self.step + 1, "approved")
    }

    /// A person sent the answer they read at `at` back with `note`: the step
    /// it approves runs again, unless the run no longer waits there.
    pub fn revised(&mut self, at: &ApprovalAt, note: String) -> Action {
        if self.approval_at().as_ref() != Some(at) {
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

    /// The driver could not go on, for `why`, which was of `kind`.
    pub fn failed(&mut self, why: String, kind: Failure) -> Action {
        if self.over() {
            return Action::Idle;
        }
        self.fail(why, kind)
    }

    /// End failed, for `why`, of `kind`.
    fn fail(&mut self, why: String, kind: Failure) -> Action {
        self.failure = Some(kind);
        self.finish(Outcome::Failed(why))
    }

    /// Carry on in a new session, after a restart or after its agent or
    /// session went: the step again, from the mark it kept, so work done
    /// before still counts as done in this step. A run that ended on its own
    /// outcome does not come back.
    ///
    /// A run never started starts here.
    pub fn resume(&mut self) -> Action {
        if self.outcome.as_ref().is_some_and(|o| !o.resumable()) {
            return Action::Idle;
        }
        self.outcome = None;
        self.failure = None;
        if !self.begun() {
            return self.enter(self.step, "started");
        }
        self.close_visit("interrupted");
        if let Some(step) = self.current().map(|step| step.id.clone()) {
            self.open_visit(step);
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
            self.close_visit(&Outcome::Done.said());
            self.log(from, "done".to_string(), why);
            self.awaiting = Await::Nothing;
            self.outcome = Some(Outcome::Done);
            return Action::Finish(Outcome::Done);
        };
        self.close_visit(why);
        self.open_visit(to.clone());
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
                    _ => self.fail(
                        "the project has no check command to run".to_string(),
                        Failure::Configuration,
                    ),
                }
            }
            Some(StepKind::Approval { .. }) => {
                self.awaiting = Await::Approval;
                Action::AwaitApproval
            }
            Some(kind) if kind.on_forge() && self.setup.forge.is_none() => self.enter(
                self.step + 1,
                "no forge serves it: the branch is the result",
            ),
            Some(StepKind::Push) => match self.marks.verified_at.clone() {
                Some(commit) => {
                    self.awaiting = Await::Forge;
                    Action::Push(commit)
                }
                None => self.fail(
                    "no command has passed on a commit to push".to_string(),
                    Failure::Other,
                ),
            },
            Some(StepKind::PullRequest) => {
                self.awaiting = Await::Forge;
                Action::OpenPullRequest
            }
            Some(StepKind::StatusChecks { wait, .. }) => {
                self.awaiting = Await::StatusChecks;
                Action::AwaitStatusChecks {
                    wait: crate::unattended::parse_every(&wait)
                        .or_else(|| crate::unattended::parse_every(super::template::DEFAULT_WAIT))
                        .unwrap_or_default(),
                    pushed: self.marks.verified_at.clone(),
                }
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
        self.close_visit(&outcome.said());
        self.awaiting = Await::Nothing;
        self.outcome = Some(outcome.clone());
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
            Some(step) if self.begun() => step.id.clone(),
            _ => "start".to_string(),
        }
    }

    fn log_here(&mut self, why: &str) {
        let here = self.step_id();
        self.log(here.clone(), here, why);
    }

    fn log(&mut self, from: String, to: String, why: &str) {
        self.history.push(Transition {
            at: now(),
            from,
            to,
            why: why.to_string(),
        });
        if self.history.len() > HISTORY_MAX {
            self.history.remove(0);
        }
    }
}

/// Seconds past the epoch.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
