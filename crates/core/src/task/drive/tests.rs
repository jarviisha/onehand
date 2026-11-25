use super::*;
use crate::acp::{AcpEvent, ConfigChoice, ConfigOption, Mode, ReqTx};
use crate::connector::{Check, CheckState, PrState, PullRequest};
use crate::task::Task;
use crate::workflow::{builtin, Brief, Setup, StepKind, StepSpec, Template};
use crate::workflow::{CommandResult, Facts, Mark, Outcome};
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
    chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    (chat, rx)
}

/// A drive and its session, played the way the app plays them.
struct Rig {
    drive: RunDrive,
    chat: Chat,
    /// What the last call asked of the session, beside its picks.
    asked: Vec<Asked>,
    _rx: tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>,
    now: crate::Now,
    /// The session of every lane but the run's, once opened.
    others: std::collections::BTreeMap<Lane, (Chat, RxOf)>,
    /// Everything every call answered, in order.
    seen: Vec<Effect>,
    /// Every note said, with the lane whose session it was said in.
    said: Vec<(Lane, String)>,
}

impl Rig {
    fn start(template: Template, mode: Option<&str>, chat: (Chat, RxOf)) -> (Self, Vec<Effect>) {
        let task = Task::new("t1".into(), template, brief(), setup(mode));
        let mut run = task.runs[0].clone();
        let first = run.resume(0);
        Self::start_run(run, first, chat)
    }

    fn start_run(run: Run, first: Action, (chat, rx): (Chat, RxOf)) -> (Self, Vec<Effect>) {
        let now = crate::Now::read();
        let (drive, fx) = RunDrive::start("t1".into(), run, first, 0, now, Some(&chat));
        let mut rig = Self {
            drive,
            chat,
            asked: Vec::new(),
            _rx: rx,
            now,
            others: Default::default(),
            seen: fx.clone(),
            said: Vec::new(),
        };
        // The first session is the first lane's.
        let lane = rig.drive.current_lane().clone();
        if lane != Lane::Run {
            let first = std::mem::take(&mut rig.chat);
            let (_, rx) = connected();
            rig.others
                .insert(lane.clone(), (first, std::mem::replace(&mut rig._rx, rx)));
        }
        rig.play(&lane);
        (rig, fx)
    }

    /// The session of `lane`, when it has one.
    fn chat_of(&self, lane: &Lane) -> Option<&Chat> {
        match lane {
            Lane::Run => Some(&self.chat),
            Lane::Step(_) => self.others.get(lane).map(|(chat, _)| chat),
        }
    }

    fn chat_of_mut(&mut self, lane: &Lane) -> Option<&mut Chat> {
        match lane {
            Lane::Run => Some(&mut self.chat),
            Lane::Step(_) => self.others.get_mut(lane).map(|(chat, _)| chat),
        }
    }

    /// Give the run a call with its current lane's session, as the app
    /// does, and play what it asked of that session there.
    fn call(
        &mut self,
        call: impl FnOnce(&mut RunDrive, Option<&Chat>, Now) -> Vec<Effect>,
    ) -> Vec<Effect> {
        let lane = self.drive.current_lane().clone();
        let chat = match &lane {
            Lane::Run => Some(&self.chat),
            Lane::Step(_) => self.others.get(&lane).map(|(chat, _)| chat),
        };
        let fx = call(&mut self.drive, chat, self.now);
        self.play(&lane);
        self.seen.extend(fx.clone());
        fx
    }

    fn on(&mut self, input: Input) -> Vec<Effect> {
        self.call(|drive, chat, now| drive.handle(input, chat, now))
    }

    fn approve(&mut self, at: ApprovalAt) -> Vec<Effect> {
        self.call(|drive, chat, now| drive.approve(at, chat, now))
    }

    fn stop(&mut self) -> Vec<Effect> {
        self.call(|drive, chat, now| drive.stop(chat, now))
    }

    /// What the app does to `lane`'s session for what the run asked of it.
    fn play(&mut self, lane: &Lane) {
        let picks = self.drive.take_picks();
        let asked = self.drive.take_asked();
        self.asked = asked.clone();
        for asked in &asked {
            if let Asked::Note(note) = asked {
                self.said.push((lane.clone(), note.clone()));
            }
        }
        let Some(chat) = self.chat_of_mut(lane) else {
            assert!(picks.is_empty(), "picks with no session: {picks:?}");
            return;
        };
        // What the link makes on the session first: the run's mode and picks.
        for pick in picks {
            chat.pick(&pick);
        }
        // Then the rest of what it asked of the session, in order.
        for asked in &asked {
            match asked {
                Asked::Prompt { text, .. } => {
                    assert!(chat.submit(text, &[]), "the session took {text:?}");
                }
                Asked::Cancel => chat.cancel_turn(),
                Asked::Name(name) => chat.custom_title = Some(name.clone()),
                Asked::Note(_) => {}
            }
        }
    }

    /// The notes the last call said, in order.
    fn notes(&self) -> Vec<&str> {
        self.asked
            .iter()
            .filter_map(|asked| match asked {
                Asked::Note(note) => Some(note.as_str()),
                Asked::Prompt { .. } | Asked::Cancel | Asked::Name(_) => None,
            })
            .collect()
    }

    /// Whether the last call sent the run's prompt.
    fn submitted(&self) -> bool {
        self.asked.iter().any(|asked| match asked {
            Asked::Prompt { .. } => true,
            Asked::Note(_) | Asked::Cancel | Asked::Name(_) => false,
        })
    }

    /// The agent answers the turn under way with `text`, in the current
    /// lane's session.
    fn turn_ends(&mut self, text: &str) -> Vec<Effect> {
        let lane = self.drive.current_lane().clone();
        let chat = self.chat_of_mut(&lane).expect("the lane has its session");
        chat.apply(AcpEvent::AgentChunk {
            text: text.into(),
            message: None,
        });
        chat.apply(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });
        self.on(Input::TurnEnded)
    }
}

type RxOf = tokio::sync::mpsc::UnboundedReceiver<crate::acp::AcpRequest>;

fn has(fx: &[Effect], wanted: impl Fn(&Effect) -> bool) -> bool {
    fx.iter().any(wanted)
}

/// An effect's name, every variant listed so a new one must be named here;
/// a clock and a signal carry their own kind.
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
        Effect::Arm { timer, .. } => match timer {
            Timer::Timeout => "Arm(Timeout)",
            Timer::Checks => "Arm(Checks)",
        },
        Effect::Defer(_) => "Defer",
        Effect::Save => "Save",
        Effect::Refresh => "Refresh",
        Effect::SessionChanged => "SessionChanged",
        Effect::Signal(signal) => match signal {
            Signal::SaveFailed(_) => "Signal(SaveFailed)",
            Signal::PinFailed(_) => "Signal(PinFailed)",
        },
        Effect::Finish { .. } => "Finish",
        Effect::OpenSession { .. } => "OpenSession",
        Effect::AskRoom { .. } => "AskRoom",
        Effect::LeaveRoom => "LeaveRoom",
    }
}

fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter().map(name).collect()
}

fn finished(fx: &[Effect]) -> Option<&Outcome> {
    fx.iter().find_map(|e| match e {
        Effect::Finish { outcome } => Some(outcome),
        Effect::Pin { .. }
        | Effect::Measure { .. }
        | Effect::ReadFacts { .. }
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
        | Effect::Refresh
        | Effect::SessionChanged
        | Effect::Signal(_)
        | Effect::OpenSession { .. }
        | Effect::AskRoom { .. }
        | Effect::LeaveRoom => None,
    })
}

/// Why the run failed, if it finished failed.
fn failed(fx: &[Effect]) -> Option<&str> {
    match finished(fx) {
        Some(Outcome::Failed(why)) => Some(why),
        Some(Outcome::Done | Outcome::Stopped(_) | Outcome::Exhausted { .. }) | None => None,
    }
}

/// A checkout run through its plan, at its approval: started, pinned,
/// measured, its plan prompted and answered, and the next marks landed.
fn at_approval() -> Rig {
    let (mut rig, fx) = Rig::start(checkout(), None, connected());
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    let fx = rig.on(Input::Pinned(Ok("c0".into())));
    assert!(names(&fx).contains(&"Measure"), "{fx:?}");
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.submitted(), "{fx:?}");
    let fx = rig.turn_ends("The plan.");
    assert!(names(&fx).contains(&"ReadFacts"), "{fx:?}");
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    let fx = match names(&fx).contains(&"Pin") {
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
    let fx = rig.approve(at);
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    assert!(!rig.submitted(), "the prompt waits on the pin");
    assert!(
        rig.stop().is_empty() && rig.asked.is_empty(),
        "held while the pin lands"
    );
    let fx = rig.on(Input::Pinned(Ok("c2".into())));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(!rig.submitted(), "no prompt to a run said to be over");
}

#[test]
fn a_turn_ending_while_a_pin_is_held_is_not_judged() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    rig.approve(at);
    let fx = rig.on(Input::TurnEnded);
    assert!(!names(&fx).contains(&"ReadFacts"), "{fx:?}");
}

/// A run at its approval waits on a person as one held by a card does,
/// though its agent's turn is over: its session's row says so, as it does
/// for a card.
#[test]
fn an_approval_waits_on_a_person_until_answered() {
    let mut rig = at_approval();
    assert!(rig.drive.waits_on_person(false) && !rig.drive.card());
    let at = approval_at(&rig);
    rig.approve(at);
    assert!(!rig.drive.waits_on_person(true));
}

#[test]
fn an_approval_answers() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    rig.approve(at.clone());
    // A second press of the same approval is judged stale and moves nothing.
    rig.on(Input::Pinned(Ok("c2".into())));
    let fx = rig.approve(at);
    assert!(!names(&fx).contains(&"Pin") && !rig.submitted(), "{fx:?}");
}

fn checking() -> (Rig, Vec<Effect>) {
    let task = Task::check("t1".into(), "make check".into(), setup(None));
    let mut run = task.runs[0].clone();
    let first = run.resume(0);
    let (mut rig, fx) = Rig::start_run(run, first, connected());
    let fx = match names(&fx).contains(&"Pin") {
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
    assert!(names(&fx).contains(&"RunCommand"), "{fx:?}");
    let fx = rig.stop();
    assert!(names(&fx).contains(&"CallOffCommand"));
    assert_eq!(finished(&fx), None, "ended only once the command exits");
    assert!(rig.drive.stopping());
    let fx = rig.on(Input::CommandExited(ran(false)));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn the_first_stop_reason_is_the_one_kept() {
    let (mut rig, _) = checking();
    rig.stop();
    rig.now += Duration::from_secs(60 * 60 * 24);
    rig.on(Input::Elapsed(Timer::Timeout));
    let fx = rig.on(Input::CommandExited(ran(true)));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn a_command_that_passes_is_said_and_judged() {
    let (mut rig, _) = checking();
    let fx = rig.on(Input::CommandExited(ran(true)));
    assert!(rig.notes().contains(&"The command passed"));
    assert!(finished(&fx).is_some() || names(&fx).contains(&"Pin"));
}

#[test]
fn the_budget_does_not_run_while_a_person_is_waited_on() {
    let mut rig = at_approval();
    let fired = rig.drive.run().template.timeout_limit() * 2;
    rig.now += fired;
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
    assert_eq!(finished(&fx), None, "{fx:?}");
    assert!(names(&fx).contains(&"Arm(Timeout)"), "{fx:?}");
}

#[test]
fn the_clock_ends_the_run_once_nothing_is_left() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.now += rig.drive.run().template.timeout_limit() + Duration::from_secs(1);
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
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
    rig.stop();
    rig.on(Input::CommandExited(ran(true)));
    assert!(rig.stop().is_empty() && rig.asked.is_empty());
    for input in [
        Input::TurnEnded,
        Input::Elapsed(Timer::Timeout),
        Input::Appended,
    ] {
        assert!(rig.on(input).is_empty() && rig.asked.is_empty());
    }
}

#[test]
fn a_gone_session_only_lets_the_run_end() {
    let mut rig = at_approval();
    let at = approval_at(&rig);
    let now = rig.now;
    // The session went while the next step's mark was pinned.
    let fx = rig.drive.approve(at, None, now);
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    let fx = rig.drive.handle(Input::Pinned(Ok("c2".into())), None, now);
    assert!(fx.is_empty(), "nothing to prompt: {fx:?}");
    let fx = rig.drive.stop(None, now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(rig.drive.take_asked().is_empty(), "nowhere to say it");
}

#[test]
fn the_step_is_announced_once_per_step() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    let steps = |rig: &Rig| {
        rig.notes()
            .iter()
            .filter(|n| n.starts_with("Step "))
            .count()
    };
    rig.on(Input::Pinned(Ok("c0".into())));
    let a = steps(&rig);
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert_eq!(a + steps(&rig), 1);
}

#[test]
fn spent_time_is_folded_into_what_is_saved() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.now += Duration::from_secs(90);
    let fx = rig.on(Input::Pinned(Ok("c0".into())));
    assert!(names(&fx).contains(&"Save"));
    assert_eq!(rig.drive.run().spent_secs, 90);
}

// ── Sending, coming up, settings ─────────────────────────────────────────

#[test]
fn a_prompt_waits_for_the_agent_to_come_up() {
    let (mut rig, _) = Rig::start(checkout(), None, (Chat::default(), connected().1));
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(!rig.submitted(), "not up yet");
    let (tx, rx): (ReqTx, _) = tokio::sync::mpsc::unbounded_channel();
    rig._rx = rx;
    rig.chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    let fx = rig.on(Input::Appended);
    assert!(rig.submitted(), "{fx:?}");
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
    assert!(failed(&fx).is_some(), "{fx:?}");
    assert!(!rig.submitted());
}

#[test]
fn the_runs_mode_is_set_before_its_first_prompt() {
    let (mut rig, _) = Rig::start(checkout(), Some("plan"), with_modes(&["default", "plan"]));
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.chat.pick_waiting(crate::chat::MODE_SELECTOR));
    assert!(rig.notes().contains(&"Mode plan set"), "{fx:?}");
    assert!(!rig.submitted(), "the prompt waits for the mode's answer");
    rig.chat.apply(AcpEvent::PickConfirmed {
        group: crate::chat::MODE_SELECTOR.into(),
        value: "plan".into(),
    });
    let fx = rig.on(Input::Appended);
    assert_eq!(fx, vec![Effect::Defer(Input::PicksAnswered)]);
    let fx = rig.on(Input::PicksAnswered);
    assert!(rig.submitted(), "{fx:?}");
}

