//! What a press from outside the app does to a session, and what a stop from
//! outside takes back, as the bridge carries them today.

use super::super::{ChatPane, SETTLED};
use crate::chat::session::now;
use onehand_core::acp::{
    AcpEvent, AcpRequest, ElicitField, ElicitOutcome, Elicitation, PermissionOption,
    PermissionRequest, ReqTx,
};
use onehand_core::chat::drive::{Code, Effect, Intake, Origin};
use onehand_core::chat::{Chat, SessionDrive};
use onehand_core::remote::Press;
use std::path::PathBuf;

type Rx = tokio::sync::mpsc::UnboundedReceiver<AcpRequest>;

/// A session whose agent is up and keeps no transcript, and what it sends.
fn connected() -> (SessionDrive, Rx) {
    let mut drive = SessionDrive::new(Chat::new(1, PathBuf::from("/repo"), "agent".into(), None));
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    feed(
        &mut drive,
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
    );
    feed(&mut drive, AcpEvent::SessionId("s1".into()));
    (drive, rx)
}

fn feed(drive: &mut SessionDrive, event: AcpEvent) -> Vec<Effect> {
    let (mut fx, _) = drive.apply(event, now());
    fx.extend(drive.end_batch());
    fx
}

/// What the effects asked of the agent, in order.
fn requests(fx: Vec<Effect>) -> Vec<AcpRequest> {
    fx.into_iter()
        .filter_map(|effect| match effect {
            Effect::Request(request) => Some(request),
            _ => None,
        })
        .collect()
}

/// A turn running with a permission of two options parked on it: its index.
fn permission(drive: &mut SessionDrive) -> usize {
    drive.prompt("go", &[], Origin::Person, Code::Here, now());
    let option = |id: &str, name: &str| PermissionOption {
        id: id.into(),
        name: name.into(),
        kind: "allow_once".into(),
    };
    feed(
        drive,
        AcpEvent::Permission(PermissionRequest {
            rpc_id: serde_json::Value::from(7),
            tool_call_id: None,
            kind: onehand_core::acp::ToolKind::Execute,
            child: None,
            title: "Run command".into(),
            options: vec![option("allow-1", "Allow once"), option("deny", "Deny")],
        }),
    );
    drive.items.len() - 1
}

/// A turn running with a one-field question parked on it: its index.
fn question(drive: &mut SessionDrive) -> usize {
    drive.prompt("go", &[], Origin::Person, Code::Here, now());
    // Built from its wire shape: the field's free-text key is core's own.
    let field: ElicitField = serde_json::from_value(serde_json::json!({
        "key": "q0",
        "title": null,
        "description": null,
        "kind": {"Select": [
            {"value": "a", "label": "A", "description": null},
            {"value": "b", "label": "B", "description": null},
        ]},
        "custom_key": null,
    }))
    .unwrap();
    feed(
        drive,
        AcpEvent::Elicitation(Elicitation {
            rpc_id: serde_json::Value::from(9),
            tool_call_id: None,
            message: "pick".into(),
            fields: vec![field],
            child: None,
        }),
    );
    drive.items.len() - 1
}

fn permission_answered(fx: Vec<Effect>) -> Option<String> {
    requests(fx).into_iter().find_map(|request| match request {
        AcpRequest::PermissionResponse { option_id, .. } => option_id,
        _ => None,
    })
}

/// A press answers the card it names with the option it names, and says the
/// option's name.
#[test]
fn a_permission_press_answers_its_card_with_its_option() {
    let (mut drive, _rx) = connected();
    let item = permission(&mut drive);
    let press = Press::Permission {
        uid: 1,
        item,
        option: 1,
    };
    let (said, fx) = ChatPane::apply_press(&mut drive, press);
    assert_eq!(said, "Deny.");
    assert_eq!(permission_answered(fx).as_deref(), Some("deny"));
}

/// A second press on a card already answered changes nothing and says so.
#[test]
fn a_press_on_a_settled_card_is_said_settled() {
    let (mut drive, _rx) = connected();
    let item = permission(&mut drive);
    let press = || Press::Permission {
        uid: 1,
        item,
        option: 0,
    };
    ChatPane::apply_press(&mut drive, press());
    let (said, fx) = ChatPane::apply_press(&mut drive, press());
    assert_eq!(said, SETTLED);
    assert!(fx.is_empty());
}

