use super::*;
use crate::acp::{
    AcpRequest, ConfigChoice, ConfigOption, PermissionOption, PermissionRequest, ReqTx, Usage,
};
use std::path::PathBuf;

type Rx = tokio::sync::mpsc::UnboundedReceiver<AcpRequest>;

/// A connected session that keeps a transcript, and what it sends.
///
/// The store is a path nothing is written to: a save is only built here, and
/// the drop that would write the rest is told there is nowhere to write.
fn connected() -> (SessionDrive, Rx) {
    let store = PathBuf::from("/nonexistent/onehand-drive-tests");
    let chat = Chat::new(1, PathBuf::from("/repo"), "agent".into(), Some(store));
    let mut drive = SessionDrive::new(chat);
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

/// One event in its own batch, as a front end with nothing else waiting
/// would fold it: what the event said, then what the batch's end said.
fn feed(drive: &mut SessionDrive, event: AcpEvent) -> Vec<Effect> {
    let (mut fx, _) = drive.apply(event);
    fx.extend(drive.end_batch());
    fx
}

fn turn_ends() -> AcpEvent {
    AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    }
}

fn chunk(text: &str) -> AcpEvent {
    AcpEvent::AgentChunk {
        text: text.into(),
        message: None,
    }
}

fn permission() -> PermissionRequest {
    PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind: crate::acp::ToolKind::Execute,
        child: None,
        title: "Run command".into(),
        options: vec![PermissionOption {
            id: "allow-1".into(),
            name: "Allow once".into(),
            kind: "allow_once".into(),
        }],
    }
}

/// Effects as names, for asserting their order.
fn names(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|effect| match effect {
            Effect::Signal(signal) => format!("{signal:?}"),
            Effect::Changed => "Changed".into(),
            Effect::OfferChanged => "OfferChanged".into(),
            Effect::Save(_) => "Save".into(),
            Effect::Arm { .. } => "Arm".into(),
            Effect::Disarm => "Disarm".into(),
            Effect::SendCode { .. } => "SendCode".into(),
            Effect::PromptAgain => "PromptAgain".into(),
        })
        .collect()
}

fn sent(rx: &mut Rx) -> Option<String> {
    match rx.try_recv() {
        Ok(AcpRequest::Prompt { text, .. }) => Some(text),
        _ => None,
    }
}

fn armed(fx: &[Effect]) -> Option<u64> {
    fx.iter().find_map(|effect| match effect {
        Effect::Arm { token, .. } => Some(*token),
        _ => None,
    })
}

fn take_save(fx: Vec<Effect>) -> Box<PendingWrite> {
    fx.into_iter()
        .find_map(|effect| match effect {
            Effect::Save(write) => Some(write),
            _ => None,
        })
        .expect("a save")
}

impl Drop for SessionDrive {
    fn drop(&mut self) {
        // Nowhere to write: see `connected`.
        self.chat.store = None;
    }
}

// Intake, by origin.

#[test]
fn an_idle_session_takes_every_origins_prompt() {
    for origin in [
        Origin::Person,
        Origin::Remote,
        Origin::Tools,
        Origin::Run,
        Origin::Issue,
    ] {
        let (mut drive, mut rx) = connected();
        let (intake, fx) = drive.prompt("go", &[], origin, Code::Here);
        assert_eq!(intake, Intake::Sent, "{origin:?}");
        assert_eq!(names(&fx), ["Changed"]);
        assert_eq!(sent(&mut rx).as_deref(), Some("go"));
    }
}

#[test]
fn a_person_and_the_remote_queue_behind_a_turn_and_the_rest_are_refused() {
    for (origin, queues) in [
        (Origin::Person, true),
        (Origin::Remote, true),
        (Origin::Tools, false),
        (Origin::Run, false),
        (Origin::Issue, false),
    ] {
        let (mut drive, _rx) = connected();
        drive.prompt("first", &[], Origin::Person, Code::Here);
        let (intake, _) = drive.prompt("second", &[], origin, Code::Here);
        match queues {
            true => assert_eq!(intake, Intake::Queued, "{origin:?}"),
            false => assert_eq!(
                intake,
                Intake::Refused(Refusal::Blocked(SubmitBlock::Busy)),
                "{origin:?}"
            ),
        }
    }
}

#[test]
fn a_person_replaces_what_waits_and_the_remote_does_not() {
    let (mut drive, _rx) = connected();
    drive.prompt("first", &[], Origin::Person, Code::Here);
    drive.prompt("second", &[], Origin::Person, Code::Here);
    let (intake, _) = drive.prompt("third", &[], Origin::Person, Code::Here);
    assert_eq!(intake, Intake::Queued);
    let (intake, _) = drive.prompt("fourth", &[], Origin::Remote, Code::Here);
    assert_eq!(intake, Intake::Refused(Refusal::AlreadyQueued));
    assert_eq!(
        drive.queued.as_ref().map(|q| q.text.as_str()),
        Some("third")
    );
}

