//! A run's drive: what every run decides as it watches its session, does
//! what its engine asks, and reports back what happened.
//!
//! **Decided here, carried out elsewhere.** A [`RunDrive`] is given a
//! person's commands as methods ([`RunDrive::approve`], [`RunDrive::revise`],
//! [`RunDrive::stop`]) and told what happened ([`Input`]) with the run's
//! session as it stands, and answers with what to do ([`Effect`]): pin a
//! mark, send a prompt, run a command, call the forge, wait, say something,
//! save, finish. It has no timer, no thread and
//! no I/O of its own, so every rule below is a rule a test can hold it to; the
//! front end does the work and brings back what came of it.
//!
//! **The mode and the settings picks are asked of the session**, never made
//! on its model here: the drive reads its session and keeps the picks it
//! decided ([`RunDrive::take_picks`]), which the link makes through the
//! session's own drive before anything else reaches the run. The next pick
//! is planned only once the agent answered the last one.
//!
//! **Every Stop goes through the engine the same way.** A person's Stop, a
//! prompt of their own, the timeout, the agent going and the session closing
//! each end the run with [`Run::stopped`], which never judges the turn that
//! was under way — a cut-short turn passing as finished work is the mistake
//! two carriers disagreeing made before.

use crate::chat::{Chat, Link};

use crate::unattended::Budget;
use crate::workflow::{Action, ApprovalAt, Failure, Pick, Run, Stop};
use std::time::{Duration, Instant};

mod forge;
pub mod link;
mod pin;
mod settings;
mod signals;
mod steps;

use pin::{Answer, Held};
pub use signals::{Effect, Input, Signal, Timer};

/// A run under way on one session.
pub struct RunDrive {
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
    /// Picks decided and not yet handed to the session, in order.
    picks: Vec<Pick>,
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
    /// The step's command, or a forge call a Stop waits for, is running.
    command: bool,
    /// The forge call the run is in, or the status checks it waits on.
    forging: Option<forge::Forging>,
    /// How the run ends once its called-off command has exited.
    stopping: Option<Stop>,
    /// A write of the run failed and was said; nothing more is said until
    /// one lands.
    save_failed: bool,
    /// The run has finished: nothing more is done.
    over: bool,
    /// The run never had a session: a check, which only runs a command.
    /// It has no timeout, and what needs no session is still done.
    sessionless: bool,
}

