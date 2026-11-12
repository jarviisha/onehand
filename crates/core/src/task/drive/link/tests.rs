//! A run and its session, both real, joined by the link alone, with no front
//! end: a front end's part is only to carry out what comes back.

use super::*;
use crate::acp::{AcpEvent, AcpRequest, ReqTx};
use crate::chat::drive::Effect as SessionEffect;
use crate::chat::Chat;
use crate::task::drive::Effect;
use crate::task::Task;
use crate::workflow::{builtin, Brief, Mark, Setup};
use std::path::PathBuf;

type Rx = tokio::sync::mpsc::UnboundedReceiver<AcpRequest>;

fn session() -> (SessionDrive, Rx) {
    let mut session = SessionDrive::new(Chat::default());
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    session.apply(
        AcpEvent::Connected {
            tx,
            resumed: false,
            forkable: false,
        },
        now(),
    );
    session.end_batch();
    (session, rx)
}

fn run() -> (String, Run, Action) {
    run_in(None)
}

/// A run whose setup puts the agent in `mode`.
fn run_in(mode: Option<&str>) -> (String, Run, Action) {
    let brief = Brief {
        title: "Fix the thing".into(),
        body: "It is broken.".into(),
        instructions: None,
    };
    let setup = Setup {
        repo: PathBuf::from("/repo"),
        dir: PathBuf::from("/repo"),
        branch: None,
        agent: None,
        check: Some("make check".into()),
        mode: mode.map(str::to_string),
        forge: None,
    };
    let task = Task::new("t1".into(), builtin::all().remove(0), brief, setup);
    let mut run = task.runs[0].clone();
    let first = run.resume(0);
    ("t1".into(), run, first)
}

/// The two drives and what joins them: the run's effects on its session go
/// through the link, the session's signals come back through it.
struct Pair {
    drive: RunDrive,
    session: SessionDrive,
    rx: Rx,
    now: crate::Now,
}

impl Pair {
    fn start() -> (Self, Vec<Effect>) {
        Self::start_on(session(), run())
    }

    fn start_on(
        (mut session, rx): (SessionDrive, Rx),
        (task, run, first): (String, Run, Action),
    ) -> (Self, Vec<Effect>) {
        let now = now();
        let (drive, fx, asked) = start(task, run, first, 0, false, now, Some(&mut session));
        let mut pair = Self {
            drive,
            session,
            rx,
            now,
        };
        pair.send(asked);
        let fx = pair.carry(fx);
        (pair, fx)
    }

    /// Tell the run `input`, and carry out what it says on its session.
    fn on(&mut self, input: Input) -> Vec<Effect> {
        let (fx, asked) = handle(&mut self.drive, input, Some(&mut self.session), self.now);
        self.send(asked);
        self.carry(fx)
    }

    /// Send what the session asks of its agent on its channel, as its
    /// carrier does.
    fn send(&self, fx: Vec<SessionEffect>) {
        send(&self.session, fx);
    }

    /// Carry out the run's session effects through the link; what is left is
    /// the front end's (pins, reads, saves, notes).
    fn carry(&mut self, fx: Vec<Effect>) -> Vec<Effect> {
        let mut left = Vec::new();
        for effect in fx {
            match effect {
                Effect::Submit(text) => {
                    let (answer, fx) = submit(&mut self.session, &text, now());
                    self.send(fx);
                    if let Some(input) = answer {
                        left.extend(self.on(input));
                    }
                }
                Effect::CancelTurn => {
                    let fx = cancel(&mut self.session);
                    self.send(fx);
                }
                other @ (Effect::Pin { .. }
                | Effect::Measure { .. }
                | Effect::ReadFacts { .. }
                | Effect::Note(_)
                | Effect::RunCommand { .. }
                | Effect::CallOffCommand
                | Effect::Push { .. }
                | Effect::FindPullRequest
                | Effect::OpenPullRequest
                | Effect::ReadLogs { .. }
                | Effect::MarkReady { .. }
                | Effect::Arm { .. }
                | Effect::Defer(_)
                | Effect::Save
                | Effect::Unattended(_)
                | Effect::Refresh
                | Effect::SessionChanged
                | Effect::Signal(_)
                | Effect::Finish { .. }) => left.push(other),
            }
        }
        left
    }

