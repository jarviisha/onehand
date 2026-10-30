use super::*;
use crate::acp::{
    AcpEvent, BackgroundReport, PermissionOption, PermissionRequest, ReqTx, ToolKind,
};
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

/// A worker the lead gave turn 1, as the open leaves it.
fn prompted() -> (Chat, Rx, Held) {
    let (mut chat, rx) = connected();
    assert!(chat.submit("do it", &[]));
    let held = Held {
        turn: Some(1),
        sent: chat.prompts_sent,
        dirs: Vec::new(),
    };
    (chat, rx, held)
}

fn ended(chat: &mut Chat) {
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
}

fn line<'a>(state: &'a str, key: &str) -> &'a str {
    state
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("no `{key}` in\n{state}"))
}

#[test]
fn a_connected_idle_worker_reads_none_reported() {
    let (chat, _rx) = connected();
    let held = Held {
        turn: None,
        sent: 0,
        dirs: Vec::new(),
    };
    let state = state(&chat, &held);
    assert_eq!(line(&state, "link"), "connected");
    assert_eq!(line(&state, "turn"), "none");
    assert_eq!(line(&state, "waiting"), "none");
    assert_eq!(line(&state, "activity"), "none reported");
    assert_eq!(line(&state, "held by"), "lead");
    assert_eq!(line(&state, "turns"), "none sent");
    assert_eq!(line(&state, "mode"), "the agent's own");
    assert_eq!(line(&state, "model"), "the agent's own");
    assert_eq!(line(&state, "dirs"), "none");
}

#[test]
fn a_running_turn_is_running_and_not_ended() {
    let (chat, _rx, held) = prompted();
    let state = state(&chat, &held);
    assert_eq!(line(&state, "turn"), "running");
    assert_eq!(line(&state, "activity"), "running: the turn");
    assert_eq!(line(&state, "turns"), "sent 1, not yet ended");
}

#[test]
fn a_card_waiting_is_counted() {
    let (mut chat, _rx, held) = prompted();
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
    assert_eq!(line(&state(&chat, &held), "waiting"), "1 permission");
}

#[test]
fn a_failed_turn_says_its_words() {
    let (mut chat, _rx, held) = prompted();
    chat.apply(AcpEvent::Error {
        text: "Your workspace is out of credits".into(),
        detail: None,
    });
    ended(&mut chat);
    assert_eq!(
        line(&state(&chat, &held), "turn"),
        "failed: Your workspace is out of credits"
    );
}

#[test]
fn a_cancelled_turn_says_cancelled() {
    let (mut chat, _rx, held) = prompted();
    chat.cancel_turn();
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    });
    assert_eq!(line(&state(&chat, &held), "turn"), "cancelled");
}

#[test]
fn background_after_the_turn_is_activity_not_the_turn() {
    let (mut chat, _rx, held) = prompted();
    chat.apply(AcpEvent::Background(BackgroundReport::Spawned {
        id: "b1".into(),
        name: "npm test".into(),
        kind: "shell".into(),
    }));
    ended(&mut chat);
    let state = state(&chat, &held);
    assert_eq!(line(&state, "turn"), "ended");
    assert!(
        line(&state, "activity").starts_with("running: 1 in background: npm test"),
        "{state}"
    );
}

/// A lost link clears what core held: empty values then prove nothing.
#[test]
fn a_lost_link_makes_activity_unknown() {
    let (mut chat, _rx, held) = prompted();
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    let state = state(&chat, &held);
    assert_eq!(line(&state, "link"), "lost");
    assert_eq!(line(&state, "activity"), "unknown");
}

#[test]
fn the_person_prompting_holds_the_worker() {
    let (mut chat, _rx, held) = prompted();
    ended(&mut chat);
    assert!(chat.submit("my own words", &[]));
    ended(&mut chat);
    let state = state(&chat, &held);
    assert_eq!(line(&state, "held by"), "person");
    assert_eq!(line(&state, "turns"), "sent 1, not yet ended");
}

#[test]
fn the_lead_s_turn_over_is_ended() {
    let (mut chat, _rx, held) = prompted();
    ended(&mut chat);
    assert_eq!(line(&state(&chat, &held), "turns"), "sent 1, ended 1");
}

#[test]
fn dirs_are_named() {
    let (chat, _rx) = connected();
    let held = Held {
        turn: None,
        sent: 0,
        dirs: vec!["/run".into(), "/r/.git".into()],
    };
    assert_eq!(line(&state(&chat, &held), "dirs"), "/run, /r/.git");
}

#[test]
fn counts_read_in_the_singular_for_one() {
    assert_eq!(subagents_said(1), "1 subagent");
    assert_eq!(subagents_said(2), "2 subagents");
}

/// A turn cut off by a lost link did not end in any way anyone knows.
#[test]
fn a_turn_cut_off_by_the_link_is_not_ended() {
    let (mut chat, _rx, held) = prompted();
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    let state = state(&chat, &held);
    assert_eq!(line(&state, "turn"), "lost with the link");
    assert_eq!(line(&state, "turns"), "sent 1, not yet ended");
}

/// The next prompt after a reconnect clears what the cut-off turn said.
#[test]
fn a_new_turn_after_a_cut_off_one_reads_as_itself() {
    let (mut chat, _rx, held) = prompted();
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    let (tx, _rx2): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    assert!(chat.submit("again", &[]));
    assert_eq!(line(&state(&chat, &held), "turn"), "running");
}

/// A restart builds the chat anew: what was sent before it still counts,
/// or the person's next prompt would read as the lead's.
#[test]
fn a_restarted_chat_keeps_the_prompts_sent() {
    let (old, _rx, held) = prompted();
    let (mut fresh, _rx2) = connected();
    fresh.carry_sent(old.prompts_sent);
    assert!(fresh.submit("my own words", &[]));
    assert_eq!(line(&state(&fresh, &held), "held by"), "person");
}

#[test]
fn what_it_runs_with_reads_as_the_open_says_it() {
    let (chat, _rx) = connected();
    assert_eq!(mode_said(&chat), "the agent's own");
    assert_eq!(model_said(&chat), "the agent's own");
    assert_eq!(dirs_said(&[]), "none");
}
