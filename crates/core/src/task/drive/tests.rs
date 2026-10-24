use super::*;
use crate::acp::{AcpEvent, ConfigChoice, ConfigOption, Mode, ReqTx};
use crate::task::Task;
use crate::workflow::{builtin, Brief, Setup, StepKind, Template};
use crate::workflow::{CommandResult, Facts, Mark};
use std::path::PathBuf;

fn brief() -> Brief {
    Brief {
        title: "Fix the thing".into(),
        body: "It is broken.".into(),
        instructions: None,
    }
}

fn setup(mode: Option<&str>) -> Setup {
    Setup {
        repo: PathBuf::from("/repo"),
        dir: PathBuf::from("/repo"),
        branch: None,
        agent: None,
        check: Some("make check".into()),
        mode: mode.map(str::to_string),
        forge: None,
    }
}

fn checkout() -> Template {
    builtin::all().remove(0)
}

fn mark(head: &str, digest: &str) -> Mark {
    Mark {
        head: head.into(),
        digests: vec![digest.into()],
    }
}

fn facts(head: &str, digest: &str) -> Facts {
    Facts {
        head: head.into(),
        dirty: false,
        commits: 0,
        digest: digest.into(),
    }
}

/// A connected session, and what it sends kept alive.
fn connected() -> (
    Chat,
    tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>,
) {
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Connected { tx, resumed: false });
    (chat, rx)
}

/// A drive and its session, played the way the app plays them.
struct Rig {
    drive: Drive,
    chat: Chat,
    _rx: tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>,
    now: Instant,
}

impl Rig {
    fn start(template: Template, mode: Option<&str>, chat: (Chat, RxOf)) -> (Self, Vec<Effect>) {
        let task = Task::new("t1".into(), template, brief(), setup(mode));
        let mut run = task.runs[0].clone();
        let first = run.resume();
        Self::start_run(run, first, chat)
    }

    fn start_run(run: Run, first: Action, (chat, rx): (Chat, RxOf)) -> (Self, Vec<Effect>) {
        let now = Instant::now();
        let mut chat = chat;
        let (drive, fx) = Drive::start("t1".into(), run, first, 0, false, now, Some(&mut chat));
        let mut rig = Self {
            drive,
            chat,
            _rx: rx,
            now,
        };
        rig.play(&fx);
        (rig, fx)
    }

    fn on(&mut self, input: Input) -> Vec<Effect> {
        let fx = self.drive.on(input, Some(&mut self.chat), self.now);
        self.play(&fx);
        fx
    }

    /// What the app does to the session for an effect.
    fn play(&mut self, fx: &[Effect]) {
        for effect in fx {
            match effect {
                Effect::Submit(text) => {
                    assert!(self.chat.submit(text, &[]), "the session took {text:?}");
                }
                Effect::CancelTurn => self.chat.cancel_turn(),
                _ => {}
            }
        }
    }

    /// The agent answers the turn under way with `text`.
    fn turn_ends(&mut self, text: &str) -> Vec<Effect> {
        self.chat.apply(AcpEvent::AgentChunk(text.into()));
        self.chat.apply(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });
        self.on(Input::TurnEnded)
    }
}

type RxOf = tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>;

fn has(fx: &[Effect], wanted: impl Fn(&Effect) -> bool) -> bool {
    fx.iter().any(wanted)
}

fn finished(fx: &[Effect]) -> Option<&Outcome> {
    fx.iter().find_map(|e| match e {
        Effect::Finish { outcome } => Some(outcome),
        _ => None,
    })
}

fn submitted(fx: &[Effect]) -> bool {
    has(fx, |e| matches!(e, Effect::Submit(_)))
}

/// A checkout run through its plan, at its approval: started, pinned,
/// measured, its plan prompted and answered, and the next marks landed.
fn at_approval() -> Rig {
    let (mut rig, fx) = Rig::start(checkout(), None, connected());
    assert!(has(&fx, |e| matches!(e, Effect::Pin { .. })), "{fx:?}");
    let fx = rig.on(Input::Pinned(Ok("c0".into())));
    assert!(has(&fx, |e| matches!(e, Effect::Measure { .. })), "{fx:?}");
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(submitted(&fx), "{fx:?}");
    let fx = rig.turn_ends("The plan.");
    assert!(
        has(&fx, |e| matches!(e, Effect::ReadFacts { .. })),
        "{fx:?}"
    );
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    let fx = match has(&fx, |e| matches!(e, Effect::Pin { .. })) {
        true => rig.on(Input::Pinned(Ok("c1".into()))),
        false => fx,
    };
    assert!(rig.drive.run().awaiting_approval(), "{fx:?}");
    rig
}