    /// The agent's event reaches the session, and what the session signals
    /// reaches the run.
    fn agent(&mut self, event: AcpEvent) -> Vec<Effect> {
        let (mut fx, _) = self.session.apply(event, now());
        fx.extend(self.session.end_batch());
        let mut out = Vec::new();
        let (requests, fx): (Vec<_>, Vec<_>) = fx
            .into_iter()
            .partition(|effect| session_name(effect) == "Request");
        self.send(requests);
        for effect in fx {
            match effect {
                SessionEffect::Signal(signal) => {
                    if let Some(input) = input_of(&signal, &self.drive, &self.session) {
                        out.extend(self.on(input));
                    }
                }
                SessionEffect::Changed
                | SessionEffect::OfferChanged
                | SessionEffect::Save(_)
                | SessionEffect::Arm { .. }
                | SessionEffect::Disarm
                | SessionEffect::SendCode { .. }
                | SessionEffect::PromptAgain
                | SessionEffect::Request(_)
                | SessionEffect::SaveTitle { .. }
                | SessionEffect::Look(_)
                | SessionEffect::Tell { .. } => {}
            }
        }
        out
    }
}

/// Send what `fx` asks of `session`'s agent on its channel, as a carrier
/// does.
fn send(session: &SessionDrive, fx: Vec<SessionEffect>) {
    for effect in fx {
        match effect {
            SessionEffect::Request(request) => {
                if let Some(tx) = session.agent_channel() {
                    let _ = tx.send(request);
                }
            }
            SessionEffect::Signal(_)
            | SessionEffect::Changed
            | SessionEffect::OfferChanged
            | SessionEffect::Save(_)
            | SessionEffect::Arm { .. }
            | SessionEffect::Disarm
            | SessionEffect::SendCode { .. }
            | SessionEffect::PromptAgain
            | SessionEffect::SaveTitle { .. }
            | SessionEffect::Look(_)
            | SessionEffect::Tell { .. } => {}
        }
    }
}

/// A run effect's name, every variant listed so a new one must be named here.
fn name(e: &Effect) -> &'static str {
    match e {
        Effect::Pin { .. } => "Pin",
        Effect::Measure { .. } => "Measure",
        Effect::ReadFacts { .. } => "ReadFacts",
        Effect::Submit(_) => "Submit",
        Effect::CancelTurn => "CancelTurn",
        Effect::Note(_) => "Note",
        Effect::RunCommand { .. } => "RunCommand",
        Effect::CallOffCommand => "CallOffCommand",
        Effect::Push { .. } => "Push",
        Effect::FindPullRequest => "FindPullRequest",
        Effect::OpenPullRequest => "OpenPullRequest",
        Effect::ReadLogs { .. } => "ReadLogs",
        Effect::MarkReady { .. } => "MarkReady",
        Effect::Arm { .. } => "Arm",
        Effect::Defer(_) => "Defer",
        Effect::Save => "Save",
        Effect::Unattended(_) => "Unattended",
        Effect::Refresh => "Refresh",
        Effect::SessionChanged => "SessionChanged",
        Effect::Signal(_) => "Signal",
        Effect::Finish { .. } => "Finish",
    }
}

fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter().map(name).collect()
}

/// A session effect's name, every variant listed.
fn session_name(e: &SessionEffect) -> &'static str {
    match e {
        SessionEffect::Signal(_) => "Signal",
        SessionEffect::Changed => "Changed",
        SessionEffect::OfferChanged => "OfferChanged",
        SessionEffect::Save(_) => "Save",
        SessionEffect::Arm { .. } => "Arm",
        SessionEffect::Disarm => "Disarm",
        SessionEffect::SendCode { .. } => "SendCode",
        SessionEffect::PromptAgain => "PromptAgain",
        SessionEffect::Request(_) => "Request",
        SessionEffect::SaveTitle { .. } => "SaveTitle",
        SessionEffect::Look(_) => "Look",
        SessionEffect::Tell { .. } => "Tell",
    }
}

/// A request's name, every variant listed.
fn request_name(r: &AcpRequest) -> &'static str {
    match r {
        AcpRequest::Prompt { .. } => "Prompt",
        AcpRequest::Cancel => "Cancel",
        AcpRequest::PermissionResponse { .. } => "PermissionResponse",
        AcpRequest::ElicitationResponse { .. } => "ElicitationResponse",
        AcpRequest::SetMode(_) => "SetMode",
        AcpRequest::SetConfigOption { .. } => "SetConfigOption",
        AcpRequest::StopBackground { .. } => "StopBackground",
    }
}

fn now() -> crate::Now {
    crate::Now {
        secs: crate::chat::now_secs(),
        at: std::time::Instant::now(),
    }
}

