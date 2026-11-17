use super::*;
use crate::chat::model::Link;
use crate::control::kept::{KeptClosed, KeptWorker};
use crate::worktree::occupants::Occupants;

fn asked(prompt: &str) -> Open {
    Open {
        lead: 1,
        project: "repo".into(),
        agent: "agent".into(),
        prompt: Some(prompt.into()),
        dirs: Vec::new(),
        request: "r1".into(),
        turn: Some(1),
        model: None,
        name: None,
        back: None,
    }
}

fn projects() -> Vec<(String, PathBuf)> {
    vec![("repo".into(), PathBuf::from("/repo"))]
}

fn resolved() -> Resolved {
    Resolved {
        folder: PathBuf::from("/repo"),
        root: PathBuf::from("/repo"),
        same: vec![PathBuf::from("/repo")],
        dirs: Vec::new(),
        occupants: Occupants::None,
        canonical: vec![(PathBuf::from("/repo"), PathBuf::from("/repo"))],
    }
}

fn world() -> World {
    World {
        lead_here: true,
        counts: vec![(PathBuf::from("/repo"), 0)],
        live: vec![1],
        opened: Vec::new(),
    }
}

fn lead_on() -> ControlDrive {
    let mut drive = lead_kept();
    drive.switch(1, true);
    drive
}

/// A drive whose lead 1 said its conversation `c1`, kept in `/store/c1`:
/// what it keeps is written there.
fn lead_kept() -> ControlDrive {
    let mut drive = ControlDrive::default();
    said_conversation(&mut drive, 1, "c1", 0);
    drive
}

/// The open's key and effects, when it resolves; a panic when it does not.
fn resolves(begun: Result<Begun, String>) -> (Key, Vec<Effect>) {
    match begun {
        Ok(Begun::Resolve { key, fx, .. }) => (key, fx),
        Ok(Begun::Said(said)) => panic!("said: {said}"),
        Err(why) => panic!("refused: {why}"),
    }
}

/// Each effect by its name, with the fields the tests read.
fn shown(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|effect| match effect {
            Effect::ToolsChanged(uid) => format!("ToolsChanged {uid}"),
            Effect::Start { folder, .. } => format!("Start {folder}"),
            Effect::Watch(uid) => format!("Watch {uid}"),
            Effect::Unwatch(uid) => format!("Unwatch {uid}"),
            Effect::Pick { uid, .. } => format!("Pick {uid}"),
            Effect::Begin { uid, prompt } => format!("Begin {uid} {prompt:?}"),
            Effect::Arm { token, wait } => format!("Arm {token} {wait:?}"),
            Effect::Drop { uid, added, .. } => format!("Drop {uid} {added}"),
            Effect::Answer { .. } => "Answer".into(),
            Effect::Changed => "Changed".into(),
            Effect::Send { uid, turn, text } => format!("Send {uid} {turn} {text}"),
            Effect::TurnAnswer { uid, outcome } => format!("TurnAnswer {uid} {outcome:?}"),
            Effect::Keep { lead, kept, .. } => {
                format!("Keep {lead} {} {}", kept.workers.len(), kept.closed.len())
            }
        })
        .collect()
}

fn standing(standing: Standing) -> String {
    match standing {
        Standing::On => "On".into(),
        Standing::Off => "Off".into(),
        Standing::Only(tools) => format!("Only {tools:?}"),
        Standing::Refused(why) => format!("Refused {why}"),
    }
}

/// A drive whose lead 1's open of `r1` started worker 7: its key.
fn up(drive: &mut ControlDrive) -> Key {
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    assert_eq!(shown(&fx), ["Start repo"]);
    let fx = drive.handle(Input::Started {
        key: key.clone(),
        started: Ok(7),
    });
    assert!(shown(&fx).contains(&"Watch 7".to_string()), "{fx:?}");
    key
}

fn connected() -> Chat {
    let mut chat = Chat::default();
    chat.link = Link::Connected;
    chat
}

fn answer(fx: &[Effect]) -> Option<&Result<String, String>> {
    fx.iter().find_map(|effect| match effect {
        Effect::Answer { outcome, .. } => Some(outcome),
        Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Drop { .. }
        | Effect::Changed
        | Effect::Keep { .. }
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => None,
    })
}

fn facts(intake: Option<Intake>) -> Facts {
    Facts {
        intake,
        sent: 1,
        mode: "the agent's own".into(),
        model: "the agent's own".into(),
        name: "repo".into(),
    }
}

/// A worker comes up: its first prompt goes, then the relation is written
/// and the open answered with what it runs with.
#[test]
fn a_worker_up_is_given_its_prompt_then_answered() {
    let mut drive = lead_on();
    up(&mut drive);
    assert_eq!(drive.lead_of(7), Some(1), "a worker from its start");
    assert!(
        drive
            .handle(Input::Woke {
                uid: 7,
                chat: Some(&Chat::default()),
                lead_here: true
            })
            .is_empty(),
        "still connecting"
    );
    let fx = drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    assert_eq!(shown(&fx), [r#"Begin 7 Some("go")"#]);
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    let said = answer(&fx).unwrap().as_ref().unwrap();
    assert!(
        said.contains("name: repo") && said.ends_with("turn: 1 accepted"),
        "{said}"
    );
    assert_eq!(
        drive.workers_of(1),
        [Worker {
            uid: 7,
            turn: Some(1),
            sent: Some(1),
            person: false,
        }]
    );
    // The same request again answers the same, opening nothing.
    let again = match drive.open_worker(asked("go"), &projects(), None) {
        Ok(Begun::Said(again)) => again,
        Ok(Begun::Resolve { key, .. }) => panic!("opened again: {key:?}"),
        Err(why) => panic!("refused: {why}"),
    };
    assert_eq!(&again, said);
}

/// A first prompt the agent does not take fails the open, closing its
/// session and freeing its request, folder and place.
#[test]
fn a_prompt_not_taken_leaves_nothing_behind() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Queued)),
    });
    assert!(shown(&fx).contains(&"Drop 7 false".to_string()), "{fx:?}");
    assert_eq!(
        answer(&fx),
        Some(&Err("the prompt was not accepted".into()))
    );
    assert!(drive.workers_of(1).is_empty());
    resolves(drive.open_worker(asked("go"), &projects(), None));
}