fn approval_at(rig: &Rig) -> ApprovalAt {
    rig.drive.run().approval_at().expect("waits on an approval")
}

#[test]
fn a_pin_holds_what_is_asked_meanwhile_and_an_ending_wins() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    let fx = rig.on(Input::Approved { at });
    assert!(has(&fx, |e| matches!(e, Effect::Pin { .. })), "{fx:?}");
    assert!(!submitted(&fx), "the prompt waits on the pin");
    assert!(rig.on(Input::Stop).is_empty(), "held while the pin lands");
    let fx = rig.on(Input::Pinned(Ok("c2".into())));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(!submitted(&fx), "no prompt to a run said to be over");
}

#[test]
fn a_turn_ending_while_a_pin_is_held_is_not_judged() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    rig.on(Input::Approved { at });
    let fx = rig.on(Input::TurnEnded);
    assert!(
        !has(&fx, |e| matches!(e, Effect::ReadFacts { .. })),
        "{fx:?}"
    );
}

/// A run at its approval waits on a person as one held by a card does,
/// though its agent's turn is over: its session's row says so, as it does
/// for a card.
#[test]
fn an_approval_waits_on_a_person_until_answered() {
    let mut rig = at_approval();
    assert!(rig.drive.waits_on_person() && !rig.drive.card());
    let at = approval_at(&rig);
    rig.on(Input::Approved { at });
    assert!(!rig.drive.waits_on_person());
}

#[test]
fn an_approval_answers_and_says_so() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    let fx = rig.on(Input::Approved { at: at.clone() });
    assert!(has(&fx, |e| matches!(e, Effect::Answered)));
    // A second press of the same approval is judged stale and moves nothing.
    rig.on(Input::Pinned(Ok("c2".into())));
    let fx = rig.on(Input::Approved { at });
    assert!(
        !has(&fx, |e| matches!(e, Effect::Pin { .. } | Effect::Submit(_))),
        "{fx:?}"
    );
}

fn checking() -> (Rig, Vec<Effect>) {
    let task = Task::check("t1".into(), "make check".into(), setup(None));
    let mut run = task.runs[0].clone();
    let first = run.resume();
    let (mut rig, fx) = Rig::start_run(run, first, connected());
    let fx = match has(&fx, |e| matches!(e, Effect::Pin { .. })) {
        true => rig.on(Input::Pinned(Ok("c0".into()))),
        false => fx,
    };
    (rig, fx)
}

fn ran(passed: bool) -> CommandResult {
    CommandResult {
        passed,
        exit: Some(if passed { 0 } else { 1 }),
        tail: "ok".into(),
        commit: Some("a".into()),
        digest: Some("d1".into()),
    }
}

#[test]
fn a_stop_while_a_command_runs_waits_for_it() {
    let (mut rig, fx) = checking();
    assert!(
        has(&fx, |e| matches!(e, Effect::RunCommand { .. })),
        "{fx:?}"
    );
    let fx = rig.on(Input::Stop);
    assert!(has(&fx, |e| matches!(e, Effect::CallOffCommand)));
    assert_eq!(finished(&fx), None, "ended only once the command exits");
    assert!(rig.drive.stopping());
    let fx = rig.on(Input::CommandExited(ran(false)));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn the_first_stop_reason_is_the_one_kept() {
    let (mut rig, _) = checking();
    rig.on(Input::Stop);
    rig.now += Duration::from_secs(60 * 60 * 24);
    rig.on(Input::ClockFired);
    let fx = rig.on(Input::CommandExited(ran(true)));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn a_command_that_passes_is_said_and_judged() {
    let (mut rig, _) = checking();
    let fx = rig.on(Input::CommandExited(ran(true)));
    assert!(has(
        &fx,
        |e| matches!(e, Effect::Note(n) if n == "The command passed")
    ));
    assert!(finished(&fx).is_some() || has(&fx, |e| matches!(e, Effect::Pin { .. })));
}

#[test]
fn the_budget_does_not_run_while_a_person_is_waited_on() {
    let mut rig = at_approval();
    let fired = rig.drive.run().template.timeout_limit() * 2;
    rig.now += fired;
    let fx = rig.on(Input::ClockFired);
    assert_eq!(finished(&fx), None, "{fx:?}");
    assert!(has(&fx, |e| matches!(e, Effect::ArmClock(_))), "{fx:?}");
}

#[test]
fn the_clock_ends_the_run_once_nothing_is_left() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.now += rig.drive.run().template.timeout_limit() + Duration::from_secs(1);
    let fx = rig.on(Input::ClockFired);
    // Held behind the first pin, as every ending is; it wins once it lands.
    let fx = match finished(&fx) {
        Some(_) => fx,
        None => rig.on(Input::Pinned(Ok("c0".into()))),
    };
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::TimedOut)));
}

