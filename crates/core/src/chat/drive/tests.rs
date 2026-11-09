use super::*;
use crate::acp::{
    AcpRequest, ConfigChoice, ConfigOption, PermissionOption, PermissionRequest, ReqTx, Usage,
};
use crate::workspace::HostName;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

/// What a session sent its agent, read off in order as a channel is.
#[derive(Clone, Default)]
struct Rx(Rc<RefCell<VecDeque<AcpRequest>>>);

impl Rx {
    fn try_recv(&mut self) -> Result<AcpRequest, ()> {
        self.0.borrow_mut().pop_front().ok_or(())
    }
}

/// The time now, as a front end reads it for the drive.
fn now() -> Now {
    Now {
        secs: crate::chat::now_secs(),
        at: std::time::Instant::now(),
    }
}

/// A drive with the one part of a front end these tests need: every request
/// it answers is taken off, as its carrier sends it to the agent. Its methods
/// stand in front of the drive's own of the same name.
struct Front {
    drive: SessionDrive,
    sent: Rx,
}

impl std::ops::Deref for Front {
    type Target = SessionDrive;
    fn deref(&self) -> &SessionDrive {
        &self.drive
    }
}

impl std::ops::DerefMut for Front {
    fn deref_mut(&mut self) -> &mut SessionDrive {
        &mut self.drive
    }
}

impl Front {
    fn new(drive: SessionDrive) -> (Self, Rx) {
        let sent = Rx::default();
        (
            Self {
                drive,
                sent: sent.clone(),
            },
            sent,
        )
    }

    fn take(&self, fx: &[Effect]) {
        for effect in fx {
            if let Effect::Request(request) = effect {
                self.sent.0.borrow_mut().push_back(request.clone());
            }
        }
    }

    fn prompt(
        &mut self,
        text: &str,
        staged: &[StagedAttachment],
        origin: Origin,
        code: Code,
    ) -> (Intake, Vec<Effect>) {
        let out = self.drive.prompt(text, staged, origin, code, now());
        self.take(&out.1);
        out
    }

    fn prompt_when_up(&mut self, text: &str, origin: Origin) -> (Intake, Vec<Effect>) {
        let out = self.drive.prompt_when_up(text, origin, now());
        self.take(&out.1);
        out
    }

    fn resend(&mut self) -> (Intake, Vec<Effect>) {
        let out = self.drive.resend(now());
        self.take(&out.1);
        out
    }

    fn stop(&mut self) -> Vec<Effect> {
        let out = self.drive.stop();
        self.take(&out);
        out
    }

    fn apply(&mut self, event: AcpEvent) -> (Vec<Effect>, Next) {
        let out = self.drive.apply(event, now());
        self.take(&out.0);
        out
    }

    fn handle(&mut self, input: Input) -> Vec<Effect> {
        let out = self.drive.handle(input, now());
        self.take(&out);
        out
    }

    fn read_back(
        &mut self,
        kept: impl IntoIterator<Item = AcpEvent>,
        away: Option<(HostName, Option<u64>)>,
    ) -> Vec<Effect> {
        let out = self.drive.read_back(kept, away, now());
        self.take(&out);
        out
    }

    fn watched(&mut self, event: AcpEvent, link: &HostLink) -> (Vec<Effect>, Next) {
        let out = self.drive.apply_watched(event, Some(link), now());
        self.take(&out.0);
        out
    }

    fn ask_pick(&mut self, idx: usize, field: usize, option: usize) -> (bool, Vec<Effect>) {
        let out = self.drive.ask_pick(idx, field, option);
        self.take(&out.1);
        out
    }

    fn ask_skip(&mut self, idx: usize, field: usize) -> (Option<usize>, Vec<Effect>) {
        let out = self.drive.ask_skip(idx, field);
        self.take(&out.1);
        out
    }

    fn ask_advance(&mut self, idx: usize, field: usize) -> (Option<usize>, Vec<Effect>) {
        let out = self.drive.ask_advance(idx, field);
        self.take(&out.1);
        out
    }

