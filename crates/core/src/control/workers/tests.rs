use super::*;

#[test]
fn a_reserved_folder_refuses_a_second_open() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    let why = held.take(Path::new("/wt")).unwrap_err();
    assert!(why.contains("/wt"), "{why}");
}

#[test]
fn release_after_a_failed_open_frees_it() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    held.release(Path::new("/wt"));
    assert!(held.take(Path::new("/wt")).is_ok());
}

#[test]
fn a_bound_worker_closing_frees_its_folder() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    held.bind(Path::new("/wt"), 7);
    assert!(held.take(Path::new("/wt")).is_err());
    held.release_worker(7);
    assert!(held.take(Path::new("/wt")).is_ok());
}

#[test]
fn release_worker_leaves_other_folders() {
    let mut held = Reservations::default();
    held.take(Path::new("/a")).unwrap();
    held.bind(Path::new("/a"), 7);
    held.take(Path::new("/b")).unwrap();
    held.bind(Path::new("/b"), 8);
    held.release_worker(7);
    assert!(held.take(Path::new("/b")).is_err());
}

/// A worker whose session went without being closed (its window closed)
/// leaves its folder held; the next open lets it go.
#[test]
fn a_worker_no_longer_live_frees_its_folder() {
    let mut held = Reservations::default();
    held.take(Path::new("/a")).unwrap();
    held.bind(Path::new("/a"), 7);
    held.take(Path::new("/b")).unwrap();
    held.bind(Path::new("/b"), 8);
    held.take(Path::new("/c")).unwrap();
    held.prune(|uid| uid == 8);
    assert!(held.take(Path::new("/a")).is_ok());
    assert!(held.take(Path::new("/b")).is_err());
    // An open still in flight holds its folder whatever is live.
    assert!(held.take(Path::new("/c")).is_err());
}

fn asked(prompt: &str) -> Asked {
    Asked {
        project: "/wt".into(),
        agent: "Claude".into(),
        prompt: Some(prompt.into()),
        dirs: Vec::new(),
        turn: Some(1),
        model: None,
        name: None,
    }
}

#[test]
fn a_first_request_is_new() {
    let mut requests = Requests::default();
    assert_eq!(requests.see(1, "a", &asked("go")), Ok(Seen::New));
}

#[test]
fn the_same_request_while_opening_says_so() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    assert_eq!(requests.see(1, "a", &asked("go")), Ok(Seen::Opening(None)));
    requests.started(1, "a", 7);
    assert_eq!(
        requests.see(1, "a", &asked("go")),
        Ok(Seen::Opening(Some(7)))
    );
}

#[test]
fn a_finished_request_answers_again() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    requests.answered(1, "a", "session: r-7".into());
    assert_eq!(
        requests.see(1, "a", &asked("go")),
        Ok(Seen::Answered("session: r-7".into()))
    );
}

#[test]
fn a_failed_request_opens_afresh() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    requests.failed(1, "a");
    assert_eq!(requests.see(1, "a", &asked("go")), Ok(Seen::New));
}

#[test]
fn the_same_request_with_other_arguments_is_refused() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    let why = requests.see(1, "a", &asked("stop")).unwrap_err();
    assert!(why.contains("request a"), "{why}");
}

#[test]
fn the_same_request_with_another_name_is_refused() {
    let mut requests = Requests::default();
    let first = Asked {
        name: Some("a · one".into()),
        ..asked("go")
    };
    requests.see(1, "a", &first).unwrap();
    let second = Asked {
        name: Some("a · two".into()),
        ..asked("go")
    };
    assert!(requests.see(1, "a", &second).is_err());
}

#[test]
fn requests_are_per_lead() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    assert_eq!(requests.see(2, "a", &asked("stop")), Ok(Seen::New));
}

#[test]
fn forgetting_a_lead_forgets_its_requests() {
    let mut requests = Requests::default();
    requests.see(1, "a", &asked("go")).unwrap();
    requests.see(2, "b", &asked("go")).unwrap();
    requests.forget_lead(1);
    assert_eq!(requests.see(1, "a", &asked("stop")), Ok(Seen::New));
    assert_eq!(requests.see(2, "b", &asked("go")), Ok(Seen::Opening(None)));
}

