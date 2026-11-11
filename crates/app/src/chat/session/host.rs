//! A session whose agent runs on a host: where its events come from.
//!
//! The attach and every call to the host are the ACP runtime's, and come back
//! here through oneshots awaited in the session's own task, so the pump reads
//! a host session's events exactly as it reads a local adapter's.

use super::{ChatEvent, ChatSession};
use crate::state::Shared;
use gpui::{AsyncApp, WeakEntity};
use onehand_core::acp::Opening;
use onehand_core::chat::Chat;
use onehand_core::config::AgentSpec;
use onehand_core::host::client::{HostConn, HostError};
use onehand_core::host::copy;
use onehand_core::host::session::host_restart;
use onehand_core::host::session::{
    Carried, HostBegin, OfflineStart, Restarted, offline_start, reads_copy,
};
use onehand_core::host::wire::SessionInfo;
use onehand_core::workspace::HostName;
use onehand_io::acp::Events;
use onehand_io::acp::HostStart;
use std::path::PathBuf;

/// Which host a session runs on, in which folder there, and how its events
/// begin there.
pub(crate) struct BeginOnHost {
    pub(crate) host: HostName,
    pub(crate) cwd: PathBuf,
    pub(crate) begin: HostBegin,
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
        HostBegin::Refuse(why) => return onehand_io::acp::ended(why.into()),
        HostBegin::Open => Ok(HostStart::Open { cwd, spec, opening }),
        HostBegin::Watch(id) => Ok(HostStart::Watch(id)),
        HostBegin::Rewatch(id, carried) => Err((id, carried, cwd, spec)),
    };
    let attaching = cx.update(|cx| crate::hosts::conn(cx, &host));
    let conn = match attaching.await {
        Ok(conn) => conn,
        Err(failure) => {
            let known = match &decided {
                Ok(HostStart::Watch(id)) | Err((id, ..)) => Some(id),
                Ok(HostStart::Open { .. }) => None,
            };
            let kept = match reads_copy(&failure, known).cloned() {
                Some(id) => {
                    let host = host.clone();
                    cx.background_executor()
                        .spawn(
                            async move { copy::Copy::open(&copy::copies_dir(), &host, &id).read() },
                        )
                        .await
                }
                None => Vec::new(),
            };
            return match offline_start(kept.len(), failure) {
                OfflineStart::Read { away, ends } => {
                    offline(host, &kept, away, session, cx);
                    match ends {
                        Some(why) => onehand_io::acp::ended(why),
                        None => onehand_io::acp::silent(),
                    }
                }
                OfflineStart::End(why) => {
                    if let Err((_, carried, _, _)) = decided {
                        carry(carried, session, cx);
                    }
                    onehand_io::acp::ended(why)
                }
                OfflineStart::Wait { host, busy, terms } => {
                    if let Err((_, carried, _, _)) = decided {
                        carry(carried, session, cx);
                    }
                    wait(host, busy, terms, session, cx);
                    onehand_io::acp::silent()
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
                    return onehand_io::acp::ended(why);
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
    let _ = session.update(cx, |session, cx| {
        let fx = session.chat.read_back(
            copy::events(kept),
            away.then_some((host, last)),
            super::now(),
        );
        session.perform(fx, cx);
        cx.notify();
    });
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
        let fx = session.chat.wait_on_host(host, n, terms);
        cx.emit(ChatEvent::HostBusy);
        session.perform(fx, cx);
        cx.notify();
    });
}