#[test]
fn a_finished_drive_ignores_everything() {
    let (mut rig, _) = checking();
    rig.on(Input::Stop);
    rig.on(Input::CommandExited(ran(true)));
    for input in [
        Input::Stop,
        Input::TurnEnded,
        Input::ClockFired,
        Input::Appended,
    ] {
        assert!(rig.on(input).is_empty());
    }
}

#[test]
fn a_gone_session_only_lets_the_run_end() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    let now = rig.now;
    // The session went while the next step's mark was pinned.
    let fx = rig.drive.on(Input::Approved { at }, None, now);
    assert!(has(&fx, |e| matches!(e, Effect::Pin { .. })), "{fx:?}");
    let fx = rig.drive.on(Input::Pinned(Ok("c2".into())), None, now);
    assert!(fx.is_empty(), "nothing to prompt: {fx:?}");
    let fx = rig.drive.on(Input::Stop, None, now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(
        !has(&fx, |e| matches!(e, Effect::Note(_))),
        "nowhere to say it"
    );
}

#[test]
fn the_step_is_announced_once_per_step() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    let a = rig.on(Input::Pinned(Ok("c0".into())));
    let b = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    let notes = |fx: &[Effect]| {
        fx.iter()
            .filter(|e| matches!(e, Effect::Note(n) if n.starts_with("Step ")))
            .count()
    };
    assert_eq!(notes(&a) + notes(&b), 1, "{a:?} {b:?}");
}

#[test]
fn spent_time_is_folded_into_what_is_saved() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.now += Duration::from_secs(90);
    let fx = rig.on(Input::Pinned(Ok("c0".into())));
    assert!(has(&fx, |e| matches!(e, Effect::Save)));
    assert_eq!(rig.drive.run().spent_secs, 90);
}

// ── Sending, coming up, settings ─────────────────────────────────────────

#[test]
fn a_prompt_waits_for_the_agent_to_come_up() {
    let (mut rig, _) = Rig::start(checkout(), None, (Chat::default(), connected().1));
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(!submitted(&fx), "not up yet");
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    rig._rx = rx;
    rig.chat.apply(AcpEvent::Connected { tx, resumed: false });
    let fx = rig.on(Input::Appended);
    assert!(submitted(&fx), "{fx:?}");
    assert!(has(&fx, |e| matches!(e, Effect::CameUp)));
}

#[test]
fn a_prompt_of_a_persons_own_is_a_take_over() {
    let mut rig = at_approval();
    assert!(rig.chat.submit("my own words", &[]));
    let fx = rig.on(Input::Appended);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::TakenOver)));
}

fn with_modes(ids: &[&str]) -> (Chat, RxOf) {
    let (mut chat, rx) = connected();
    chat.apply(AcpEvent::Modes {
        current: ids.first().map(|id| id.to_string()),
        available: ids
            .iter()
            .map(|id| Mode {
                id: id.to_string(),
                name: id.to_string(),
            })
            .collect(),
    });
    (chat, rx)
}

#[test]
fn a_mode_the_agent_does_not_offer_fails_the_run_on_its_mode() {
    let (mut rig, _) = Rig::start(checkout(), Some("acceptEdits"), with_modes(&["default"]));
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(has(&fx, |e| matches!(e, Effect::Pause { .. })), "{fx:?}");
    assert!(matches!(finished(&fx), Some(Outcome::Failed(_))), "{fx:?}");
    assert!(!submitted(&fx));
}

#[test]
fn the_runs_mode_is_set_before_its_first_prompt() {
    let (mut rig, _) = Rig::start(checkout(), Some("plan"), with_modes(&["default", "plan"]));
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.chat.pick_waiting(crate::chat::MODE_SELECTOR));
    assert!(
        has(
            &fx,
            |e| matches!(e, Effect::Note(n) if n == "Mode plan set")
        ),
        "{fx:?}"
    );
    assert!(!submitted(&fx), "the prompt waits for the mode's answer");
    rig.chat.apply(AcpEvent::PickConfirmed {
        group: crate::chat::MODE_SELECTOR.into(),
        value: "plan".into(),
    });
    let fx = rig.on(Input::Appended);
    assert_eq!(fx, vec![Effect::Defer(Input::PicksAnswered)]);
    let fx = rig.on(Input::PicksAnswered);
    assert!(submitted(&fx), "{fx:?}");
}