impl RunDrive {
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
        chat: Option<&Chat>,
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
            picks: Vec::new(),
            noted: None,
            budget: Budget::start(limit, now),
            card: false,
            after_work: false,
            command: false,
            forging: None,
            stopping: None,
            save_failed: false,
            over: false,
            sessionless: chat.is_none(),
        };
        let mut fx = Vec::new();
        if !drive.sessionless {
            fx.push(Effect::Arm {
                timer: Timer::Timeout,
                after: limit,
            });
        }
        drive.act(first, chat, now, &mut fx);
        (drive, fx)
    }

    /// The picks the run decided since last asked, to be made on its session
    /// in order before anything else reaches the run.
    pub(crate) fn take_picks(&mut self) -> Vec<Pick> {
        std::mem::take(&mut self.picks)
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

    /// A person approved what they read at `at`. Held while a mark is
    /// pinned, and refused by the engine once the run no longer waits there.
    pub fn approve(&mut self, at: ApprovalAt, chat: Option<&Chat>, now: Instant) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.answer(Answer::Approved(at), chat, now, fx);
            fx.push(Effect::Answered);
        })
    }

    /// A person sent what they read at `at` back with `note`, as
    /// [`Self::approve`] is judged.
    pub fn revise(
        &mut self,
        at: ApprovalAt,
        note: String,
        chat: Option<&Chat>,
        now: Instant,
    ) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.answer(Answer::Revised(at, note), chat, now, fx);
            fx.push(Effect::Answered);
        })
    }

    /// A person pressed Stop: the turn under way is cancelled and the run
    /// ends, once a command running has exited.
    pub fn stop(&mut self, chat: Option<&Chat>, now: Instant) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.cut(Stop::ByPerson, chat, now, fx)
        })
    }

    /// Do `command`, unless the run has finished, which does nothing more.
    fn command_with(
        &mut self,
        chat: Option<&Chat>,
        now: Instant,
        command: impl FnOnce(&mut Self, Option<&Chat>, Instant, &mut Vec<Effect>),
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        if !self.over {
            command(self, chat, now, &mut fx);
        }
        fx
    }

    /// Something happened: what to do about it. A run that has finished does
    /// nothing more.
    pub fn handle(&mut self, input: Input, chat: Option<&Chat>, now: Instant) -> Vec<Effect> {
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
                if self.card && chat.is_some_and(|chat| !chat.awaiting_permission()) {
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
            Input::Pushed(pushed) => self.pushed(pushed, chat, now, &mut fx),
            Input::PullRequestFound(found) => self.found(found, chat, now, &mut fx),
            Input::PullRequestOpened(read) => self.pull_request_opened(read, chat, now, &mut fx),
            Input::LogsRead(logs) => self.logs_read(logs, chat, now, &mut fx),
            Input::MarkedReady(ready) => self.marked_ready(ready, chat, now, &mut fx),
            Input::Elapsed(Timer::Checks) => self.look_again(&mut fx),
            Input::NoForge(why) => self.no_forge(why, chat, now, &mut fx),
            Input::Saved(Ok(())) => self.save_failed = false,
            Input::Saved(Err(why)) => {
                if !std::mem::replace(&mut self.save_failed, true) {
                    fx.push(Effect::Signal(Signal::SaveFailed(why)));
                }
            }
            // The run's timeout: when it has run out, look again, since time
            // spent waiting on a person did not count; end the run once
            // nothing is left.
            Input::Elapsed(Timer::Timeout) => {
                let left = self.budget.left(now);
                match left.is_zero() {
                    true => self.cut(Stop::TimedOut, chat, now, &mut fx),
                    false => fx.push(Effect::Arm {
                        timer: Timer::Timeout,
                        after: left,
                    }),
                }
            }
        }
        fx
    }

    /// End the run as `stop` with the engine, after a called-off command.
    fn ended(
        &mut self,
        stop: Stop,
        chat: Option<&Chat>,
        now: Instant,
        mut fx: Vec<Effect>,
    ) -> Vec<Effect> {
        self.advance(chat, now, &mut fx, move |run| run.stopped(stop));
        fx
    }

    fn on_event(&mut self, input: Input, chat: Option<&Chat>, now: Instant, fx: &mut Vec<Effect>) {
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
            | Input::Pinned(_)
            | Input::Measured(_)
            | Input::TurnRead(_)
            | Input::NotTaken
            | Input::CommandExited(_)
            | Input::Pushed(_)
            | Input::PullRequestFound(_)
            | Input::PullRequestOpened(_)
            | Input::LogsRead(_)
            | Input::MarkedReady(_)
            | Input::NoForge(_)
            | Input::Elapsed(_)
            | Input::Saved(_) => {}
        }
    }
    /// A person's answer to the approval the run waits on: held while a mark
    /// is pinned, then judged by the engine against the visit it was drawn
    /// from. The clock goes on only once the engine took it, so a refused
    /// press leaves the run waiting as it was.
    fn answer(&mut self, answer: Answer, chat: Option<&Chat>, now: Instant, fx: &mut Vec<Effect>) {
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
    fn cut(&mut self, stop: Stop, chat: Option<&Chat>, now: Instant, fx: &mut Vec<Effect>) {
        if self.hold(true, Held::Cut(stop)) {
            return;
        }
        if chat.is_some_and(|chat| chat.busy) {
            fx.push(Effect::CancelTurn);
        }
        self.end(stop, chat, now, fx);
    }

    /// End the run as `stop` — at once, or, while the step's command runs,
    /// once that command and everything it started have been stopped and have
    /// exited; while a forge call runs, once it has landed. A run said to be
    /// stopped never leaves a build or a test writing to the work behind it.
    /// The first reason given is the one the run ends with.
    fn end(&mut self, stop: Stop, chat: Option<&Chat>, now: Instant, fx: &mut Vec<Effect>) {
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
        chat: Option<&Chat>,
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
}

#[cfg(test)]
mod tests;
