use super::host::{
    Carried, OfflineStart, Restarted, connection_lost, host_restart, new_chat, offline_line,
    offline_start,
};
use crate::acp::HostStart;
use crate::hosts::HostFailure;
use onehand_core::acp::Opening;
use onehand_core::chat::drive::Effect;
use onehand_core::chat::{ChatItem, SessionDrive};
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
        stored: onehand_core::chat::SessionDrive::new(chat).take_snapshot(),
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
    // A turn's end is when a transcript is written. The write is only built
    // here, and building it moves the mark, so the drop writes nothing either.
    let saves = |on_host: bool| {
        let mut chat = new_chat(1, PathBuf::from("/srv/app"), &spec, on_host);
        chat.session_id = Some("acp-1".into());
        chat.items.push(ChatItem::notice("said".to_string()));
        let mut drive = SessionDrive::new(chat);
        let (fx, _) = drive.apply(onehand_core::acp::AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });
        fx.iter().any(|effect| matches!(effect, Effect::Save(_)))
    };
    assert!(!saves(true), "a host chat writes nothing");
    assert!(saves(false), "a local chat writes its turn");
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
        terms: 0,
    };
    assert_eq!(offline_start(3, &busy), OfflineStart::Wait);
    assert_eq!(offline_start(0, &busy), OfflineStart::Wait);
    // A host refused reads the copy too; that it is not waited on is the
    // caller's.
    let refused = HostFailure::Refused("vps is not a machine this build runs on".into());
    assert_eq!(offline_start(3, &refused), OfflineStart::Read);
    assert_eq!(offline_start(0, &refused), OfflineStart::End);
}

