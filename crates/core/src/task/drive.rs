//! What the one driver of every run decides: it watches the run's session,
//! does what the run's engine asks, and reports back what happened.
//!
//! **Decided here, carried out elsewhere.** A [`Drive`] is told what
//! happened ([`Input`]) with the run's session as it stands, and answers with
//! what to do ([`Effect`]): pin a mark, send a prompt, run a command, call the
//! forge, wait, say something, save, finish. It has no timer, no thread and
//! no I/O of its own, so every rule below is a rule a test can hold it to; the
//! front end does the work and brings back what came of it.
//!
//! **The mode and the settings picks are made on the session itself**, here,
//! as they are decided: the next pick is planned against what the session
//! says is still waiting, and a pick that has not been made yet would read as
//! one already answered.
//!
//! **Every Stop goes through the engine the same way.** A person's Stop, a
//! prompt of their own, the timeout, the agent going and the session closing
//! each end the run with [`Run::stopped`], which never judges the turn that
//! was under way — a cut-short turn passing as finished work is the mistake
//! two drivers disagreeing made before.

use crate::chat::{Chat, Link};
use crate::preflight::{found_late, mode_refused, About};
use crate::task::marks;
use crate::unattended::Budget;
use crate::workflow::{Action, ApprovalAt, Failure, Outcome, Run, Stop};
use std::time::{Duration, Instant};

mod settings;
mod signals;

pub use signals::{Effect, Input};

/// The longest answer kept from a turn. An answer the length of a book is cut
/// rather than carried whole into the run's file and the next prompt.
const ANSWER_MAX: usize = 20_000;

/// A call held until a mark is pinned.
#[derive(Debug, Clone)]
enum Held {
    Answer(Answer),
    Cut(Stop),
    End(Stop),
}

/// A person's answer to an approval.
#[derive(Debug, Clone)]
enum Answer {
    Approved(ApprovalAt),
    Revised(ApprovalAt, String),
}

/// A run under way on one session.
pub struct Drive {
    /// The task the run is of.
    task: String,
    run: Run,
    /// The run is of an issue: a setting it fails on pauses unattended runs.
    issue: bool,
    /// How many of the run's boundaries have had their mark pinned, or tried.
    pinned: usize,
    /// `Some` while a mark is being pinned and the action the engine asked
    /// for waits on it, holding what a person or an event asked meanwhile.
    /// Done at once, an approval or a Stop would move the run on under the
    /// waiting action, which would then be carried out stale; held, each
    /// runs once that action has. An ending held runs instead of the action,
    /// so a prompt is never sent to a run already said to be over. A turn
    /// ending meanwhile is not the one the action will send.
    held: Option<Vec<(bool, Held)>>,
    /// The action waiting on the pin, with the boundaries it pins from and to.
    waiting: Option<(Action, usize, usize)>,
    /// How many prompts the run has sent its session: any more, and a person
    /// is driving.
    sent: usize,
    /// How many transcript items there were when the run's last prompt went:
    /// the turn's answer is what the agent said after that.
    answer_from: usize,
    /// When the run's last prompt went: background work started before it is
    /// not the step's to wait for.
    sent_at: Option<Instant>,
    /// The turn's answer, kept while its work is read.
    answer: Option<String>,
    /// A prompt waiting for the agent to come up.
    pending: Option<String>,
    /// The agent has been seen up in this session, and the run's mode set.
    up: bool,
    /// The visit whose settings were last put in force.
    settled: Option<u32>,
    /// The open visit's settings being put in force, holding its prompt.
    picking: Option<settings::Picking>,
    /// The run's own mode, picked when the agent came up and not yet
    /// answered: a refusal of it is the run's mode refused, not a step's.
    mode_out: Option<String>,
    /// The step the transcript was last told of.
    noted: Option<usize>,
    /// How much of the timeout is left; it does not run while a card or an
    /// approval waits on a person.
    budget: Budget,
    /// Working time spent before this session took the run up.
    spent_before: u64,
    /// The agent parked a card nobody has answered yet.
    card: bool,
    /// The turn sent ended with the agent still at work beside it: the step
    /// is judged once that work, and the agent's answer to it, are done.
    after_work: bool,
    /// The step's command, or a forge call, is running.
    command: bool,
    /// How the run ends once its called-off command has exited.
    stopping: Option<Stop>,
    /// The run has finished: nothing more is done.
    over: bool,
    /// The run never had a session: a check, which only runs a command.
    /// It has no timeout, and what needs no session is still done.
    sessionless: bool,
}

