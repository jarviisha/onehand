use super::*;
use crate::chat::{Chat, SessionDrive};
use crate::Now;
use std::path::PathBuf;

fn now() -> Now {
    Now {
        secs: 0,
        at: std::time::Instant::now(),
    }
}

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
    let mut chat = Chat::new(1, PathBuf::from("/srv/app"), "claude".into(), None);
    chat.session_id = Some("acp-1".into());
    Carried {
        stored: SessionDrive::new(chat).take_snapshot(Now {
            secs: 0,
            at: std::time::Instant::now(),
        }),
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

/// A host out of reach shows what the laptop kept of the session when it
/// kept anything; with nothing kept it ends as before; a host busy on another
/// build is waited on whatever was kept.
#[test]
fn offline_start_reads_the_copy_only_when_there_is_one() {
    let away = || HostFailure::Other("Could not reach vps".into());
    assert_eq!(
        offline_start(3, away()),
        OfflineStart::Read(AfterCopy::Away)
    );
    assert!(!offline_start(3, away()).keeps_carried());
    assert!(offline_start(0, away()).keeps_carried());
    assert_eq!(
        offline_start(0, away()),
        OfflineStart::End("Could not reach vps".into())
    );
    let busy = || HostFailure::Busy {
        host: crate::workspace::HostName("vps".into()),
        busy: vec![HostSession("h-1".into())],
        terms: 2,
    };
    let waits = OfflineStart::Wait {
        host: crate::workspace::HostName("vps".into()),
        busy: 1,
        terms: 2,
    };
    assert_eq!(offline_start(3, busy()), waits);
    assert_eq!(offline_start(0, busy()), waits);
    assert!(waits.keeps_carried());
    // A host refused, or held by another machine, reads the copy too, and
    // ends on why rather than being waited on.
    let refused = || HostFailure::Refused("vps is not a machine this build runs on".into());
    assert_eq!(
        offline_start(3, refused()),
        OfflineStart::Read(AfterCopy::Ends(
            "vps is not a machine this build runs on".into()
        ))
    );
    assert!(matches!(offline_start(0, refused()), OfflineStart::End(_)));
}

/// A busy host is waited on whatever was kept; any other failure reads the
/// copy of the host session it knows.
#[test]
fn only_a_host_out_of_reach_refused_or_taken_reads_its_copy() {
    let id = HostSession("h-1".into());
    let busy = HostFailure::Busy {
        host: crate::workspace::HostName("vps".into()),
        busy: Vec::new(),
        terms: 0,
    };
    assert_eq!(reads_copy(&busy, Some(&id)), None);
    assert_eq!(
        reads_copy(&HostFailure::Taken("x".into()), Some(&id)),
        Some(&id)
    );
    assert_eq!(reads_copy(&HostFailure::Other("x".into()), None), None);
}

/// Only a restart reads a host session again. A conversation picked from the
/// history opens anew on it, and a listed session opened is watched; what a
/// re-watch would carry rides with it rather than being adopted at once.
#[test]
fn only_a_restart_rewatches_a_host_session() {
    type Start = super::Start<()>;
    let snapshot = || {
        let mut chat = Chat::new(1, std::path::PathBuf::from("/srv/app"), "c".into(), None);
        chat.session_id = Some("acp-1".into());
        SessionDrive::new(chat).take_snapshot(now())
    };
    let id = HostSession("h-1".into());

    let (begin, start, sent) = host_begin(Start::Restart(snapshot(), true), Some(&id), 3);
    assert_eq!(
        sent, 0,
        "a re-watch counts its prompts again from the journal"
    );
    assert!(matches!(
        &begin,
        HostBegin::Rewatch(known, carried)
            if *known == id && carried.stored.is_some() && carried.sent == 3 && carried.after_service
    ));
    assert!(matches!(start, Start::Fresh), "nothing adopted at once");

    let picked = snapshot().expect("a snapshot");
    let (begin, start, _) = host_begin(Start::Resume(picked), Some(&id), 3);
    assert!(matches!(begin, HostBegin::Open));
    assert!(matches!(start, Start::Resume(_)));

    let (begin, start, _) = host_begin(Start::Restart(None, false), None, 0);
    assert!(matches!(begin, HostBegin::Open));
    assert!(matches!(start, Start::Restart(None, false)));

    let (begin, start, _) = host_begin(Start::Watch(id.clone()), None, 0);
    assert!(matches!(&begin, HostBegin::Watch(known) if *known == id));
    assert!(matches!(start, Start::Fresh));

    // A fork is refused there, and kept to be adopted as a failed one.
    let (begin, start, _) = host_begin(Start::Fork(()), Some(&id), 0);
    assert!(matches!(begin, HostBegin::Refuse("Not on a host yet")));
    assert!(matches!(start, Start::Fork(())));
}

/// A start opens anew, on the conversation it names, or as its fork says.
#[test]
fn a_start_opens_as_it_says() {
    type Start = super::Start<&'static str>;
    let fork = |from: &&str| Opening::Resume(from.to_string());
    assert!(matches!(Start::Fresh.opening(fork), Opening::Fresh));
    assert!(matches!(
        Start::Restart(None, false).opening(fork),
        Opening::Fresh
    ));
    let stored = SessionDrive::new({
        let mut chat = Chat::new(1, PathBuf::from("/p"), "c".into(), None);
        chat.session_id = Some("acp-1".into());
        chat
    })
    .take_snapshot(now())
    .unwrap();
    assert!(matches!(Start::Resume(stored).opening(fork), Opening::Resume(id) if id == "acp-1"));
    assert!(matches!(Start::Fork("orig").opening(fork), Opening::Resume(id) if id == "orig"));
}
