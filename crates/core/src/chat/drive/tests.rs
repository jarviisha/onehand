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
            Effect::SaveTitle { .. } => "SaveTitle".into(),
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

// Sending a failed turn's prompt again.

#[test]
fn a_failed_turns_prompt_goes_again() {
    let (mut drive, mut rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    sent(&mut rx);
    feed(
        &mut drive,
        AcpEvent::TurnEnded {
            stop_reason: "refusal".into(),
        },
    );
    let (intake, _) = drive.resend();
    assert_eq!(intake, Intake::Sent);
    assert_eq!(sent(&mut rx).as_deref(), Some("go"));
}

#[test]
fn a_resend_while_the_turn_still_runs_is_refused_and_leaves_what_waits() {
    let (mut drive, mut rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    sent(&mut rx);
    drive.prompt("waiting", &[], Origin::Person, Code::Here);
    // An error said before the turn it ends has closed.
    feed(
        &mut drive,
        AcpEvent::Error {
            text: "overloaded".into(),
            detail: None,
        },
    );
    assert!(drive.busy && drive.resend_text().is_some());
    let (intake, _) = drive.resend();
    assert_eq!(intake, Intake::Refused(Refusal::Blocked(SubmitBlock::Busy)));
    assert_eq!(
        drive.queued.as_ref().map(|q| q.text.as_str()),
        Some("waiting")
    );
    assert!(sent(&mut rx).is_none());
}

#[test]
fn nothing_failed_nothing_goes_again() {
    let (mut drive, mut rx) = connected();
    assert_eq!(drive.resend().0, Intake::Refused(Refusal::NotTaken));
    assert!(sent(&mut rx).is_none());
}

// Answering cards.

/// A question parked on a running turn, of one single-select field, or of a
/// single-select then a multi-select: its live index.
fn ask(drive: &mut SessionDrive, two: bool) -> usize {
    use crate::acp::{ElicitChoice, ElicitField, ElicitKind, Elicitation};
    let choice = |v: &str| ElicitChoice {
        value: v.into(),
        label: v.to_uppercase(),
        description: None,
    };
    let field = |key: &str, kind| ElicitField {
        key: key.into(),
        title: None,
        description: None,
        kind,
        custom_key: None,
    };
    let mut fields = vec![field(
        "q0",
        ElicitKind::Select(vec![choice("a"), choice("b")]),
    )];
    if two {
        fields.push(field(
            "q1",
            ElicitKind::MultiSelect(vec![choice("x"), choice("y")]),
        ));
    }
    drive.prompt("go", &[], Origin::Person, Code::Here);
    feed(
        drive,
        AcpEvent::Elicitation(Elicitation {
            rpc_id: serde_json::Value::from(9),
            tool_call_id: None,
            message: "pick".into(),
            fields,
            child: None,
        }),
    );
    drive.items.len() - 1
}

fn answered(rx: &mut Rx) -> Option<crate::acp::ElicitOutcome> {
    std::iter::from_fn(|| rx.try_recv().ok()).find_map(|request| match request {
        AcpRequest::ElicitationResponse { outcome, .. } => Some(outcome),
        _ => None,
    })
}

#[test]
fn a_pick_answers_a_one_question_form_and_only_that() {
    let (mut drive, mut rx) = connected();
    let one = ask(&mut drive, false);
    assert!(drive.ask_pick(one, 0, 1).0, "nothing left to decide");
    assert!(matches!(
        answered(&mut rx),
        Some(crate::acp::ElicitOutcome::Accept(_))
    ));

    let (mut drive, mut rx) = connected();
    let two = ask(&mut drive, true);
    assert!(!drive.ask_pick(two, 0, 1).0);
    assert!(
        answered(&mut rx).is_none(),
        "a second question is still open"
    );
}

#[test]
fn skipping_walks_on_and_the_last_skip_sends_what_the_rest_hold() {
    let (mut drive, mut rx) = connected();
    let two = ask(&mut drive, true);
    drive.ask_pick(two, 0, 0);
    assert_eq!(drive.ask_skip(two, 0).0, Some(1), "the next question shows");
    // The pick above was dropped with the skip, so nothing is left: declined.
    assert_eq!(drive.ask_skip(two, 1).0, None);
    assert!(matches!(
        answered(&mut rx),
        Some(crate::acp::ElicitOutcome::Decline)
    ));

    let (mut drive, mut rx) = connected();
    let two = ask(&mut drive, true);
    drive.ask_pick(two, 0, 0);
    assert_eq!(drive.ask_advance(two, 0).0, Some(1));
    assert_eq!(drive.ask_skip(two, 1).0, None);
    assert!(
        matches!(
            answered(&mut rx),
            Some(crate::acp::ElicitOutcome::Accept(_))
        ),
        "the answer above the skipped question is sent"
    );
}

#[test]
fn moving_on_from_the_last_question_sends_the_form() {
    let (mut drive, mut rx) = connected();
    let two = ask(&mut drive, true);
    drive.ask_pick(two, 0, 1);
    drive.ask_advance(two, 0);
    assert_eq!(drive.ask_advance(two, 1).0, None);
    assert!(matches!(
        answered(&mut rx),
        Some(crate::acp::ElicitOutcome::Accept(_))
    ));
}

#[test]
fn a_form_is_sent_only_once_it_holds_an_answer() {
    let (mut drive, mut rx) = connected();
    let two = ask(&mut drive, true);
    drive.ask_submit(two);
    assert!(answered(&mut rx).is_none());
    drive.ask_type(two, 0, "mine".into());
    assert!(drive.ask_at(two).is_some_and(|a| a.custom[0] == "mine"));
    drive.ask_submit(two);
    assert!(matches!(
        answered(&mut rx),
        Some(crate::acp::ElicitOutcome::Accept(_))
    ));
}

#[test]
fn a_declined_form_sends_no_answer() {
    let (mut drive, mut rx) = connected();
    let one = ask(&mut drive, false);
    drive.ask_decline(one);
    assert!(matches!(
        answered(&mut rx),
        Some(crate::acp::ElicitOutcome::Decline)
    ));
    assert!(drive.ask_at(one).is_none(), "and the card is settled");
}

// A conversation's name.

/// A connected session whose conversation is on disk already, as a rename
/// writes only a conversation that has a file.
fn written() -> (SessionDrive, Rx) {
    let (mut drive, rx) = connected();
    drive.prompt("go", &[], Origin::Person, Code::Here);
    take_save(feed(&mut drive, turn_ends()));
    (drive, rx)
}

fn title_write(fx: &[Effect]) -> Option<(u64, Option<String>)> {
    fx.iter().find_map(|effect| match effect {
        Effect::SaveTitle { change, title, .. } => Some((*change, title.clone())),
        _ => None,
    })
}

#[test]
fn a_rename_shows_at_once_and_is_written() {
    let (mut drive, _rx) = written();
    let (renamed, fx) = drive.rename("  the  plan ");
    assert!(renamed);
    assert_eq!(drive.custom_title.as_deref(), Some("the plan"));
    assert_eq!(title_write(&fx), Some((1, Some("the plan".into()))));
    assert!(!drive.rename("   ").0, "a blank name is not a rename");
}

#[test]
fn a_conversation_with_no_file_is_renamed_and_not_written() {
    let (mut drive, _rx) = connected();
    let (renamed, fx) = drive.rename("named");
    assert!(renamed);
    assert!(title_write(&fx).is_none(), "the first turn's end writes it");
}

#[test]
fn the_newest_name_failing_puts_back_the_last_written_once() {
    let (mut drive, _rx) = written();
    let (first, _) = title_write(&drive.rename("one").1).unwrap();
    drive.handle(Input::TitleSaved {
        change: first,
        title: Some("one".into()),
        result: Ok(()),
    });
    let (second, title) = title_write(&drive.rename("two").1).unwrap();
    let fx = drive.handle(Input::TitleSaved {
        change: second,
        title,
        result: Err("read-only".into()),
    });
    assert_eq!(names(&fx), ["RenameFailed(\"read-only\")"]);
    assert_eq!(
        drive.custom_title.as_deref(),
        Some("one"),
        "the last written"
    );
}

#[test]
fn a_name_failing_behind_a_newer_one_changes_nothing() {
    let (mut drive, _rx) = written();
    let (first, title) = title_write(&drive.rename("one").1).unwrap();
    drive.rename("two");
    let fx = drive.handle(Input::TitleSaved {
        change: first,
        title,
        result: Err("read-only".into()),
    });
    assert!(fx.is_empty(), "the newer write carries what is shown");
    assert_eq!(drive.custom_title.as_deref(), Some("two"));
}

#[test]
fn a_reset_failing_puts_the_given_name_back() {
    let (mut drive, _rx) = written();
    let (first, title) = title_write(&drive.rename("mine").1).unwrap();
    drive.handle(Input::TitleSaved {
        change: first,
        title,
        result: Ok(()),
    });
    let (reset, title) = title_write(&drive.reset_title()).unwrap();
    assert!(drive.custom_title.is_none());
    drive.handle(Input::TitleSaved {
        change: reset,
        title,
        result: Err("gone".into()),
    });
    assert_eq!(drive.custom_title.as_deref(), Some("mine"));
}

// A session on a host.

#[test]
fn a_lost_host_drops_the_held_prompt_and_changes_the_batch() {
    let (mut drive, tx, mut rx) = connecting();
    drive.prompt_when_up("go", Origin::Tools);
    let fx = drive.host_lost("Out of reach of box".into());
    assert_eq!(names(&fx), ["Appended"]);
    assert!(drive.held.is_none(), "dropped, as on any lost link");
    assert_eq!(names(&drive.end_batch()), ["Changed"]);
    // The same session coming up later sends nothing it was not asked again.
    feed(
        &mut drive,
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
    );
    assert!(sent(&mut rx).is_none());
}

#[test]
fn what_was_kept_is_read_back_without_a_word_of_turns() {
    let (mut drive, _rx) = connected();
    let fx = drive.read_back(
        [
            AcpEvent::UserChunk("then".into()),
            chunk("an old answer"),
            turn_ends(),
        ],
        Some("Out of reach of box".into()),
    );
    assert_eq!(names(&fx), ["Changed", "Appended"], "no TurnEnded, no Save");
    assert!(matches!(
        drive.items.last(),
        Some(crate::chat::ChatItem::Notice { text, .. }) if text == "Out of reach of box"
    ));
}