/// A lead switched off or closed abandons its opens at once.
#[test]
fn a_lead_that_goes_abandons_its_open() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.switch(1, false);
    assert_eq!(
        answer(&fx),
        Some(&Err("the lead went before its worker was ready".into()))
    );
    assert!(
        shown(&fx).iter().any(|e| e.starts_with("Drop 7 ")),
        "{fx:?}"
    );

    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.forget(1);
    assert!(answer(&fx).is_some_and(Result::is_err));
    assert!(drive.lead_of(7).is_none());
}

/// A lead whose window closed is found gone at the next wake.
#[test]
fn a_wake_without_its_lead_fails() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: false,
    });
    assert!(answer(&fx).is_some_and(Result::is_err));
}

/// Past its deadline an open fails; a deadline of an open already over
/// does nothing.
#[test]
fn an_open_past_its_deadline_fails_once() {
    let mut drive = lead_on();
    let (key, fx) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let token = match fx[..] {
        [Effect::Arm { token, wait }] => {
            assert_eq!(wait, OPEN_DEADLINE);
            token
        }
        []
        | [_, _, ..]
        | [Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Keep { .. }
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. }] => panic!("the deadline is armed at the call: {fx:?}"),
    };
    drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    drive.handle(Input::Started {
        key: key.clone(),
        started: Ok(7),
    });
    let fx = drive.handle(Input::Elapsed { token });
    assert_eq!(
        answer(&fx),
        Some(&Err("the agent was not ready within 120 seconds".into()))
    );
    assert!(drive.handle(Input::Elapsed { token }).is_empty());
}

/// A session open on the folder, a process in it, or a lead at its cap
/// refuses the open before anything starts.
#[test]
fn a_folder_in_use_or_a_full_cap_refuses() {
    let mut drive = lead_on();
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let busy = World {
        counts: vec![(PathBuf::from("/repo"), 1)],
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: busy,
    });
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("already has a session")));

    // A failed request opens afresh.
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let full = World {
        opened: (2..6).map(|uid| (uid, PathBuf::from("/x"))).collect(),
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: full,
    });
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("4 sessions open")));
    // The folder was given back.
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    assert_eq!(shown(&fx), ["Start repo"]);
}

/// A prompt needs a turn, and a turn a prompt.
#[test]
fn a_prompt_and_its_turn_go_together() {
    let mut drive = lead_on();
    let mut open = asked("go");
    open.turn = None;
    assert!(drive.open_worker(open, &projects(), None).is_err());
}

/// Standing, read on every message.
#[test]
fn standing_follows_the_switches() {
    let mut drive = lead_kept();
    let first = standing(drive.standing(&Peer::Outside));
    assert!(
        first.starts_with("Refused ") && first.contains("Settings"),
        "{first}"
    );
    drive.set_outside(true);
    assert_eq!(standing(drive.standing(&Peer::Outside)), "On");
    let token = drive.mint();
    let peer = Peer::Session(token.clone());
    assert_eq!(standing(drive.standing(&peer)), "Off", "unclaimed");
    drive.claim(&token, 3);
    drive.grant(
        3,
        &Grant {
            tools: vec!["list_issues".into()],
            labels: Vec::new(),
        },
    );
    assert_eq!(standing(drive.standing(&peer)), r#"Only ["list_issues"]"#);
    assert_eq!(shown(&drive.switch(3, true)), ["ToolsChanged 3"]);
    assert_eq!(standing(drive.standing(&peer)), "On");
    drive.release(&token);
    assert!(standing(drive.standing(&peer)).starts_with("Refused "));
}

/// Give back: offered while the person holds a worker whose lead is open and
/// on; given back, the lead holds it.
#[test]
fn a_worker_given_back_is_the_leads_again() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    assert!(!drive.give_back_offered(7, 1, true), "the lead holds it");
    assert!(drive.give_back_offered(7, 2, true));
    assert!(!drive.give_back_offered(7, 2, false), "its lead is gone");
    assert!(drive.give_back(7, 2, true).is_some());
    assert!(!drive.give_back_offered(7, 2, true));
    assert_eq!(drive.last(7).map(|last| last.sent), Some(2));
}

