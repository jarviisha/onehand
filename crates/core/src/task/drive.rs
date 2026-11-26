//! A run's drive: what every run decides as it watches its session, does
//! what its engine asks, and reports back what happened.
//!
//! **Decided here, carried out elsewhere.** A [`RunDrive`] is given a
//! person's commands as methods ([`RunDrive::approve`], [`RunDrive::revise`],
//! [`RunDrive::stop`]) and told what happened ([`Input`]) with the run's
//! session as it stands, and answers with what to do ([`Effect`]): pin a
//! mark, run a command, call the forge, wait, save, finish. It has no timer,
//! no thread and no I/O of its own, so every rule below is a rule a test can
//! hold it to; the front end does the work and brings back what came of it.
//!
//! **Its commands are the link's to give.** Nothing outside this crate calls
//! them: a front end gives each through [`link::Joined`], which holds the
//! run with its session for the call and makes what the run asked of its
//! session there before it answers.
//!
//! **What the run asks of its session is never a front end's to carry.** The
//! mode and the settings picks are not made on the session's model here: the
//! drive reads its session and keeps the picks it decided
//! ([`RunDrive::take_picks`]), and after them its notes, its prompt and its
//! cancel ([`RunDrive::take_asked`]), in order. The link makes each through
//! the session's own drive before anything else reaches the run. The next
//! pick is planned only once the agent answered the last one.
//!
//! **Every Stop goes through the engine the same way.** A person's Stop, a
//! prompt of their own, the timeout, the agent going and the session closing
//! each end the run with [`Run::stopped`], which never judges the turn that
//! was under way — a cut-short turn passing as finished work is the mistake
//! two carriers disagreeing made before.

use crate::chat::{Chat, Link};
use crate::Now;

use crate::workflow::{Action, ApprovalAt, Failure, Pick, Run, Stop, Verdict};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

mod budget;
mod forge;
mod lanes;
pub mod link;
mod pin;
mod room;
mod settings;
mod signals;
mod steps;

use budget::Budget;
use lanes::LaneState;
pub use lanes::{Lane, NotOpened};
use pin::{Answer, Held};
pub(crate) use signals::Asked;
pub use signals::{Aim, Effect, Input, Signal, Timer};

/// The soonest a timeout timer that found time left looks again.
const RECHECK_FLOOR: Duration = Duration::from_secs(1);
/// A run under way, on its sessions: one for the run, and one for each
/// step naming its agent.
pub struct RunDrive {
    /// The task the run is of.
    task: String,
    run: Run,
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
    /// The lane the run works in now: every event, command and prompt is
    /// its session's.
    lane: Lane,
    /// What the run keeps of the current lane's session.
    here: LaneState,
    /// What it keeps of every other lane it has worked in.
    away: BTreeMap<Lane, LaneState>,
    /// How many transcript items there were when the run's last prompt went:
    /// the turn's answer is what the agent said after that.
    answer_from: usize,
    /// When the run's last prompt went: background work started before it is
    /// not the step's to wait for.
    sent_at: Option<Instant>,
    /// The turn's answer, kept while its work is read.
    answer: Option<String>,
    /// The last verdict the review's agent gave in the turn under way, with
    /// the review's visit it was given at. Not kept in the run: a restart
    /// prompts the review again.
    verdict: Option<(u32, Verdict)>,
    /// Picks decided and not yet handed to the session, in order.
    picks: Vec<Pick>,
    /// What else the run asked of its session and the link has not made yet,
    /// in order: its notes, its prompt, its cancel.
    asked: Vec<Asked>,
    /// The step the transcript was last told of.
    noted: Option<usize>,
    /// How much of the timeout is left; it does not run while a card or an
    /// approval waits on a person.
    budget: Budget,
    /// Working time spent before this session took the run up.
    spent_before: u64,
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
    /// The step the run was given room at, while it holds it.
    room: Option<usize>,
    /// The run has finished: nothing more is done.
    over: bool,
    /// An Extend pressed while the turn cancelled at running out was still
    /// ending: done once it has.
    after_drain: Option<Answer>,
    /// Out of time, its agent does nothing more: the cancelled turn ended and
    /// the work it left is done.
    rested: bool,
    /// Extend pressed on a board task whose team was full, and when, waiting
    /// for a place.
    queued_extend: Option<(ApprovalAt, u64)>,
    /// The run never had a session: a check, which only runs a command.
    /// It has no timeout, and what needs no session is still done.
    sessionless: bool,
}

