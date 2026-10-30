use super::Reservations;
use std::path::{Path, PathBuf};

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

use super::requests::{Asked, Requests, Seen};

fn asked(prompt: &str) -> Asked {
    Asked {
        project: "/wt".into(),
        agent: "Claude".into(),
        prompt: Some(prompt.into()),
        dirs: Vec::new(),
        turn: Some(1),
        model: None,
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

/// An outside program is told every worker-only argument it named, not
/// only the first.
#[test]
fn an_outside_program_hears_every_worker_input_it_named() {
    let open = super::Open {
        caller: None,
        project: "app".into(),
        agent: "Claude".into(),
        prompt: None,
        dirs: vec!["/run".into()],
        request: None,
        turn: Some(1),
        model: Some("strong".into()),
    };
    let why = super::worker_only(&open).unwrap();
    assert!(
        why.contains("`dirs`") && why.contains("`turn`") && why.contains("`model`"),
        "{why}"
    );
    assert!(!why.contains("`request`"), "{why}");
}

#[test]
fn an_outside_program_naming_none_is_let_through() {
    let open = super::Open {
        caller: None,
        project: "app".into(),
        agent: "Claude".into(),
        prompt: Some("hi".into()),
        dirs: Vec::new(),
        request: None,
        turn: None,
        model: None,
    };
    assert_eq!(super::worker_only(&open), None);
}

use super::requests::Relation;
use super::state::{Whose, whose};

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
    use super::sessions_in;
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