/// A turn the worker's agent did not take is refused and moves nothing on.
#[test]
fn a_turn_not_taken_moves_nothing() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    let fx = turn_sent(&mut drive, 7, 2, Some((Intake::Queued, 2)));
    assert_eq!(
        shown(&fx),
        [r#"TurnAnswer 7 Err("the prompt was not accepted")"#]
    );
    assert_eq!(drive.last(7).map(|last| last.turn), Some(Some(1)));
    let fx = turn_sent(&mut drive, 7, 2, Some((Intake::Sent, 2)));
    assert_eq!(
        shown(&fx).last().map(String::as_str),
        Some(r#"TurnAnswer 7 Ok("turn 2 accepted")"#)
    );
    assert_eq!(drive.last(7).map(|last| last.turn), Some(Some(2)));
}

/// The open answers with a handle minted for the worker, which names its
/// session while it is open.
#[test]
fn the_open_answers_with_a_stable_handle() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    let said = answer(&fx).unwrap().as_ref().unwrap();
    let handle = said
        .lines()
        .find_map(|line| line.strip_prefix("session: "))
        .unwrap();
    assert!(handle.starts_with("w-"), "{said}");
    assert_eq!(drive.uid_of(handle), Some(7));
    assert_eq!(drive.handle_of(7).as_deref(), Some(handle));
    assert!(matches!(
        drive.find(1, handle, |_| true),
        Ok(Found::Open { uid: 7 })
    ));
    assert!(drive.find(2, handle, |_| true).is_err(), "another lead's");
    drive.forget(7);
    assert_eq!(drive.uid_of(handle), None);
    assert!(matches!(
        drive.find(1, handle, |_| false),
        Ok(Found::Closed)
    ));
}

/// Worker `uid` was given turn `turn`, `next`, which came to `went`.
fn turn_sent(
    drive: &mut ControlDrive,
    uid: u64,
    turn: u64,
    went: Option<(Intake, usize)>,
) -> Vec<Effect> {
    drive.handle(Input::TurnSent {
        uid,
        turn,
        text: "next".into(),
        went,
    })
}

/// Lead 1's worker 7, open: its relation written.
fn begun(drive: &mut ControlDrive) -> Vec<Effect> {
    up(drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    })
}

/// What the last `Keep` in `fx` keeps, for lead `lead`.
fn kept(fx: &[Effect], lead: u64) -> Option<&Kept> {
    fx.iter().rev().find_map(|effect| match effect {
        Effect::Keep { lead: of, kept, .. } if *of == lead => Some(kept),
        Effect::Keep { .. }
        | Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => None,
    })
}

fn said_conversation(drive: &mut ControlDrive, uid: u64, id: &str, sent: usize) -> Vec<Effect> {
    said_over(drive, uid, id, sent, false)
}

/// Session `uid` said its conversation `id`, its prompts sent, and whether
/// its turn is over (`over`).
fn said_over(drive: &mut ControlDrive, uid: u64, id: &str, sent: usize, over: bool) -> Vec<Effect> {
    drive.handle(Input::Conversation {
        uid,
        session_id: id.into(),
        prompts_sent: sent,
        turn_over: over,
        dir: Some(PathBuf::from(format!("/store/{id}"))),
    })
}

#[test]
fn a_begun_worker_is_kept_once_its_conversation_is_known() {
    let mut drive = lead_on();
    let fx = begun(&mut drive);
    assert_eq!(kept(&fx, 1).map(|kept| kept.workers.len()), Some(0));
    let fx = said_conversation(&mut drive, 7, "c7", 1);
    let kept = kept(&fx, 1).expect("kept");
    assert_eq!(kept.workers.len(), 1);
    assert_eq!(kept.workers[0].conversation, "c7");
    assert_eq!(kept.workers[0].agent, "agent");
    assert_eq!(kept.workers[0].folder, PathBuf::from("/repo"));
    assert_eq!(kept.workers[0].turn, Some(1));
    assert!(!kept.workers[0].person);
    // Said again unchanged, nothing is written.
    assert!(said_conversation(&mut drive, 7, "c7", 1).is_empty());
}

#[test]
fn a_conversation_known_before_the_relation_is_kept_with_it() {
    let mut drive = lead_on();
    up(&mut drive);
    said_conversation(&mut drive, 7, "c7", 0);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    assert_eq!(kept(&fx, 1).unwrap().workers[0].conversation, "c7");
}

#[test]
fn a_worker_the_person_took_is_kept_held() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_conversation(&mut drive, 7, "c7", 1);
    let fx = said_conversation(&mut drive, 7, "c7", 2);
    assert!(kept(&fx, 1).unwrap().workers[0].person);
    assert!(drive.last(7).unwrap().person);
    let fx = drive.give_back(7, 2, true).expect("offered");
    assert!(!kept(&fx, 1).unwrap().workers[0].person);
    assert!(!drive.last(7).unwrap().person);
}

#[test]
fn a_lead_turn_never_reads_as_the_persons() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_conversation(&mut drive, 7, "c7", 1);
    // The lead's own prompt is counted before its turn is.
    said_conversation(&mut drive, 7, "c7", 2);
    let fx = turn_sent(&mut drive, 7, 2, Some((Intake::Sent, 2)));
    assert!(
        shown(&fx).contains(&r#"TurnAnswer 7 Ok("turn 2 accepted")"#.to_string()),
        "{fx:?}"
    );
    assert!(!drive.last(7).unwrap().person);
    let kept = kept(&fx, 1).unwrap();
    assert!(!kept.workers[0].person);
    assert_eq!(kept.workers[0].turn, Some(2));
}

#[test]
fn closing_a_worker_keeps_a_closed_record() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_conversation(&mut drive, 7, "c7", 1);
    let handle = drive.handle_of(7).unwrap();
    let fx = drive.forget(7);
    let kept = kept(&fx, 1).expect("kept");
    assert!(kept.workers.is_empty());
    assert_eq!(kept.closed, [KeptClosed { handle, why: None }]);
}

#[test]
fn closing_the_lead_closes_its_workers_for_good() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_conversation(&mut drive, 7, "c7", 1);
    let handle = drive.handle_of(7).unwrap();
    let fx = drive.forget(1);
    let kept = kept(&fx, 1).expect("kept");
    assert!(kept.workers.is_empty());
    assert_eq!(
        kept.closed,
        [KeptClosed {
            handle,
            why: Some("its lead was closed".into())
        }]
    );
}

