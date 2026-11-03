use super::host::{host_restart, new_chat};
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

/// A host session the host still runs is read again, not opened anew.
#[test]
fn restart_rewatches_a_running_host_session() {
    let info = listed(false);
    let start = host_restart(
        Some(&info),
        Opening::Resume("acp-1".into()),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    );
    assert!(
        matches!(&start, HostStart::Watch(id) if id.0 == "h-1"),
        "a running session is watched"
    );
}

/// One that ended, or that the host no longer lists, opens a new host session
/// on the agent's own conversation, or a fresh one when there is none.
#[test]
fn restart_resumes_an_ended_host_session() {
    let info = listed(true);
    let start = host_restart(
        Some(&info),
        Opening::Resume("acp-1".into()),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    );
    assert!(matches!(
        &start,
        HostStart::Open { opening: Opening::Resume(id), cwd, .. }
            if id == "acp-1" && cwd.as_path() == std::path::Path::new("/srv/app")
    ));

    let start = host_restart(
        None,
        Opening::Fresh,
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
