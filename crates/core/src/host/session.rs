//! How a session on a host begins: on a new host session, on one the host
//! keeps, again after a restart, or from the copy this machine kept when the
//! host cannot be reached.
//!
//! What the front end hands its runtime to begin is [`HostStart`]; which one
//! is decided here, from how the conversation starts ([`Start`]), the host
//! session it was on, what the host lists, and why an attach failed.

use super::wire::{HostSession, SessionInfo};
use super::HostFailure;
use crate::acp::Opening;
use crate::chat::ConversationSnapshot;
use crate::config::AgentSpec;
use std::path::PathBuf;

/// How a host session's events begin.
pub enum HostStart {
    /// Start a new session on the host.
    Open {
        cwd: PathBuf,
        spec: AgentSpec,
        opening: Opening,
    },
    /// Read one the host already keeps.
    Watch(HostSession),
}

/// How a session's conversation starts; `F` is what a fork starts from.
pub enum Start<F> {
    Fresh,
    /// On a saved conversation, resumed.
    Resume(ConversationSnapshot),
    /// As a fork of another.
    Fork(F),
    /// On a host session the host already keeps.
    Watch(HostSession),
    /// Again on what it was on, the conversation taken from the session
    /// being replaced: a restart; `true` when it follows the host service's
    /// own restart.
    Restart(Option<ConversationSnapshot>, bool),
}

impl<F> Start<F> {
    /// How the agent opens the conversation this start is on: anew, on the
    /// conversation a saved or restarted one names, or as the fork `fork`
    /// says. A watch opens nothing of its own: the host's session is read.
    pub fn opening(&self, fork: impl FnOnce(&F) -> Opening) -> Opening {
        match self {
            Self::Fresh | Self::Watch(_) | Self::Restart(None, _) => Opening::Fresh,
            Self::Resume(stored) | Self::Restart(Some(stored), _) => {
                Opening::Resume(stored.session_id.clone())
            }
            Self::Fork(from) => fork(from),
        }
    }
}

/// How a session in a host project begins on the host, given how its
/// conversation starts, the host session it was on and the prompts it sent;
/// with the start that is left for the front end to adopt at once, and the
/// prompts the new session counts as sent from the start.
///
/// Only a restart reads the host session again. What it took from the old
/// session rides with the re-watch, adopted only if the host has ended the
/// session and a new one opens in its place: a re-watch replays the whole
/// journal, prompts and all, and a copy adopted beside it is shown twice; so
/// a re-watch counts its prompts again as the journal replays them, and what
/// it carries is for an open in its place.
pub fn host_begin<F>(
    start: Start<F>,
    known: Option<&HostSession>,
    sent: usize,
) -> (HostBegin, Start<F>, usize) {
    match (start, known) {
        (Start::Watch(id), _) => (HostBegin::Watch(id), Start::Fresh, sent),
        (Start::Restart(stored, after_service), Some(id)) => (
            HostBegin::Rewatch(
                id.clone(),
                Carried {
                    stored,
                    sent,
                    after_service,
                },
            ),
            Start::Fresh,
            0,
        ),
        // Adopted as a local fork is, so the refusal ends it as a failed one.
        (start @ Start::Fork(_), _) => (HostBegin::Refuse("Not on a host yet"), start, sent),
        (start @ (Start::Fresh | Start::Resume(_) | Start::Restart(..)), _) => {
            (HostBegin::Open, start, sent)
        }
    }
}

