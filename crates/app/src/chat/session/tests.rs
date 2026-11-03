use super::host::{Carried, host_restart, new_chat};
use crate::acp::HostStart;
use onehand_core::acp::Opening;
use onehand_core::chat::ChatItem;
use onehand_core::config::AgentSpec;
use onehand_core::host::wire::{HostSession, SessionInfo};
use std::path::PathBuf;

fn listed(ended: bool) -> SessionInfo {
    SessionInfo {
        session: HostSession("h-1".into()),
        cwd: PathBuf::from("/srv/app"),
        agent: "claude".into(),
        busy: false,
        ended,
    }
}

/// What was carried from the session being restarted: a snapshot of a
/// conversation the agent knows as `acp-1`, and two prompts sent.
fn carried() -> Carried {
    let mut chat = new_chat(1, PathBuf::from("/srv/app"), &AgentSpec::default(), true);
    chat.session_id = Some("acp-1".into());
    Carried {
        stored: chat.take_snapshot(),
        sent: 2,
    }
}

/// A host session the host still runs is read again, not opened anew, and
/// nothing is carried into it: its journal is the whole transcript, prompts
/// and all.
#[test]
fn restart_rewatches_a_running_host_session() {
    let info = listed(false);
    let (start, carry) = host_restart(
        Some(&info),
        carried(),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    );
    assert!(
        matches!(&start, HostStart::Watch(id) if id.0 == "h-1"),
        "a running session is watched"
    );
    assert!(carry.is_none(), "nothing adopted, no prompts carried");
}

/// One that ended, or that the host no longer lists, opens a new host session
/// on the agent's own conversation, which is adopted until its replay lands;
/// with none, a fresh one.
#[test]
fn restart_resumes_an_ended_host_session() {
    let info = listed(true);
    let (start, carry) = host_restart(
        Some(&info),
        carried(),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    );
    assert!(matches!(
        &start,
        HostStart::Open { opening: Opening::Resume(id), cwd, .. }
            if id == "acp-1" && cwd.as_path() == std::path::Path::new("/srv/app")
    ));
    let carry = carry.expect("an open carries what the restart took");
    assert!(carry.stored.is_some());
    assert_eq!(carry.sent, 2);

    let (start, _) = host_restart(
        None,
        Carried {
            stored: None,
            sent: 0,
        },
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    );
    assert!(matches!(
        &start,
        HostStart::Open {
            opening: Opening::Fresh,
            ..
        }
    ));
}

/// A host session's transcript is the host's to keep: nothing of it is
/// written to this machine's store.
#[test]
fn a_host_chat_has_no_store() {
    let spec = AgentSpec {
        name: "claude".into(),
        ..AgentSpec::default()
    };
    let mut on_host = new_chat(1, PathBuf::from("/srv/app"), &spec, true);
    let mut local = new_chat(2, PathBuf::from("/tmp/app"), &spec, false);
    for chat in [&mut on_host, &mut local] {
        chat.session_id = Some("acp-1".into());
        chat.items.push(ChatItem::notice("said".to_string()));
    }
    assert!(on_host.flush().is_none(), "a host chat writes nothing");
    assert!(local.flush().is_some(), "a local chat writes its turn");
}
