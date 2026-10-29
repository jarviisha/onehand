use super::*;
use crate::acp::{AcpEvent, ConfigChoice, ConfigOption, Mode, ReqTx};
use crate::chat::{Chat, MODE_SELECTOR};

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

/// A connected chat offering modes `default` (in force) and `acceptEdits`,
/// and a model option `weak` (in force) or `strong`.
fn offering() -> (Chat, Rx) {
    let (mut chat, rx) = connected();
    chat.apply(AcpEvent::Modes {
        current: Some("default".into()),
        available: ["default", "acceptEdits"]
            .iter()
            .map(|id| Mode {
                id: id.to_string(),
                name: id.to_string(),
            })
            .collect(),
    });
    let choice = |value: &str| ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
    };
    chat.apply(AcpEvent::ConfigOptions(vec![ConfigOption {
        id: "model".into(),
        name: "Model".into(),
        category: Some("model".into()),
        current: Some("weak".into()),
        choices: vec![choice("weak"), choice("strong")],
    }]));
    (chat, rx)
}

#[test]
fn waits_while_connecting() {
    assert_eq!(step(&Chat::default(), None, None, &[]), Step::Wait);
}

#[test]
fn a_lost_link_is_refused_with_its_words() {
    let (mut chat, _rx) = connected();
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    let Step::Refused(why) = step(&chat, None, None, &[]) else {
        panic!("a lost link must refuse");
    };
    assert!(why.starts_with("the agent did not start"), "{why}");
    assert!(why.contains("adapter exited"), "{why}");
}

#[test]
fn picks_the_mode_then_the_model() {
    let (mut chat, _rx) = offering();
    let first = step(&chat, Some("acceptEdits"), Some("strong"), &[]);
    assert_eq!(first, Step::Pick(Pick::Mode("acceptEdits".into())));
    chat.pick(&Pick::Mode("acceptEdits".into()));
    assert_eq!(
        step(
            &chat,
            Some("acceptEdits"),
            Some("strong"),
            &[MODE_SELECTOR.into()]
        ),
        Step::Wait,
        "a pick out is waited for"
    );
    chat.apply(AcpEvent::PickConfirmed {
        group: MODE_SELECTOR.into(),
        value: "acceptEdits".into(),
    });
    let second = step(
        &chat,
        Some("acceptEdits"),
        Some("strong"),
        &[MODE_SELECTOR.into()],
    );
    assert_eq!(
        second,
        Step::Pick(Pick::Option {
            id: "model".into(),
            value: "strong".into()
        })
    );
    chat.pick(&Pick::Option {
        id: "model".into(),
        value: "strong".into(),
    });
    chat.apply(AcpEvent::PickConfirmed {
        group: "model".into(),
        value: "strong".into(),
    });
    let picked = [MODE_SELECTOR.to_string(), "model".to_string()];
    assert_eq!(
        step(&chat, Some("acceptEdits"), Some("strong"), &picked),
        Step::Ready
    );
}

#[test]
fn a_model_not_offered_is_refused_naming_the_offer() {
    let (chat, _rx) = offering();
    let Step::Refused(why) = step(&chat, None, Some("huge"), &[]) else {
        panic!("a model not offered must refuse");
    };
    assert!(why.contains("weak, strong"), "{why}");
}

#[test]
fn a_mode_not_offered_is_refused() {
    let (chat, _rx) = offering();
    assert!(matches!(
        step(&chat, Some("plan"), None, &[]),
        Step::Refused(_)
    ));
}

#[test]
fn no_model_option_is_refused() {
    let (chat, _rx) = connected();
    assert_eq!(
        step(&chat, None, Some("strong"), &[]),
        Step::Refused("the agent offers no models".into())
    );
}

/// The chat puts the old value back on a refusal: that is not the model
/// asked for, so the open is refused rather than ready on the wrong model.
#[test]
fn a_refused_pick_is_not_ready() {
    let (mut chat, _rx) = offering();
    chat.pick(&Pick::Option {
        id: "model".into(),
        value: "strong".into(),
    });
    chat.apply(AcpEvent::PickRefused {
        group: "model".into(),
        why: "not on this plan".into(),
    });
    let Step::Refused(why) = step(&chat, None, Some("strong"), &["model".into()]) else {
        panic!("a refused pick must refuse");
    };
    assert!(why.contains("did not take"), "{why}");
}

#[test]
fn nothing_wanted_is_ready_at_once() {
    let (chat, _rx) = connected();
    assert_eq!(step(&chat, None, None, &[]), Step::Ready);
}

#[test]
fn what_is_in_force_already_is_not_picked_again() {
    let (chat, _rx) = offering();
    assert_eq!(step(&chat, Some("default"), Some("weak"), &[]), Step::Ready);
}

#[test]
fn the_handle_names_the_run() {
    assert_eq!(handle("3f2a9c01", 7), "3f2a9c01-7");
}

#[test]
fn parse_handle_reads_this_run() {
    assert_eq!(parse_handle("3f2a9c01-7", "3f2a9c01"), Ok(7));
}

#[test]
fn parse_handle_refuses_another_run() {
    let why = parse_handle("00000000-7", "3f2a9c01").unwrap_err();
    assert!(why.contains("earlier run"), "{why}");
}

#[test]
fn parse_handle_refuses_nonsense() {
    for text in ["7", "3f2a9c01-", "3f2a9c01-x", ""] {
        let why = parse_handle(text, "3f2a9c01").unwrap_err();
        assert!(why.contains("not a session's handle"), "{text}: {why}");
    }
}