/// How a host session's events begin.
pub enum HostBegin {
    /// A new host session, opened as the session's opening says.
    Open,
    /// The host session named, read whatever state it is in.
    Watch(HostSession),
    /// The host session named, read again while the host still runs it;
    /// otherwise a new one, on what was carried.
    Rewatch(HostSession, Carried),
    /// Nothing begins, for this reason.
    Refuse(&'static str),
}

/// What a restart took from the session it replaces, for a host session
/// opened in place of one the host has ended.
pub struct Carried {
    pub stored: Option<ConversationSnapshot>,
    /// The prompts the old session had sent.
    pub sent: usize,
    /// Whether the restart follows the host service's own, which ended every
    /// session it ran: one on screen with nothing carried had not shown yet.
    pub after_service: bool,
}

/// How a host session read from its copy goes on.
#[derive(Debug, PartialEq, Eq)]
pub enum AfterCopy {
    /// Out of reach: it says so, and waits silently to be restarted.
    Away,
    /// Its host refused it, or another machine holds it: not waited on, it
    /// ends on why, as a lost agent Restart tries again.
    Ends(String),
}

/// How a host session whose host could not be attached begins.
#[derive(Debug, PartialEq, Eq)]
pub enum OfflineStart {
    /// From the copy this machine kept of its journal, the whole transcript,
    /// so what a re-watch carried is left out rather than shown twice.
    Read(AfterCopy),
    /// It ends on why the attach failed: nothing was kept to show, so what a
    /// re-watch carried is kept on screen.
    End(String),
    /// It waits on the host's `busy` sessions and `terms` terminals, as any
    /// session on it does, what a re-watch carried kept on screen.
    Wait {
        host: crate::workspace::HostName,
        busy: usize,
        terms: usize,
    },
}

/// Which kept copy a host session whose host could not be attached reads
/// (`known`, the host session it is on, if it is known): one out of reach,
/// refused or held by another machine is read from what this machine kept
/// of it; a busy host is waited on whatever was kept.
pub fn reads_copy<'a>(
    failure: &HostFailure,
    known: Option<&'a HostSession>,
) -> Option<&'a HostSession> {
    match failure {
        HostFailure::Busy { .. } => None,
        HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_) => known,
    }
}

/// How a host session begins when its host could not be attached, given how
/// many entries this machine kept of its journal and why the attach failed.
pub fn offline_start(kept: usize, failure: HostFailure) -> OfflineStart {
    match failure {
        HostFailure::Busy { host, busy, terms } => OfflineStart::Wait {
            host,
            busy: busy.len(),
            terms,
        },
        HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why)
            if kept == 0 =>
        {
            OfflineStart::End(why)
        }
        HostFailure::Other(_) => OfflineStart::Read(AfterCopy::Away),
        HostFailure::Refused(why) | HostFailure::Taken(why) => {
            OfflineStart::Read(AfterCopy::Ends(why))
        }
    }
}

impl OfflineStart {
    /// Whether what a re-watch carried is kept on screen: only when no copy
    /// is read, since the copy is the whole transcript.
    pub fn keeps_carried(&self) -> bool {
        match self {
            OfflineStart::Read(_) => false,
            OfflineStart::End(_) | OfflineStart::Wait { .. } => true,
        }
    }
}

/// What a restart on a host comes to.
pub enum Restarted {
    /// It begins so, with what was carried to adopt, if anything.
    Begin(HostStart, Option<Carried>),
    /// It ends on why, with what was carried to keep on screen.
    Ended(String, Carried),
}

/// How a restarted host session begins, given what the host lists for the
/// session it was on: read again while the host runs it, with nothing
/// carried, since its journal is the whole transcript; otherwise opened on
/// the agent's own conversation (or a fresh one), with what was carried to
/// adopt until the agent's replay lands. An ended one with nothing carried,
/// after the host service's own restart, is read from its journal instead,
/// so it is shown rather than replaced; any other restart with nothing
/// carried had an agent that never got going, and tries again.
///
/// A host that could not say what it runs ends the restart on why, with what
/// was carried handed back to keep on screen: opening anyway could start a
/// second session beside one that still runs.
pub fn host_restart(
    listed: Result<Option<&SessionInfo>, String>,
    carried: Carried,
    cwd: PathBuf,
    spec: AgentSpec,
) -> Restarted {
    match listed {
        Err(why) => Restarted::Ended(why, carried),
        Ok(Some(info)) if !info.ended => {
            Restarted::Begin(HostStart::Watch(info.session.clone()), None)
        }
        // Ended with nothing carried by the service's restart: the session
        // never showed here (that restart came first), so its journal is
        // read rather than a blank one opened.
        Ok(Some(info)) if carried.stored.is_none() && carried.after_service => {
            Restarted::Begin(HostStart::Watch(info.session.clone()), None)
        }
        Ok(Some(_) | None) => {
            let opening = carried.stored.as_ref().map_or(Opening::Fresh, |stored| {
                Opening::Resume(stored.session_id.clone())
            });
            Restarted::Begin(HostStart::Open { cwd, spec, opening }, Some(carried))
        }
    }
}

#[cfg(test)]
mod tests;