#[test]
fn a_blank_prompt_is_refused_for_what_it_is() {
    let (mut drive, _rx) = connected();
    let (intake, _) = drive.prompt("  ", &[], Origin::Person, Code::Here);
    assert_eq!(
        intake,
        Intake::Refused(Refusal::Blocked(SubmitBlock::Empty))
    );
}

#[test]
fn a_queued_prompt_goes_out_when_the_turn_ends() {
    let (mut drive, mut rx) = connected();
    drive.prompt("first", &[], Origin::Person, Code::Here);
    assert_eq!(sent(&mut rx).as_deref(), Some("first"));
    drive.prompt("second", &[], Origin::Person, Code::Here);
    assert!(sent(&mut rx).is_none(), "queued is not sent");

    feed(&mut drive, turn_ends());
    assert_eq!(sent(&mut rx).as_deref(), Some("second"));
    assert!(drive.busy, "the queued prompt opened its own turn");
    assert!(drive.queued.is_none());
}

#[test]
fn prompted_since_counts_what_went_not_what_waits() {
    let (mut drive, _rx) = connected();
    drive.prompt("lead", &[], Origin::Tools, Code::Here);
    let sent = drive.prompts_sent;
    drive.prompt("person", &[], Origin::Person, Code::Here);
    assert!(!drive.prompted_since(sent));
    feed(&mut drive, turn_ends());
    assert!(drive.prompted_since(sent));
}

// An offloaded project's code.

#[test]
fn a_persons_prompt_waits_for_code_already_going_up() {
    let (mut drive, mut rx) = connected();
    let (intake, fx) = drive.prompt("go", &[], Origin::Person, Code::Going);
    assert_eq!(intake, Intake::Waiting);
    assert!(fx.is_empty());
    assert!(sent(&mut rx).is_none());
}

#[test]
fn a_persons_prompt_sends_the_code_first_and_goes_again_once_it_is_up() {
    let (mut drive, mut rx) = connected();
    let line = Code::MustGo {
        line: "Sending the code to box…".into(),
    };
    let (intake, fx) = drive.prompt("go", &[], Origin::Person, line);
    assert_eq!(intake, Intake::CodeFirst);
    assert_eq!(names(&fx), ["Appended", "Changed", "SendCode"]);
    assert!(sent(&mut rx).is_none(), "nothing goes before the code");
    let Some(Effect::SendCode { token }) = fx.last() else {
        unreachable!()
    };

    let fx = drive.handle(Input::CodeSent {
        token: *token,
        result: Ok(()),
        still: true,
    });
    assert_eq!(names(&fx), ["Appended", "Changed", "PromptAgain"]);
}

#[test]
fn code_that_went_up_under_a_changed_prompt_sends_nothing() {
    let (mut drive, _rx) = connected();
    let line = Code::MustGo { line: "up".into() };
    let (_, fx) = drive.prompt("go", &[], Origin::Person, line);
    let Some(Effect::SendCode { token }) = fx.last() else {
        unreachable!()
    };
    let fx = drive.handle(Input::CodeSent {
        token: *token,
        result: Ok(()),
        still: false,
    });
    assert_eq!(names(&fx), ["Appended", "Changed"]);
}

#[test]
fn code_that_could_not_go_up_is_said() {
    let (mut drive, _rx) = connected();
    let (_, fx) = drive.prompt(
        "go",
        &[],
        Origin::Person,
        Code::MustGo { line: "up".into() },
    );
    let Some(Effect::SendCode { token }) = fx.last() else {
        unreachable!()
    };
    let fx = drive.handle(Input::CodeSent {
        token: *token,
        result: Err("no route".into()),
        still: true,
    });
    assert_eq!(
        names(&fx),
        ["Appended", "Changed", "CodeFailed(\"no route\")"]
    );
}

/// Recorded, not dropped: the line said about the send is taken back, and
/// nothing is sent on an answer the session no longer waits for.
#[test]
fn a_late_code_answer_takes_back_its_line_and_sends_nothing() {
    let (mut drive, _rx) = connected();
    drive.prompt(
        "go",
        &[],
        Origin::Person,
        Code::MustGo { line: "up".into() },
    );
    let fx = drive.handle(Input::CodeSent {
        token: 99,
        result: Ok(()),
        still: true,
    });
    assert_eq!(names(&fx), ["Appended", "Changed"]);
}