fn relation(lead: u64) -> Relation {
    Relation {
        lead,
        turn: Some(1),
        text: Some("go".into()),
        sent: 1,
        dirs: Vec::new(),
    }
}

#[test]
fn a_worker_of_the_caller_is_mine() {
    assert!(matches!(
        whose(Some(&relation(1)), None, 1, true),
        Whose::Mine
    ));
}

#[test]
fn another_lead_s_worker_is_not_yours() {
    assert!(matches!(
        whose(Some(&relation(2)), None, 1, true),
        Whose::NotYours
    ));
}

#[test]
fn a_session_no_lead_opened_is_not_yours() {
    assert!(matches!(whose(None, None, 1, true), Whose::NotYours));
}

#[test]
fn a_worker_of_the_caller_that_closed_is_closed() {
    assert!(matches!(whose(None, Some(1), 1, false), Whose::Closed));
}

/// Another lead's worker closing is none of the caller's business: the same
/// refusal as for any session that is not its own.
#[test]
fn another_lead_s_closed_worker_is_not_yours() {
    assert!(matches!(whose(None, Some(2), 1, false), Whose::NotYours));
}

#[test]
fn a_uid_never_seen_is_not_yours() {
    assert!(matches!(whose(None, None, 1, false), Whose::NotYours));
}

/// One worker per folder holds for folders inside each other too: two
/// agents on `/repo` and `/repo/sub` would write the same files.
#[test]
fn a_folder_inside_a_reserved_one_is_refused() {
    let mut held = Reservations::default();
    held.take(Path::new("/repo")).unwrap();
    let why = held.take(Path::new("/repo/sub")).unwrap_err();
    assert!(why.contains("/repo"), "{why}");
}

#[test]
fn a_folder_holding_a_reserved_one_is_refused() {
    let mut held = Reservations::default();
    held.take(Path::new("/repo/sub")).unwrap();
    assert!(held.take(Path::new("/repo")).is_err());
}

#[test]
fn a_sibling_with_a_common_prefix_is_free() {
    let mut held = Reservations::default();
    held.take(Path::new("/repo")).unwrap();
    assert!(held.take(Path::new("/repo2")).is_ok());
}

/// Any window's session on the worker's folder, or on a folder inside it,
/// refuses the open; one holding the worker's folder (a lead in the main
/// checkout, its worktrees beneath) does not.
#[test]
fn sessions_on_the_folder_or_inside_it_refuse() {
    let roots = vec![
        (PathBuf::from("/repo"), PathBuf::from("/repo"), 1),
        (PathBuf::from("/repo/sub"), PathBuf::from("/repo/sub"), 1),
        (PathBuf::from("/other"), PathBuf::from("/other"), 2),
        (
            PathBuf::from("/repo/empty"),
            PathBuf::from("/repo/empty"),
            0,
        ),
    ];
    assert_eq!(
        sessions_in(Path::new("/repo"), &roots),
        vec![PathBuf::from("/repo"), PathBuf::from("/repo/sub")]
    );
    assert!(sessions_in(Path::new("/repo/.worktrees/a"), &roots).is_empty());
}

#[test]
fn give_back_is_offered_only_while_the_person_holds_it() {
    let held = relation(1);
    assert!(!offered(Some(&held), 1, true, true), "the lead holds it");
    assert!(offered(Some(&held), 2, true, true));
    assert!(!offered(None, 2, true, true), "no lead to give it to");
}

#[test]
fn give_back_is_not_offered_once_the_lead_is_off_or_gone() {
    let held = relation(1);
    assert!(!offered(Some(&held), 2, false, true));
    assert!(!offered(Some(&held), 2, true, false));
}

/// Given back, the lead holds it again: what the person sent counts as sent.
#[test]
fn given_back_the_lead_holds_it() {
    let mut chat = crate::chat::model::Chat::default();
    chat.prompts_sent = 3;
    let mut given = relation(1);
    given.sent = chat.prompts_sent;
    let held = crate::control::state::Held {
        turn: given.turn,
        sent: given.sent,
        dirs: Vec::new(),
    };
    let said = crate::control::state::state(&chat, &held);
    assert!(said.contains("held by: lead"), "{said}");
    assert!(!offered(Some(&given), chat.prompts_sent, true, true));
}
