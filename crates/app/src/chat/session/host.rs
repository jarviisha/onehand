//! A session whose agent runs on a host: where its events come from.
//!
//! The attach and every call to the host are the ACP runtime's, and come back
//! here through oneshots awaited in the session's own task, so the pump reads
//! a host session's events exactly as it reads a local adapter's.

use super::{ChatEvent, ChatSession};
use crate::acp::{Events, HostStart};
use crate::hosts::HostFailure;
use crate::state::Shared;
use futures::channel::oneshot;
use gpui::{AsyncApp, WeakEntity};
use onehand_core::acp::Opening;
use onehand_core::chat::Chat;
use onehand_core::config::AgentSpec;
use onehand_core::host::client::HostConn;
use onehand_core::host::wire::{HostSession, SessionInfo};
use onehand_core::workspace::HostName;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Which host a session runs on, and how its events begin there.
pub(crate) struct OnHost {
    pub(crate) host: HostName,
    pub(crate) begin: HostBegin,
}

/// How a host session's events begin.
pub(crate) enum HostBegin {
    /// A new host session, opened as the session's opening says.
    Open,
    /// The host session named, read whatever state it is in.
    Watch(HostSession),
    /// The host session named, read again while the host still runs it;
    /// otherwise a new one, opened as the session's opening says.
    Rewatch(HostSession),
}

/// The attach to a host, under way.
pub(super) type Attaching = Pin<Box<dyn Future<Output = Result<HostConn, HostFailure>>>>;

/// Where a session's events come from.
pub(super) enum Source {
    Local(Events),
    /// The attach, then what to begin once it lands.
    Host(Box<(Attaching, (OnHost, PathBuf, AgentSpec, Opening))>),
}

/// The transcript model a session starts from. A host session's transcript is
/// the host's to keep, so it is never written to this machine's store.
pub(super) fn new_chat(uid: u64, root: PathBuf, spec: &AgentSpec, on_host: bool) -> Chat {
    let store = (!on_host).then(onehand_core::chat::conversations_dir);
    let mut chat = Chat::new(uid, root, spec.name.clone(), store);
    chat.login_hint = spec.login_hint();
    chat
}

/// How a restarted host session begins, given what the host lists for the
/// session it was on: read again while the host runs it, otherwise opened as
/// `opening` says (the agent's own conversation resumed, or a fresh one).
pub(super) fn host_restart(
    info: Option<&SessionInfo>,
    opening: Opening,
    cwd: PathBuf,
    spec: AgentSpec,
) -> HostStart {
    match info {
        Some(info) if !info.ended => HostStart::Watch(info.session.clone()),
        Some(_) | None => HostStart::Open { cwd, spec, opening },
    }
}

/// The events of a host session, once `attached` has landed; a failure is
/// the one `Disconnected` that says why.
pub(super) async fn host_events(
    attached: Result<HostConn, HostFailure>,
    (on_host, cwd, spec, opening): (OnHost, PathBuf, AgentSpec, Opening),
    session: &WeakEntity<ChatSession>,
    cx: &mut AsyncApp,
) -> Events {
    let conn = match attached {
        Ok(conn) => conn,
        Err(failure) => return crate::acp::ended(said(failure, session, cx)),
    };
    let start = match on_host.begin {
        HostBegin::Open => HostStart::Open { cwd, spec, opening },
        HostBegin::Watch(id) => HostStart::Watch(id),
        HostBegin::Rewatch(id) => {
            let listed = list(conn.clone(), cx).await;
            host_restart(
                listed.iter().find(|info| info.session == id),
                opening,
                cwd,
                spec,
            )
        }
    };
    let (events, told) = cx.update(|cx| Shared::global(cx).acp.host_events(conn, start));
    // Told before any event is forwarded, and dropped untold when the open
    // failed, in which case the events say so.
    if let Ok(id) = told.await {
        let _ = session.update(cx, |_, cx| cx.emit(ChatEvent::HostSession(id)));
    }
    events
}

/// What the host lists, read on the ACP runtime; nothing when it cannot say.
async fn list(conn: HostConn, cx: &mut AsyncApp) -> Vec<SessionInfo> {
    let (tx, rx) = oneshot::channel();
    cx.update(|cx| {
        Shared::global(cx).acp.spawn(async move {
            let _ = tx.send(conn.list().await);
        })
    });
    rx.await.ok().and_then(Result::ok).unwrap_or_default()
}

/// The sentence a failed attach ends the session on. A busy host is also
/// told to the pane, which asks whether to restart it.
fn said(failure: HostFailure, session: &WeakEntity<ChatSession>, cx: &mut AsyncApp) -> String {
    match failure {
        HostFailure::Missing(why) | HostFailure::Other(why) => why,
        HostFailure::Busy { host, build, busy } => {
            let _ = session.update(cx, |_, cx| cx.emit(ChatEvent::HostBusy { build, busy }));
            format!("{host} runs another build of the host service, with sessions mid-turn")
        }
    }
}
