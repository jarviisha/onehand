//! A run and its session, both real, joined by the link alone, with no front
//! end: a front end's part is only to carry out what comes back.

use super::*;
use crate::acp::{AcpEvent, AcpRequest, ReqTx};
use crate::chat::drive::Effect as SessionEffect;
use crate::chat::{Chat, ChatItem};
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
        let (drive, out) = Joined::start(task, run, first, 0, now, Some(&mut session));
        let pair = Self {
            drive,
            session,
            rx,
            now,
        };
        pair.send(out.session);
        (pair, out.run)
    }

    /// Tell the run `input`; what it asks of its session is made there by
    /// the link, and what is left is the front end's (pins, reads, saves).
    fn on(&mut self, input: Input) -> Vec<Effect> {
        let out = Joined::new(&mut self.drive, Some(&mut self.session)).handle(input, self.now);
        self.send(out.session);
        out.run
    }

    /// A person pressed Stop.
    fn stop(&mut self) -> Vec<Effect> {
        let out = Joined::new(&mut self.drive, Some(&mut self.session)).stop(self.now);
        self.send(out.session);
        out.run
    }

    /// Send what the session asks of its agent on its channel, as its
    /// carrier does.
    fn send(&self, fx: Vec<SessionEffect>) {
        send(&self.session, fx);
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
                | SessionEffect::Unsent(_)
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
            | SessionEffect::Unsent(_)
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
        Effect::Refresh => "Refresh",
        Effect::SessionChanged => "SessionChanged",
        Effect::Signal(_) => "Signal",
        Effect::Finish { .. } => "Finish",
        Effect::OpenSession { .. } => "OpenSession",
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
        SessionEffect::Unsent(_) => "Unsent",
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

/// The lines onehand said in `session`'s transcript and the prompts sent
/// there, in order.
fn lines(session: &SessionDrive) -> Vec<String> {
    session
        .items
        .iter()
        .filter_map(|item| match item {
            ChatItem::Notice { text, .. } => Some(format!("note: {text}")),
            ChatItem::User(_) => Some("prompt".to_string()),
            ChatItem::Agent(_)
            | ChatItem::Thought(_)
            | ChatItem::Tool(_)
            | ChatItem::Plan(_)
            | ChatItem::Permission(_)
            | ChatItem::Ask(_)
            | ChatItem::Subagent(_) => None,
        })
        .collect()
}

#[test]
fn a_runs_step_is_said_in_its_transcript_before_its_prompt() {
    let (mut pair, _) = Pair::start();
    pair.on(Input::Pinned(Ok("c0".into())));
    let fx = pair.on(Input::Measured(Ok(mark("a"))));
    let lines = lines(&pair.session);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("note: Step 1 of "), "{lines:?}");
    assert_eq!(lines[1], "prompt");
    assert!(names(&fx).contains(&"Save"), "{fx:?}");
}

/// A session that does not take the run's prompt fails the run in the same
/// call: nothing is left for a front end to report.
#[test]
fn a_prompt_its_session_does_not_take_fails_the_run_at_once() {
    let (session, rx) = session();
    let (mut pair, _) = Pair::start_on((session, rx), run());
    pair.on(Input::Pinned(Ok("c0".into())));
    // A turn of somebody else's runs, which no prompt of the run's began.
    let mut busy = Chat::default();
    let (tx, _rx2): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    busy.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    busy.busy = true;
    pair.session = SessionDrive::new(busy);
    let fx = pair.on(Input::Measured(Ok(mark("a"))));
    let names = names(&fx);
    let place = |name| names.iter().position(|n| *n == name);
    assert!(place("Finish").is_some(), "{fx:?}");
    assert!(
        place("Finish") < place("Save"),
        "the run is over before the save its prompt was followed by: {names:?}"
    );
    assert!(
        lines(&pair.session)
            .iter()
            .all(|line| line.starts_with("note: ")),
        "no prompt went"
    );
    assert!(pair.session.queued.is_none(), "a run's prompt never queues");
}

