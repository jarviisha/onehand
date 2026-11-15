//! The link between a run and the session it is driven on: what the session
//! tells the run, and what the run asks of its session.
//!
//! **Routing is behaviour, so it lives here and not in a front end.** Which
//! of a session's signals moves a run, which streamed chunk is worth asking it
//! about, that a run's picks are made before its notes and its prompt, that
//! its prompt never queues and a refused one fails it, and that only a
//! running turn is cancelled: each is a rule a front end would otherwise
//! restate in its event handlers, untested. A front end joins the two drives
//! it holds for one call ([`Joined`]), and carries out what comes back
//! ([`Out`]): the session's effects, then the run's.

use super::{Asked, Effect, Input, RunDrive};
use crate::chat::drive::{Code, Effect as SessionEffect, Intake, Origin, SessionDrive, Signal};
use crate::chat::Chat;
use crate::workflow::{Action, ApprovalAt, Run};
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
    /// The session is named for the task unless it has a name already, so the
    /// rail tells runs apart rather than calling each by the opening line of
    /// its first prompt. Before that prompt the name is only held, and the
    /// first turn's save writes it.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        task: String,
        run: Run,
        first: Action,
        pinned: usize,
        issue: bool,
        now: Now,
        mut session: Option<&mut SessionDrive>,
    ) -> (RunDrive, Out) {
        let mut out = Out::default();
        if let Some(session) = session.as_deref_mut() {
            // Named or not, a session a run starts on is drawn again.
            out.told = true;
            if session.custom_title.is_none() {
                out.session = session.rename(&run.brief.title).1;
            }
        }
        let chat = session.as_deref().map(|session| &**session);
        let (mut drive, fx) = RunDrive::start(task, run, first, pinned, issue, now, chat);
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

    /// A person sent what they read at `at` back with `note`.
    pub fn revise(&mut self, at: ApprovalAt, note: String, now: Now) -> Out {
        self.give(now, |run, chat| run.revise(at, note, chat, now))
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
    /// the rest in order, until it asks nothing more: a prompt the session
    /// does not take is told to the run at once, which may ask again.
    fn settle(&mut self, fx: Vec<Effect>, now: Now, out: &mut Out) {
        out.run.extend(fx);
        loop {
            let picks = self.run.take_picks();
            let asked = self.run.take_asked();
            if picks.is_empty() && asked.is_empty() {
                return;
            }
            // Without a session there is nothing to pick on or say to, and
            // no one to take a prompt.
            let Some(session) = self.session.as_deref_mut() else {
                for asked in asked {
                    match asked {
                        Asked::Prompt(_) => {
                            out.run.extend(self.run.handle(Input::NotTaken, None, now))
                        }
                        Asked::Note(_) | Asked::Cancel => {}
                    }
                }
                continue;
            };
            for pick in &picks {
                out.session.extend(session.pick(pick));
            }
            for asked in asked {
                out.told = true;
                match asked {
                    Asked::Note(text) => out.session.extend(session.note(text)),
                    Asked::Cancel => out.session.extend(cancel(session)),
                    Asked::Prompt(text) => {
                        let (answer, fx) = submit(session, &text, now);
                        out.session.extend(fx);
                        if let Some(input) = answer {
                            out.run
                                .extend(self.run.handle(input, Some(&**session), now));
                        }
                    }
                }
            }
        }
    }
}

/// What `signal`, from the session `drive` runs on, tells the run, if
/// anything.
///
/// A streamed chunk is asked about first, read-only: most of what a session
/// says is its reply streaming in, and a chunk that cannot move the run must
/// not tell everything watching the run that it changed.
pub fn input_of(signal: &Signal, drive: &RunDrive, session: &SessionDrive) -> Option<Input> {
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

/// The session changed in some way: what that tells the run, which is only
/// something when a card it waits on may have been answered, or work a turn
/// left running may have settled.
pub fn notified(drive: &RunDrive, session: &SessionDrive) -> Option<Input> {
    drive.heeds_notified(session).then_some(Input::Notified)
}

/// The run's prompt, sent to its session: what the session's carrier does
/// with it, and what to tell the run when the session did not take it. A
/// run's prompt never queues.
fn submit(session: &mut SessionDrive, text: &str, now: Now) -> (Option<Input>, Vec<SessionEffect>) {
    let (intake, fx) = session.prompt(text, &[], Origin::Run, Code::Here, now);
    ((intake != Intake::Sent).then_some(Input::NotTaken), fx)
}

/// The run's cancel: only a turn that runs is stopped.
fn cancel(session: &mut SessionDrive) -> Vec<SessionEffect> {
    match session.busy {
        true => session.stop(),
        false => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
