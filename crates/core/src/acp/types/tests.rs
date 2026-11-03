use super::*;
use serde::Serialize;

fn option(kind: &str) -> PermissionOption {
    PermissionOption {
        id: "id".into(),
        name: "name".into(),
        kind: kind.into(),
    }
}

fn sample_permission() -> PermissionRequest {
    PermissionRequest {
        rpc_id: serde_json::json!(7),
        tool_call_id: Some("t1".into()),
        kind: ToolKind::Execute,
        title: "ls".into(),
        options: vec![option("allow_once"), option("reject_once")],
        child: None,
    }
}

fn sample_elicitation() -> Elicitation {
    let choice = |v: &str| ElicitChoice {
        value: v.into(),
        label: v.into(),
        description: None,
    };
    Elicitation {
        rpc_id: serde_json::json!("e1"),
        tool_call_id: None,
        message: "pick".into(),
        fields: vec![ElicitField {
            key: "k".into(),
            title: Some("K".into()),
            description: None,
            kind: ElicitKind::Select(vec![choice("a"), choice("b")]),
            custom_key: Some("k_custom".into()),
        }],
        child: Some("c".into()),
    }
}

fn sample_tool_call() -> ToolCall {
    ToolCall {
        id: "t1".into(),
        title: "ls".into(),
        description: None,
        kind: ToolKind::Execute,
        status: ToolStatus::Pending,
        content: vec![ToolContent::Text("out".into())],
    }
}

fn round_trip<T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug>(v: T) {
    let line = serde_json::to_string(&v).unwrap();
    let back: T = serde_json::from_str(&line).unwrap();
    assert_eq!(format!("{v:?}"), format!("{back:?}"));
}

#[test]
fn events_round_trip() {
    for ev in [
        AcpEvent::AgentChunk {
            text: "hi".into(),
            message: Some("m1".into()),
        },
        AcpEvent::Permission(sample_permission()),
        AcpEvent::Elicitation(sample_elicitation()),
        AcpEvent::ToolCall(sample_tool_call()),
        AcpEvent::ToolUpdate(ToolCallUpdate {
            id: "t1".into(),
            content: Some(vec![ToolContent::Image(std::sync::Arc::new(vec![1, 2]))]),
            ..Default::default()
        }),
        AcpEvent::InChild {
            child: "c".into(),
            event: Box::new(AcpEvent::Interrupted),
        },
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
    ] {
        round_trip(ev);
    }
}

#[test]
fn connected_is_never_written() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let ev = AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    };
    assert!(serde_json::to_string(&ev).is_err());
}

#[test]
fn requests_round_trip() {
    for req in [
        AcpRequest::Prompt {
            text: "go".into(),
            attachments: vec![Attachment {
                path: "/a/b".into(),
            }],
        },
        AcpRequest::PermissionResponse {
            rpc_id: serde_json::json!(7),
            option_id: Some("allow_once".into()),
        },
        AcpRequest::ElicitationResponse {
            rpc_id: serde_json::json!("e1"),
            outcome: ElicitOutcome::Decline,
        },
        AcpRequest::SetMode("plan".into()),
    ] {
        round_trip(req);
    }
}

#[test]
fn an_opening_round_trips() {
    use crate::acp::client::Opening;
    round_trip(Opening::Fork {
        from: "s".into(),
        message: "m".into(),
    });
}

/// The two grants are *not* the same answer, and the classification is the
/// only thing that keeps them apart: they share a prefix, so any test of
/// the raw string that is cheap enough to write inline is a test that reads
/// "allow for ever" as "allow".
#[test]
fn the_two_grants_are_told_apart() {
    assert_eq!(option("allow_once").weight(), PermissionWeight::AllowOnce);
    assert_eq!(
        option("allow_always").weight(),
        PermissionWeight::AllowAlways
    );
}

#[test]
fn a_refusal_and_an_unknown_kind_both_land_on_deny() {
    assert_eq!(option("reject_once").weight(), PermissionWeight::Deny);
    assert_eq!(option("reject_always").weight(), PermissionWeight::Deny);
    // An adapter this build has never met offers something it cannot
    // classify; the safe reading of an unreadable option is "no".
    assert_eq!(option("allow_on_tuesdays").weight(), PermissionWeight::Deny);
    assert_eq!(option("").weight(), PermissionWeight::Deny);
}

#[test]
fn the_refusal_key_refuses_once_whatever_order_the_agent_lists_them_in() {
    let request = |kinds: &[&str]| PermissionRequest {
        rpc_id: Value::Null,
        tool_call_id: None,
        kind: ToolKind::Other,
        title: String::new(),
        child: None,
        options: kinds
            .iter()
            .map(|kind| PermissionOption {
                id: kind.to_string(),
                name: kind.to_string(),
                kind: kind.to_string(),
            })
            .collect(),
    };
    let keyed =
        |kinds: &[&str], weight| request(kinds).keyed(weight).map(|option| option.id.clone());
    let deny = PermissionWeight::Deny;
    assert_eq!(
        keyed(&["reject_always", "reject_once"], deny).as_deref(),
        Some("reject_once")
    );
    assert_eq!(
        keyed(&["reject_always"], deny).as_deref(),
        Some("reject_always")
    );
    assert_eq!(
        keyed(&["allow_always", "allow_once"], PermissionWeight::AllowOnce).as_deref(),
        Some("allow_once")
    );
    assert_eq!(keyed(&["allow_once"], PermissionWeight::AllowAlways), None);
}

#[test]
fn a_choice_that_throws_work_away_is_read_off_its_label() {
    let choice = |label: &str| ElicitChoice {
        value: "v".into(),
        label: label.into(),
        description: None,
    };
    assert!(choice("Drop them").is_destructive());
    assert!(choice("Delete the migrations").is_destructive());
    assert!(choice("Overwrite").is_destructive());
    assert!(!choice("Backfill them").is_destructive());
    assert!(!choice("Leave them").is_destructive());
    // Whole words only, or half the vocabulary of a safe option reads as a
    // warning.
    assert!(!choice("Undroppable columns").is_destructive());
    assert!(!choice("Removable media").is_destructive());
}

#[test]
fn an_image_is_written_as_a_base64_string() {
    let c = ToolContent::Image(std::sync::Arc::new(vec![0, 1, 254, 255]));
    let v = serde_json::to_value(&c).unwrap();
    assert!(v["Image"].is_string());
    round_trip(c);
}
