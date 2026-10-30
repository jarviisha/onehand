use super::*;
use crate::acp::{AcpEvent, PermissionOption, PermissionRequest, ReqTx, ToolKind};
use crate::chat::Chat;

type Rx = tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>;

fn connected() -> (Chat, Rx) {
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    (chat, rx)
}

/// A worker whose lead sent turn 1, `go`, now over.
fn after_turn_one() -> (Chat, Rx, Last) {
    let (mut chat, rx) = connected();
    assert!(chat.submit("go", &[]));
    ended(&mut chat);
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    (chat, rx, last)
}

fn ended(chat: &mut Chat) {
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
}

fn card(chat: &mut Chat) {
    chat.apply(AcpEvent::Permission(PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind: ToolKind::Execute,
        child: None,
        title: "cargo test".into(),
        options: vec![PermissionOption {
            id: "allow".into(),
            name: "Allow once".into(),
            kind: "allow_once".into(),
        }],
    }));
}

#[test]
fn a_next_turn_goes() {
    let (chat, _rx, last) = after_turn_one();
    assert_eq!(send(&chat, &last, 2, "next"), Ok(Send::Go));
}

#[test]
fn the_same_turn_and_text_is_already_sent() {
    let (chat, _rx, last) = after_turn_one();
    assert_eq!(send(&chat, &last, 1, "go"), Ok(Send::AlreadySent(1)));
}

/// A retry while the turn it started still runs is answered, not refused
/// as busy.
#[test]
fn the_same_turn_while_it_runs_is_already_sent() {
    let (mut chat, _rx) = connected();
    assert!(chat.submit("go", &[]));
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    assert_eq!(send(&chat, &last, 1, "go"), Ok(Send::AlreadySent(1)));
}

#[test]
fn the_same_turn_with_other_text_is_refused() {
    let (chat, _rx, last) = after_turn_one();
    let why = send(&chat, &last, 1, "other").unwrap_err();
    assert!(why.contains("turn 1"), "{why}");
}

#[test]
fn a_turn_below_the_last_is_refused() {
    let (chat, _rx, mut last) = after_turn_one();
    last.turn = Some(3);
    let why = send(&chat, &last, 2, "late").unwrap_err();
    assert!(why.contains("turn 2") && why.contains("turn 3"), "{why}");
}

#[test]
fn a_busy_worker_refuses_a_new_turn_without_queueing() {
    let (mut chat, _rx) = connected();
    assert!(chat.submit("go", &[]));
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    assert!(send(&chat, &last, 2, "next").is_err());
    assert!(chat.queued.is_none());
}

#[test]
fn a_lost_worker_refuses_in_its_words() {
    let (mut chat, _rx, last) = after_turn_one();
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    assert!(send(&chat, &last, 2, "next").is_err());
}

#[test]
fn a_worker_the_person_holds_refuses_a_send() {
    let (mut chat, _rx, last) = after_turn_one();
    assert!(chat.submit("my own words", &[]));
    ended(&mut chat);
    let why = send(&chat, &last, 2, "next").unwrap_err();
    assert!(why.contains("person"), "{why}");
}

#[test]
fn a_running_turn_can_be_cancelled() {
    let (mut chat, _rx) = connected();
    assert!(chat.submit("go", &[]));
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    assert_eq!(cancel(&chat, &last), Ok(1));
}

#[test]
fn a_card_waiting_refuses_the_cancel() {
    let (mut chat, _rx) = connected();
    assert!(chat.submit("go", &[]));
    card(&mut chat);
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    let why = cancel(&chat, &last).unwrap_err();
    assert!(why.contains("card"), "{why}");
}

#[test]
fn no_turn_running_refuses_the_cancel() {
    let (chat, _rx, last) = after_turn_one();
    assert!(cancel(&chat, &last).is_err());
}

#[test]
fn the_person_holding_refuses_the_cancel() {
    let (mut chat, _rx, last) = after_turn_one();
    assert!(chat.submit("my own words", &[]));
    let why = cancel(&chat, &last).unwrap_err();
    assert!(why.contains("person"), "{why}");
}

/// A working subagent's card outlives the turn: the cancel is refused as a
/// card waiting, never as "no turn".
#[test]
fn a_subagent_s_card_after_the_turn_refuses_the_cancel() {
    let (mut chat, _rx) = connected();
    assert!(chat.submit("go", &[]));
    chat.apply(AcpEvent::SubagentSpawned {
        id: "c1".into(),
        name: "Explorer".into(),
        task: "read".into(),
    });
    chat.apply(AcpEvent::Permission(PermissionRequest {
        rpc_id: serde_json::Value::from(9),
        tool_call_id: None,
        kind: ToolKind::Execute,
        child: Some("c1".into()),
        title: "cargo test".into(),
        options: vec![],
    }));
    ended(&mut chat);
    let last = Last {
        turn: Some(1),
        text: Some("go".into()),
        sent: chat.prompts_sent,
    };
    let why = cancel(&chat, &last).unwrap_err();
    assert!(why.contains("card"), "{why}");
}
