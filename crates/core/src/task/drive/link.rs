//! The link between a run and the session it is driven on: what the session
//! tells the run, and what the run asks of its session.
//!
//! **Routing is behaviour, so it lives here and not in a front end.** Which
//! of a session's signals moves a run, which streamed chunk is worth asking it
//! about, that a run's picks are made before its notes and its prompt, and
//! that its prompt never queues and a refused one is told to it where it was
//! asked: each is a rule a front end would otherwise restate in its event
//! handlers, untested. A front end joins the two drives
//! it holds for one call ([`Joined`]), and carries out what comes back
//! ([`Out`]): the session's effects, then the run's.

use super::{Asked, Effect, Input, Lane, RunDrive};
use crate::chat::drive::{Code, Effect as SessionEffect, Intake, Origin, SessionDrive, Signal};
use crate::chat::Chat;
use crate::workflow::{Action, ApprovalAt, Run, Verdict};
use crate::Now;

/// What a joined call says to do: the session's effects first, then the
/// run's, each in its own order.
#[derive(Default)]
pub struct Out {
    pub run: Vec<Effect>,
    pub session: Vec<SessionEffect>,
    /// The session was told something its viewers see (a note, a prompt, a
    /// cancel, a name): tell whatever watches it.
    pub told: bool,
}

/// A run and the session it is driven on, joined for one call. A check's
/// run, and one whose session went, has none.
pub struct Joined<'a> {
    run: &'a mut RunDrive,
    session: Option<&'a mut SessionDrive>,
}

impl<'a> Joined<'a> {
    pub fn new(run: &'a mut RunDrive, session: Option<&'a mut SessionDrive>) -> Self {
        Self { run, session }
    }

    /// Start driving `run` on `session`, with `first` as [`Run::resume`]
    /// said: the run's drive, and what starting it says to do.
    ///
    /// The session is named for the task, and for the step when it is a
    /// step's lane, unless it has a name already, so the rail tells runs
    /// apart rather than calling each by the opening line of its first
    /// prompt. Before that prompt the name is only held, and the
    /// first turn's save writes it.
    pub fn start(
        task: String,
        run: Run,
        first: Action,
        pinned: usize,
        now: Now,
        mut session: Option<&mut SessionDrive>,
    ) -> (RunDrive, Out) {
        let mut out = Out::default();
        if let Some(session) = session.as_deref_mut() {
            if session.custom_title.is_none() {
                let (named, fx) = session.rename(&Lane::at(&run).name(&run));
                out.told = named;
                out.session = fx;
            }
        }
        let chat = session.as_deref().map(|session| &**session);
        let (mut drive, fx) = RunDrive::start(task, run, first, pinned, now, chat);
        Joined::new(&mut drive, session).settle(fx, now, &mut out);
        (drive, out)
    }

    /// Tell the run what happened, with its session as it stands.
    pub fn handle(&mut self, input: Input, now: Now) -> Out {
        self.give(now, |run, chat| run.handle(input, chat, now))
    }

    /// A person approved what they read at `at`.
    pub fn approve(&mut self, at: ApprovalAt, now: Now) -> Out {
        self.give(now, |run, chat| run.approve(at, chat, now))
    }

    /// A person gave the run that ran out of time at `at` its timeout again.
    pub fn extend(&mut self, at: ApprovalAt, now: Now) -> Out {
        self.give(now, |run, chat| run.extend(at, chat, now))
    }

    /// A person sent what they read at `at` back with `note`.
    pub fn revise(&mut self, at: ApprovalAt, note: String, now: Now) -> Out {
        self.give(now, |run, chat| run.revise(at, note, chat, now))
    }

    /// The review's agent, in `lane`'s session, gave `verdict`: what it is
    /// told, or why it was refused. Held until the review's turn ends, so it
    /// asks nothing of the session.
    pub fn verdict(&mut self, lane: &Lane, verdict: Verdict) -> Result<String, String> {
        self.run.verdict(lane, verdict)
    }

    /// A person pressed Stop.
    pub fn stop(&mut self, now: Now) -> Out {
        self.give(now, |run, chat| run.stop(chat, now))
    }

    fn give(
        &mut self,
        now: Now,
        command: impl FnOnce(&mut RunDrive, Option<&Chat>) -> Vec<Effect>,
    ) -> Out {
        let fx = command(self.run, self.session.as_deref().map(|session| &**session));
        let mut out = Out::default();
        self.settle(fx, now, &mut out);
        out
    }

    /// Make what the run asked of its session there, picks first and then
    /// the rest in order, and add what the run said to do, `fx`, to `out`.
    fn settle(&mut self, fx: Vec<Effect>, now: Now, out: &mut Out) {
        let fx = self.made(fx, now, out);
        out.run.extend(fx);
    }

