//! A session whose agent runs on a host: where its events come from.
//!
//! The attach and every call to the host are the ACP runtime's, and come back
//! here through oneshots awaited in the session's own task, so the pump reads
//! a host session's events exactly as it reads a local adapter's.

use super::{ChatEvent, ChatSession};
use crate::acp::{Events, HostStart};
use crate::hosts::HostFailure;
use crate::state::Shared;
use gpui::{AsyncApp, WeakEntity};
use onehand_core::acp::{AcpEvent, Opening};
use onehand_core::chat::{Chat, ConversationSnapshot};
use onehand_core::config::AgentSpec;
use onehand_core::host::client::{HostConn, HostError};
use onehand_core::host::copy;
use onehand_core::host::wire::{HostSession, SessionInfo};
use onehand_core::workspace::HostName;
use std::path::PathBuf;

/// Which host a session runs on, in which folder there, and how its events
/// begin there.
pub(crate) struct BeginOnHost {
    pub(crate) host: HostName,
    pub(crate) cwd: PathBuf,
    pub(crate) begin: HostBegin,
}

/// How a host session's events begin.
pub(crate) enum HostBegin {
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
pub(crate) struct Carried {
    pub(crate) stored: Option<ConversationSnapshot>,
    /// The prompts the old session had sent.
    pub(crate) sent: usize,
    /// Whether the restart follows the host service's own, which ended every
    /// session it ran: one on screen with nothing carried had not shown yet.
    pub(crate) after_service: bool,
}

/// Where a session's events come from.
pub(super) enum Source {
    Local(Events),
    /// The host, and what to begin there once it is attached.
    Host(Box<(BeginOnHost, PathBuf, AgentSpec, Opening)>),
}

/// The transcript model a session starts from. A host session's transcript is
/// the host's to keep, so it is never written to this machine's store.
pub(super) fn new_chat(uid: u64, root: PathBuf, spec: &AgentSpec, on_host: bool) -> Chat {
    let store = (!on_host).then(onehand_core::chat::conversations_dir);
    let mut chat = Chat::new(uid, root, spec.name.clone(), store);
    chat.login_hint = spec.login_hint();
    chat
}

/// How a host session whose host could not be attached begins.
#[derive(Debug, PartialEq)]
pub(super) enum OfflineStart {
    /// From the copy this machine kept of its journal.
    Read,
    /// It ends on why the attach failed: nothing was kept to show.
    End,
    /// It waits on the host's busy sessions, as any session on it does.
    Wait,
}

/// How a host session begins when its host could not be attached, given how
/// many entries this machine kept of its journal and why the attach failed.
pub(super) fn offline_start(kept: usize, failure: &HostFailure) -> OfflineStart {
    match failure {
        HostFailure::Busy { .. } => OfflineStart::Wait,
        HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_) if kept == 0 => {
            OfflineStart::End
        }
        HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_) => {
            OfflineStart::Read
        }
    }
}

/// The line under a host session read from this machine's copy: how recent
/// the copy is when its entries kept a time, `last` (seconds since the
/// epoch), and that it carries on once the host is back.
pub(super) fn offline_line(host: &HostName, last: Option<u64>, now: u64) -> String {
    let upto = last.map_or_else(String::new, |at| {
        format!(", up to {}", onehand_core::rel_time(now, at))
    });
    format!(
        "Not connected to {host}. This is the copy kept on this machine{upto}; \
         it carries on once the host is back"
    )
}