impl Drive {
    /// Drive `run`, the last run of `task`, starting with `first`: what
    /// [`Run::resume`] said to do. `pinned` boundaries of it had their mark
    /// pinned before; `issue` says it is an issue's run.
    pub fn start(
        task: String,
        run: Run,
        first: Action,
        pinned: usize,
        issue: bool,
        now: Instant,
        chat: Option<&mut Chat>,
    ) -> (Self, Vec<Effect>) {
        let limit = run
            .template
            .timeout_limit()
            .saturating_sub(Duration::from_secs(run.spent_secs));
        let mut drive = Self {
            task,
            spent_before: run.spent_secs,
            run,
            issue,
            pinned,
            held: None,
            waiting: None,
            sent: 0,
            answer_from: 0,
            sent_at: None,
            answer: None,
            pending: None,
            up: false,
            settled: None,
            picking: None,
            mode_out: None,
            noted: None,
            budget: Budget::start(limit, now),
            card: false,
            after_work: false,
            command: false,
            stopping: None,
            over: false,
            sessionless: chat.is_none(),
        };
        let mut fx = Vec::new();
        if !drive.sessionless {
            fx.push(Effect::ArmClock(limit));
        }
        drive.act(first, chat, now, &mut fx);
        (drive, fx)
    }

    /// The task the run is of.
    pub fn task(&self) -> &str {
        &self.task
    }

    /// The run as it stands; what [`Effect::Save`] and [`Effect::Finish`]
    /// keep.
    pub fn run(&self) -> &Run {
        &self.run
    }

    /// How many of the run's boundaries have had their mark pinned, or tried.
    pub fn pinned(&self) -> usize {
        self.pinned
    }

    /// The run waits on a person: an approval, or a card the agent parked.
    pub fn waits_on_person(&self) -> bool {
        self.run.awaiting_approval() || self.card
    }

    /// The agent parked a card nobody has answered yet.
    pub fn card(&self) -> bool {
        self.card
    }

    /// Whether [`Input::Notified`] could move the run, read without moving
    /// it: a card may have been answered, or the work the step's turn left
    /// running has ended.
    pub fn heeds_notified(&self, chat: &Chat) -> bool {
        self.card || (self.after_work && !self.step_at_work(chat))
    }

    /// Whether the agent still works on the step: background work started
    /// since its prompt, a subagent, or its own answer to them. A server an
    /// earlier step left running is not this step's to wait for.
    fn step_at_work(&self, chat: &Chat) -> bool {
        chat.running_since(self.sent_at)
    }

    /// Stop was asked for, and waits on a command or forge call to land.
    pub fn stopping(&self) -> bool {
        self.stopping.is_some()
    }

    /// Whether [`Input::Appended`] could move the run, read without moving
    /// it: a prompt held until the agent came up, a prompt a person sent, or
    /// picks under way. Anything else the transcript grows by is the reply
    /// streaming in, which a front end should not hand over chunk by chunk.
    pub fn heeds_appended(&self, chat: &Chat) -> bool {
        (self.pending.is_some() && chat.link == Link::Connected)
            || chat.prompted_beyond(self.sent)
            || self.picking.is_some()
    }

    /// Working time spent on the run, sessions before this one included.
    pub fn spent_secs(&self, now: Instant) -> u64 {
        let used = self.budget.limit().saturating_sub(self.budget.left(now));
        self.spent_before + used.as_secs()
    }