/// A watch ends on a lost connection only when the connection closed without
/// another machine taking the host; open, the session itself ended.
#[test]
fn a_watch_ended_by_a_lost_connection_is_told_from_the_others() {
    assert!(connection_lost(true, false));
    assert!(!connection_lost(true, true), "taken");
    assert!(!connection_lost(false, false), "the session's own end");
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

// The session's carrier: what it does with the drive's effects, held to its
// contract with a test context and a channel standing in for the adapter.

mod carrier {
    use super::super::{ChatEvent, ChatSession, Source};
    use crate::acp::Events;
    use futures::channel::mpsc;
    use gpui::{AppContext as _, Entity, TestAppContext};
    use onehand_core::acp::{AcpEvent, ReqTx};
    use onehand_core::chat::Chat;
    use onehand_core::chat::drive::{Code, Origin, QUIET_SAVE, Signal};
    use onehand_core::config::AgentSpec;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Instant;

    type Seen = Rc<RefCell<Vec<Signal>>>;

    /// A session folding what is sent on the returned channel into `chat`,
    /// and every signal it gave, in order.
    fn driven(
        cx: &mut TestAppContext,
        chat: Chat,
    ) -> (Entity<ChatSession>, mpsc::Sender<AcpEvent>, Seen) {
        let (tx, rx) = mpsc::channel(64);
        let source = Source::Local(Events::from_channel(rx));
        let spec = AgentSpec::default();
        let session =
            cx.new(|cx| ChatSession::begin(chat, &spec, false, source, Instant::now(), None, cx));
        let seen = Seen::default();
        let heard = seen.clone();
        cx.update(|cx| {
            cx.subscribe(&session, move |_, event: &ChatEvent, _| {
                if let ChatEvent::Session(signal) = event {
                    heard.borrow_mut().push(signal.clone());
                }
            })
            .detach()
        });
        (session, tx, seen)
    }

    /// The agent comes up, and the session names its conversation.
    fn up(
        tx: &mut mpsc::Sender<AcpEvent>,
    ) -> tokio::sync::mpsc::UnboundedReceiver<onehand_core::acp::AcpRequest> {
        let (req, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
        tx.try_send(AcpEvent::Connected {
            tx: req,
            resumed: false,
            forkable: false,
        })
        .unwrap();
        tx.try_send(AcpEvent::SessionId("s1".into())).unwrap();
        rx
    }

    fn prompt(session: &Entity<ChatSession>, cx: &mut TestAppContext) {
        session.update(cx, |session, cx| {
            session.prompt("go", &[], Origin::Person, Code::Here, cx);
        });
    }

    fn turn(tx: &mut mpsc::Sender<AcpEvent>) {
        tx.try_send(AcpEvent::AgentChunk {
            text: "done".into(),
            message: None,
        })
        .unwrap();
        tx.try_send(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        })
        .unwrap();
    }

    fn count(seen: &Seen, wanted: &Signal) -> usize {
        seen.borrow().iter().filter(|s| *s == wanted).count()
    }

    /// A chat whose transcript cannot be written: its store is under a
    /// file, so no directory can be made there.
    fn unwritable() -> (Chat, PathBuf) {
        let file = std::env::temp_dir().join(format!(
            "onehand-carrier-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&file, b"").unwrap();
        let store = file.join("conversations");
        (
            Chat::new(1, PathBuf::from("/repo"), "agent".into(), Some(store)),
            file,
        )
    }

    #[gpui::test]
    fn signals_reach_watchers_in_order_and_once(cx: &mut TestAppContext) {
        let (session, mut tx, seen) = driven(cx, Chat::default());
        let _rx = up(&mut tx);
        cx.run_until_parked();
        prompt(&session, cx);
        turn(&mut tx);
        cx.run_until_parked();
        let seen = seen.borrow();
        let tail = &seen[seen.len() - 3..];
        assert_eq!(
            tail,
            [Signal::Appended, Signal::Appended, Signal::TurnEnded],
            "{seen:?}"
        );
        assert_eq!(seen.iter().filter(|s| **s == Signal::TurnEnded).count(), 1);
    }

    /// What the drive asks of the agent is sent on its channel, in order:
    /// the prompt, then the stop.
    #[gpui::test]
    fn requests_reach_the_agent_in_order(cx: &mut TestAppContext) {
        let (session, mut tx, _) = driven(cx, Chat::default());
        let mut rx = up(&mut tx);
        cx.run_until_parked();
        prompt(&session, cx);
        session.update(cx, |s, cx| s.stop(cx));
        assert!(matches!(
            rx.try_recv(),
            Ok(onehand_core::acp::AcpRequest::Prompt { text, .. }) if text == "go"
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(onehand_core::acp::AcpRequest::Cancel)
        ));
        assert!(rx.try_recv().is_err(), "each once");
    }

    #[gpui::test]
    fn a_failed_save_reaches_the_drive_and_is_said_once(cx: &mut TestAppContext) {
        let (chat, file) = unwritable();
        let (session, mut tx, seen) = driven(cx, chat);
        let _rx = up(&mut tx);
        cx.run_until_parked();
        for _ in 0..2 {
            prompt(&session, cx);
            turn(&mut tx);
            cx.run_until_parked();
        }
        let said = seen
            .borrow()
            .iter()
            .filter(|s| matches!(s, Signal::SaveFailed(_)))
            .count();
        assert_eq!(said, 1, "{:?}", seen.borrow());
        drop(session);
        cx.run_until_parked();
        std::fs::remove_file(file).ok();
    }

    /// The save is carried out and its answer comes back: the store cannot
    /// be written, so the answer is a failure the drive says.
    #[gpui::test]
    fn the_quiet_wait_runs_out_into_a_save(cx: &mut TestAppContext) {
        let (chat, file) = unwritable();
        let (session, mut tx, seen) = driven(cx, chat);
        let _rx = up(&mut tx);
        cx.run_until_parked();
        tx.try_send(AcpEvent::AgentChunk {
            text: "by itself".into(),
            message: None,
        })
        .unwrap();
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s._armed.is_some()));
        cx.executor().advance_clock(QUIET_SAVE * 2);
        cx.run_until_parked();
        assert_eq!(count(&seen, &Signal::CycleEnded), 1, "{:?}", seen.borrow());
        let failed = seen
            .borrow()
            .iter()
            .filter(|s| matches!(s, Signal::SaveFailed(_)))
            .count();
        assert_eq!(failed, 1, "{:?}", seen.borrow());
        drop(session);
        cx.run_until_parked();
        std::fs::remove_file(file).ok();
    }

    #[gpui::test]
    fn a_stream_that_ends_calls_off_the_wait_and_its_answer(cx: &mut TestAppContext) {
        let (session, mut tx, seen) = driven(cx, Chat::default());
        let _rx = up(&mut tx);
        cx.run_until_parked();
        tx.try_send(AcpEvent::AgentChunk {
            text: "by itself".into(),
            message: None,
        })
        .unwrap();
        cx.run_until_parked();
        drop(tx);
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s._armed.is_none()));
        cx.executor().advance_clock(QUIET_SAVE * 2);
        cx.run_until_parked();
        assert_eq!(count(&seen, &Signal::CycleEnded), 0);
        assert_eq!(count(&seen, &Signal::Disconnected), 1);
    }

    #[gpui::test]
    fn a_closed_session_leaves_nothing_running(cx: &mut TestAppContext) {
        let (chat, file) = unwritable();
        let (session, mut tx, seen) = driven(cx, chat);
        let _rx = up(&mut tx);
        cx.run_until_parked();
        prompt(&session, cx);
        turn(&mut tx);
        tx.try_send(AcpEvent::AgentChunk {
            text: "by itself".into(),
            message: None,
        })
        .unwrap();
        // One task at a time, which moves no clock, until the pump has folded
        // all three: the turn's save is on its way and a quiet wait is armed.
        while session.read_with(cx, |s, _| s._armed.is_none()) {
            assert!(cx.executor().tick(), "the pump stalled");
        }
        let failed = |seen: &Seen| {
            seen.borrow()
                .iter()
                .filter(|s| matches!(s, Signal::SaveFailed(_)))
                .count()
        };
        assert_eq!(failed(&seen), 0, "the save has not answered yet");
        drop(session);
        // An entity is released when the app next flushes its effects, as
        // any update does.
        cx.update(|_| {});
        cx.run_until_parked();
        cx.executor().advance_clock(QUIET_SAVE * 2);
        cx.run_until_parked();
        assert!(tx.is_closed(), "the pump went with the session");
        // Neither the wait nor the save's answer ran anything for it.
        assert_eq!(count(&seen, &Signal::CycleEnded), 0, "{:?}", seen.borrow());
        assert_eq!(failed(&seen), 0, "{:?}", seen.borrow());
        std::fs::remove_file(file).ok();
    }
}