/// What a restart on a host comes to.
pub(super) enum Restarted {
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
pub(super) fn host_restart(
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

/// The events of a host session, once its host is attached; a refusal or a
/// failure is the one `Disconnected` that says why.
pub(super) async fn host_events(
    (on_host, cwd, spec, opening): (BeginOnHost, PathBuf, AgentSpec, Opening),
    session: &WeakEntity<ChatSession>,
    cx: &mut AsyncApp,
) -> Events {
    let BeginOnHost { host, begin, .. } = on_host;
    // Either the start, known already, or what a re-watch decides on once
    // the host has said what it runs.
    let decided = match begin {
        HostBegin::Refuse(why) => return crate::acp::ended(why.into()),
        HostBegin::Open => Ok(HostStart::Open { cwd, spec, opening }),
        HostBegin::Watch(id) => Ok(HostStart::Watch(id)),
        HostBegin::Rewatch(id, carried) => Err((id, carried, cwd, spec)),
    };
    let attaching = cx.update(|cx| crate::hosts::conn(cx, &host));
    let conn = match attaching.await {
        Ok(conn) => conn,
        Err(failure) => {
            // A host session out of reach is read from what this machine
            // kept of it; a busy host is waited on whatever was kept.
            let known = match &decided {
                Ok(HostStart::Watch(id)) | Err((id, ..)) => Some(id.clone()),
                Ok(HostStart::Open { .. }) => None,
            };
            let kept = match (&failure, known) {
                (
                    HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_),
                    Some(id),
                ) => {
                    let host = host.clone();
                    cx.background_executor()
                        .spawn(
                            async move { copy::Copy::open(&copy::copies_dir(), &host, &id).read() },
                        )
                        .await
                }
                (
                    HostFailure::Other(_)
                    | HostFailure::Refused(_)
                    | HostFailure::Taken(_)
                    | HostFailure::Busy { .. },
                    _,
                ) => Vec::new(),
            };
            return match offline_start(kept.len(), &failure) {
                // The copy is the whole transcript, prompts and all, so what
                // a re-watch carried is left out rather than shown twice.
                // A host refused, or held by another machine, is not
                // waited on: what was kept is shown, and the session ends on
                // why, as a lost agent Restart tries again.
                OfflineStart::Read => match failure {
                    HostFailure::Refused(why) | HostFailure::Taken(why) => {
                        offline(host, &kept, false, session, cx);
                        crate::acp::ended(why)
                    }
                    HostFailure::Other(_) | HostFailure::Busy { .. } => {
                        offline(host, &kept, true, session, cx);
                        crate::acp::silent()
                    }
                },
                OfflineStart::End | OfflineStart::Wait => {
                    // No journal follows, so a re-watch's copy is kept on screen.
                    if let Err((_, carried, _, _)) = decided {
                        carry(carried, session, cx);
                    }
                    match failure {
                        HostFailure::Other(why)
                        | HostFailure::Refused(why)
                        | HostFailure::Taken(why) => crate::acp::ended(why),
                        HostFailure::Busy { host, busy, terms } => {
                            wait(host, busy.len(), terms, session, cx);
                            crate::acp::silent()
                        }
                    }
                }
            };
        }
    };
    let start = match decided {
        Ok(start) => start,
        Err((id, carried, cwd, spec)) => {
            let listed = list(conn.clone(), cx)
                .await
                .map_err(|e| format!("Could not read the sessions on {host}: {e}"));
            let found = listed
                .as_ref()
                .map(|listed| listed.iter().find(|info| info.session == id))
                .map_err(Clone::clone);
            match host_restart(found, carried, cwd, spec) {
                Restarted::Begin(start, carried) => {
                    if let Some(carried) = carried {
                        carry(carried, session, cx);
                    }
                    start
                }
                Restarted::Ended(why, carried) => {
                    carry(carried, session, cx);
                    return crate::acp::ended(why);
                }
            }
        }
    };
    let _ = session.update(cx, |session, _| {
        session.host_conn = Some((host.clone(), conn.clone()));
    });
    let (events, told) = cx.update(|cx| {
        Shared::global(cx)
            .acp
            .host_events(conn, host.clone(), start)
    });
    // Told before any event is forwarded, and dropped untold when the open
    // failed, in which case the events say so.
    if let Ok(id) = told.await {
        let _ = session.update(cx, |_, cx| cx.emit(ChatEvent::HostSession(id)));
    }
    events
}

/// Put what a restart took back into the session: the prompts it had sent
/// and the conversation it showed.
fn carry(
    Carried { stored, sent, .. }: Carried,
    session: &WeakEntity<ChatSession>,
    cx: &mut AsyncApp,
) {
    let _ = session.update(cx, |session, cx| {
        if sent > 0 {
            session.chat.carry_sent(sent);
        }
        if let Some(stored) = stored {
            session.adopt(stored, cx);
        }
    });
}

/// What the host lists, read on the ACP runtime.
pub(crate) async fn list(conn: HostConn, cx: &mut AsyncApp) -> Result<Vec<SessionInfo>, HostError> {
    cx.update(|cx| crate::hosts::on_runtime(cx, async move { conn.list().await }))
        .await
}

/// Show what this machine kept of the session's journal, and, `away`, the
/// line saying it is out of reach until the host is back. Applied to the chat directly rather than sent down the events,
/// so nothing that hears a session (a desktop notice, a run) takes the old
/// turns for new ones.
fn offline(
    host: HostName,
    kept: &onehand_core::host::journal::Entries,
    away: bool,
    session: &WeakEntity<ChatSession>,
    cx: &mut AsyncApp,
) {
    let last = kept.iter().rev().map(|(_, at, _)| *at).find(|at| *at != 0);
    let line = offline_line(&host, last, onehand_core::chat::now_secs());
    let _ = session.update(cx, |session, cx| {
        let fx = session
            .chat
            .read_back(copy::events(kept), away.then_some(line));
        if away {
            session.host_offline = Some(host);
        }
        session.perform(fx, cx);
        cx.notify();
    });
}

/// Whether `event` ends the session's watch because the connection to its
/// host was lost, closed without another machine taking it: then the session
/// says it is out of reach, rather than that its agent went, and keeps what
/// it has until the host is back. Decided on the connection, never on words.
///
/// The line that says so, for the drive's [`SessionDrive::host_lost`], or
/// `None` when the event is not such an end.
///
/// [`SessionDrive::host_lost`]: onehand_core::chat::SessionDrive::host_lost
pub(super) fn went_away(session: &mut ChatSession, event: &AcpEvent) -> Option<String> {
    if !matches!(event, AcpEvent::Disconnected(_)) {
        return None;
    }
    let (host, _) = session
        .host_conn
        .as_ref()
        .filter(|(_, conn)| connection_lost(conn.is_closed(), conn.taken()))?;
    let host = host.clone();
    let now = onehand_core::chat::now_secs();
    let line = offline_line(&host, Some(now), now);
    session.host_offline = Some(host);
    Some(line)
}

/// Whether a watch that ended did so because its connection was lost: closed,
/// and not by the host handing it to another machine. Open, it was the
/// session's own end.
pub(super) fn connection_lost(closed: bool, taken: bool) -> bool {
    closed && !taken
}

/// Say on the session that it waits on `host`'s `n` busy sessions and
/// `terms` open terminals, as a line of its own rather than a failure: once
/// they are done, it is restarted, and nothing has to be tried again by hand.
fn wait(
    host: HostName,
    n: usize,
    terms: usize,
    session: &WeakEntity<ChatSession>,
    cx: &mut AsyncApp,
) {
    let _ = session.update(cx, |session, cx| {
        let fx = session
            .chat
            .say_host_wait(crate::hosts::waiting_line(&host, n, terms));
        session.host_wait = Some(host);
        cx.emit(ChatEvent::HostBusy);
        session.perform(fx, cx);
        cx.notify();
    });
}