fn wanting_model() -> Template {
    let mut t = checkout();
    let StepKind::Agent { options, .. } = &mut t.steps[0].kind else {
        panic!("the first step of checkout() is an agent step")
    };
    options.insert("model".into(), "strong".into());
    t
}

fn with_models() -> (Chat, RxOf) {
    let (mut chat, rx) = connected();
    let choice = |value: &str| ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
    };
    chat.apply(AcpEvent::ConfigOptions(vec![ConfigOption {
        id: "model".into(),
        name: "Model".into(),
        category: None,
        current: Some("weak".into()),
        choices: vec![choice("weak"), choice("strong")],
    }]));
    (chat, rx)
}

#[test]
fn picks_go_one_at_a_time_and_the_prompt_waits() {
    let (mut rig, _) = Rig::start(wanting_model(), None, with_models());
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.chat.pick_waiting("model"), "{fx:?}");
    assert!(!submitted(&fx));
    rig.chat.apply(AcpEvent::PickConfirmed {
        group: "model".into(),
        value: "strong".into(),
    });
    let fx = rig.on(Input::PicksAnswered);
    assert!(submitted(&fx), "{fx:?}");
    assert!(has(
        &fx,
        |e| matches!(e, Effect::Note(n) if n.starts_with("Settings: "))
    ));
}

#[test]
fn a_refused_pick_fails_the_run_at_the_step() {
    let (mut rig, _) = Rig::start(wanting_model(), None, with_models());
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    rig.chat.apply(AcpEvent::PickRefused {
        group: "model".into(),
        why: "no".into(),
    });
    let fx = rig.on(Input::PickRefused("no".into()));
    assert!(
        matches!(finished(&fx), Some(Outcome::Failed(why)) if why.contains("strong")),
        "{fx:?}"
    );
    assert!(
        !has(&fx, |e| matches!(e, Effect::Pause { .. })),
        "a launcher run pauses nothing"
    );
}

#[test]
fn a_refusal_of_the_runs_mode_is_not_a_steps() {
    let (mut rig, _) = Rig::start(checkout(), Some("plan"), with_modes(&["default", "plan"]));
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    rig.chat.apply(AcpEvent::PickRefused {
        group: crate::chat::MODE_SELECTOR.into(),
        why: "not now".into(),
    });
    let fx = rig.on(Input::PickRefused("not now".into()));
    assert!(has(
        &fx,
        |e| matches!(e, Effect::Pause { fix, .. } if fix == "unattended.mode is fixed")
    ));
    assert!(matches!(finished(&fx), Some(Outcome::Failed(why)) if why.contains("mode `plan`")));
}

#[test]
fn a_persons_own_pick_refused_is_not_the_runs() {
    let mut rig = at_approval();
    let fx = rig.on(Input::PickRefused("no".into()));
    assert!(fx.is_empty(), "{fx:?}");
}

#[test]
fn a_prompt_not_taken_fails_the_run() {
    let mut rig = at_approval();
    let fx = rig.on(Input::NotTaken);
    assert!(matches!(finished(&fx), Some(Outcome::Failed(_))));
}

// ── The session's events ─────────────────────────────────────────────────

/// A checkout run with its plan prompt sent, waiting on the turn.
fn in_turn() -> Rig {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(submitted(&fx));
    rig
}

#[test]
fn a_cancelled_turn_is_a_persons_stop() {
    let mut rig = in_turn();
    rig.chat.cancel_turn();
    rig.chat.apply(AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    });
    let fx = rig.on(Input::TurnEnded);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn a_failed_turn_fails_the_run_in_its_words() {
    let mut rig = in_turn();
    rig.chat.apply(AcpEvent::Error {
        text: "The turn failed: out of credit".into(),
        detail: None,
    });
    rig.chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    let fx = rig.on(Input::TurnEnded);
    assert!(
        matches!(finished(&fx), Some(Outcome::Failed(why)) if why.contains("out of credit")),
        "{fx:?}"
    );
}

#[test]
fn a_card_pauses_the_budget_and_says_it_waits() {
    let mut rig = in_turn();
    let fx = rig.on(Input::AwaitingUser);
    assert!(has(&fx, |e| matches!(e, Effect::Waiting)));
    assert!(rig.drive.card() && rig.drive.waits_on_person());
    rig.now += rig.drive.run().template.timeout_limit() * 2;
    let fx = rig.on(Input::ClockFired);
    assert_eq!(finished(&fx), None, "a person is waited on");
}