/// A press that names no card, or no option of it, grants nothing.
#[test]
fn a_press_naming_nothing_grants_nothing() {
    let (mut drive, _rx) = connected();
    let item = permission(&mut drive);
    let (said, fx) = ChatPane::apply_press(
        &mut drive,
        Press::Permission {
            uid: 1,
            item: item + 5,
            option: 0,
        },
    );
    assert_eq!((said.as_str(), fx.len()), (SETTLED, 0));
    let (said, fx) = ChatPane::apply_press(
        &mut drive,
        Press::Permission {
            uid: 1,
            item,
            option: 9,
        },
    );
    assert_eq!(said, "That permission offers nothing this can answer with.");
    assert!(fx.is_empty());
    assert!(drive.awaiting_permission(), "the card still waits");
}

/// A question's press picks the choice and sends the form, saying its label.
#[test]
fn a_question_press_picks_and_sends() {
    let (mut drive, _rx) = connected();
    let item = question(&mut drive);
    let (said, fx) = ChatPane::apply_press(
        &mut drive,
        Press::Question {
            uid: 1,
            item,
            choice: 1,
        },
    );
    assert_eq!(said, "B.");
    let outcome = requests(fx).into_iter().find_map(|request| match request {
        AcpRequest::ElicitationResponse { outcome, .. } => Some(outcome),
        _ => None,
    });
    assert!(
        matches!(outcome, Some(ElicitOutcome::Accept(_))),
        "{outcome:?}"
    );
    let (said, _) = ChatPane::apply_press(
        &mut drive,
        Press::Question {
            uid: 1,
            item,
            choice: 0,
        },
    );
    assert_eq!(said, SETTLED);
}

/// Skip declines the question; a second Skip finds nothing to skip.
#[test]
fn a_skip_declines_the_question_once() {
    let (mut drive, _rx) = connected();
    let item = question(&mut drive);
    let (said, fx) = ChatPane::apply_press(&mut drive, Press::Skip { uid: 1, item });
    assert_eq!(said, "Skipped — the agent carries on without an answer.");
    assert!(requests(fx).iter().any(|request| matches!(
        request,
        AcpRequest::ElicitationResponse {
            outcome: ElicitOutcome::Decline,
            ..
        }
    )));
    let (said, _) = ChatPane::apply_press(&mut drive, Press::Skip { uid: 1, item });
    assert_eq!(said, SETTLED);
}

/// A picker the agent does not offer is said to have moved.
#[test]
fn an_option_no_longer_offered_says_so() {
    let (mut drive, _rx) = connected();
    let (said, fx) = ChatPane::apply_press(
        &mut drive,
        Press::Option {
            uid: 1,
            group: "model".into(),
            choice: 0,
        },
    );
    assert_eq!(
        said,
        "That isn't offered any more — ask for /options again."
    );
    assert!(fx.is_empty());
}

/// A stop from outside takes back what waited behind the turn before the
/// cancel goes, so the turn's end sends nothing.
#[test]
fn a_remote_stop_takes_back_the_queued_prompt_before_it_cancels() {
    let (mut drive, _rx) = connected();
    let (_, mut fx) = drive.prompt("first", &[], Origin::Remote, Code::Here, now());
    let (intake, queued) = drive.prompt("second", &[], Origin::Remote, Code::Here, now());
    assert_eq!(intake, Intake::Queued);
    fx.extend(queued);
    let taken = drive.unqueue().expect("the second prompt waited");
    assert_eq!(taken.text, "second");
    fx.extend(drive.stop());
    fx.extend(feed(
        &mut drive,
        AcpEvent::TurnEnded {
            stop_reason: "cancelled".into(),
        },
    ));
    let prompts: Vec<_> = requests(fx)
        .into_iter()
        .filter_map(|request| match request {
            AcpRequest::Prompt { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, ["first"], "nothing went after the stop");
}

/// Why the order matters: a plain stop leaves the queued prompt to go at the
/// turn's end.
#[test]
fn a_plain_stop_lets_the_queued_prompt_go() {
    let (mut drive, _rx) = connected();
    let (_, mut fx) = drive.prompt("first", &[], Origin::Remote, Code::Here, now());
    fx.extend(
        drive
            .prompt("second", &[], Origin::Remote, Code::Here, now())
            .1,
    );
    fx.extend(drive.stop());
    fx.extend(feed(
        &mut drive,
        AcpEvent::TurnEnded {
            stop_reason: "cancelled".into(),
        },
    ));
    let prompts: Vec<_> = requests(fx)
        .into_iter()
        .filter_map(|request| match request {
            AcpRequest::Prompt { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, ["first", "second"]);
}