#[test]
fn an_old_handle_of_a_closed_lead_reads_closed() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_conversation(&mut drive, 7, "c7", 1);
    let handle = drive.handle_of(7).unwrap();
    let fx = drive.forget(1);
    let kept = kept(&fx, 1).unwrap().clone();
    // A later run: the lead's conversation opened again in session 3.
    let mut later = ControlDrive::default();
    later.switch(3, true);
    let _ = later.back(3, kept, &projects());
    assert!(matches!(
        later.find(3, &handle, |_| false),
        Ok(Found::Closed)
    ));
    assert!(later.find(4, &handle, |_| false).is_err(), "not another's");
}

/// A connected chat whose model option offers `weak` and `strong`, with
/// `current` in force.
fn on_model(current: &str) -> Chat {
    use crate::acp::{AcpEvent, ConfigChoice, ConfigOption};
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    // The agent's end stays open for the picks sent to it.
    std::mem::forget(rx);
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Connected {
        tx,
        resumed: false,
        forkable: false,
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
        current: Some(current.into()),
        choices: vec![choice("weak"), choice("strong")],
    }]));
    chat
}

fn woke_on(drive: &mut ControlDrive, chat: &Chat) -> Vec<String> {
    shown(&drive.handle(Input::Woke {
        uid: 7,
        chat: Some(chat),
        lead_here: true,
    }))
}

#[test]
fn a_turn_with_a_model_picks_before_sending() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    let shown = shown(&fx);
    assert_eq!(shown[0], "Watch 7");
    assert!(shown[1].starts_with("Arm "), "{shown:?}");
    assert_eq!(woke_on(&mut drive, &on_model("weak")), ["Pick 7"]);
}

#[test]
fn a_turn_with_a_model_sends_once_in_force() {
    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    assert_eq!(
        woke_on(&mut drive, &on_model("strong")),
        ["Unwatch 7", "Send 7 2 next"]
    );
    // Over: a later wake is no turn's.
    assert_eq!(woke_on(&mut drive, &on_model("strong")), ["Unwatch 7"]);
}

#[test]
fn a_model_not_offered_refuses_the_turn() {
    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.turn_with_model(7, 2, "next".into(), "huge".into());
    let fx = woke_on(&mut drive, &on_model("weak"));
    assert_eq!(fx.len(), 2, "{fx:?}");
    assert_eq!(fx[0], "Unwatch 7");
    assert!(
        fx[1].starts_with("TurnAnswer 7 Err") && fx[1].contains("strong"),
        "{fx:?}"
    );
}

#[test]
fn a_model_pick_that_never_answers_runs_out() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    let token = match fx[1] {
        Effect::Arm { token, .. } => token,
        Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Keep { .. }
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => panic!("{fx:?}"),
    };
    woke_on(&mut drive, &on_model("weak"));
    assert_eq!(
        shown(&drive.handle(Input::Elapsed { token })),
        [
            "Unwatch 7",
            r#"TurnAnswer 7 Err("the agent did not answer the model pick in time")"#
        ]
    );
    assert!(drive.handle(Input::Elapsed { token }).is_empty());
}

/// What lead 1 kept of one worker on `/repo`, held by the person when
/// `person` says so.
fn kept_one(person: bool) -> Kept {
    Kept {
        workers: vec![KeptWorker {
            handle: "w-00000000000000000000000000000001".into(),
            conversation: "c7".into(),
            agent: "agent".into(),
            folder: PathBuf::from("/repo"),
            dirs: vec![PathBuf::from("/x")],
            name: Some("fix".into()),
            turn: Some(3),
            text: Some("go on".into()),
            person,
            unended: false,
        }],
        closed: vec![KeptClosed {
            handle: "w-00000000000000000000000000000002".into(),
            why: None,
        }],
    }
}

const BACK: &str = "w-00000000000000000000000000000001";

/// Lead 1, its conversation opened again keeping `kept`: the key its one
/// worker comes back by.
fn coming_back(drive: &mut ControlDrive, kept: Kept) -> Key {
    let begun = drive.back(1, kept, &projects());
    assert_eq!(begun.len(), 1);
    match begun.into_iter().next() {
        Some(Begun::Resolve { key, target, fx }) => {
            assert!(fx.is_empty(), "{fx:?}");
            assert!(matches!(target, Target::Project(dir) if dir == std::path::Path::new("/repo")));
            key
        }
        Some(Begun::Said(said)) => panic!("said: {said}"),
        None => panic!("nothing comes back"),
    }
}

/// Worker 9 came back for lead 1 from `kept`.
fn came_back(drive: &mut ControlDrive, kept: Kept) -> Vec<Effect> {
    let key = coming_back(drive, kept);
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    let resume = fx.iter().find_map(|effect| match effect {
        Effect::Start { resume, dirs, .. } => Some((resume.clone(), dirs.clone())),
        Effect::ToolsChanged(_)
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Keep { .. }
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => None,
    });
    assert_eq!(
        resume,
        Some((Some("c7".to_string()), vec![PathBuf::from("/x")]))
    );
    drive.handle(Input::Started {
        key,
        started: Ok(9),
    })
}

