//! The link between a run and the session it is driven on: what the session
//! tells the run, and what the run's effects on its session do there.
//!
//! **Routing is behaviour, so it lives here and not in a front end.** Which
//! of a session's signals moves a run, which streamed chunk is worth asking it
//! about, that a run's prompt never queues, and that only a running turn is
//! cancelled: each is a rule a front end would otherwise restate in its event
//! handlers, untested. A front end calls these with the two drives it holds
//! and carries out what comes back.

use super::{Input, RunDrive};
use crate::chat::drive::{Code, Effect as SessionEffect, Intake, Origin, SessionDrive, Signal};
use crate::chat::{Chat, Now};
use crate::workflow::{Action, Run};
use std::time::Instant;

/// Start driving `run` on `session`: what [`RunDrive::start`] answers, and what
/// putting the first step's settings in force asks of the session.
#[allow(clippy::too_many_arguments)]
pub fn start(
    task: String,
    run: Run,
    first: Action,
    pinned: usize,
    issue: bool,
    now: Instant,
    mut session: Option<&mut SessionDrive>,
) -> (RunDrive, Vec<super::Effect>, Vec<SessionEffect>) {
    let chat = session.as_deref_mut().map(SessionDrive::chat_mut);
    let (drive, fx) = RunDrive::start(task, run, first, pinned, issue, now, chat);
    (drive, fx, asked_of(session))
}

/// Tell `drive` what happened, with its session as it stands: what the run
/// does next, and what that asks of the session.
pub fn handle(
    drive: &mut RunDrive,
    input: Input,
    session: Option<&mut SessionDrive>,
    now: Instant,
) -> (Vec<super::Effect>, Vec<SessionEffect>) {
    command(drive, session, |drive, chat| drive.handle(input, chat, now))
}

/// Give `drive` a command, `give`, with its session as it stands: what the
/// run does next, and what that asks of the session.
pub fn command(
    drive: &mut RunDrive,
    mut session: Option<&mut SessionDrive>,
    give: impl FnOnce(&mut RunDrive, Option<&mut Chat>) -> Vec<super::Effect>,
) -> (Vec<super::Effect>, Vec<SessionEffect>) {
    let fx = give(drive, session.as_deref_mut().map(SessionDrive::chat_mut));
    (fx, asked_of(session))
}

/// What the run's drive queued for the agent on its session: a step's mode
/// and picks.
fn asked_of(session: Option<&mut SessionDrive>) -> Vec<SessionEffect> {
    session.map_or_else(Vec::new, |session| session.sent(Vec::new()))
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
        Signal::Disconnected => Some(Input::Disconnected),
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
pub fn submit(
    session: &mut SessionDrive,
    text: &str,
    now: Now,
) -> (Option<Input>, Vec<SessionEffect>) {
    let (intake, fx) = session.prompt(text, &[], Origin::Run, Code::Here, now);
    ((intake != Intake::Sent).then_some(Input::NotTaken), fx)
}

/// The run's cancel: only a turn that runs is stopped.
pub fn cancel(session: &mut SessionDrive) -> Vec<SessionEffect> {
    match session.busy {
        true => session.stop(),
        false => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