    /// Something happened: what to do about it. A run that has finished does
    /// nothing more.
    pub fn on(&mut self, input: Input, chat: Option<&mut Chat>, now: Instant) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.over {
            return fx;
        }
        match input {
            Input::Appended
            | Input::TurnEnded
            | Input::AwaitingUser
            | Input::Disconnected
            | Input::PickRefused(_) => self.on_event(input, chat, now, &mut fx),
            Input::Notified => {
                if self.card
                    && chat
                        .as_deref()
                        .is_some_and(|chat| !chat.awaiting_permission())
                {
                    self.card = false;
                    self.budget.resume(now);
                    fx.push(Effect::Answered);
                }
                if let Some(chat) = chat.filter(|chat| self.after_work && !self.step_at_work(chat))
                {
                    self.after_work = false;
                    if self.run.awaiting_turn() && self.held.is_none() {
                        self.after_turn(chat, &mut fx);
                    }
                }
            }
            Input::SessionGone => self.end(Stop::Closed, chat, now, &mut fx),
            Input::PicksAnswered => {
                if let Some(chat) = chat {
                    settings::answered(self, chat, now, &mut fx);
                }
            }
            Input::Approved { at } => {
                self.answer(Answer::Approved(at), chat, now, &mut fx);
                fx.push(Effect::Answered);
            }
            Input::Revised { at, note } => {
                self.answer(Answer::Revised(at, note), chat, now, &mut fx);
                fx.push(Effect::Answered);
            }
            Input::Stop => self.cut(Stop::ByPerson, chat, now, &mut fx),
            Input::Pinned(pinned) => self.landed(pinned, chat, now, &mut fx),
            Input::Measured(Ok(mark)) => {
                self.advance(chat, now, &mut fx, move |run| run.measured(mark))
            }
            Input::Measured(Err(why)) | Input::TurnRead(Err(why)) => {
                self.advance(chat, now, &mut fx, move |run| {
                    run.failed(format!("the work could not be read: {why}"), Failure::Other)
                })
            }
            Input::TurnRead(Ok(facts)) => {
                let answer = self.answer.take().unwrap_or_default();
                self.advance(chat, now, &mut fx, move |run| {
                    run.turn_ended(&facts, &answer)
                })
            }
            Input::NotTaken => self.advance(chat, now, &mut fx, |run| {
                run.failed(
                    "the agent did not take the prompt".to_string(),
                    Failure::Other,
                )
            }),
            Input::CommandExited(ran) => {
                self.command = false;
                // Called off: the command has exited by now, so the run can end.
                if let Some(stop) = self.stopping.take() {
                    return self.ended(stop, chat, now, fx);
                }
                if chat.is_some() {
                    let said = match ran.passed {
                        true => "The command passed",
                        false => "The command failed",
                    };
                    fx.push(Effect::Note(said.to_string()));
                }
                self.advance(chat, now, &mut fx, move |run| run.command_finished(ran));
            }
            Input::ForgeDone(done) => {
                self.command = false;
                if let Some(stop) = self.stopping.take() {
                    return self.ended(stop, chat, now, fx);
                }
                self.advance(chat, now, &mut fx, move |run| run.forge_done(done));
            }
            Input::ChecksSeen(seen) => {
                self.budget.resume(now);
                self.advance(chat, now, &mut fx, move |run| run.status_checks_seen(seen));
            }
            // The run's timeout: when it has run out, look again, since time
            // spent waiting on a person did not count; end the run once
            // nothing is left.
            Input::ClockFired => {
                let left = self.budget.left(now);
                match left.is_zero() {
                    true => self.cut(Stop::TimedOut, chat, now, &mut fx),
                    false => fx.push(Effect::ArmClock(left)),
                }
            }
        }
        fx
    }

    /// End the run as `stop` with the engine, after a called-off command.
    fn ended(
        &mut self,
        stop: Stop,
        chat: Option<&mut Chat>,
        now: Instant,
        mut fx: Vec<Effect>,
    ) -> Vec<Effect> {
        self.advance(chat, now, &mut fx, move |run| run.stopped(stop));
        fx
    }

    fn on_event(
        &mut self,
        input: Input,
        chat: Option<&mut Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        let Some(chat) = chat else {
            return;
        };
        // A turn ending while a mark is pinned is not the turn the run will send.
        let judge_turn = self.run.awaiting_turn() && self.held.is_none();
        if self.pending.is_some() && chat.link == Link::Connected {
            if let Some(text) = self.pending.take() {
                self.send(text, chat, now, fx);
            }
            return;
        }
        match input {
            Input::Appended => {
                if chat.prompted_beyond(self.sent) {
                    self.end(Stop::TakenOver, Some(chat), now, fx);
                } else if self.picking.is_some() {
                    // After the rest of what the same answer emitted, so a
                    // refusal is judged before the next pick is planned.
                    fx.push(Effect::Defer(Input::PicksAnswered));
                }
            }
            // A Stop pressed in the composer, or from the remote bridge: the
            // person's word that this is not to go on.
            Input::TurnEnded if chat.cancelled => self.end(Stop::ByPerson, Some(chat), now, fx),
            // A turn that failed (an error, a refusal, a limit reached) did not
            // do the work, so there is nothing to judge: the run fails on its
            // words.
            Input::TurnEnded if judge_turn && chat.failed() => {
                let why = chat.failure().unwrap_or_default().to_string();
                self.advance(Some(chat), now, fx, move |run| {
                    run.failed(why, Failure::Other)
                });
            }
            // What the agent left running is part of the step: a build in the
            // background, a subagent, and what it says once they are done.
            Input::TurnEnded if judge_turn && self.step_at_work(chat) => self.after_work = true,
            Input::TurnEnded if judge_turn => self.after_turn(chat, fx),
            Input::TurnEnded => {}
            // A person answers the card; the clock waits for them.
            Input::AwaitingUser => {
                self.card = true;
                self.budget.pause(now);
                fx.push(Effect::Waiting);
            }
            Input::Disconnected => self.end(Stop::LinkLost, Some(chat), now, fx),
            Input::PickRefused(why) if self.picking.is_some() => {
                settings::refused(self, chat, &why, now, fx)
            }
            // Not the run's: a person's own pick in the composer.
            Input::PickRefused(_) => {}
            Input::Notified
            | Input::SessionGone
            | Input::PicksAnswered
            | Input::Approved { .. }
            | Input::Revised { .. }
            | Input::Stop
            | Input::Pinned(_)
            | Input::Measured(_)
            | Input::TurnRead(_)
            | Input::NotTaken
            | Input::CommandExited(_)
            | Input::ForgeDone(_)
            | Input::ChecksSeen(_)
            | Input::ClockFired => {}
        }
    }

    /// Hold `held`, which ends the run when `ends`, while a mark is being
    /// pinned: whether it was held.
    fn hold(&mut self, ends: bool, held: Held) -> bool {
        match &mut self.held {
            Some(list) => {
                list.push((ends, held));
                true
            }
            None => false,
        }
    }

    /// A person's answer to the approval the run waits on: held while a mark
    /// is pinned, then judged by the engine against the visit it was drawn
    /// from. The clock goes on only once the engine took it, so a refused
    /// press leaves the run waiting as it was.
    fn answer(
        &mut self,
        answer: Answer,
        chat: Option<&mut Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        if self.hold(false, Held::Answer(answer.clone())) {
            return;
        }
        let action = match answer {
            Answer::Approved(at) => self.run.approved(&at),
            Answer::Revised(at, note) => self.run.revised(&at, note),
        };
        if action != Action::Idle {
            self.budget.resume(now);
        }
        self.act(action, chat, now, fx);
    }

    /// Cancel the turn under way and end the run as `stop`, as one call: held
    /// together while a mark is pinned, so the cancel never runs before a
    /// prompt the pin is still holding back.
    fn cut(&mut self, stop: Stop, chat: Option<&mut Chat>, now: Instant, fx: &mut Vec<Effect>) {
        if self.hold(true, Held::Cut(stop)) {
            return;
        }
        if chat.as_deref().is_some_and(|chat| chat.busy) {
            fx.push(Effect::CancelTurn);
        }
        self.end(stop, chat, now, fx);
    }

    /// End the run as `stop` — at once, or, while the step's command runs,
    /// once that command and everything it started have been stopped and have
    /// exited; while a forge call runs, once it has landed. A run said to be
    /// stopped never leaves a build or a test writing to the work behind it.
    /// The first reason given is the one the run ends with.
    fn end(&mut self, stop: Stop, chat: Option<&mut Chat>, now: Instant, fx: &mut Vec<Effect>) {
        if self.hold(true, Held::End(stop)) {
            return;
        }
        match self.command {
            true => {
                fx.push(Effect::CallOffCommand);
                self.stopping.get_or_insert(stop);
                // Its Stop waits on its button until the command has exited.
                fx.push(Effect::Refresh);
            }
            false => self.advance(chat, now, fx, move |run| run.stopped(stop)),
        }
    }

    /// Report to the run's engine, then do what it says.
    fn advance(
        &mut self,
        chat: Option<&mut Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
        report: impl FnOnce(&mut Run) -> Action,
    ) {
        if self.over {
            return;
        }
        let action = report(&mut self.run);
        self.act(action, chat, now, fx);
    }

    /// Do what the engine said, once the work is pinned at every boundary the
    /// run has crossed since the last pin: the mark of a step's start lands
    /// before its prompt or command touches the work. A mark that cannot be
    /// pinned is said and left out; it never stops the run.
    fn act(&mut self, action: Action, chat: Option<&mut Chat>, now: Instant, fx: &mut Vec<Effect>) {
        if ends_run(&action) || action == Action::Idle {
            return self.carry_out(action, chat, now, fx);
        }
        let refs = marks::refs_from(&self.task, &self.run, self.pinned);
        if refs.is_empty() {
            return self.carry_out(action, chat, now, fx);
        }
        let from = self.pinned;
        self.held = Some(Vec::new());
        self.waiting = Some((action, from, from + refs.len()));
        fx.push(Effect::Pin {
            dir: self.run.setup.dir.clone(),
            refs,
        });
    }

    /// The marks landed, or did not: do the action that waited on them, and
    /// then what was asked meanwhile.
    fn landed(
        &mut self,
        pinned: Result<String, String>,
        chat: Option<&mut Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        let Some((action, from, total)) = self.waiting.take() else {
            return;
        };
        match &pinned {
            Ok(commit) => self.run.pinned(commit, from),
            Err(why) => eprintln!("onehand: a step's mark was not pinned: {why}"),
        }
        self.pinned = total;
        let mut held = self.held.take().unwrap_or_default();
        // An ending asked meanwhile wins over the action, which would only
        // start work on a run that is over; the first one asked is its
        // reason, and the rest find nothing left to do.
        let mut chat = chat;
        if let Some(at) = held.iter().position(|(ends, _)| *ends) {
            let (_, ending) = held.swap_remove(at);
            return self.replay(ending, chat, now, fx);
        }
        self.carry_out(action, chat.as_deref_mut(), now, fx);
        // In the order asked; one that starts another pin holds the rest.
        for (_, then) in held {
            self.replay(then, chat.as_deref_mut(), now, fx);
        }
    }

    fn replay(&mut self, held: Held, chat: Option<&mut Chat>, now: Instant, fx: &mut Vec<Effect>) {
        match held {
            Held::Answer(answer) => self.answer(answer, chat, now, fx),
            Held::Cut(stop) => self.cut(stop, chat, now, fx),
            Held::End(stop) => self.end(stop, chat, now, fx),
        }
    }

    /// Do what the engine said, now.
    fn carry_out(
        &mut self,
        action: Action,
        chat: Option<&mut Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        let Some(chat) = chat else {
            return self.carry_out_without_session(action, now, fx);
        };
        if !ends_run(&action) {
            self.announce_step(fx);
        }
        let dir = self.run.setup.dir.clone();
        match action {
            Action::Measure => fx.push(Effect::Measure { dir }),
            Action::Prompt(text) => self.send(text, chat, now, fx),
            Action::RunCommand(command) => {
                self.command = true;
                fx.push(Effect::Note(format!("Running: {command}")));
                fx.push(Effect::RunCommand { dir, command });
            }
            Action::AwaitApproval => {
                self.budget.pause(now);
                fx.push(Effect::Note(
                    "Waiting for approval: Continue or Revise… under the header".to_string(),
                ));
                fx.push(Effect::Waiting);
            }
            Action::Push(commit) => {
                fx.push(Effect::Note(format!(
                    "Pushing {}, which the check passed on",
                    short(&commit)
                )));
                // Held as a step's command is, so a Stop waits for the forge
                // call in flight and the run is said to be stopped only once
                // it has landed, and no pull request is opened after that.
                self.command = true;
                fx.push(Effect::Forge {
                    commit: Some(commit),
                });
            }
            Action::OpenPullRequest => {
                fx.push(Effect::Note("Opening the pull request".to_string()));
                self.command = true;
                fx.push(Effect::Forge { commit: None });
            }
            Action::AwaitStatusChecks { wait, pushed } => {
                self.budget.pause(now);
                fx.push(Effect::Note(
                    "Waiting for the pull request's status checks".to_string(),
                ));
                fx.push(Effect::WatchChecks { pushed, wait });
                // Its slot is free now, for another issue to take up.
                fx.push(Effect::Waiting);
            }
            Action::Finish(outcome) => return self.finish(Some(chat), outcome, now, fx),
            Action::Idle => return,
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Save);
        fx.push(Effect::Refresh);
    }

    /// Do what the engine said with no session to do it in: a run that never
    /// had one runs its command and measures its work; one whose session went
    /// is only ended.
    fn carry_out_without_session(&mut self, action: Action, now: Instant, fx: &mut Vec<Effect>) {
        let dir = self.run.setup.dir.clone();
        match action {
            Action::Finish(outcome) => return self.finish(None, outcome, now, fx),
            Action::RunCommand(command) if self.sessionless => {
                self.command = true;
                fx.push(Effect::RunCommand { dir, command });
            }
            Action::Measure if self.sessionless => fx.push(Effect::Measure { dir }),
            Action::Measure
            | Action::Prompt(_)
            | Action::RunCommand(_)
            | Action::AwaitApproval
            | Action::Push(_)
            | Action::OpenPullRequest
            | Action::AwaitStatusChecks { .. }
            | Action::Idle => return,
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Save);
        fx.push(Effect::Refresh);
    }

    /// Tell the transcript the step the run is now in, once per step entered.
    /// A prompt's run has no steps to tell apart, so it says none.
    fn announce_step(&mut self, fx: &mut Vec<Effect>) {
        let fresh = self.noted != Some(self.run.step);
        self.noted = Some(self.run.step);
        let Some(step) = self.run.current() else {
            return;
        };
        if fresh && !self.run.template.is_prompt() {
            fx.push(Effect::Note(format!(
                "Step {} of {}: {}",
                self.run.step + 1,
                self.run.template.steps.len(),
                step.label
            )));
        }
    }

    /// Send `text`, or keep it until the agent is up.
    fn send(&mut self, text: String, chat: &mut Chat, now: Instant, fx: &mut Vec<Effect>) {
        if chat.link != Link::Connected {
            self.pending = Some(text);
            return;
        }
        if chat.prompted_beyond(self.sent) {
            return self.end(Stop::TakenOver, Some(chat), now, fx);
        }
        if !self.came_up(chat, now, fx) {
            return;
        }
        let Some(text) = settings::settle(self, text, chat, now, fx) else {
            return;
        };
        self.sent += 1;
        self.answer_from = chat.items.len();
        self.sent_at = Some(now);
        fx.push(Effect::Submit(text));
    }

    /// The agent is up, the first time in this session: put it in the run's
    /// mode before its first prompt, and say so to an issue the run works.
    /// Whether the prompt may go: an agent that does not offer the mode fails
    /// the run, since one left in the mode that asks before every edit would
    /// park at the first.
    fn came_up(&mut self, chat: &mut Chat, now: Instant, fx: &mut Vec<Effect>) -> bool {
        if std::mem::replace(&mut self.up, true) {
            return true;
        }
        fx.push(Effect::CameUp);
        let Some(mode) = self.run.setup.mode.clone() else {
            self.start_settings(chat, None);
            return true;
        };
        let offered: Vec<String> = chat.modes.iter().map(|m| m.id.clone()).collect();
        if let Some(why) = mode_refused(&mode, &offered) {
            fx.push(Effect::Pause {
                why: why.clone(),
                fix: "unattended.mode is fixed".to_string(),
            });
            self.advance(Some(chat), now, fx, move |run| {
                run.failed(why, found_late(About::Mode))
            });
            return false;
        }
        chat.set_mode(&mode);
        self.mode_out = Some(mode.clone());
        fx.push(Effect::Note(format!("Mode {mode} set")));
        self.start_settings(chat, Some(mode));
        true
    }

    /// Keep what the agent came up running as the run's starting settings,
    /// in the run's `mode` when it has one.
    fn start_settings(&mut self, chat: &Chat, mode: Option<String>) {
        let mut start = chat.in_force();
        if mode.is_some() {
            start.mode = mode;
        }
        self.run.start_settings(start);
    }

    /// A turn the run sent ended: read the work, then let the engine judge it.
    fn after_turn(&mut self, chat: &Chat, fx: &mut Vec<Effect>) {
        let Some(from) = self.run.marks.step_from.clone() else {
            return;
        };
        self.answer = Some(turn_answer(chat, self.answer_from));
        fx.push(Effect::ReadFacts {
            dir: self.run.setup.dir.clone(),
            from,
        });
    }

    /// End the run: say how in its transcript, then let the front end keep it
    /// and let its place go. Nothing is done after this.
    fn finish(
        &mut self,
        chat: Option<&mut Chat>,
        outcome: Outcome,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        self.over = true;
        if chat.is_some() {
            let said = outcome.said_in(&self.run.template);
            let said = match outcome.resumable() {
                true => format!("{said}; resume it from the Tasks page"),
                false => said,
            };
            fx.push(Effect::Note(said));
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Finish { outcome });
    }
}

/// Whether doing `action` ends the run, which is the one action after which
/// there is no step to announce and nothing to save.
fn ends_run(action: &Action) -> bool {
    match action {
        Action::Finish(_) => true,
        Action::Measure
        | Action::Prompt(_)
        | Action::RunCommand(_)
        | Action::AwaitApproval
        | Action::Push(_)
        | Action::OpenPullRequest
        | Action::AwaitStatusChecks { .. }
        | Action::Idle => false,
    }
}

/// A commit as a person reads it.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

/// The whole of the answer to the prompt sent at `from` items, cut at
/// [`ANSWER_MAX`] characters.
fn turn_answer(chat: &Chat, from: usize) -> String {
    let prose = chat.prose_since(from);
    match prose.char_indices().nth(ANSWER_MAX) {
        Some((cut, _)) => format!("{}\n\n(cut at {ANSWER_MAX} characters)", &prose[..cut]),
        None => prose,
    }
}

#[cfg(test)]
mod tests;