/// A check's run has no session: it is driven through the same link, and
/// nothing is asked of anyone.
#[test]
fn a_run_with_no_session_asks_nothing_of_one() {
    let (task, run, first) = run();
    let (mut drive, out) = Joined::start(task, run, first, 0, now(), None);
    assert!(out.session.is_empty() && !out.told);
    let out = Joined::new(&mut drive, None).stop(now());
    assert!(out.session.is_empty() && !out.told);
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

/// Only a turn that runs is cancelled, which the run decides: a Stop on an
/// idle session asks its agent nothing, and one on a running turn cancels it.
#[test]
fn a_stop_cancels_only_a_turn_that_runs() {
    let (mut pair, _) = Pair::start();
    pair.on(Input::Pinned(Ok("c0".into())));
    let fx = pair.stop();
    assert!(names(&fx).contains(&"Finish"), "{fx:?}");
    assert!(pair.rx.try_recv().is_err(), "nothing to cancel");

    let (mut pair, _) = Pair::start();
    pair.on(Input::Pinned(Ok("c0".into())));
    pair.on(Input::Measured(Ok(mark("a"))));
    assert_eq!(
        pair.rx.try_recv().ok().as_ref().map(request_name),
        Some("Prompt")
    );
    pair.stop();
    assert_eq!(
        pair.rx.try_recv().ok().as_ref().map(request_name),
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

/// A run of `steps`, each an agent step, on the agent each names.
fn run_of(steps: &[(&str, &str, Option<&str>)]) -> (String, Run, Action) {
    let (task, mut run, _) = run();
    run.template.steps = steps
        .iter()
        .map(|(id, label, agent)| crate::workflow::StepSpec {
            limit: None,
            id: id.to_string(),
            label: label.to_string(),
            kind: crate::workflow::StepKind::Agent {
                prompt: format!("{label} it."),
                gates: Vec::new(),
                keep_answer: false,
                mode: None,
                options: Default::default(),
                agent: agent.map(str::to_string),
            },
        })
        .collect();
    run.step = 0;
    let first = run.resume(0);
    (task, run, first)
}

#[test]
fn a_step_lanes_session_is_named_for_the_task_and_step() {
    // The first session is the first step's lane.
    let (pair, _) = Pair::start_on(session(), run_of(&[("review", "Review", Some("Claude"))]));
    let named = pair.session.custom_title.as_deref();
    assert_eq!(named, Some("Fix the thing · Review"));

    // A lane opened later is named once it is bound.
    let steps = [
        ("implement", "Implement", None),
        ("review", "Review", Some("Claude")),
    ];
    let (mut pair, mut fx) = Pair::start_on(session(), run_of(&steps));
    for _ in 0..12 {
        if names(&fx).contains(&"OpenSession") {
            break;
        }
        fx = match names(&fx) {
            n if n.contains(&"Pin") => pair.on(Input::Pinned(Ok("c".into()))),
            n if n.contains(&"Measure") => pair.on(Input::Measured(Ok(mark("a")))),
            n if n.contains(&"ReadFacts") => pair.on(Input::TurnRead(Ok(crate::workflow::Facts {
                head: "a".into(),
                dirty: false,
                commits: 0,
                digest: "d0".into(),
            }))),
            _ => pair.agent(AcpEvent::TurnEnded {
                stop_reason: "end_turn".into(),
            }),
        };
    }
    assert!(names(&fx).contains(&"OpenSession"), "{fx:?}");
    let (mut lane, _rx) = session();
    let out = Joined::new(&mut pair.drive, Some(&mut lane)).handle(
        Input::LaneOpened {
            lane: Lane::Step("review".into()),
            result: Ok(()),
        },
        pair.now,
    );
    assert!(out.told);
    assert_eq!(lane.custom_title.as_deref(), Some("Fix the thing · Review"));
    assert!(lane.busy, "the review's prompt went to its own session");
    assert_eq!(pair.session.custom_title.as_deref(), Some("Fix the thing"));

    // The run's own session, left, is heard of only for a person's prompt
    // there and for its going.
    let run = Lane::Run;
    let heard =
        |signal: &Signal, pair: &Pair| lane_input_of(signal, &run, &pair.drive, &pair.session);
    assert_eq!(heard(&Signal::Appended, &pair), None);
    assert_eq!(heard(&Signal::TurnEnded, &pair), None);
    let gone = Signal::Disconnected(crate::chat::drive::Ended::Lost);
    assert_eq!(
        heard(&gone, &pair),
        Some(Input::LaneGone { lane: Lane::Run })
    );
    pair.session
        .prompt("Mine", &[], Origin::Person, Code::Here, now());
    assert_eq!(
        heard(&Signal::Appended, &pair),
        Some(Input::LaneAppended { lane: Lane::Run })
    );
}