fn wanting_model() -> Template {
    let mut t = checkout();
    match &mut t.steps[0].kind {
        StepKind::Agent { options, .. } => options.insert("model".into(), "strong".into()),
        other @ (StepKind::Review { .. }
        | StepKind::Command { .. }
        | StepKind::Approval { .. }
        | StepKind::Push
        | StepKind::PullRequest
        | StepKind::StatusChecks { .. }) => {
            panic!("the first step of checkout() is an agent step: {other:?}")
        }
    };
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
    assert!(!rig.submitted());
    rig.chat.apply(AcpEvent::PickConfirmed {
        group: "model".into(),
        value: "strong".into(),
    });
    let fx = rig.on(Input::PicksAnswered);
    assert!(rig.submitted(), "{fx:?}");
    assert!(rig.notes().iter().any(|n| n.starts_with("Settings: ")));
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
        failed(&fx).is_some_and(|why| why.contains("strong")),
        "{fx:?}"
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
    assert!(failed(&fx).is_some_and(|why| why.contains("mode `plan`")));
}

#[test]
fn a_persons_own_pick_refused_is_not_the_runs() {
    let mut rig = at_approval();
    let fx = rig.on(Input::PickRefused("no".into()));
    assert!(fx.is_empty() && rig.asked.is_empty(), "{fx:?}");
}

#[test]
fn a_prompt_not_taken_fails_the_run() {
    let mut rig = at_approval();
    let fx = rig.on(Input::NotTaken);
    assert!(failed(&fx).is_some());
}

// ── The session's events ─────────────────────────────────────────────────

/// A checkout run with its plan prompt sent, waiting on the turn.
fn in_turn() -> Rig {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.submitted());
    rig
}

#[test]
fn a_step_is_judged_once_the_work_its_turn_left_is_done() {
    use crate::acp::{BackgroundReport, Usage};
    let read = |fx: &[Effect]| names(fx).contains(&"ReadFacts");
    let mut rig = in_turn();
    rig.chat
        .apply(AcpEvent::Background(BackgroundReport::Spawned {
            id: "t1".into(),
            name: "cargo test".into(),
            kind: "shell".into(),
        }));
    let fx = rig.turn_ends("Tests run in the background.");
    assert!(!read(&fx), "the build still runs: {fx:?}");
    assert!(!rig.drive.heeds_notified(&rig.chat), "nothing to learn yet");
    assert!(!read(&rig.on(Input::Notified)));

    rig.chat
        .apply(AcpEvent::Background(BackgroundReport::Ended {
            id: "t1".into(),
            end: crate::acp::BackgroundEnd::Completed,
        }));
    assert!(
        !read(&rig.on(Input::Notified)),
        "its answer is still to come"
    );
    rig.chat.apply(AcpEvent::AgentChunk {
        text: "The tests pass.".into(),
        message: None,
    });
    assert!(!read(&rig.on(Input::Notified)));
    rig.chat.apply(AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: None,
        cycle_end: true,
        marks_cycles: true,
    }));
    assert!(rig.drive.heeds_notified(&rig.chat));
    let fx = rig.on(Input::Notified);
    assert!(read(&fx), "{fx:?}");
    assert!(!rig.drive.heeds_notified(&rig.chat));
}

#[test]
fn work_an_earlier_step_left_running_does_not_hold_this_one() {
    use crate::acp::BackgroundReport;
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.chat.tell_time(crate::Now {
        secs: crate::chat::now_secs(),
        at: Instant::now(),
    });
    rig.chat
        .apply(AcpEvent::Background(BackgroundReport::Spawned {
            id: "server".into(),
            name: "npm run dev".into(),
            kind: "shell".into(),
        }));
    // The step's prompt goes after the server started.
    std::thread::sleep(std::time::Duration::from_millis(2));
    rig.now = crate::Now::read();
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.submitted());
    let fx = rig.turn_ends("The plan.");
    assert!(names(&fx).contains(&"ReadFacts"), "{fx:?}");
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
        failed(&fx).is_some_and(|why| why.contains("out of credit")),
        "{fx:?}"
    );
}

#[test]
fn a_card_pauses_the_budget() {
    let mut rig = in_turn();
    rig.on(Input::AwaitingUser);
    assert!(
        rig.drive.card() && rig.drive.waits_on_person(true) && !rig.drive.waits_on_person(false)
    );
    rig.now += rig.drive.run().template.timeout_limit() * 2;
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
    assert_eq!(finished(&fx), None, "a person is waited on");
}

#[test]
fn an_answered_card_resumes() {
    let mut rig = in_turn();
    rig.on(Input::AwaitingUser);
    assert!(rig.on(Input::Notified).is_empty());
    assert!(!rig.drive.card());
    assert!(rig.on(Input::Notified).is_empty() && rig.asked.is_empty());
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
    let fx = rig.drive.handle(Input::SessionGone, None, rig.now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::Closed)));
}

#[test]
fn a_turn_read_judges_the_answer_since_the_prompt() {
    let mut rig = in_turn();
    rig.turn_ends("The plan.");
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    assert!(
        rig.drive.run().awaiting_approval() || names(&fx).contains(&"Pin"),
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
    rig.chat.apply(AcpEvent::AgentChunk {
        text: "thinking".into(),
        message: None,
    });
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
    waiting.chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
    });
    assert!(
        waiting.drive.heeds_appended(&waiting.chat),
        "the held prompt can go"
    );
}

// ── A drive without a session: a check ───────────────────────────────────

fn check_drive() -> (RunDrive, Vec<Effect>) {
    let task = Task::check("t1".into(), "make check".into(), setup(None));
    let mut run = task.runs[0].clone();
    let first = run.resume(0);
    RunDrive::start("t1".into(), run, first, 0, crate::Now::read(), None)
}

#[test]
fn a_check_runs_its_command_and_finishes_without_a_session() {
    let (mut drive, fx) = check_drive();
    let now = crate::Now::read();
    let fx = match names(&fx).contains(&"Pin") {
        true => drive.handle(Input::Pinned(Ok("c0".into())), None, now),
        false => fx,
    };
    assert!(names(&fx).contains(&"RunCommand"), "{fx:?}");
    let fx = drive.handle(Input::CommandExited(ran(true)), None, now);
    let fx = match names(&fx).contains(&"Pin") {
        true => drive.handle(Input::Pinned(Ok("c1".into())), None, now),
        false => fx,
    };
    assert_eq!(finished(&fx), Some(&Outcome::Done), "{fx:?}");
}

