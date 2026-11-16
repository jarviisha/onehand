use super::*;
use crate::chat::model::Link;
use crate::control::kept::KeptClosed;
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
    let mut drive = ControlDrive::default();
    drive.switch(1, true);
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
            Effect::Keep { lead, kept } => {
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
        | Effect::Keep { .. } => None,
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
        | Effect::Keep { .. }] => panic!("the deadline is armed at the call: {fx:?}"),
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
    let mut drive = ControlDrive::default();
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
    let (said, fx) = drive.sent(7, 2, "next", &Intake::Queued, 2);
    assert!(said.is_err() && fx.is_empty());
    assert_eq!(drive.last(7).map(|last| last.turn), Some(Some(1)));
    assert_eq!(
        drive.sent(7, 2, "next", &Intake::Sent, 2).0,
        Ok("turn 2 accepted".into())
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
        Effect::Keep { lead: of, kept } if *of == lead => Some(kept),
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
        | Effect::Changed => None,
    })
}

fn said_conversation(drive: &mut ControlDrive, uid: u64, id: &str, sent: usize) -> Vec<Effect> {
    drive.handle(Input::Conversation {
        uid,
        session_id: id.into(),
        prompts_sent: sent,
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
    let (said, fx) = drive.sent(7, 2, "next", &Intake::Sent, 2);
    assert_eq!(said, Ok("turn 2 accepted".into()));
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