    fn ask_submit(&mut self, idx: usize) -> Vec<Effect> {
        let out = self.drive.ask_submit(idx);
        self.take(&out);
        out
    }

    fn ask_decline(&mut self, idx: usize) -> Vec<Effect> {
        let out = self.drive.ask_decline(idx);
        self.take(&out);
        out
    }
}

/// A connected session that keeps a transcript, and what it sends.
///
/// The store is a path nothing can be written to: a save is only built here,
/// and the drop that writes the rest fails there, as it may.
fn connected() -> (Front, Rx) {
    let store = PathBuf::from("/nonexistent/onehand-drive-tests");
    let chat = Chat::new(1, PathBuf::from("/repo"), "agent".into(), Some(store));
    let (mut drive, rx) = Front::new(SessionDrive::new(chat));
    let (tx, _): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
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
fn feed(drive: &mut Front, event: AcpEvent) -> Vec<Effect> {
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

/// Effects as names, for asserting their order. Requests to the agent are
/// left out: they are read off what was sent.
fn names(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter(|effect| !matches!(effect, Effect::Request(_)))
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
            Effect::Request(_) => unreachable!("left out above"),
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

/// The turn a queued prompt opens is written whole, when it ends: the save
/// of the turn before leaves it out, so its line is not frozen on disk while
/// the turn is still running, without the time it finished.
#[test]
fn a_queued_prompt_is_written_with_its_own_turn() {
    let (mut drive, _rx) = connected();
    drive.prompt("first", &[], Origin::Person, Code::Here);
    drive.prompt("second", &[], Origin::Person, Code::Here);
    let first = take_save(feed(&mut drive, turn_ends()));
    assert!(drive.busy, "the queued prompt went");
    assert!(
        !first.lines.iter().any(|line| line.contains("second")),
        "{:?}",
        first.lines
    );

    let second = take_save(feed(&mut drive, turn_ends()));
    let line = second
        .lines
        .iter()
        .find(|line| line.contains("\"second\""))
        .expect("written with its own turn");
    assert!(line.contains("completed_at"), "{line}");
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

fn connecting() -> (Front, ReqTx, Rx) {
    let (mut drive, rx) = Front::new(SessionDrive::new(Chat::default()));
    let (tx, _): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    drive.chat.store = None;
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
    drive.chat.set_config_option("effort", "high");
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
fn ask(drive: &mut Front, two: bool) -> usize {
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
fn written() -> (Front, Rx) {
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
    let lost = HostLink {
        host: HostName("box".into()),
        closed: true,
        taken: false,
    };
    let (fx, next) = drive.watched(AcpEvent::Disconnected("lost the connection".into()), &lost);
    assert_eq!(names(&fx), ["Appended"]);
    assert!(matches!(next, Next::Stop), "the batch ends there");
    assert!(drive.held.is_none(), "dropped, as on any lost link");
    assert_eq!(drive.host_away(), Some(&HostName("box".into())));
    assert!(matches!(
        drive.items.last(),
        Some(crate::chat::ChatItem::Notice { text, .. })
            if text.starts_with("Not connected to box. This is the copy kept on this machine, up to")
    ));
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
        Some((HostName("box".into()), None)),
    );
    assert_eq!(names(&fx), ["Changed", "Appended"], "no TurnEnded, no Save");
    assert!(matches!(
        drive.items.last(),
        Some(crate::chat::ChatItem::Notice { text, .. })
            if text == "Not connected to box. This is the copy kept on this machine; \
                        it carries on once the host is back"
    ));
    assert_eq!(drive.host_away(), Some(&HostName("box".into())));
    let (was, _) = drive.drive.forget_host_away();
    assert!(was);
    assert!(drive.host_away().is_none());
}

/// A watch that ends with its connection taken by another machine, or on
/// its own end with the connection open, is folded as any end is: the
/// session's agent is lost, and nothing waits for the host.
#[test]
fn only_a_lost_connection_is_out_of_reach() {
    for (closed, taken) in [(true, true), (false, false)] {
        let (mut drive, _rx) = connected();
        let link = HostLink {
            host: HostName("box".into()),
            closed,
            taken,
        };
        drive.watched(AcpEvent::Disconnected("gone".into()), &link);
        assert!(drive.host_away().is_none(), "{closed} {taken}");
    }
}

/// A session waiting on its host's busy sessions says so, and stops
/// waiting once told; being out of reach is another wait, kept apart.
#[test]
fn a_busy_host_is_waited_on_apart_from_one_away() {
    let (mut drive, _rx) = connected();
    drive.drive.wait_on_host(HostName("box".into()), 2, 0);
    assert_eq!(drive.host_waiting(), Some(&HostName("box".into())));
    assert!(drive.host_away().is_none());
    assert!(!drive.drive.forget_host_away().0, "not away");
    assert!(drive.drive.forget_host_wait().0);
    assert!(drive.host_waiting().is_none());
    assert!(!drive.drive.forget_host_wait().0, "said once");
}

/// The copy's notice says how recent it is when its entries kept a time, and
/// leaves that out when none did.
#[test]
fn the_offline_line_says_how_recent_the_copy_is() {
    let vps = HostName("vps".into());
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

// Closing with a save on its way.

/// A session on a store that can be written, under a directory of its own.
fn on_disk(name: &str) -> (Front, Rx, PathBuf) {
    let store =
        std::env::temp_dir().join(format!("onehand-drive-close-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&store);
    let chat = Chat::new(
        1,
        PathBuf::from("/repo"),
        "agent".into(),
        Some(store.clone()),
    );
    let (mut drive, rx) = Front::new(SessionDrive::new(chat));
    let (tx, _): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    feed(
        &mut drive,
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
    );
    feed(&mut drive, AcpEvent::SessionId("s1".into()));
    (drive, rx, store)
}

fn turn(drive: &mut Front, prompt: &str, answer: &str) -> Vec<Effect> {
    drive.prompt(prompt, &[], Origin::Person, Code::Here);
    feed(drive, chunk(answer));
    feed(drive, turn_ends())
}

/// What the file says, prompts and answers alike, once each in order.
fn written_lines(store: &std::path::Path) -> Vec<String> {
    let dir = crate::chat::store::conv_dir(store, "s1");
    std::fs::read_to_string(dir.join("items.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A session closed while its last turn's save is on its way, and
/// that save failing then: the turn is on disk all the same, as the close
/// wrote the whole transcript rather than what came after that save.
#[test]
fn a_session_closed_with_a_save_on_its_way_keeps_that_turn_if_the_save_fails() {
    let (mut drive, _rx, store) = on_disk("fails");
    let first = take_save(turn(&mut drive, "one", "first answer"));
    crate::chat::store::commit(&first).unwrap();
    drive.handle(Input::Saved {
        write: first,
        result: Ok(()),
    });
    // The second turn's save is on its way, and never lands.
    let _lost = take_save(turn(&mut drive, "two", "second answer"));
    drop(drive);
    let lines = written_lines(&store);
    assert!(
        lines.iter().any(|l| l.contains("second answer")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("first answer")),
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// A session restarted while its last turn's save is on its way, and that
/// save failing then: the turn is on disk all the same, as the session that
/// carries on writes the whole transcript at its first save.
#[test]
fn a_session_restarted_with_a_save_on_its_way_keeps_that_turn_if_the_save_fails() {
    let (mut drive, _rx, store) = on_disk("restarted");
    let first = take_save(turn(&mut drive, "one", "first answer"));
    crate::chat::store::commit(&first).unwrap();
    drive.handle(Input::Saved {
        write: first,
        result: Ok(()),
    });
    // The second turn's save is on its way, and never lands.
    let _lost = take_save(turn(&mut drive, "two", "second answer"));
    let snapshot = drive.drive.take_snapshot(now()).unwrap();
    assert!(snapshot.rewrite, "a save was on its way");
    drop(drive);
    let chat = Chat::new(
        1,
        PathBuf::from("/repo"),
        "agent".into(),
        Some(store.clone()),
    );
    let mut next = SessionDrive::new(chat);
    next.resume(snapshot);
    drop(next);
    let lines = written_lines(&store);
    assert!(
        lines.iter().any(|l| l.contains("second answer")),
        "{lines:?}"
    );
    assert_eq!(
        lines.iter().filter(|l| l.contains("first answer")).count(),
        1,
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// With every save answered, the next session carries on from the mark.
#[test]
fn a_session_restarted_with_nothing_on_its_way_carries_on_from_the_mark() {
    let (mut drive, _rx, store) = on_disk("settled");
    let first = take_save(turn(&mut drive, "one", "first answer"));
    crate::chat::store::commit(&first).unwrap();
    drive.handle(Input::Saved {
        write: first,
        result: Ok(()),
    });
    let snapshot = drive.drive.take_snapshot(now()).unwrap();
    assert!(!snapshot.rewrite);
    drop(drive);
    let _ = std::fs::remove_dir_all(&store);
}

/// And that save landing late, after the close wrote the whole file, adds
/// nothing a second time.
#[test]
fn a_save_landing_after_the_close_wrote_the_whole_file_adds_nothing_twice() {
    let (mut drive, _rx, store) = on_disk("lands");
    let first = take_save(turn(&mut drive, "one", "first answer"));
    crate::chat::store::commit(&first).unwrap();
    drive.handle(Input::Saved {
        write: first,
        result: Ok(()),
    });
    let late = take_save(turn(&mut drive, "two", "second answer"));
    drop(drive);
    let closed = written_lines(&store);
    crate::chat::store::commit(&late).unwrap();
    assert_eq!(
        written_lines(&store),
        closed,
        "the late save's lines are in it"
    );
    assert_eq!(
        closed
            .iter()
            .filter(|l| l.contains("second answer"))
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// With nothing on its way, a close writes only what is left, as ever.
#[test]
fn a_session_closed_with_nothing_on_its_way_adds_only_the_rest() {
    let (mut drive, _rx, store) = on_disk("idle");
    let first = take_save(turn(&mut drive, "one", "first answer"));
    crate::chat::store::commit(&first).unwrap();
    drive.handle(Input::Saved {
        write: first,
        result: Ok(()),
    });
    let before = written_lines(&store);
    drop(drive);
    assert_eq!(
        written_lines(&store),
        before,
        "nothing left, nothing written"
    );
    let _ = std::fs::remove_dir_all(&store);
}

// The time, told rather than read.

/// What the session writes down and how long it says a step took come from
/// the time its front end told it, never from a clock of its own.
#[test]
fn a_session_stamps_and_times_by_the_time_it_is_told() {
    let (mut front, _rx) = connected();
    let start = std::time::Instant::now();
    let at = |secs: u64| Now {
        secs: 1_000 + secs,
        at: start + std::time::Duration::from_secs(secs),
    };
    front
        .drive
        .prompt("go", &[], Origin::Person, Code::Here, at(0));
    let Some(crate::chat::ChatItem::User(prompt)) = front.items.last() else {
        panic!("no prompt");
    };
    assert_eq!(prompt.sent_at, Some(1_000));

    front.drive.apply(
        AcpEvent::ToolCall(crate::acp::ToolCall {
            id: "t".into(),
            title: "cargo test".into(),
            description: None,
            kind: crate::acp::ToolKind::Execute,
            status: crate::acp::ToolStatus::InProgress,
            content: Vec::new(),
        }),
        at(1),
    );
    front.drive.apply(
        AcpEvent::ToolUpdate(crate::acp::ToolCallUpdate {
            id: "t".into(),
            status: Some(crate::acp::ToolStatus::Completed),
            title: None,
            description: None,
            content: None,
        }),
        at(6),
    );
    let Some(crate::chat::ChatItem::Tool(step)) = front.items.last() else {
        panic!("no step");
    };
    assert_eq!(step.elapsed_secs, Some(5), "from 1 to 6, as told");
    assert_eq!(step.settled_at, Some(1_006));
}