#[test]
fn a_check_has_no_timeout() {
    let (_, fx) = check_drive();
    assert!(!names(&fx).contains(&"Arm(Timeout)"), "{fx:?}");
}

#[test]
fn a_check_stopped_before_its_command_runs_none() {
    let (mut drive, fx) = check_drive();
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    let now = crate::Now::read();
    assert!(drive.stop(None, now).is_empty());
    let fx = drive.handle(Input::Pinned(Ok("c0".into())), None, now);
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(!names(&fx).contains(&"RunCommand"));
}

#[test]
fn a_failed_write_is_said_once_until_one_lands() {
    let mut rig = at_approval();
    let failed = |fx: &[Effect]| {
        names(fx)
            .iter()
            .filter(|n| **n == "Signal(SaveFailed)")
            .count()
    };
    assert_eq!(failed(&rig.on(Input::Saved(Err("disk full".into())))), 1);
    assert_eq!(failed(&rig.on(Input::Saved(Err("disk full".into())))), 0);
    assert!(rig.on(Input::Saved(Ok(()))).is_empty() && rig.asked.is_empty());
    assert_eq!(failed(&rig.on(Input::Saved(Err("disk full".into())))), 1);
}

/// A mark that could not be pinned is told, not logged by the drive, and
/// the run goes on to the action it held back.
#[test]
fn a_mark_not_pinned_is_told_and_the_run_goes_on() {
    let (mut rig, fx) = Rig::start(checkout(), None, connected());
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    let fx = rig.on(Input::Pinned(Err("no git".into())));
    assert!(
        fx.contains(&Effect::Signal(Signal::PinFailed("no git".into()))),
        "{fx:?}"
    );
    assert!(names(&fx).contains(&"Measure"), "{fx:?}");
}

/// A run on a branch with a forge: a check that passes on `a`, then the
/// push, the pull request and its status checks.
fn forging() -> Rig {
    let mut template = Template::blank("Forge");
    let step = |id: &str, kind| StepSpec {
        limit: None,
        id: id.into(),
        label: id.into(),
        kind,
    };
    template.steps = vec![
        step(
            "verify",
            StepKind::Command {
                command: Some("make check".into()),
                on_fail: String::new(),
            },
        ),
        step("push", StepKind::Push),
        step("pr", StepKind::PullRequest),
        step(
            "checks",
            StepKind::StatusChecks {
                on_fail: String::new(),
                wait: "1h".into(),
            },
        ),
    ];
    let mut setup = setup(None);
    setup.branch = Some("workflow/fix".into());
    setup.forge = Some("github".into());
    let task = Task::new("t1".into(), template, brief(), setup);
    let mut run = task.runs[0].clone();
    let first = run.resume(0);
    let (mut rig, fx) = Rig::start_run(run, first, connected());
    let mut fx = fx;
    // Pinned, measured and checked, until the push.
    for _ in 0..8 {
        if names(&fx).contains(&"Push") {
            return rig;
        }
        let next = fx.iter().find_map(|e| match e {
            Effect::Pin { .. } => Some(Input::Pinned(Ok("c".into()))),
            Effect::Measure { .. } => Some(Input::Measured(Ok(mark("a", "d1")))),
            Effect::RunCommand { .. } => Some(Input::CommandExited(ran(true))),
            Effect::ReadFacts { .. }
            | Effect::CallOffCommand
            | Effect::Push { .. }
            | Effect::FindPullRequest
            | Effect::OpenPullRequest
            | Effect::ReadLogs { .. }
            | Effect::MarkReady { .. }
            | Effect::Arm { .. }
            | Effect::Defer(_)
            | Effect::Save
            | Effect::Refresh
            | Effect::SessionChanged
            | Effect::Signal(_)
            | Effect::Finish { .. }
            | Effect::OpenSession { .. }
            | Effect::AskRoom { .. }
            | Effect::LeaveRoom => None,
        });
        fx = rig.on(next.expect("the run moves on to its push"));
    }
    panic!("the run never pushed: {fx:?}")
}

fn pr(draft: bool, checks: &[CheckState]) -> PullRequest {
    PullRequest {
        url: "https://forge/pr/7".into(),
        number: 7,
        state: PrState::Open,
        draft,
        head: "a".into(),
        conflicting: false,
        checks: checks
            .iter()
            .enumerate()
            .map(|(i, state)| Check {
                name: format!("job{i}"),
                state: *state,
                link: None,
            })
            .collect(),
    }
}

/// From the push to waiting on the status checks, with no pull request
/// open on the branch yet.
fn watching() -> Rig {
    let mut rig = forging();
    let fx = rig.on(Input::Pushed(Ok(())));
    let fx = match names(&fx).contains(&"Pin") {
        true => rig.on(Input::Pinned(Ok("c".into()))),
        false => fx,
    };
    assert!(has(&fx, |e| *e == Effect::FindPullRequest), "{fx:?}");
    let fx = rig.on(Input::PullRequestFound(Ok(None)));
    assert!(has(&fx, |e| *e == Effect::OpenPullRequest), "{fx:?}");
    let fx = rig.on(Input::PullRequestOpened(Ok(Ok(Some(pr(true, &[]))))));
    let fx = match names(&fx).contains(&"Pin") {
        true => rig.on(Input::Pinned(Ok("c".into()))),
        false => fx,
    };
    assert!(
        has(&fx, |e| *e
            == Effect::Arm {
                timer: Timer::Checks,
                after: Duration::from_secs(60)
            }),
        "{fx:?}"
    );
    assert_eq!(
        rig.drive.run().pull_request.as_ref().map(|p| p.number),
        Some(7)
    );
    rig
}

#[test]
fn a_pull_request_open_on_the_branch_is_taken_up_not_opened_again() {
    let mut rig = forging();
    rig.on(Input::Pushed(Ok(())));
    let fx = rig.on(Input::Pinned(Ok("c".into())));
    assert!(has(&fx, |e| *e == Effect::FindPullRequest), "{fx:?}");
    let fx = rig.on(Input::PullRequestFound(Ok(Some(pr(false, &[])))));
    assert!(!has(&fx, |e| *e == Effect::OpenPullRequest), "{fx:?}");
    assert_eq!(
        rig.drive.run().pull_request.as_ref().map(|p| p.number),
        Some(7)
    );
}