    /// `fx`, the run's answer to one call, once what it asked of its session
    /// with it is made. A prompt the session does not take is told to the
    /// run at once, and what the run says to that stands where the prompt was
    /// asked, ahead of what the call answered after it: a run that ends on a
    /// refusal has finished before the save its prompt was to be followed by.
    fn made(&mut self, mut fx: Vec<Effect>, now: Now, out: &mut Out) -> Vec<Effect> {
        let picks = self.run.take_picks();
        let asked = self.run.take_asked();
        let Some(session) = self.session.as_deref_mut() else {
            // A run reads its session before it asks anything of it, so one
            // joined with none has asked nothing.
            debug_assert!(picks.is_empty() && asked.is_empty());
            return fx;
        };
        for pick in &picks {
            out.session.extend(session.pick(pick));
        }
        let mut refused = Vec::new();
        for asked in asked {
            match asked {
                Asked::Name(name) => {
                    if session.custom_title.is_none() {
                        let (named, fx) = session.rename(&name);
                        out.told |= named;
                        out.session.extend(fx);
                    }
                }
                Asked::Note(text) => {
                    out.told = true;
                    out.session.extend(session.note(text));
                }
                Asked::Cancel => {
                    out.told = true;
                    out.session.extend(session.stop());
                }
                Asked::Prompt { text, at } => {
                    out.told = true;
                    let (answer, said) = submit(session, &text, now);
                    out.session.extend(said);
                    if let Some(input) = answer {
                        refused.push((at, input));
                    }
                }
            }
        }
        // Each refusal's answer goes in where its prompt was asked, counted
        // past what earlier ones put in.
        let mut shift = 0;
        for (at, input) in refused {
            let chat = self.session.as_deref().map(|session| &**session);
            let said = self.run.handle(input, chat, now);
            let said = self.made(said, now, out);
            let at = (at + shift).min(fx.len());
            shift += said.len();
            fx.splice(at..at, said);
        }
        fx
    }
}

/// What `signal`, from the session of the run's lane `lane`, tells the
/// run, if anything: heard in full from the lane the run works in now, and
/// from another only as that lane's.
pub fn said(
    signal: &Signal,
    lane: &Lane,
    drive: &RunDrive,
    session: &SessionDrive,
) -> Option<Input> {
    match drive.current_lane() == lane {
        true => input_of(signal, drive, session),
        false => lane_input_of(signal, lane, drive, session),
    }
}

/// The session of the run's lane `lane` went: the run is kept to be
/// resumed, not ended, when it was the lane the run works in; another
/// lane's is only forgotten.
pub fn gone(drive: &RunDrive, lane: Lane) -> Input {
    match drive.current_lane() == &lane {
        true => Input::SessionGone,
        false => Input::LaneGone { lane },
    }
}

/// What `signal`, from the session `drive` runs on, tells the run, if
/// anything.
///
/// A streamed chunk is asked about first, read-only: most of what a session
/// says is its reply streaming in, and a chunk that cannot move the run must
/// not tell everything watching the run that it changed.
fn input_of(signal: &Signal, drive: &RunDrive, session: &SessionDrive) -> Option<Input> {
    match signal {
        Signal::Appended => drive.heeds_appended(session).then_some(Input::Appended),
        Signal::TurnEnded => Some(Input::TurnEnded),
        Signal::AwaitingUser(_) => Some(Input::AwaitingUser),
        Signal::Disconnected(_) => Some(Input::Disconnected),
        Signal::PickRefused(why) => Some(Input::PickRefused(why.clone())),
        // Between-turn work, a refused stop, and what a save, a code send or
        // a rename came to move no run.
        Signal::CycleEnded
        | Signal::StopRefused(_)
        | Signal::SaveFailed(_)
        | Signal::CodeFailed(_)
        | Signal::RenameFailed(_) => None,
    }
}

/// What `signal`, from the session of `lane`, which is not the run's
/// current lane, tells the run, if anything: only a person's prompt there,
/// which takes the run over, and the session going. The rest is a lane the
/// run is not working in.
fn lane_input_of(
    signal: &Signal,
    lane: &Lane,
    drive: &RunDrive,
    session: &SessionDrive,
) -> Option<Input> {
    match signal {
        Signal::Appended => drive
            .heeds_lane_appended(lane, session)
            .then(|| Input::LaneAppended { lane: lane.clone() }),
        Signal::Disconnected(_) => Some(Input::LaneGone { lane: lane.clone() }),
        Signal::TurnEnded
        | Signal::AwaitingUser(_)
        | Signal::PickRefused(_)
        | Signal::CycleEnded
        | Signal::StopRefused(_)
        | Signal::SaveFailed(_)
        | Signal::CodeFailed(_)
        | Signal::RenameFailed(_) => None,
    }
}

/// The session of the run's lane `lane` changed in some way: what that
/// tells the run, which is only something in the lane it works in, when a
/// card it waits on may have been answered, or work a turn left running may
/// have settled.
pub fn notified(drive: &RunDrive, lane: &Lane, session: &SessionDrive) -> Option<Input> {
    (drive.current_lane() == lane && drive.heeds_notified(session)).then_some(Input::Notified)
}

/// The run's prompt, sent to its session: what the session's carrier does
/// with it, and what to tell the run when the session did not take it. A
/// run's prompt never queues.
fn submit(session: &mut SessionDrive, text: &str, now: Now) -> (Option<Input>, Vec<SessionEffect>) {
    let (intake, fx) = session.prompt(text, &[], Origin::Run, Code::Here, now);
    ((intake != Intake::Sent).then_some(Input::NotTaken), fx)
}

#[cfg(test)]
mod tests;