fn mark(head: &str) -> Mark {
    Mark {
        head: head.into(),
        digests: vec!["d0".into()],
    }
}

#[test]
fn a_runs_prompt_reaches_its_session_and_the_turns_end_moves_the_run_on() {
    let (mut pair, fx) = Pair::start();
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    pair.on(Input::Pinned(Ok("c0".into())));
    pair.on(Input::Measured(Ok(mark("a"))));
    assert!(
        pair.rx.try_recv().ok().as_ref().map(request_name) == Some("Prompt"),
        "the run's prompt went to the agent"
    );
    assert!(pair.session.busy);

    pair.agent(AcpEvent::AgentChunk {
        text: "The plan.".into(),
        message: None,
    });
    let fx = pair.agent(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(
        names(&fx).contains(&"ReadFacts"),
        "the turn's end reached the run: {fx:?}"
    );
}

#[test]
fn a_runs_prompt_to_a_busy_session_is_not_taken_and_the_run_hears_it() {
    let (mut session, _rx) = session();
    session.prompt("a person's", &[], Origin::Person, Code::Here, now());
    let (answer, fx) = submit(&mut session, "the run's", now());
    assert_eq!(answer, Some(Input::NotTaken));
    assert!(fx.is_empty());
    assert!(session.queued.is_none(), "a run's prompt never queues");
}

#[test]
fn a_runs_cancel_leaves_an_idle_session_alone() {
    let (mut session, mut rx) = session();
    let fx = cancel(&mut session);
    send(&session, fx);
    assert!(rx.try_recv().is_err(), "nothing to cancel");
    let (_, fx) = session.prompt("go", &[], Origin::Person, Code::Here, now());
    send(&session, fx);
    rx.try_recv().ok();
    let fx = cancel(&mut session);
    send(&session, fx);
    assert_eq!(
        rx.try_recv().ok().as_ref().map(request_name),
        Some("Cancel")
    );
}

#[test]
fn signals_that_move_no_run_are_not_passed_on() {
    let (pair, _) = Pair::start();
    for signal in [
        Signal::CycleEnded,
        Signal::StopRefused("no".into()),
        Signal::SaveFailed("disk".into()),
        Signal::CodeFailed("net".into()),
        Signal::RenameFailed("disk".into()),
    ] {
        assert_eq!(input_of(&signal, &pair.drive, &pair.session), None);
    }
    assert_eq!(
        input_of(&Signal::TurnEnded, &pair.drive, &pair.session),
        Some(Input::TurnEnded)
    );
}

/// The run's mode is made on the session through its own drive, never on its
/// model by the run: it reaches the agent, and the prompt waits for its
/// answer.
#[test]
fn a_runs_mode_reaches_the_agent_through_the_sessions_drive() {
    let (mut session, rx) = session();
    session.apply(
        AcpEvent::Modes {
            current: Some("default".into()),
            available: ["default", "plan"]
                .map(|id| crate::acp::Mode {
                    id: id.into(),
                    name: id.into(),
                })
                .to_vec(),
        },
        now(),
    );
    session.end_batch();
    let (mut pair, _) = Pair::start_on((session, rx), run_in(Some("plan")));
    pair.on(Input::Pinned(Ok("c0".into())));
    pair.on(Input::Measured(Ok(mark("a"))));
    let sent: Vec<AcpRequest> = std::iter::from_fn(|| pair.rx.try_recv().ok()).collect();
    assert_eq!(sent.len(), 1, "{sent:?}");
    match &sent[0] {
        AcpRequest::SetMode(mode) => assert_eq!(mode, "plan", "{sent:?}"),
        other @ (AcpRequest::Prompt { .. }
        | AcpRequest::Cancel
        | AcpRequest::PermissionResponse { .. }
        | AcpRequest::ElicitationResponse { .. }
        | AcpRequest::SetConfigOption { .. }
        | AcpRequest::StopBackground { .. }) => panic!("not the mode: {other:?}"),
    }
    assert!(pair.session.pick_waiting(crate::chat::MODE_SELECTOR));
    assert!(!pair.session.busy, "the prompt waits for the mode's answer");
}

#[test]
fn a_runs_session_is_named_for_its_task_unless_it_has_a_name() {
    let (pair, _) = Pair::start();
    assert_eq!(pair.session.custom_title.as_deref(), Some("Fix the thing"));
    let (mut session, rx) = session();
    session.rename("Mine");
    let (pair, _) = Pair::start_on((session, rx), run());
    assert_eq!(pair.session.custom_title.as_deref(), Some("Mine"));
}