/// A Stop while the branch is read for a pull request waits for the read,
/// and then opens nothing.
#[test]
fn a_stop_while_the_pull_request_is_looked_for_opens_none() {
    let mut rig = forging();
    rig.on(Input::Pushed(Ok(())));
    rig.on(Input::Pinned(Ok("c".into())));
    let fx = rig.stop();
    assert!(finished(&fx).is_none(), "waits for the read: {fx:?}");
    let fx = rig.on(Input::PullRequestFound(Ok(None)));
    assert!(!has(&fx, |e| *e == Effect::OpenPullRequest), "{fx:?}");
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

/// An answer the run did not ask for now (a log read, a draft taken out of
/// draft) lands on nothing: the run still waits on its status checks, and
/// looks again on the clock.
#[test]
fn a_forge_answer_not_asked_for_leaves_the_checks_waited_on() {
    let mut rig = watching();
    assert!(rig.on(Input::LogsRead(Vec::new())).is_empty() && rig.asked.is_empty());
    assert!(rig.on(Input::MarkedReady(Ok(()))).is_empty() && rig.asked.is_empty());
    let fx = rig.on(Input::Elapsed(Timer::Checks));
    assert_eq!(fx, vec![Effect::FindPullRequest]);
}

#[test]
fn checks_still_running_are_looked_at_again_on_the_clock() {
    let mut rig = watching();
    let fx = rig.on(Input::Elapsed(Timer::Checks));
    assert_eq!(fx, vec![Effect::FindPullRequest]);
    let fx = rig.on(Input::PullRequestFound(Ok(Some(pr(
        true,
        &[CheckState::Pending],
    )))));
    assert_eq!(
        fx,
        vec![Effect::Arm {
            timer: Timer::Checks,
            after: Duration::from_secs(60)
        }]
    );
}

/// All passing takes the draft out of draft, and a Stop meanwhile waits for
/// it to land.
#[test]
fn passing_checks_take_the_draft_out_of_draft_and_a_stop_waits_for_it() {
    let mut rig = watching();
    rig.on(Input::Elapsed(Timer::Checks));
    let fx = rig.on(Input::PullRequestFound(Ok(Some(pr(
        true,
        &[CheckState::Passed],
    )))));
    assert_eq!(fx, vec![Effect::MarkReady { number: 7 }]);
    let fx = rig.stop();
    assert!(finished(&fx).is_none(), "waits for the draft: {fx:?}");
    let fx = rig.on(Input::MarkedReady(Ok(())));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
}

#[test]
fn a_draft_that_cannot_leave_draft_is_waited_on() {
    let mut rig = watching();
    rig.on(Input::Elapsed(Timer::Checks));
    rig.on(Input::PullRequestFound(Ok(Some(pr(
        true,
        &[CheckState::Passed],
    )))));
    let fx = rig.on(Input::MarkedReady(Err("forbidden".into())));
    assert!(names(&fx).contains(&"Arm(Checks)"), "{fx:?}");
}

/// Failing checks hand at most three logs to the repair, a log that could
/// not be read saying why.
#[test]
fn failing_checks_carry_at_most_three_logs_to_the_repair() {
    let mut rig = watching();
    rig.on(Input::Elapsed(Timer::Checks));
    let failed = [CheckState::Failed; 4];
    let fx = rig.on(Input::PullRequestFound(Ok(Some(pr(false, &failed)))));
    let checks = fx.iter().find_map(|e| match e {
        Effect::ReadLogs { checks } => Some(checks.len()),
        Effect::Pin { .. }
        | Effect::Measure { .. }
        | Effect::ReadFacts { .. }
        | Effect::RunCommand { .. }
        | Effect::CallOffCommand
        | Effect::Push { .. }
        | Effect::FindPullRequest
        | Effect::OpenPullRequest
        | Effect::MarkReady { .. }
        | Effect::Arm { .. }
        | Effect::Defer(_)
        | Effect::Save
        | Effect::Refresh
        | Effect::SessionChanged
        | Effect::Signal(_)
        | Effect::Finish { .. }
        | Effect::OpenSession { .. }
        | Effect::AskRoom { .. }
        | Effect::LeaveRoom => None,
    });
    assert_eq!(checks, Some(3), "{fx:?}");
    let logs = vec![
        ("job0".to_string(), Ok("boom".to_string())),
        ("job1".to_string(), Err("gone".to_string())),
    ];
    rig.on(Input::LogsRead(logs));
    let kept = rig
        .drive
        .run()
        .visits()
        .last()
        .and_then(|v| v.output.clone());
    let kept = kept.unwrap_or_default();
    assert!(
        kept.contains("boom") && kept.contains("(no log: gone)"),
        "{kept}"
    );
}

/// A look that lands after the run stopped finds nothing to do.
#[test]
fn a_look_after_the_run_stopped_is_dropped() {
    let mut rig = watching();
    rig.on(Input::Elapsed(Timer::Checks));
    let fx = rig.stop();
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::ByPerson)));
    assert!(rig
        .on(Input::PullRequestFound(Ok(Some(pr(
            true,
            &[CheckState::Passed]
        )))))
        .is_empty());
}

/// An agent step of `id`, on `agent` when it names one.
fn agent_step(id: &str, label: &str, agent: Option<&str>) -> StepSpec {
    StepSpec {
        limit: None,
        id: id.into(),
        label: label.into(),
        kind: StepKind::Agent {
            prompt: format!("{label} it."),
            gates: Vec::new(),
            keep_answer: true,
            mode: None,
            options: Default::default(),
            agent: agent.map(str::to_string),
        },
    }
}

fn verify_step(on_fail: &str) -> StepSpec {
    StepSpec {
        limit: None,
        id: "verify".into(),
        label: "Verify".into(),
        kind: StepKind::Command {
            command: Some("make check".into()),
            on_fail: on_fail.into(),
        },
    }
}

/// Implement on the run's agent, verify, then review on Claude, and an
/// approval that can send the work back to implement.
fn reviewed() -> Template {
    let mut template = Template::blank("Reviewed");
    template.steps = vec![
        agent_step("implement", "Implement", None),
        verify_step("implement"),
        agent_step("review", "Review", Some("Claude")),
        StepSpec {
            limit: None,
            id: "approve".into(),
            label: "Approve".into(),
            kind: StepKind::Approval {
                of: "implement".into(),
            },
        },
    ];
    template
}

fn review() -> Lane {
    Lane::Step("review".into())
}