#[test]
fn a_kept_worker_comes_back_under_its_lead() {
    // A new run: the lead's tools are off until the person switches them on.
    let mut drive = lead_kept();
    let fx = came_back(&mut drive, kept_one(false));
    assert_eq!(
        shown(&fx),
        ["Changed", "Keep 1 1 1"],
        "no pick, prompt or answer"
    );
    assert_eq!(drive.lead_of(9), Some(1));
    assert_eq!(drive.uid_of(BACK), Some(9));
    let last = drive.last(9).unwrap();
    assert_eq!((last.turn, last.text.as_deref()), (Some(3), Some("go on")));
    assert_eq!(drive.held(9).unwrap().dirs, [PathBuf::from("/x")]);
    // It counts under its lead and holds its folder again.
    assert_eq!(drive.opened_by(1), [9]);
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    drive.switch(1, true);
    let live = World {
        live: vec![1, 9],
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key,
        resolved: Ok(resolved()),
        world: live,
    });
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("already has a worker")));
}

#[test]
fn a_kept_worker_whose_folder_is_gone_is_closed_with_why() {
    let mut drive = lead_kept();
    let key = coming_back(&mut drive, kept_one(false));
    let why = "/repo: No such file or directory (os error 2)".to_string();
    let fx = drive.handle(Input::Resolved {
        key,
        resolved: Err(why.clone()),
        world: world(),
    });
    assert_eq!(shown(&fx), ["Keep 1 0 2"]);
    let kept = kept(&fx, 1).unwrap();
    assert!(kept.closed.contains(&KeptClosed {
        handle: BACK.into(),
        why: Some(why.clone()),
    }));
    assert!(matches!(drive.find(1, BACK, |_| false), Ok(Found::Closed)));
    assert_eq!(drive.why_closed(BACK), Some(why));
}

#[test]
fn a_kept_worker_on_a_held_folder_does_not_come_back() {
    let mut drive = lead_kept();
    let key = coming_back(&mut drive, kept_one(false));
    // Its conversation is open as an ordinary session on that folder.
    let busy = World {
        counts: vec![(PathBuf::from("/repo"), 1)],
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key,
        resolved: Ok(resolved()),
        world: busy,
    });
    assert_eq!(shown(&fx), ["Keep 1 0 2"]);
    assert!(drive
        .why_closed(BACK)
        .is_some_and(|why| why.contains("already has a session")));
    assert_eq!(drive.uid_of(BACK), None);
}

#[test]
fn a_kept_worker_whose_lead_went_does_not_come_back() {
    let mut drive = lead_kept();
    let key = coming_back(&mut drive, kept_one(false));
    let gone = World {
        lead_here: false,
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key,
        resolved: Ok(resolved()),
        world: gone,
    });
    assert_eq!(shown(&fx), ["Keep 1 0 2"]);
}

#[test]
fn a_held_kept_worker_comes_back_held() {
    let mut drive = lead_kept();
    came_back(&mut drive, kept_one(true));
    let last = drive.last(9).unwrap();
    assert!(last.person);
    assert_eq!(last.sent, 0);
    assert!(drive.held(9).unwrap().person);
}

#[test]
fn a_returned_worker_prompted_by_the_person_is_held() {
    let mut drive = lead_kept();
    came_back(&mut drive, kept_one(false));
    assert!(!drive.last(9).unwrap().person);
    let fx = said_conversation(&mut drive, 9, "c7", 1);
    assert!(kept(&fx, 1).unwrap().workers[0].person);
    assert!(drive.last(9).unwrap().person);
}

#[test]
fn closed_records_read_closed_after_back() {
    let mut drive = lead_kept();
    let _ = drive.back(1, kept_one(false), &projects());
    let closed = "w-00000000000000000000000000000002";
    assert!(matches!(
        drive.find(1, closed, |_| false),
        Ok(Found::Closed)
    ));
    assert_eq!(drive.why_closed(closed), None);
    assert!(drive.find(2, closed, |_| false).is_err());
}

#[test]
fn a_lead_with_workers_leads_workers() {
    let mut drive = lead_on();
    assert!(!drive.leads_workers(1));
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    assert!(drive.leads_workers(1), "opening one");
    drive.handle(Input::Resolved {
        key,
        resolved: Ok(resolved()),
        world: world(),
    });
    let mut drive = lead_on();
    begun(&mut drive);
    assert!(drive.leads_workers(1));
    assert!(!drive.leads_workers(7), "a worker leads nothing");
    let _ = drive.forget(7);
    assert!(!drive.leads_workers(1), "its one worker closed");
}

/// What lead 1 kept of two workers, `A` on `/repo` and `B` on `/other`.
fn kept_two() -> Kept {
    let mut kept = kept_one(false);
    let mut b = kept.workers[0].clone();
    b.handle = "w-0000000000000000000000000000000b".into();
    b.conversation = "c8".into();
    b.folder = PathBuf::from("/other");
    kept.workers.push(b);
    kept
}

fn handles_kept(kept: &Kept) -> Vec<String> {
    kept.workers
        .iter()
        .map(|worker| worker.handle.clone())
        .collect()
}