#[test]
fn an_answered_card_resumes_and_says_so() {
    let mut rig = in_turn();
    rig.on(Input::AwaitingUser);
    let fx = rig.on(Input::Notified);
    assert_eq!(fx, vec![Effect::Answered]);
    assert!(!rig.drive.card());
    assert!(rig.on(Input::Notified).is_empty(), "said once");
}

#[test]
fn a_lost_link_ends_the_run() {
    let mut rig = in_turn();
    let fx = rig.on(Input::Disconnected);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::LinkLost)));
}

#[test]
fn a_closed_session_ends_the_run() {
    let mut rig = in_turn();
    let fx = rig.drive.on(Input::SessionGone, None, rig.now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::Closed)));
}

#[test]
fn a_turn_read_judges_the_answer_since_the_prompt() {
    let mut rig = in_turn();
    rig.turn_ends("The plan.");
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    assert!(
        rig.drive.run().awaiting_approval() || has(&fx, |e| matches!(e, Effect::Pin { .. })),
        "{fx:?}"
    );
    assert_eq!(
        rig.drive.run().outputs.get("plan").map(String::as_str),
        Some("The plan.")
    );
}

/// Streamed chunks are most of what a session says: only one that can move
/// the run is worth handing over.
#[test]
fn a_chunk_is_heeded_only_when_it_can_move_the_run() {
    let mut rig = in_turn();
    rig.chat.apply(AcpEvent::AgentChunk("thinking".into()));
    assert!(!rig.drive.heeds_appended(&rig.chat), "a reply streaming in");
    let mut taken = at_approval();
    assert!(taken.chat.submit("my own words", &[]));
    assert!(taken.drive.heeds_appended(&taken.chat), "a person's prompt");

    let (mut picking, _) = Rig::start(wanting_model(), None, with_models());
    picking.on(Input::Pinned(Ok("c0".into())));
    picking.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(picking.drive.heeds_appended(&picking.chat), "a pick out");

    let (mut waiting, _) = Rig::start(checkout(), None, (Chat::default(), connected().1));
    waiting.on(Input::Pinned(Ok("c0".into())));
    waiting.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(!waiting.drive.heeds_appended(&waiting.chat), "not up yet");
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    waiting._rx = rx;
    waiting
        .chat
        .apply(AcpEvent::Connected { tx, resumed: false });
    assert!(
        waiting.drive.heeds_appended(&waiting.chat),
        "the held prompt can go"
    );
}

// ── A drive without a session: a check ───────────────────────────────────

fn check_drive() -> (Drive, Vec<Effect>) {
    let task = Task::check("t1".into(), "make check".into(), setup(None));
    let mut run = task.runs[0].clone();
    let first = run.resume();
    Drive::start("t1".into(), run, first, 0, false, Instant::now(), None)
}

#[test]
fn a_check_runs_its_command_and_finishes_without_a_session() {
    let (mut drive, fx) = check_drive();
    let now = Instant::now();
    let fx = match has(&fx, |e| matches!(e, Effect::Pin { .. })) {
        true => drive.on(Input::Pinned(Ok("c0".into())), None, now),
        false => fx,
    };
    assert!(
        has(&fx, |e| matches!(e, Effect::RunCommand { .. })),
        "{fx:?}"
    );
    let fx = drive.on(Input::CommandExited(ran(true)), None, now);
    let fx = match has(&fx, |e| matches!(e, Effect::Pin { .. })) {
        true => drive.on(Input::Pinned(Ok("c1".into())), None, now),
        false => fx,
    };
    assert_eq!(finished(&fx), Some(&Outcome::Done), "{fx:?}");
}

#[test]
fn a_check_has_no_timeout() {
    let (_, fx) = check_drive();
    assert!(!has(&fx, |e| matches!(e, Effect::ArmClock(_))), "{fx:?}");
}

#[test]
fn a_check_stopped_before_its_command_runs_none() {
    let (mut drive, fx) = check_drive();
    assert!(has(&fx, |e| matches!(e, Effect::Pin { .. })), "{fx:?}");
    let now = Instant::now();
    assert!(drive.on(Input::Stop, None, now).is_empty());
    let fx = drive.on(Input::Pinned(Ok("c0".into())), None, now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(!has(&fx, |e| matches!(e, Effect::RunCommand { .. })));
}