impl Rig {
    /// Answer what the run asks, as the app would, until `until` holds of
    /// what the last call answered, the run waits on a person, or it is
    /// over: what that call answered. A lane's session is not opened here.
    fn until(
        &mut self,
        mut fx: Vec<Effect>,
        until: impl Fn(&Self, &[Effect]) -> bool,
    ) -> Vec<Effect> {
        for _ in 0..40 {
            // An approval waits on its marks too.
            let approval = self.drive.run().awaiting_approval() && !names(&fx).contains(&"Pin");
            if until(self, &fx) || finished(&fx).is_some() || approval {
                return fx;
            }
            if self.submitted() {
                fx = self.turn_ends("Done.");
                continue;
            }
            // The work changed when the step wants it changed.
            let changes = matches!(
                self.drive.run().current().map(|step| &step.kind),
                Some(StepKind::Agent { gates, .. }) if gates.contains(&crate::workflow::GateKind::CodeChanged)
            );
            let digest = if changes { "d2" } else { "d1" };
            let next = fx.iter().find_map(|e| match e {
                Effect::Pin { .. } => Some(Input::Pinned(Ok("c".into()))),
                Effect::Measure { .. } => Some(Input::Measured(Ok(mark("a", "d1")))),
                Effect::ReadFacts { .. } => Some(Input::TurnRead(Ok(facts("a", digest)))),
                Effect::RunCommand { .. } => Some(Input::CommandExited(ran(true))),
                Effect::Defer(input) => Some(input.clone()),
                Effect::CallOffCommand
                | Effect::Push { .. }
                | Effect::FindPullRequest
                | Effect::OpenPullRequest
                | Effect::ReadLogs { .. }
                | Effect::MarkReady { .. }
                | Effect::Arm { .. }
                | Effect::Save
                | Effect::Refresh
                | Effect::SessionChanged
                | Effect::Signal(_)
                | Effect::Finish { .. }
                | Effect::OpenSession { .. }
                | Effect::AskRoom { .. }
                | Effect::LeaveRoom => None,
            });
            match next {
                Some(input) => fx = self.on(input),
                None => return fx,
            }
        }
        panic!("the run never got there: {fx:?}")
    }

    /// Open `lane`'s session, connected, and tell the run.
    fn open(&mut self, lane: Lane) -> Vec<Effect> {
        let chat = connected();
        match &lane {
            Lane::Run => {
                self.chat = chat.0;
                self._rx = chat.1;
            }
            Lane::Step(_) => {
                self.others.insert(lane.clone(), chat);
            }
        }
        self.on(Input::LaneOpened {
            lane,
            result: Ok(()),
        })
    }
}

fn asks_for_session(fx: &[Effect]) -> bool {
    has(fx, |e| matches!(e, Effect::OpenSession { .. }))
}

/// A reviewed run, its implement step passed and verified, at the review's
/// prompt: what that call answered.
fn at_review() -> (Rig, Vec<Effect>) {
    let (mut rig, fx) = Rig::start(reviewed(), None, connected());
    let fx = rig.until(fx, |_, fx| asks_for_session(fx));
    (rig, fx)
}

#[test]
fn a_step_naming_an_agent_asks_for_its_session() {
    let (rig, fx) = at_review();
    assert!(
        fx.contains(&Effect::OpenSession {
            lane: review(),
            agent: "Claude".into(),
            name: "Fix the thing · Review".into(),
        }),
        "{fx:?}"
    );
    assert_eq!(rig.drive.current_lane(), &review());
    assert!(
        !rig.submitted(),
        "nothing is sent before the session is open"
    );
    // Only implement's prompt reached the run's session.
    assert!(!rig.chat.prompted_beyond(1));
}

#[test]
fn its_prompt_goes_once_its_session_is_opened() {
    let (mut rig, _) = at_review();
    rig.open(review());
    assert!(rig.submitted());
    assert!(rig.chat_of(&review()).unwrap().prompted_beyond(0));
    assert!(!rig.chat.prompted_beyond(1), "not to the run's session");
    let named = rig.chat_of(&review()).unwrap().custom_title.as_deref();
    assert_eq!(named, Some("Fix the thing · Review"));
    let lanes: Vec<&Lane> = rig.drive.lanes().collect();
    assert_eq!(lanes, [&review(), &Lane::Run]);
}

#[test]
fn a_command_step_keeps_the_lane() {
    let mut template = Template::blank("Reviewed first");
    template.steps = vec![
        agent_step("implement", "Implement", None),
        agent_step("review", "Review", Some("Claude")),
        verify_step("implement"),
    ];
    let (mut rig, fx) = Rig::start(template, None, connected());
    rig.until(fx, |_, fx| asks_for_session(fx));
    let fx = rig.open(review());
    let fx = rig.until(fx, |_, fx| names(fx).contains(&"RunCommand"));
    assert!(names(&fx).contains(&"RunCommand"), "{fx:?}");
    assert_eq!(rig.drive.current_lane(), &review());
    assert!(
        rig.notes().contains(&"Running: make check"),
        "{:?}",
        rig.asked
    );
}

#[test]
fn going_back_reuses_the_lanes_session() {
    let (mut rig, _) = at_review();
    let fx = rig.open(review());
    rig.until(fx, |_, _| false);
    let at = approval_at(&rig);
    let fx = rig.call(|drive, chat, now| drive.revise(at, "Again.".into(), chat, now));
    let fx = rig.until(fx, |rig, _| rig.chat.prompted_beyond(1));
    assert!(
        rig.chat.prompted_beyond(1),
        "implement prompted the run's session again: {fx:?}"
    );
    assert_eq!(rig.drive.current_lane(), &Lane::Run);
    // Only the review's lane was ever opened.
    let opened = rig
        .seen
        .iter()
        .filter(|e| matches!(e, Effect::OpenSession { .. }));
    assert_eq!(opened.count(), 1);
}

#[test]
fn a_person_prompting_another_lane_takes_the_run_over() {
    let (mut rig, _) = at_review();
    rig.open(review());
    assert!(rig.chat.submit("Mine now", &[]));
    assert!(rig.drive.heeds_lane_appended(&Lane::Run, &rig.chat));
    let fx = rig.drive.handle(
        Input::LaneAppended { lane: Lane::Run },
        Some(&rig.chat),
        rig.now,
    );
    assert_eq!(
        finished(&fx),
        Some(&Outcome::Stopped(Stop::TakenOver)),
        "{fx:?}"
    );
}

#[test]
fn a_lost_lane_elsewhere_reopens_with_a_note() {
    let (mut rig, _) = at_review();
    let fx = rig.open(review());
    rig.until(fx, |_, _| false);
    let gone = rig.on(Input::LaneGone { lane: Lane::Run });
    assert!(finished(&gone).is_none(), "{gone:?}");
    assert_eq!(rig.drive.lanes().collect::<Vec<_>>(), [&review()]);
    let at = approval_at(&rig);
    let fx = rig.call(|drive, chat, now| drive.revise(at, "Again.".into(), chat, now));
    let fx = rig.until(fx, |_, fx| asks_for_session(fx));
    assert!(
        fx.contains(&Effect::OpenSession {
            lane: Lane::Run,
            agent: String::new(),
            name: "Fix the thing".into(),
        }),
        "{fx:?}"
    );
    rig.open(Lane::Run);
    assert!(
        rig.notes()
            .contains(&"The session of Implement was gone; a new one was opened."),
        "{:?}",
        rig.asked
    );
    assert!(rig.submitted());
}

#[test]
fn a_lane_that_will_not_open_fails_the_run_for_a_retry() {
    let (mut rig, _) = at_review();
    let fx = rig.on(Input::LaneOpened {
        lane: review(),
        result: Err("no such command".into()),
    });
    assert_eq!(
        failed(&fx),
        Some("The agent of Review, Claude, did not start."),
        "{fx:?}"
    );
    assert_eq!(
        rig.drive.run().failed_on(),
        Some(crate::workflow::Failure::Configuration)
    );
}

