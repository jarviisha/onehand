use super::host::{
    Carried, OfflineStart, Restarted, host_restart, new_chat, offline_line, offline_start,
};
use crate::acp::HostStart;
use crate::hosts::HostFailure;
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
        after_service: false,
    }
}

/// A host session the host still runs is read again, not opened anew, and
/// nothing is carried into it: its journal is the whole transcript, prompts
/// and all.
#[test]
fn restart_rewatches_a_running_host_session() {
    let info = listed(false);
    let Restarted::Begin(start, carry) = host_restart(
        Ok(Some(&info)),
        carried(),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) else {
        panic!("the host listed it");
    };
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
    let Restarted::Begin(start, carry) = host_restart(
        Ok(Some(&info)),
        carried(),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) else {
        panic!("the host listed it");
    };
    assert!(matches!(
        &start,
        HostStart::Open { opening: Opening::Resume(id), cwd, .. }
            if id == "acp-1" && cwd.as_path() == std::path::Path::new("/srv/app")
    ));
    let carry = carry.expect("an open carries what the restart took");
    assert!(carry.stored.is_some());
    assert_eq!(carry.sent, 2);

    let Restarted::Begin(start, _) = host_restart(
        Ok(None),
        Carried {
            stored: None,
            sent: 0,
            after_service: false,
        },
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) else {
        panic!("the host listed none");
    };
    assert!(matches!(
        &start,
        HostStart::Open {
            opening: Opening::Fresh,
            ..
        }
    ));
}

/// One the host lists as ended, with nothing carried, after the host's
/// service restarted (a session pressed on the project page and restarted
/// by that before it ever showed), is read from its journal rather than
/// opened blank; a later Restart resumes it from what that journal showed.
#[test]
fn restart_reads_an_ended_host_session_when_nothing_was_carried() {
    let info = listed(true);
    let Restarted::Begin(start, carry) = host_restart(
        Ok(Some(&info)),
        Carried {
            stored: None,
            sent: 0,
            after_service: true,
        },
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) else {
        panic!("the host listed it");
    };
    assert!(
        matches!(&start, HostStart::Watch(id) if id.0 == "h-1"),
        "an ended session with nothing carried is read from its journal"
    );
    assert!(carry.is_none(), "its journal is the whole transcript");
}

/// Any other restart of an ended session with nothing carried (its agent was
/// not found, or died before it named a conversation) opens a fresh one: a
/// journal that ended so would only be read again, ended, at every Restart.
#[test]
fn an_ordinary_restart_of_an_ended_session_with_nothing_carried_opens_anew() {
    let info = listed(true);
    let Restarted::Begin(start, _) = host_restart(
        Ok(Some(&info)),
        Carried {
            stored: None,
            sent: 0,
            after_service: false,
        },
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) else {
        panic!("the host listed it");
    };
    assert!(
        matches!(
            &start,
            HostStart::Open {
                opening: Opening::Fresh,
                ..
            }
        ),
        "an ordinary restart opens anew"
    );
}

/// A host that cannot say what it runs ends the restart, saying why, rather
/// than open a second session beside one that may still run; what was
/// carried comes back to be kept on screen.
#[test]
fn a_restart_the_host_cannot_list_for_ends_on_the_error() {
    let (why, carry) = match host_restart(
        Err("Could not read the sessions on box: timed out".into()),
        carried(),
        PathBuf::from("/srv/app"),
        AgentSpec::default(),
    ) {
        Restarted::Begin(..) => panic!("a restart the host could not list for must not begin"),
        Restarted::Ended(why, carry) => (why, carry),
    };
    assert!(why.contains("box"), "{why}");
    assert!(carry.stored.is_some());
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

/// A host out of reach shows what the laptop kept of the session when it
/// kept anything; with nothing kept it ends as before; a host busy on another
/// build is waited on whatever was kept.
#[test]
fn offline_start_reads_the_copy_only_when_there_is_one() {
    let away = HostFailure::Other("Could not reach vps".into());
    assert_eq!(offline_start(3, &away), OfflineStart::Read);
    assert_eq!(offline_start(0, &away), OfflineStart::End);
    let busy = HostFailure::Busy {
        host: onehand_core::workspace::HostName("vps".into()),
        busy: Vec::new(),
    };
    assert_eq!(offline_start(3, &busy), OfflineStart::Wait);
    assert_eq!(offline_start(0, &busy), OfflineStart::Wait);
}

/// The copy's notice says how recent it is when its entries kept a time, and
/// leaves that out when none did.
#[test]
fn the_offline_line_says_how_recent_the_copy_is() {
    let vps = onehand_core::workspace::HostName("vps".into());
    assert_eq!(
        offline_line(&vps, Some(1_000), 1_000 + 3 * 3600),
        "Not connected to vps. This is the copy kept on this machine, up to 3h ago; \
         it carries on once the host is back"
    );
    assert_eq!(
        offline_line(&vps, None, 5_000),
        "Not connected to vps. This is the copy kept on this machine; \
         it carries on once the host is back"
    );
}