// What the reader has open, on top of what an item does by itself.

mod folds {
    use super::super::{ChatSession, Source};
    use crate::acp::Events;
    use futures::channel::mpsc;
    use gpui::{AppContext as _, Entity, TestAppContext};
    use onehand_core::chat::{Chat, TranscriptItemId};
    use onehand_core::config::AgentSpec;
    use std::time::Instant;

    fn session(
        cx: &mut TestAppContext,
    ) -> (
        Entity<ChatSession>,
        mpsc::Sender<onehand_core::acp::AcpEvent>,
    ) {
        let (tx, rx) = mpsc::channel(8);
        let source = Source::Local(Events::from_channel(rx));
        let spec = AgentSpec::default();
        let session = cx.new(|cx| {
            ChatSession::begin(
                Chat::default(),
                &spec,
                false,
                source,
                Instant::now(),
                None,
                cx,
            )
        });
        (session, tx)
    }

    const STEP: TranscriptItemId = TranscriptItemId::Live(0);

    /// **A failure opens a step, and the reader can still shut it.** Were the
    /// failure to force the row open, the control that shuts it would do
    /// nothing in the one state that most wants a way out.
    #[gpui::test]
    fn a_step_that_opens_by_itself_shuts_when_told_and_stays_shut(cx: &mut TestAppContext) {
        let (session, _tx) = session(cx);
        session.update(cx, |s, _| {
            assert!(s.item_is_open(STEP, true), "open by itself");
            s.toggle_item(STEP, true);
            assert!(!s.item_is_open(STEP, true), "shut when told");
            // What the item does by itself moves on; the reader's choice keeps.
            assert!(!s.item_is_open(STEP, false));
            s.toggle_item(STEP, false);
            assert!(s.item_is_open(STEP, false), "and opens when told");
        });
    }

    #[gpui::test]
    fn output_wells_open_apart_and_move_no_layout(cx: &mut TestAppContext) {
        let (session, _tx) = session(cx);
        session.update(cx, |s, _| {
            let before = s.folds();
            s.toggle_out(STEP, 0);
            assert!(s.out_is_open(STEP, 0));
            assert!(!s.out_is_open(STEP, 1));
            assert!(!s.item_is_open(STEP, false), "the card itself stays shut");
            assert_eq!(s.folds(), before, "an output well moves no run");
            s.toggle_item(STEP, false);
            assert_eq!(s.folds(), before + 1, "a card's fold does");
        });
    }

    /// Kept by position, a choice made before the transcript was numbered
    /// afresh would land on another item.
    #[gpui::test]
    fn a_renumbered_transcript_forgets_what_was_open(cx: &mut TestAppContext) {
        let (session, _tx) = session(cx);
        session.update(cx, |s, cx| {
            let fx = s.chat.say_host_wait("Sending the code".into());
            s.perform(fx, cx);
            let fx = s.chat.note("after it".to_string());
            s.perform(fx, cx);
            let after = TranscriptItemId::Live(1);
            s.toggle_item(after, false);
            s.toggle_out(after, 0);
            let fx = s.chat.unsay_host_wait();
            s.perform(fx, cx);
            s.sync_md(cx);
            // The item moved up to where the line was; its old place is now
            // nobody's, and what was chosen there is gone with the numbering.
            assert!(!s.item_is_open(after, false));
            assert!(!s.out_is_open(after, 0));
        });
    }
}
