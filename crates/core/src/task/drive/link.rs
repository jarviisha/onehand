//! The link between a run and the session it is driven on: what the session
//! tells the run, and what the run's effects on its session do there.
//!
//! **Routing is behaviour, so it lives here and not in a front end.** Which
//! of a session's signals moves a run, which streamed chunk is worth asking it
//! about, that a run's prompt never queues, and that only a running turn is
//! cancelled: each is a rule a front end would otherwise restate in its event
//! handlers, untested. A front end calls these with the two drives it holds
//! and carries out what comes back.

use super::{Drive, Input};
use crate::chat::drive::{Code, Effect as SessionEffect, Intake, Origin, SessionDrive, Signal};
use crate::workflow::{Action, Run};
use std::time::Instant;

/// Start driving `run` on `session`: what [`Drive::start`] answers.
#[allow(clippy::too_many_arguments)]
pub fn start(
    task: String,
    run: Run,
    first: Action,
    pinned: usize,
    issue: bool,
    now: Instant,
    session: Option<&mut SessionDrive>,
) -> (Drive, Vec<super::Effect>) {
    let chat = session.map(SessionDrive::chat_mut);
    Drive::start(task, run, first, pinned, issue, now, chat)
}

/// Tell `drive` what happened, with its session as it stands.
pub fn on(
    drive: &mut Drive,
    input: Input,
    session: Option<&mut SessionDrive>,
    now: Instant,
) -> Vec<super::Effect> {
    drive.on(input, session.map(SessionDrive::chat_mut), now)
}

/// What `signal`, from the session `drive` runs on, tells the run, if
/// anything.
///
/// A streamed chunk is asked about first, read-only: most of what a session
/// says is its reply streaming in, and a chunk that cannot move the run must
/// not tell everything watching the run that it changed.
pub fn input_of(signal: &Signal, drive: &Drive, session: &SessionDrive) -> Option<Input> {
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
pub fn notified(drive: &Drive, session: &SessionDrive) -> Option<Input> {
    drive.heeds_notified(session).then_some(Input::Notified)
}

/// The run's prompt, sent to its session: what the session's carrier does
/// with it, and what to tell the run when the session did not take it. A
/// run's prompt never queues.
pub fn submit(session: &mut SessionDrive, text: &str) -> (Option<Input>, Vec<SessionEffect>) {
    let (intake, fx) = session.prompt(text, &[], Origin::Run, Code::Here);
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
