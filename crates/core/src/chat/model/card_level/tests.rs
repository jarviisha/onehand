use super::*;
use crate::acp::{AcpEvent, ElicitField, ElicitKind, Elicitation, PermissionOption};
use tokio::sync::mpsc::UnboundedReceiver;

fn up() -> (Chat, UnboundedReceiver<AcpRequest>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    (chat, rx)
}

fn asks(kind: ToolKind) -> AcpEvent {
    AcpEvent::Permission(PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind,
        child: None,
        title: "do it".into(),
        options: vec![
            PermissionOption {
                id: "allow-1".into(),
                name: "Allow once".into(),
                kind: "allow_once".into(),
            },
            PermissionOption {
                id: "reject-1".into(),
                name: "Deny".into(),
                kind: "reject_once".into(),
            },
        ],
    })
}

fn granted(chat: &mut Chat) -> Option<String> {
    chat.take_requests().into_iter().find_map(|r| match r {
        AcpRequest::PermissionResponse { option_id, .. } => option_id,
        _ => None,
    })
}

#[test]
fn allow_edits_allows_an_edit_and_leaves_a_command() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::AllowEdits);
    let outcome = chat.apply(asks(ToolKind::Edit));
    assert_eq!(granted(&mut chat).as_deref(), Some("allow-1"));
    assert!(
        outcome.asked_user.is_none(),
        "nobody is told of a card answered"
    );
    let outcome = chat.apply(asks(ToolKind::Execute));
    assert_eq!(granted(&mut chat), None);
    assert!(outcome.asked_user.is_some());
}

#[test]
fn manual_answers_nothing() {
    let (mut chat, _rx) = up();
    chat.apply(asks(ToolKind::Edit));
    assert_eq!(granted(&mut chat), None);
}

#[test]
fn bypass_answers_a_question() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::Bypass);
    chat.apply(AcpEvent::Elicitation(Elicitation {
        rpc_id: serde_json::json!(1),
        tool_call_id: None,
        child: None,
        message: "which?".into(),
        fields: vec![ElicitField {
            key: "q".into(),
            title: None,
            description: None,
            kind: ElicitKind::Text,
            custom_key: None,
        }],
    }));
    let answered = chat
        .take_requests()
        .into_iter()
        .any(|r| matches!(r, AcpRequest::ElicitationResponse { .. }));
    assert!(answered);
}

#[test]
fn raising_the_level_answers_what_waits() {
    let (mut chat, _rx) = up();
    chat.apply(asks(ToolKind::Execute));
    assert_eq!(granted(&mut chat), None);
    chat.set_cards(Cards::Bypass);
    assert_eq!(granted(&mut chat).as_deref(), Some("allow-1"));
}

#[test]
fn the_level_is_kept_with_the_conversation() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::AllowEdits);
    let prefs = chat.prefs();
    assert_eq!(prefs.cards, Cards::AllowEdits);
    let mut back = Chat::default();
    back.take_prefs_cards(&prefs);
    assert_eq!(back.cards(), Cards::AllowEdits);
}