#[test]
fn a_busy_offloaded_session_queues_as_any_does() {
    let (mut drive, _rx) = connected();
    drive.prompt("first", &[], Origin::Person, Code::Here);
    let (intake, fx) = drive.prompt(
        "second",
        &[],
        Origin::Person,
        Code::MustGo { line: "up".into() },
    );
    assert_eq!(intake, Intake::Queued);
    assert!(fx.is_empty());
}

#[test]
fn only_a_person_waits_on_the_code() {
    let (mut drive, mut rx) = connected();
    let (intake, _) = drive.prompt("go", &[], Origin::Remote, Code::Going);
    assert_eq!(intake, Intake::Sent);
    assert_eq!(sent(&mut rx).as_deref(), Some("go"));
}

// A first prompt held until the agent is up.

fn connecting() -> (SessionDrive, ReqTx, Rx) {
    let mut drive = SessionDrive::new(Chat::default());
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    drive.chat_mut().store = None;
    (drive, tx, rx)
}

#[test]
fn a_prompt_held_until_the_agent_is_up_goes_then() {
    let (mut drive, tx, mut rx) = connecting();
    let (intake, _) = drive.prompt_when_up("go", Origin::Tools);
    assert_eq!(intake, Intake::Held);
    let fx = feed(
        &mut drive,
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
    );
    assert!(names(&fx).contains(&"Changed".to_string()));
    assert_eq!(sent(&mut rx).as_deref(), Some("go"));
}

#[test]
fn a_held_prompt_is_dropped_when_the_agent_never_comes_up() {
    let (mut drive, _tx, mut rx) = connecting();
    drive.prompt_when_up("go", Origin::Tools);
    feed(&mut drive, AcpEvent::Disconnected("gone".into()));
    assert!(sent(&mut rx).is_none());
    assert!(drive.held.is_none());
}

#[test]
fn issue_work_holds_off_a_session_a_person_prompted_first() {
    let (mut drive, mut rx) = connected();
    drive.prompt("mine", &[], Origin::Person, Code::Here);
    sent(&mut rx);
    let (intake, _) = drive.prompt_when_up("the issue", Origin::Issue);
    assert!(matches!(intake, Intake::Refused(_)));
    assert!(sent(&mut rx).is_none());
}

#[test]
fn a_prompt_to_a_session_already_up_goes_at_once() {
    let (mut drive, mut rx) = connected();
    let (intake, _) = drive.prompt_when_up("go", Origin::Issue);
    assert_eq!(intake, Intake::Sent);
    assert_eq!(sent(&mut rx).as_deref(), Some("go"));
}

// Events, batches and the save at a turn's end.