#[test]
fn a_sibling_failing_to_come_back_keeps_the_others() {
    let b = "w-0000000000000000000000000000000b";
    let mut drive = lead_kept();
    let begun = drive.back(1, kept_two(), &projects());
    let keys: Vec<Key> = begun
        .into_iter()
        .map(|begun| match begun {
            Begun::Resolve { key, .. } => key,
            Begun::Said(said) => panic!("said: {said}"),
        })
        .collect();
    let fx = drive.handle(Input::Resolved {
        key: keys[0].clone(),
        resolved: Err("/repo: No such file or directory".into()),
        world: world(),
    });
    assert_eq!(handles_kept(kept(&fx, 1).unwrap()), [b]);
    drive.handle(Input::Resolved {
        key: keys[1].clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    let fx = drive.handle(Input::Started {
        key: keys[1].clone(),
        started: Ok(9),
    });
    assert_eq!(handles_kept(kept(&fx, 1).expect("kept once back")), [b]);
}

#[test]
fn closing_the_lead_mid_coming_back_writes_one_final_keep() {
    let (a, b) = (BACK, "w-0000000000000000000000000000000b");
    let mut drive = lead_kept();
    let keys: Vec<Key> = drive
        .back(1, kept_two(), &projects())
        .into_iter()
        .map(|begun| match begun {
            Begun::Resolve { key, .. } => key,
            Begun::Said(said) => panic!("said: {said}"),
        })
        .collect();
    drive.handle(Input::Resolved {
        key: keys[0].clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    drive.handle(Input::Started {
        key: keys[0].clone(),
        started: Ok(9),
    });
    // B is still coming back as the lead closes.
    let fx = drive.forget(1);
    let keeps: Vec<&Kept> = fx
        .iter()
        .filter_map(|effect| match effect {
            Effect::Keep { lead: 1, kept, .. } => Some(kept),
            Effect::Keep { .. }
            | Effect::ToolsChanged(_)
            | Effect::Start { .. }
            | Effect::Watch(_)
            | Effect::Unwatch(_)
            | Effect::Pick { .. }
            | Effect::Begin { .. }
            | Effect::Arm { .. }
            | Effect::Drop { .. }
            | Effect::Answer { .. }
            | Effect::Changed
            | Effect::Send { .. }
            | Effect::TurnAnswer { .. } => None,
        })
        .collect();
    assert_eq!(keeps.len(), 1, "{fx:?}");
    assert!(keeps[0].workers.is_empty());
    let closed: Vec<&str> = keeps[0].closed.iter().map(|c| c.handle.as_str()).collect();
    assert!(closed.contains(&a) && closed.contains(&b), "{closed:?}");
}

#[test]
fn a_lead_closed_or_switched_off_ends_its_turns_waiting_on_a_model() {
    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    let fx = shown(&drive.switch(1, false));
    assert!(
        fx.contains(&r#"TurnAnswer 7 Err("the lead's tools were switched off")"#.to_string()),
        "{fx:?}"
    );
    assert!(
        woke_on(&mut drive, &on_model("strong")) == ["Unwatch 7"],
        "nothing sent"
    );

    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    let fx = shown(&drive.forget(1));
    assert!(
        fx.contains(&r#"TurnAnswer 7 Err("the lead was closed")"#.to_string()),
        "{fx:?}"
    );
}

#[test]
fn a_turn_waiting_on_its_model_refuses_another() {
    let mut drive = lead_on();
    begun(&mut drive);
    assert_eq!(drive.turn_waiting(7, 2, "next"), None);
    let _ = drive.turn_with_model(7, 2, "next".into(), "strong".into());
    let waiting = "turn 2 is waiting on its model pick".to_string();
    assert_eq!(drive.turn_waiting(7, 2, "next"), Some(Ok(waiting.clone())));
    assert_eq!(drive.turn_waiting(7, 3, "other"), Some(Err(waiting)));
}

#[test]
fn a_worker_coming_back_counts_under_its_leads_cap() {
    let mut drive = lead_on();
    let _back = coming_back(&mut drive, kept_one(false));
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let three = World {
        opened: (2..5).map(|uid| (uid, PathBuf::from("/x"))).collect(),
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key,
        resolved: Ok(resolved()),
        world: three,
    });
    let why = answer(&fx).unwrap().as_ref().unwrap_err();
    assert!(
        why.contains("4 sessions open") && why.contains("coming back"),
        "{why}"
    );
}

#[test]
fn a_late_start_of_a_coming_back_that_failed_drops_its_project() {
    let mut drive = lead_kept();
    let key = coming_back(&mut drive, kept_one(false));
    // A worktree no project of the window holds: its project is added.
    let added = Resolved {
        same: Vec::new(),
        ..resolved()
    };
    drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(added),
        world: world(),
    });
    let _ = drive.forget(1);
    let fx = drive.handle(Input::Started {
        key,
        started: Ok(9),
    });
    assert_eq!(shown(&fx), ["Drop 9 true"]);
    let root = fx.iter().find_map(|effect| match effect {
        Effect::Drop { root, .. } => Some(root.clone()),
        Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Keep { .. }
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => None,
    });
    assert_eq!(root, Some(PathBuf::from("/repo")));
}

#[test]
fn a_lead_keeps_only_its_newest_ended_workers() {
    let mut drive = ControlDrive::default();
    let closed: Vec<KeptClosed> = (0..70)
        .map(|n| KeptClosed {
            handle: format!("w-{n:032x}"),
            why: None,
        })
        .collect();
    let seventy = Kept {
        workers: Vec::new(),
        closed,
    };
    let _ = drive.back(1, seventy, &projects());
    let fx = said_conversation(&mut drive, 1, "c1", 0);
    let written = kept(&fx, 1).expect("kept");
    assert_eq!(written.closed.len(), 64);
    assert_eq!(
        written.closed[0].handle,
        format!("w-{:032x}", 6),
        "the oldest went"
    );
    assert_eq!(written.closed[63].handle, format!("w-{:032x}", 69));
    let oldest = format!("w-{:032x}", 0);
    assert!(drive.find(1, &oldest, |_| false).is_err());
}

#[test]
fn ended_workers_stay_with_the_conversation_they_ended_in() {
    let mut drive = lead_kept();
    said_conversation(&mut drive, 1, "c1", 0);
    let _ = drive.back(1, kept_one(false), &projects());
    let closed = "w-00000000000000000000000000000002";
    // Resume in this session: the lead's session goes on another
    // conversation, leading no worker.
    let fx = drive.handle(Input::Resolved {
        key: (1, BACK.to_string()),
        resolved: Err("gone".into()),
        world: world(),
    });
    assert!(kept(&fx, 1).is_some());
    let fx = said_conversation(&mut drive, 1, "c2", 0);
    assert!(kept(&fx, 1).is_none(), "nothing moves: {fx:?}");
    assert!(drive.find(1, closed, |_| false).is_err());
    // A worker opened on the new conversation writes there, and leaves the
    // file of the one before alone.
    drive.switch(1, true);
    let fx = begun(&mut drive);
    assert_eq!(keep_at(&fx, 1), Some((store("c2"), None, false)));
}

#[test]
fn a_lead_turn_is_kept_unended_until_it_ends() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = said_conversation(&mut drive, 7, "c7", 1);
    assert!(
        kept(&fx, 1).unwrap().workers[0].unended,
        "its first turn runs"
    );
    let fx = said_over(&mut drive, 7, "c7", 1, true);
    assert!(!kept(&fx, 1).unwrap().workers[0].unended, "and ended");
    assert!(said_over(&mut drive, 7, "c7", 1, true).is_empty());
}

#[test]
fn a_worker_coming_back_keeps_its_turn_unended_until_a_new_one() {
    let mut drive = lead_kept();
    let mut kept_unended = kept_one(false);
    kept_unended.workers[0].unended = true;
    came_back(&mut drive, kept_unended);
    assert!(drive.held(9).unwrap().unended);
    // Idle and connected, it has run nothing in this run.
    let fx = said_over(&mut drive, 9, "c7", 0, true);
    assert!(
        kept(&fx, 1).is_none_or(|kept| kept.workers[0].unended),
        "{fx:?}"
    );
    assert!(drive.held(9).unwrap().unended);
    let _ = turn_sent(&mut drive, 9, 4, Some((Intake::Sent, 1)));
    let fx = said_over(&mut drive, 9, "c7", 1, true);
    assert!(!kept(&fx, 1).unwrap().workers[0].unended);
}

#[test]
fn a_link_lost_mid_turn_keeps_it_unended() {
    let mut drive = lead_on();
    begun(&mut drive);
    said_over(&mut drive, 7, "c7", 1, false);
    assert!(drive.held(7).unwrap().unended);
}

#[test]
fn workers_come_back_in_the_order_kept() {
    let mut drive = lead_kept();
    let keys: Vec<Key> = drive
        .back(1, kept_two(), &projects())
        .into_iter()
        .map(|begun| match begun {
            Begun::Resolve { key, .. } => key,
            Begun::Said(said) => panic!("said: {said}"),
        })
        .collect();
    for (key, folder) in keys.iter().zip(["/repo", "/other"]) {
        let at = Resolved {
            folder: PathBuf::from(folder),
            ..resolved()
        };
        drive.handle(Input::Resolved {
            key: key.clone(),
            resolved: Ok(at),
            world: world(),
        });
    }
    // The second kept starts first, so it has the lower uid.
    drive.handle(Input::Started {
        key: keys[1].clone(),
        started: Ok(11),
    });
    drive.handle(Input::Started {
        key: keys[0].clone(),
        started: Ok(12),
    });
    let uids: Vec<u64> = drive.workers_of(1).iter().map(|w| w.uid).collect();
    assert_eq!(uids, [12, 11]);
}

#[test]
fn an_opening_worker_lists_after_the_older_ones() {
    let mut drive = lead_on();
    begun(&mut drive);
    let mut second = asked("more");
    second.request = "r2".into();
    second.project = "repo".into();
    let (key, _) = resolves(drive.open_worker(second, &projects(), None));
    let other = Resolved {
        folder: PathBuf::from("/other"),
        ..resolved()
    };
    drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(other),
        world: world(),
    });
    drive.handle(Input::Started {
        key,
        started: Ok(3),
    });
    let uids: Vec<u64> = drive.workers_of(1).iter().map(|w| w.uid).collect();
    assert_eq!(uids, [7, 3]);
}