impl RunDrive {
    /// Drive `run`, the last run of `task`, starting with `first`: what
    /// [`Run::resume`] said to do. `pinned` boundaries of it had their mark
    /// pinned before.
    pub(crate) fn start(
        task: String,
        run: Run,
        first: Action,
        pinned: usize,
        now: Now,
        chat: Option<&Chat>,
    ) -> (Self, Vec<Effect>) {
        let limit = run.template.timeout_limit().map(|limit| {
            (limit + Duration::from_secs(run.extended_secs))
                .saturating_sub(Duration::from_secs(run.spent_secs))
        });
        let lane = Lane::at(&run);
        let mut drive = Self {
            task,
            spent_before: run.spent_secs,
            run,
            pinned,
            held: None,
            waiting: None,
            lane,
            here: LaneState::new(chat.is_some()),
            away: BTreeMap::new(),
            answer_from: 0,
            sent_at: None,
            answer: None,
            verdict: None,
            picks: Vec::new(),
            asked: Vec::new(),
            noted: None,
            budget: Budget::start(limit, now.at, false),
            after_work: false,
            command: false,
            forging: None,
            stopping: None,
            save_failed: false,
            room: None,
            over: false,
            after_drain: None,
            rested: false,
            queued_extend: None,
            sessionless: chat.is_none(),
        };
        let mut fx = Vec::new();
        if let Some(after) = limit.filter(|_| !drive.sessionless) {
            fx.push(Effect::Arm {
                timer: Timer::Timeout,
                after,
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

    /// What else the run asked of its session since last asked, to be made
    /// there in order, after its picks.
    pub(crate) fn take_asked(&mut self) -> Vec<Asked> {
        std::mem::take(&mut self.asked)
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

    /// The run waits on a person at a session: an approval, which marks every
    /// lane's, or a card the agent parked, which is only in the current
    /// lane's (`current`).
    pub fn waits_on_person(&self, current: bool) -> bool {
        self.run.awaiting_approval()
            || (self.run.awaiting_extend() && self.queued_extend.is_none())
            || (current && self.here.card)
    }

    /// The agent has the step: its prompt went and its turn, or the work it
    /// left running, is not over and judged yet. Only then can the run run
    /// out, so there is always a turn to cancel and never a prompt to send
    /// into a run that ran out.
    fn agent_has_the_step(&self) -> bool {
        self.run.awaiting_turn()
            && self.held.is_none()
            && self.here.pending.is_none()
            && self.here.picking.is_none()
            && self.answer.is_none()
    }

    /// Whether the run holds no place on a team: out of time and at rest, or
    /// waiting for one to be extended in.
    pub fn gives_place(&self) -> bool {
        (self.run.awaiting_extend() && self.rested) || self.queued_extend.is_some()
    }

    /// When an extend waiting for a team place asked for it, in seconds.
    pub fn queued_extend_at(&self) -> Option<u64> {
        self.queued_extend.as_ref().map(|(_, asked)| *asked)
    }

    /// The extend waiting for a place, now given one: what it extends.
    pub(crate) fn take_queued_extend(&mut self) -> Option<ApprovalAt> {
        self.queued_extend.take().map(|(at, _)| at)
    }

    /// The agent parked a card nobody has answered yet.
    pub fn card(&self) -> bool {
        self.here.card
    }

    /// Whether [`Input::Notified`] could move the run, read without moving
    /// it: a card may have been answered, or the work the step's turn left
    /// running has ended.
    pub fn heeds_notified(&self, chat: &Chat) -> bool {
        self.here.card || (self.after_work && !self.step_at_work(chat))
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
        (self.here.pending.is_some() && chat.link == Link::Connected)
            || chat.prompted_beyond(self.here.sent)
            || self.here.picking.is_some()
    }

    /// Working time spent on the run, sessions before this one included.
    pub fn spent_secs(&self, now: Now) -> u64 {
        self.spent_before + self.budget.spent(now.at).as_secs()
    }

    /// A person approved what they read at `at`. Held while a mark is
    /// pinned, and refused by the engine once the run no longer waits there.
    pub(crate) fn approve(&mut self, at: ApprovalAt, chat: Option<&Chat>, now: Now) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.answer(Answer::Approved(at), chat, now, fx);
        })
    }

    /// A person sent what they read at `at` back with `note`, as
    /// [`Self::approve`] is judged.
    pub(crate) fn revise(
        &mut self,
        at: ApprovalAt,
        note: String,
        chat: Option<&Chat>,
        now: Now,
    ) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.answer(Answer::Revised(at, note), chat, now, fx);
        })
    }

    /// A person pressed Stop: the turn under way is cancelled and the run
    /// ends, once a command running has exited.
    pub(crate) fn stop(&mut self, chat: Option<&Chat>, now: Now) -> Vec<Effect> {
        // A run that ran out of time and is stopped ends as a timeout: that is
        // what stopped it.
        let stop = match self.run.awaiting_extend() {
            true => Stop::TimedOut,
            false => Stop::ByPerson,
        };
        self.command_with(chat, now, move |d, chat, now, fx| {
            d.cut(stop, chat, now, fx)
        })
    }

    /// A person pressed Extend on a board task out of time at `at` whose team
    /// has no place: it waits for one, said so. Refused unless the run ran
    /// out there.
    pub(crate) fn queue_extend(&mut self, at: ApprovalAt, now: Now) -> Vec<Effect> {
        if self.over || self.run.out_of_time_at().as_ref() != Some(&at) {
            return Vec::new();
        }
        self.queued_extend = Some((at, now.secs));
        self.asked
            .push(Asked::Note("Waiting for a team place.".to_string()));
        vec![Effect::Refresh]
    }

    /// A person gave the run that ran out at `at` its timeout again. Held
    /// while a mark is pinned or the cancelled turn is still ending, and
    /// refused by the engine once the run no longer waits there.
    pub(crate) fn extend(&mut self, at: ApprovalAt, chat: Option<&Chat>, now: Now) -> Vec<Effect> {
        self.command_with(chat, now, |d, chat, now, fx| {
            d.answer(Answer::Extended(at), chat, now, fx);
        })
    }

    /// The review's agent, in `lane`'s session, gave `verdict`: held until
    /// the review's turn ends, the last one given counting. Refused unless
    /// the run waits on the turn of a review whose lane is `lane`; answered
    /// with what the agent is told.
    pub(crate) fn verdict(&mut self, lane: &Lane, verdict: Verdict) -> Result<String, String> {
        let at = self.run.review_visit().filter(|_| {
            !self.over
                && self.held.is_none()
                && self.answer.is_none()
                && Lane::at(&self.run) == *lane
        });
        let Some(at) = at else {
            return Err("No review is waiting on this session.".to_string());
        };
        let said = match &verdict {
            Verdict::Approve => "Approved. The review ends when this turn does.".to_string(),
            Verdict::SendBack(note) if note.trim().is_empty() => {
                return Err("A send back needs a note saying what to change.".to_string())
            }
            Verdict::SendBack(_) => match self.run.rejects_to_label() {
                Some(label) => {
                    format!("Sent back to {label}. The review ends when this turn does.")
                }
                None => return Err("This review has no step to send the work back to.".to_string()),
            },
        };
        self.verdict = Some((at, verdict));
        Ok(said)
    }

    /// Do `command`, unless the run has finished, which does nothing more.
    fn command_with(
        &mut self,
        chat: Option<&Chat>,
        now: Now,
        command: impl FnOnce(&mut Self, Option<&Chat>, Now, &mut Vec<Effect>),
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        if !self.over {
            command(self, chat, now, &mut fx);
        }
        fx
    }

    /// Something happened: what to do about it. A run that has finished does
    /// nothing more.
    pub(crate) fn handle(&mut self, input: Input, chat: Option<&Chat>, now: Now) -> Vec<Effect> {
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
                if self.here.card && chat.is_some_and(|chat| !chat.awaiting_permission()) {
                    self.here.card = false;
                    // A card the cancelled turn left does not start a run
                    // that waits to be extended.
                    if !self.run.awaiting_extend() {
                        self.budget.resume(now.at);
                    }
                }
                // Out of time, the work the cancelled turn left has ended.
                if self.run.awaiting_extend()
                    && chat.is_some_and(|chat| !chat.busy && !self.step_at_work(chat))
                {
                    self.rest(&mut fx);
                }
                if let Some(chat) = chat.filter(|chat| self.after_work && !self.step_at_work(chat))
                {
                    self.after_work = false;
                    if self.run.awaiting_turn() && self.held.is_none() {
                        self.after_turn(chat, now, &mut fx);
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
                self.advance(chat, now, &mut fx, move |run| run.measured(mark, now.secs))
            }
            Input::Measured(Err(why)) | Input::TurnRead(Err(why)) => {
                self.advance(chat, now, &mut fx, move |run| {
                    run.failed(
                        format!("the work could not be read: {why}"),
                        Failure::Other,
                        now.secs,
                    )
                })
            }
            Input::TurnRead(Ok(facts)) => {
                let answer = self.answer.take().unwrap_or_default();
                // Only a verdict given at this visit of the review is its.
                let verdict = self
                    .verdict
                    .take()
                    .filter(|(at, _)| self.run.review_visit() == Some(*at));
                self.advance(chat, now, &mut fx, move |run| match verdict {
                    Some((_, verdict)) => run.reviewed(Some(verdict), &answer, now.secs),
                    None => run.turn_ended(&facts, &answer, now.secs),
                })
            }
            Input::NotTaken => self.advance(chat, now, &mut fx, |run| {
                run.failed(
                    "the agent did not take the prompt".to_string(),
                    Failure::Other,
                    now.secs,
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
                    self.asked.push(Asked::Note(said.to_string()));
                }
                self.advance(chat, now, &mut fx, move |run| {
                    run.command_finished(ran, now.secs)
                });
            }
            Input::Pushed(pushed) => self.pushed(pushed, chat, now, &mut fx),
            Input::PullRequestFound(found) => self.found(found, chat, now, &mut fx),
            Input::PullRequestOpened(read) => self.pull_request_opened(read, chat, now, &mut fx),
            Input::LogsRead(logs) => self.logs_read(logs, chat, now, &mut fx),
            Input::MarkedReady(ready) => self.marked_ready(ready, chat, now, &mut fx),
            Input::Elapsed(Timer::Checks) => self.look_again(&mut fx),
            Input::NoForge(why) => self.no_forge(why, chat, now, &mut fx),
            Input::LaneOpened { lane, result } => self.opened(lane, result, chat, now, &mut fx),
            Input::LaneAppended { lane } => self.appended_in(lane, chat, now, &mut fx),
            Input::LaneGone { lane } => self.gone(lane, chat, now, &mut fx),
            Input::Saved(Ok(())) => self.save_failed = false,
            Input::Saved(Err(why)) => {
                if !std::mem::replace(&mut self.save_failed, true) {
                    fx.push(Effect::Signal(Signal::SaveFailed(why)));
                }
            }
            Input::RoomGiven => self.room_given(chat, now, &mut fx),
            // The run's timeout: when it has run out, look again, since time
            // spent waiting on a person did not count; end the run once
            // nothing is left.
            Input::Elapsed(Timer::Timeout) => match self.budget.left(now.at) {
                Some(left) if left.is_zero() && self.agent_has_the_step() => {
                    self.ran_out(chat, now, &mut fx)
                }
                // No sooner than a second: with the clock standing still and
                // a moment left, looking again at once would spin.
                Some(left) => fx.push(Effect::Arm {
                    timer: Timer::Timeout,
                    after: left.max(RECHECK_FLOOR),
                }),
                None => {}
            },
        }
        fx
    }

    /// End the run as `stop` with the engine, after a called-off command.
    fn ended(
        &mut self,
        stop: Stop,
        chat: Option<&Chat>,
        now: Now,
        mut fx: Vec<Effect>,
    ) -> Vec<Effect> {
        self.advance(chat, now, &mut fx, move |run| run.stopped(stop, now.secs));
        fx
    }

    fn on_event(&mut self, input: Input, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        let Some(chat) = chat else {
            return;
        };
        // A turn ending while a mark is pinned is not the turn the run will send.
        let judge_turn = self.run.awaiting_turn() && self.held.is_none();
        if self.here.pending.is_some() && chat.link == Link::Connected {
            if let Some(text) = self.here.pending.take() {
                self.send(text, chat, now, fx);
            }
            return;
        }
        match input {
            Input::Appended => {
                if chat.prompted_beyond(self.here.sent) {
                    self.end(Stop::TakenOver, Some(chat), now, fx);
                } else if self.here.picking.is_some() {
                    // After the rest of what the same answer emitted, so a
                    // refusal is judged before the next pick is planned.
                    fx.push(Effect::Defer(Input::PicksAnswered));
                }
            }
            // The turn cancelled when the run ran out has ended: an Extend
            // pressed meanwhile goes now. Not a person's Stop, nor an answer.
            Input::TurnEnded if self.run.awaiting_extend() => match self.after_drain.take() {
                Some(answer) => self.answer(answer, Some(chat), now, fx),
                None if !self.step_at_work(chat) => self.rest(fx),
                None => {}
            },
            // A Stop pressed in the composer, or from the remote bridge: the
            // person's word that this is not to go on.
            Input::TurnEnded if chat.cancelled => self.end(Stop::ByPerson, Some(chat), now, fx),
            // A turn that failed (an error, a refusal, a limit reached) did not
            // do the work, so there is nothing to judge: the run fails on its
            // words.
            Input::TurnEnded if judge_turn && chat.failed() => {
                let why = chat.failure().unwrap_or_default().to_string();
                self.advance(Some(chat), now, fx, move |run| {
                    run.failed(why, Failure::Other, now.secs)
                });
            }
            // What the agent left running is part of the step: a build in the
            // background, a subagent, and what it says once they are done.
            Input::TurnEnded if judge_turn && self.step_at_work(chat) => self.after_work = true,
            Input::TurnEnded if judge_turn => self.after_turn(chat, now, fx),
            Input::TurnEnded => {}
            // A person answers the card; the clock waits for them.
            Input::AwaitingUser => {
                self.here.card = true;
                self.budget.pause(now.at);
            }
            Input::Disconnected => self.end(Stop::LinkLost, Some(chat), now, fx),
            Input::PickRefused(why) if self.here.picking.is_some() => {
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
            | Input::Saved(_)
            | Input::RoomGiven
            | Input::LaneOpened { .. }
            | Input::LaneAppended { .. }
            | Input::LaneGone { .. } => {}
        }
    }
    /// A person's answer to the approval the run waits on: held while a mark
    /// is pinned, then judged by the engine against the visit it was drawn
    /// from. The clock goes on with the next prompt it leads to, so a
    /// refused press leaves the run waiting as it was.
    fn answer(&mut self, answer: Answer, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        if self.hold(false, Held::Answer(answer.clone())) {
            return;
        }
        let action = match answer {
            Answer::Approved(at) => self.run.approved(&at, now.secs),
            Answer::Revised(at, note) => self.run.revised(&at, note, now.secs),
            // The cancelled turn first ends: a prompt sent into it would
            // queue behind it and read as a person's.
            Answer::Extended(at)
                if chat.is_some_and(|chat| chat.busy)
                    && self.run.out_of_time_at().as_ref() == Some(&at) =>
            {
                self.after_drain = Some(Answer::Extended(at));
                return;
            }
            Answer::Extended(at) => {
                let by = self.run.template.timeout_limit().unwrap_or_default();
                let action = self.run.extended(&at, by, now.secs);
                if action != Action::Idle {
                    self.after_drain = None;
                    self.rested = false;
                    self.queued_extend = None;
                    self.budget.extend(by);
                    if let Some(after) = self.budget.left(now.at) {
                        fx.push(Effect::Arm {
                            timer: Timer::Timeout,
                            after,
                        });
                    }
                }
                action
            }
        };
        self.act(action, chat, now, fx);
    }

    /// The run's time ran out: its turn is cancelled and it waits for a person
    /// to extend it or stop it, in the same session.
    /// The run, out of time, came to rest: the room and the team place it
    /// held go.
    fn rest(&mut self, fx: &mut Vec<Effect>) {
        if std::mem::replace(&mut self.rested, true) {
            return;
        }
        self.left_room(fx);
        fx.push(Effect::Rested);
    }

    fn ran_out(&mut self, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        self.after_drain = None;
        self.rested = false;
        if chat.is_some_and(|chat| chat.busy) {
            self.asked.push(Asked::Cancel);
        }
        self.after_work = false;
        let label = self
            .run
            .current()
            .map(|s| s.label.clone())
            .unwrap_or_default();
        let note = format!(
            "Out of time at {label} after {} of {}. Extend to go on.",
            crate::task::work::worked(self.spent_secs(now)),
            self.run.template.timeout_said(),
        );
        let ran_out = self.run.awaiting_turn();
        self.advance(chat, now, fx, move |run| run.ran_out(now.secs));
        if ran_out {
            self.asked.push(Asked::Note(note));
        }
        // Nothing of the agent's was under way: at rest at once.
        if chat.is_some_and(|chat| !chat.busy && !self.step_at_work(chat)) {
            self.rest(fx);
        }
    }

    /// Cancel the turn under way and end the run as `stop`, as one call: held
    /// together while a mark is pinned, so the cancel never runs before a
    /// prompt the pin is still holding back.
    fn cut(&mut self, stop: Stop, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        if self.hold(true, Held::Cut(stop)) {
            return;
        }
        if chat.is_some_and(|chat| chat.busy) {
            self.asked.push(Asked::Cancel);
        }
        self.end(stop, chat, now, fx);
    }

    /// End the run as `stop` — at once, or, while the step's command runs,
    /// once that command and everything it started have been stopped and have
    /// exited; while a forge call runs, once it has landed. A run said to be
    /// stopped never leaves a build or a test writing to the work behind it.
    /// The first reason given is the one the run ends with.
    fn end(&mut self, stop: Stop, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
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
            false => self.advance(chat, now, fx, move |run| run.stopped(stop, now.secs)),
        }
    }

    /// Report to the run's engine, then do what it says.
    fn advance(
        &mut self,
        chat: Option<&Chat>,
        now: Now,
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