#[test]
fn a_run_whose_steps_name_no_agent_never_opens_a_session() {
    let (mut rig, fx) = Rig::start(checkout(), None, connected());
    rig.until(fx, |_, _| false);
    let at = approval_at(&rig);
    let fx = rig.approve(at);
    let fx = rig.until(fx, |_, _| false);
    assert_eq!(
        finished(&fx),
        Some(&Outcome::Done),
        "{:?}",
        names(&rig.seen)
    );
    assert!(!asks_for_session(&rig.seen), "{:?}", rig.seen);
    assert_eq!(rig.drive.lanes().collect::<Vec<_>>(), [&Lane::Run]);
}

#[test]
fn a_step_lane_runs_in_its_agents_mode_not_the_runs() {
    let mut template = Template::blank("Review first");
    template.steps = vec![agent_step("review", "Review", Some("Claude"))];
    let (mut rig, fx) = Rig::start(template, Some("plan"), with_modes(&["default", "plan"]));
    assert_eq!(rig.drive.current_lane(), &review());
    assert!(
        !asks_for_session(&fx),
        "the first session is the step's: {fx:?}"
    );
    rig.on(Input::Pinned(Ok("c0".into())));
    let fx = rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.submitted(), "{fx:?}");
    assert!(!rig.notes().contains(&"Mode plan set"), "{:?}", rig.asked);
    let chat = rig.chat_of(&review()).unwrap();
    assert!(!chat.pick_waiting(crate::chat::MODE_SELECTOR));
}

#[test]
fn a_run_lane_naming_no_agent_that_will_not_open_says_so_plainly() {
    let (mut rig, _) = at_review();
    let fx = rig.open(review());
    rig.until(fx, |_, _| false);
    rig.on(Input::LaneGone { lane: Lane::Run });
    let at = approval_at(&rig);
    let fx = rig.call(|drive, chat, now| drive.revise(at, "Again.".into(), chat, now));
    rig.until(fx, |_, fx| asks_for_session(fx));
    let fx = rig.on(Input::LaneOpened {
        lane: Lane::Run,
        result: Err("no agent".into()),
    });
    assert_eq!(
        failed(&fx),
        Some("The agent of Implement did not start."),
        "{fx:?}"
    );
}

#[test]
fn a_lane_entered_is_told_its_step() {
    let (mut rig, _) = at_review();
    rig.open(review());
    assert!(
        rig.notes().contains(&"Step 3 of 4: Review"),
        "{:?}",
        rig.asked
    );
    let review_said: Vec<&Lane> = (rig.said.iter())
        .filter(|(_, note)| note == "Step 3 of 4: Review")
        .map(|(lane, _)| lane)
        .collect();
    assert_eq!(review_said, [&review()], "said once, in its own lane");
    // A step in the run's own lane is said there as ever.
    assert!(rig
        .said
        .contains(&(Lane::Run, "Step 2 of 4: Verify".to_string())));
}

#[test]
fn a_person_prompting_the_current_lane_takes_the_run_over() {
    let (mut rig, _) = at_review();
    let fx = rig.open(review());
    rig.until(fx, |_, _| false);
    let chat = rig.chat_of_mut(&review()).unwrap();
    assert!(chat.submit("Mine now", &[]));
    let fx = rig.on(Input::LaneAppended { lane: review() });
    assert_eq!(
        finished(&fx),
        Some(&Outcome::Stopped(Stop::TakenOver)),
        "{fx:?}"
    );
}

#[test]
fn an_input_between_a_switch_and_its_lane_opening_sends_the_prompt_once() {
    let (mut rig, _) = at_review();
    let fx = rig.open(review());
    rig.until(fx, |_, _| false);
    let at = approval_at(&rig);
    let fx = rig.call(|drive, chat, now| drive.revise(at, "Again.".into(), chat, now));
    let fx = rig.until(fx, |_, fx| {
        has(fx, |e| {
            matches!(
                e,
                Effect::Defer(Input::LaneOpened {
                    lane: Lane::Run,
                    ..
                })
            )
        })
    });
    assert_eq!(rig.drive.current_lane(), &Lane::Run);
    assert!(
        !rig.chat.prompted_beyond(1),
        "held until the lane is told of"
    );
    // Something lands in the run's session before the deferred call: the
    // prompt may go then, in that session, but only once.
    rig.on(Input::Appended);
    rig.on(Input::Notified);
    assert!(!rig.chat.prompted_beyond(2), "{fx:?}");
    rig.on(Input::LaneOpened {
        lane: Lane::Run,
        result: Ok(()),
    });
    assert!(rig.chat.prompted_beyond(1));
    assert!(!rig.chat.prompted_beyond(2), "sent once");
}

/// Implement on the run's agent, a review of it, then verify.
fn with_a_review() -> Template {
    let mut template = Template::blank("Reviewed by an agent");
    template.steps = vec![
        agent_step("implement", "Implement", None),
        StepSpec {
            limit: None,
            id: "review".into(),
            label: "Review".into(),
            kind: StepKind::Review {
                of: "implement".into(),
                on_reject: None,
                agent: None,
                prompt: "Review it.".into(),
                mode: None,
                options: Default::default(),
            },
        },
        verify_step("implement"),
    ];
    template
}

/// A run of [`with_a_review`] in its review's turn, the prompt sent in the
/// review's own session.
fn in_review() -> Rig {
    let (mut rig, fx) = Rig::start(with_a_review(), None, connected());
    rig.until(fx, |_, fx| asks_for_session(fx));
    rig.open(review());
    assert!(rig.submitted());
    assert!(rig.drive.run().awaiting_turn());
    rig
}

fn verdict(
    rig: &mut Rig,
    lane: &Lane,
    verdict: crate::workflow::Verdict,
) -> Result<String, String> {
    rig.drive.verdict(lane, verdict)
}

#[test]
fn the_last_verdict_of_the_turn_counts() {
    use crate::workflow::Verdict;
    let mut rig = in_review();
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::SendBack("More tests.".into())),
        Ok("Sent back to Implement. The review ends when this turn does.".into())
    );
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::Approve),
        Ok("Approved. The review ends when this turn does.".into())
    );
    let fx = rig.turn_ends("Fine.");
    let fx = rig.until(fx, |_, fx| names(fx).contains(&"RunCommand"));
    assert!(names(&fx).contains(&"RunCommand"), "{fx:?}");
    assert_eq!(rig.drive.run().misses, 0);
    assert_eq!(rig.drive.run().outputs["review"], "Fine.");
}

#[test]
fn a_send_back_without_a_note_is_refused() {
    use crate::workflow::Verdict;
    let mut rig = in_review();
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::SendBack(" ".into())),
        Err("A send back needs a note saying what to change.".into())
    );
}