#[test]
fn a_lead_may_not_leave_its_conversation() {
    let mut drive = lead_on();
    assert_eq!(drive.may_leave_conversation(1), Ok(()));
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    assert_eq!(
        drive.may_leave_conversation(1),
        Err(ResumeRefused::LeadsWorkers),
        "one opening"
    );
    let _ = drive.handle(Input::Resolved {
        key,
        resolved: Err("gone".into()),
        world: world(),
    });
    begun(&mut drive);
    assert_eq!(
        drive.may_leave_conversation(1),
        Err(ResumeRefused::LeadsWorkers)
    );
    assert_eq!(drive.may_leave_conversation(7), Ok(()), "a worker");
    let _ = drive.forget(7);
    assert_eq!(drive.may_leave_conversation(1), Ok(()));
}

#[test]
fn a_conversation_open_anywhere_is_not_resumed_again() {
    let drive = ControlDrive::default();
    assert_eq!(
        drive.may_resume("c1", &["c1".into()]),
        Err(ResumeRefused::AlreadyOpen)
    );
    assert_eq!(drive.may_resume("c2", &["c1".into()]), Ok(()));
    assert_eq!(drive.may_resume("c1", &[]), Ok(()));
}

#[test]
fn a_turn_goes_as_a_send_and_is_answered_once_it_went() {
    let mut drive = lead_on();
    begun(&mut drive);
    let (sending, fx) = drive.send_prompt(7, "w", 2, "next", None, Some(&on_model("weak")));
    assert_eq!(sending, Sending::Later);
    assert_eq!(shown(&fx), ["Send 7 2 next"]);
    let fx = turn_sent(&mut drive, 7, 2, Some((Intake::Sent, 2)));
    let shown = shown(&fx);
    assert!(shown[0].starts_with("Keep 1 "), "{shown:?}");
    assert_eq!(shown[1], r#"TurnAnswer 7 Ok("turn 2 accepted")"#);
    assert_eq!(drive.last(7).unwrap().turn, Some(2));
}

#[test]
fn a_turn_sent_again_is_said_and_sends_nothing() {
    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.send_prompt(7, "w", 2, "next", None, Some(&on_model("weak")));
    let _ = turn_sent(&mut drive, 7, 2, Some((Intake::Sent, 2)));
    let mut chat = connected();
    chat.prompts_sent = 2;
    let (sending, fx) = drive.send_prompt(7, "w", 2, "next", None, Some(&chat));
    assert_eq!(sending, Sending::Said(Ok("turn 2 already sent".into())));
    assert!(fx.is_empty());
}

#[test]
fn a_turn_to_a_worker_still_connecting_is_refused() {
    let mut drive = lead_on();
    begun(&mut drive);
    let (sending, fx) = drive.send_prompt(7, "w", 2, "next", None, None);
    assert!(matches!(sending, Sending::Said(Err(_))), "{sending:?}");
    assert!(fx.is_empty());
    let (sending, fx) = drive.send_prompt(7, "w", 1, "go", None, None);
    assert_eq!(sending, Sending::Said(Err("w is still connecting".into())));
    assert!(fx.is_empty());
}

#[test]
fn a_turn_with_a_model_waits_on_it() {
    let mut drive = lead_on();
    begun(&mut drive);
    let (sending, fx) = drive.send_prompt(
        7,
        "w",
        2,
        "next",
        Some("strong".into()),
        Some(&on_model("weak")),
    );
    assert_eq!(sending, Sending::Later);
    let shown = shown(&fx);
    assert_eq!(shown[0], "Watch 7");
    assert!(shown[1].starts_with("Arm "), "{shown:?}");
    let (sending, _) = drive.send_prompt(7, "w", 3, "other", None, Some(&on_model("weak")));
    assert_eq!(
        sending,
        Sending::Said(Err("turn 2 is waiting on its model pick".into()))
    );
}

#[test]
fn a_turn_waiting_on_its_model_is_checked_again_before_it_goes() {
    let mut drive = lead_on();
    begun(&mut drive);
    let _ = drive.send_prompt(
        7,
        "w",
        2,
        "next",
        Some("strong".into()),
        Some(&on_model("weak")),
    );
    // The person prompted the worker while the model was picked.
    let mut chat = on_model("strong");
    chat.prompts_sent = 3;
    let fx = woke_on(&mut drive, &chat);
    assert_eq!(fx.len(), 2, "{fx:?}");
    assert_eq!(fx[0], "Unwatch 7");
    assert!(
        fx[1].starts_with("TurnAnswer 7 Err(\"the person holds this worker"),
        "{fx:?}"
    );
}

#[test]
fn a_turn_whose_session_went_before_it_was_sent_says_so() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = turn_sent(&mut drive, 7, 2, None);
    assert_eq!(
        shown(&fx),
        [r#"TurnAnswer 7 Err("the session was closed before the turn went")"#]
    );
}

fn store(id: &str) -> PathBuf {
    PathBuf::from(format!("/store/{id}"))
}

/// Where the last `Keep` in `fx` for lead `lead` writes, the folder it
/// empties first, and whether it is the lead's last.
fn keep_at(fx: &[Effect], lead: u64) -> Option<(PathBuf, Option<PathBuf>, bool)> {
    fx.iter().rev().find_map(|effect| match effect {
        Effect::Keep {
            lead: of,
            dir,
            moved_from,
            last,
            ..
        } if *of == lead => Some((dir.clone(), moved_from.clone(), *last)),
        Effect::Keep { .. }
        | Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed
        | Effect::Send { .. }
        | Effect::TurnAnswer { .. } => None,
    })
}

#[test]
fn a_lead_with_no_folder_yet_writes_nothing() {
    let mut drive = ControlDrive::default();
    drive.switch(1, true);
    let fx = begun(&mut drive);
    assert_eq!(keep_at(&fx, 1), None, "{fx:?}");
    let fx = said_conversation(&mut drive, 1, "c1", 0);
    assert_eq!(keep_at(&fx, 1), Some((store("c1"), None, false)));
}

#[test]
fn a_lead_whose_conversation_moved_removes_its_old_file() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = said_conversation(&mut drive, 1, "c2", 0);
    assert_eq!(
        keep_at(&fx, 1),
        Some((store("c2"), Some(store("c1")), false))
    );
    let fx = said_conversation(&mut drive, 7, "c7", 1);
    assert_eq!(keep_at(&fx, 1), Some((store("c2"), None, false)));
}

#[test]
fn a_closed_lead_writes_its_last_keep_where_its_conversation_was() {
    let mut drive = lead_on();
    begun(&mut drive);
    let fx = drive.forget(1);
    let keeps: Vec<_> = fx
        .iter()
        .filter(|effect| keep_at(std::slice::from_ref(*effect), 1).is_some())
        .collect();
    assert_eq!(keeps.len(), 1, "{fx:?}");
    assert_eq!(keep_at(&fx, 1), Some((store("c1"), None, true)));
    // Where it wrote is forgotten with it.
    let fx = said_conversation(&mut drive, 1, "c9", 0);
    assert_eq!(keep_at(&fx, 1), Some((store("c9"), None, false)));
}