#[test]
fn a_turn_end_saves_then_says_so_and_ends_the_batch() {
    let (mut drive, _rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    let (_, next) = drive.apply(chunk("done"));
    assert_eq!(next, Next::More);
    let (fx, next) = drive.apply(turn_ends());
    assert_eq!(next, Next::Stop);
    assert_eq!(names(&fx), ["Appended", "Save", "TurnEnded"]);
    assert_eq!(names(&drive.end_batch()), ["Changed"]);
}

#[test]
fn a_card_takes_the_next_event_past_the_cap_and_is_said_at_the_batch_end() {
    let (mut drive, _rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    let (fx, next) = drive.apply(AcpEvent::Permission(permission()));
    assert_eq!(next, Next::PastCap);
    assert_eq!(names(&fx), ["Appended"]);
    let end = drive.end_batch();
    assert_eq!(names(&end)[0], "AwaitingUser(Permission)");
}

#[test]
fn a_card_asked_in_a_batch_that_tells_is_said_before_what_ends_it() {
    let (mut drive, _rx) = connected();
    feed(
        &mut drive,
        AcpEvent::ConfigOptions(vec![ConfigOption {
            id: "effort".into(),
            name: "Effort".into(),
            category: None,
            current: Some("low".into()),
            choices: ["low", "high"]
                .into_iter()
                .map(|value| ConfigChoice {
                    value: value.into(),
                    name: value.into(),
                    description: None,
                })
                .collect(),
        }]),
    );
    drive.chat_mut().set_config_option("effort", "high");
    drive.prompt("go", &[], Origin::Person, Code::Here);
    let (_, next) = drive.apply(AcpEvent::Permission(permission()));
    assert_eq!(next, Next::PastCap);
    // The refusal tells a watcher something and ends the batch; the card,
    // still waiting, was asked first and is said first.
    let (fx, next) = drive.apply(AcpEvent::PickRefused {
        group: "effort".into(),
        why: "not on this plan".into(),
    });
    assert_eq!(next, Next::Stop);
    assert_eq!(
        names(&fx),
        [
            "Appended",
            "AwaitingUser(Permission)",
            "PickRefused(\"not on this plan\")"
        ]
    );
    assert!(
        !names(&drive.end_batch()).contains(&"AwaitingUser(Permission)".to_string()),
        "and only once"
    );
}

#[test]
fn a_card_the_turns_end_cancelled_is_not_said() {
    let (mut drive, _rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    drive.apply(AcpEvent::Permission(permission()));
    // A turn that ends with the card unanswered cancels it, so nothing waits
    // and nothing is said: the card went with the turn.
    let (fx, _) = drive.apply(turn_ends());
    assert_eq!(names(&fx), ["Appended", "Save", "TurnEnded"]);
    assert!(!names(&drive.end_batch()).contains(&"AwaitingUser(Permission)".to_string()));
}

#[test]
fn a_busy_session_calls_off_the_quiet_wait() {
    let (mut drive, _rx) = connected();
    feed(&mut drive, chunk("between turns"));
    drive.prompt("go", &[], Origin::Person, Code::Here);
    let fx = feed(&mut drive, chunk("working"));
    assert!(names(&fx).contains(&"Disarm".to_string()));
}

// The quiet spell between turns.

#[test]
fn what_the_agent_writes_between_turns_is_saved_after_a_quiet_spell() {
    let (mut drive, _rx) = connected();
    let fx = feed(&mut drive, chunk("by itself"));
    let token = armed(&fx).expect("a wait is armed");
    let fx = drive.handle(Input::Elapsed { token });
    assert_eq!(names(&fx)[..2], ["Save", "CycleEnded"]);
}

#[test]
fn a_wait_replaced_by_a_later_one_is_dropped() {
    let (mut drive, _rx) = connected();
    let first = armed(&feed(&mut drive, chunk("one"))).unwrap();
    let second = armed(&feed(&mut drive, chunk("two"))).unwrap();
    assert_ne!(first, second);
    assert!(drive.handle(Input::Elapsed { token: first }).is_empty());
    assert!(!drive.handle(Input::Elapsed { token: second }).is_empty());
}

#[test]
fn a_cycle_the_agent_marks_ends_the_batch_and_saves() {
    let (mut drive, _rx) = connected();
    drive.apply(chunk("by itself"));
    let (fx, next) = drive.apply(AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: None,
        cycle_end: true,
        marks_cycles: true,
    }));
    assert_eq!(next, Next::Stop);
    assert_eq!(names(&fx), ["Appended", "Save", "CycleEnded"]);
}

// What came of a save.

#[test]
fn a_failed_save_is_handed_back_and_said_once() {
    let (mut drive, _rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    let write = take_save(feed(&mut drive, turn_ends()));
    let fx = drive.handle(Input::Saved {
        write,
        result: Err("read-only".into()),
    });
    assert_eq!(names(&fx), ["SaveFailed(\"read-only\")"]);

    // Handed back: the next save carries it, and fails without a word.
    drive.prompt("again", &[], Origin::Person, Code::Here);
    let write = take_save(feed(&mut drive, turn_ends()));
    assert_eq!(
        write.from, 0,
        "the next save starts where the failed one did"
    );
    let fx = drive.handle(Input::Saved {
        write,
        result: Err("read-only".into()),
    });
    assert!(fx.is_empty());

    // A save that lands says the next failure again.
    drive.prompt("third", &[], Origin::Person, Code::Here);
    let write = take_save(feed(&mut drive, turn_ends()));
    assert!(drive
        .handle(Input::Saved {
            write,
            result: Ok(())
        })
        .is_empty());
    drive.prompt("fourth", &[], Origin::Person, Code::Here);
    let write = take_save(feed(&mut drive, turn_ends()));
    let fx = drive.handle(Input::Saved {
        write,
        result: Err("again".into()),
    });
    assert_eq!(fx.len(), 1);
}

// The stream ending, and stopping.

#[test]
fn a_stream_that_ends_calls_off_the_wait_and_says_so() {
    let (mut drive, _rx) = connected();
    feed(&mut drive, chunk("by itself"));
    let fx = drive.handle(Input::StreamEnded);
    assert_eq!(names(&fx), ["Disarm", "Disconnected"]);
    assert!(!drive.busy);
}

#[test]
fn stop_cancels_the_turn() {
    let (mut drive, mut rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    sent(&mut rx);
    drive.stop();
    assert!(matches!(rx.try_recv(), Ok(AcpRequest::Cancel)));
    assert!(drive.cancelled);
}