#[test]
fn a_verdict_from_another_lane_is_refused() {
    use crate::workflow::Verdict;
    let mut rig = in_review();
    assert_eq!(
        verdict(&mut rig, &Lane::Run, Verdict::Approve),
        Err("No review is waiting on this session.".into())
    );
    // Nothing was held: the turn ends without a verdict and is asked again.
    let fx = rig.turn_ends("Fine.");
    rig.until(fx, |rig, _| rig.drive.run().misses > 0);
    assert_eq!(rig.drive.run().misses, 1);
    assert_eq!(rig.drive.run().current().unwrap().id, "review");
}

#[test]
fn a_verdict_after_the_turn_is_refused() {
    use crate::workflow::Verdict;
    let mut rig = in_review();
    // The turn has ended and its work is being read.
    let fx = rig.turn_ends("Fine.");
    assert!(names(&fx).contains(&"ReadFacts"), "{fx:?}");
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::Approve),
        Err("No review is waiting on this session.".into())
    );
    // And once the run has gone on to verify.
    let mut rig = in_review();
    verdict(&mut rig, &review(), Verdict::Approve).unwrap();
    let fx = rig.turn_ends("Fine.");
    rig.until(fx, |_, fx| names(fx).contains(&"RunCommand"));
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::Approve),
        Err("No review is waiting on this session.".into())
    );
}

#[test]
fn a_review_runs_the_budget() {
    let mut rig = in_review();
    assert!(!rig.drive.waits_on_person(true) && !rig.drive.waits_on_person(false));
    rig.now += rig.drive.run().template.timeout_limit() + Duration::from_secs(1);
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
    assert_eq!(finished(&fx), Some(&Outcome::Stopped(Stop::TimedOut)));
}

#[test]
fn a_second_review_visit_starts_without_a_verdict() {
    use crate::workflow::Verdict;
    let mut template = with_a_review();
    template.misses = 3;
    let (mut rig, fx) = Rig::start(template, None, connected());
    rig.until(fx, |_, fx| asks_for_session(fx));
    rig.open(review());
    verdict(&mut rig, &review(), Verdict::SendBack("More tests.".into())).unwrap();
    let fx = rig.turn_ends("Not yet.");
    // Implement again, then the review's second visit.
    let fx = rig.until(fx, |rig, _| {
        rig.drive.run().current().unwrap().id == "implement"
    });
    rig.until(fx, |rig, _| {
        let run = rig.drive.run();
        run.current().unwrap().id == "review" && run.awaiting_turn()
    });
    assert_eq!(rig.drive.run().misses, 1);
    // Its turn gives no verdict: the one of the first visit does not count.
    let fx = rig.turn_ends("Fine.");
    rig.until(fx, |rig, _| rig.drive.run().misses > 1);
    assert_eq!(rig.drive.run().misses, 2);
    assert_eq!(rig.drive.run().current().unwrap().id, "review");
}

#[test]
fn a_send_back_with_nowhere_to_go_is_refused() {
    use crate::workflow::Verdict;
    let mut template = with_a_review();
    if let StepKind::Review { on_reject, .. } = &mut template.steps[1].kind {
        *on_reject = Some("gone".into());
    }
    let (mut rig, fx) = Rig::start(template, None, connected());
    rig.until(fx, |_, fx| asks_for_session(fx));
    rig.open(review());
    assert_eq!(
        verdict(&mut rig, &review(), Verdict::SendBack("More tests.".into())),
        Err("This review has no step to send the work back to.".into())
    );
}

#[test]
fn a_review_lane_is_granted_its_verdict() {
    use crate::task::Grant;
    let rig = in_review();
    let run = rig.drive.run();
    let given = Grant {
        tools: vec!["list_issues".into()],
        labels: vec!["auto".into()],
    };
    let review_grant = review().grant(run, &given);
    assert_eq!(review_grant.tools, ["list_issues", "review_verdict"]);
    assert_eq!(review_grant.labels, ["auto"]);
    assert_eq!(
        review().grant(run, &Grant::default()).tools,
        ["review_verdict"]
    );
    assert_eq!(Lane::Run.grant(run, &given), given);
}

/// The checkout template with its plan limited to one run at a time.
fn limited_plan() -> Template {
    let mut template = checkout();
    template.steps[0].limit = Some(1);
    template
}

/// A run of [`limited_plan`] given room at its plan, prompted.
fn in_limited_plan() -> Rig {
    let (mut rig, _) = Rig::start(limited_plan(), None, connected());
    let fx = rig.on(Input::RoomGiven);
    assert!(names(&fx).contains(&"Pin"), "{fx:?}");
    rig.on(Input::Pinned(Ok("c0".into())));
    rig.on(Input::Measured(Ok(mark("a", "d0"))));
    assert!(rig.submitted());
    rig
}

#[test]
fn a_full_step_waits_with_its_note_and_pauses_the_budget() {
    let (mut rig, fx) = Rig::start(limited_plan(), None, connected());
    assert!(
        fx.contains(&Effect::AskRoom {
            key: crate::task::rooms::RoomKey {
                repo: PathBuf::from("/repo"),
                workflow: "builtin:checkout".into(),
                step: "plan".into(),
            },
            limit: 1,
        }),
        "{fx:?}"
    );
    assert!(!names(&fx).contains(&"Measure"), "{fx:?}");
    assert!(rig.drive.run().awaiting_room());
    assert!(
        rig.said
            .iter()
            .any(|(_, n)| n == "Waiting for room at Plan."),
        "{:?}",
        rig.said
    );
    // Long past its timeout, it has spent none of it waiting.
    rig.now.at += Duration::from_secs(365 * 24 * 3600);
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
    assert_eq!(names(&fx), ["Arm(Timeout)"], "{fx:?}");
}

#[test]
fn room_given_starts_the_step() {
    let (mut rig, _) = Rig::start(limited_plan(), None, connected());
    let fx = rig.on(Input::RoomGiven);
    assert!(
        names(&fx).contains(&"Pin"),
        "the visit's start is marked: {fx:?}"
    );
    let fx = rig.on(Input::Pinned(Ok("c0".into())));
    assert!(names(&fx).contains(&"Measure"), "{fx:?}");
    assert!(!rig.drive.run().awaiting_room());
    // Given twice, it moves nothing.
    assert!(rig.on(Input::RoomGiven).is_empty());
}

#[test]
fn leaving_a_limited_step_says_so() {
    let mut rig = in_limited_plan();
    rig.turn_ends("The plan.");
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    assert_eq!(names(&fx).first(), Some(&"LeaveRoom"), "{fx:?}");
    assert_eq!(names(&fx).iter().filter(|n| **n == "LeaveRoom").count(), 1);
}

#[test]
fn going_back_to_the_step_it_holds_needs_no_room() {
    let mut rig = in_limited_plan();
    // No answer: the plan is asked again, in the same step.
    rig.turn_ends("");
    let fx = rig.on(Input::TurnRead(Ok(facts("a", "d0"))));
    assert!(rig.submitted(), "{fx:?}");
    assert!(
        !names(&fx).contains(&"AskRoom") && !names(&fx).contains(&"LeaveRoom"),
        "{fx:?}"
    );
    assert!(!rig.drive.run().awaiting_room());
}
