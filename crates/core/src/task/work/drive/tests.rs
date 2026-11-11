// A test that only reads the drive after a command lets its effects go.
#![allow(unused_must_use)]

use super::*;
use crate::connector::fake::Fake;
use crate::task::work::issue_work;
use crate::task::work::tests::{at, forge_flow, task};
use crate::task::{Task, Working};

static FORGE: Fake = Fake::SERVING;
static CONNECTORS: [&dyn Connector; 1] = [&FORGE];

const ROOT: &str = "/repo";
const NOW: u64 = 10_000;

fn key(number: u64) -> IssueKey {
    IssueKey {
        file: PathBuf::from("/store/repo.json"),
        number,
    }
}

/// Issue `number`'s work: the task `t`, at step `step` (6 waits on status
/// checks, past its pull request; 1 waits for approval).
fn work(number: u64, t: &Task, working: Working) -> IssueWork {
    issue_work(key(number), [(t, Some(working), None)]).unwrap()
}

fn checks(id: &str) -> Task {
    at(task(id, forge_flow(), Some("Forge")), 6, 900)
}

fn reads_pr(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|effect| match effect {
        Effect::ReadPr { generation, .. } => Some(*generation),
        Effect::ReadPrs { .. }
        | Effect::LookUp { .. }
        | Effect::ReadLeft { .. }
        | Effect::ReadReviewFiles { .. } => None,
    })
}

fn drive(page: bool, works: &[IssueWork]) -> WorkDrive {
    let mut drive = WorkDrive::new(&CONNECTORS, page);
    drive.set_works(works, None);
    drive
}

#[test]
fn an_issue_opened_reads_its_pull_request_once() {
    let t = checks("a");
    let mut drive = drive(false, &[work(1, &t, Working::Running)]);
    let root = Path::new(ROOT);
    assert!(reads_pr(&drive.show(root, Some(&key(1)), NOW)).is_some());
    assert!(reads_pr(&drive.show(root, Some(&key(1)), NOW)).is_none());
    // Its work moving reads it again.
    let moved = at(task("a", forge_flow(), Some("Forge")), 6, 950);
    drive.set_works(&[work(1, &moved, Working::Running)], Some(&key(1)));
    assert!(reads_pr(&drive.show(root, Some(&key(1)), NOW)).is_some());
}

#[test]
fn coming_back_reads_only_what_is_older_than_a_minute() {
    let t = checks("a");
    let mut drive = drive(false, &[work(1, &t, Working::Running)]);
    let root = Path::new(ROOT);
    let generation = reads_pr(&drive.show(root, Some(&key(1)), NOW)).unwrap();
    drive.handle(Input::Pr {
        generation,
        answer: Ok(None),
        now: NOW,
    });
    drive.drawn();
    drive.returned();
    assert!(reads_pr(&drive.show(root, Some(&key(1)), NOW + 30)).is_none());
    drive.drawn();
    drive.returned();
    assert!(reads_pr(&drive.show(root, Some(&key(1)), NOW + READ_AGE)).is_some());
}

#[test]
fn an_answer_about_another_issue_is_never_shown() {
    let (a, b) = (checks("a"), checks("b"));
    let works = [work(1, &a, Working::Running), work(2, &b, Working::Running)];
    let mut drive = drive(false, &works);
    let root = Path::new(ROOT);
    let old = reads_pr(&drive.show(root, Some(&key(1)), NOW)).unwrap();
    drive.show(root, Some(&key(2)), NOW);
    drive.handle(Input::Pr {
        generation: old,
        answer: Ok(None),
        now: NOW,
    });
    assert!(drive.pr_value(Some(&works[1])).is_none());
    assert!(matches!(
        drive.pr_seen(Some(&works[0])),
        super::super::PrSeen::Unread
    ));
}

#[test]
fn the_page_reads_every_projects_prs_when_shown_and_looks_up_what_a_cap_missed() {
    let t = checks("a");
    let mut drive = drive(true, &[work(1, &t, Working::Running)]);
    let projects = [Project {
        root: PathBuf::from(ROOT),
        file: key(1).file,
        forge: Some(&FORGE),
    }];
    assert!(drive.page_drawn(&projects, NOW).is_empty());
    drive.page_shown();
    let effects = drive.page_drawn(&projects, NOW);
    let [Effect::ReadPrs {
        generation,
        branches,
        ..
    }] = effects.as_slice()
    else {
        panic!("not one read");
    };
    assert!(drive.prs_reading());
    let looked = drive.handle(Input::Prs {
        generation: *generation,
        root: PathBuf::from(ROOT),
        forge: &FORGE,
        branches: branches.clone(),
        answer: Ok(PullRequests {
            by_branch: Vec::new(),
            capped: true,
        }),
        now: NOW,
    });
    assert!(!drive.prs_reading());
    assert!(
        matches!(looked.as_slice(), [Effect::LookUp { branch, .. }] if branch == "onehand/local-1")
    );
    assert_eq!(drive.prs_read_at(), Some(NOW));
}

#[test]
fn an_older_page_read_is_counted_down_and_dropped() {
    let t = checks("a");
    let mut drive = drive(true, &[work(1, &t, Working::Running)]);
    let projects = [Project {
        root: PathBuf::from(ROOT),
        file: key(1).file,
        forge: Some(&FORGE),
    }];
    drive.read_prs(&projects);
    drive.read_prs(&projects);
    let effects = drive.handle(Input::Prs {
        generation: 1,
        root: PathBuf::from(ROOT),
        forge: &FORGE,
        branches: Vec::new(),
        answer: Err("old".into()),
        now: NOW,
    });
    assert!(effects.is_empty());
    assert!(drive.prs().unwrap().get(Path::new(ROOT)).is_none());
    assert!(drive.prs_reading());
}

#[test]
fn what_the_work_left_is_read_again_when_it_moves() {
    let t = checks("a");
    let mut drive = drive(true, &[work(1, &t, Working::Running)]);
    let root = Path::new(ROOT);
    let left = |effects: &[Effect]| effects.iter().any(|e| matches!(e, Effect::ReadLeft { .. }));
    assert!(left(&drive.show(root, Some(&key(1)), NOW)));
    assert!(!left(&drive.show(root, Some(&key(1)), NOW)));
    let moved = at(task("a", forge_flow(), Some("Forge")), 4, 950);
    drive.set_works(&[work(1, &moved, Working::Running)], Some(&key(1)));
    assert!(left(&drive.show(root, Some(&key(1)), NOW)));
}

/// Issue 1's work waiting for approval at `visit` of run `r`.
fn waiting(visit: u32) -> IssueWork {
    let t = at(task("a", forge_flow(), Some("Forge")), 1, 900);
    let mut work = work(1, &t, Working::Waiting);
    work.work.review = Some(UnderReview {
        at: crate::workflow::ApprovalAt {
            run: "r".into(),
            visit,
        },
        of: "Plan".into(),
        answer: "the plan".into(),
        starts: None,
        span: None,
        check: super::super::Checked::NotRun,
    });
    work
}

#[test]
fn a_review_press_goes_where_it_was_read_and_reloads_where_the_run_moved() {
    let first = waiting(1);
    let mut drive = drive(true, std::slice::from_ref(&first));
    drive.open_review(key(1), &first.work);
    assert!(drive.review().unwrap().open_on(Some(&key(1))));
    assert_eq!(drive.revise("  ").0, Answered::Refused);

    // The run waits on a later visit now.
    drive.set_works(&[waiting(2)], Some(&key(1)));
    let (answered, _) = drive.approve();
    assert_eq!(answered, Answered::Reloaded);
    assert!(drive.review().unwrap().changed());

    let (answered, _) = drive.revise("do it again");
    assert!(matches!(
        answered,
        Answered::Sent(Press::Revise { approval, note }) if approval.at.visit == 2 && note == "do it again"
    ));
    assert_eq!(drive.review().unwrap().sent(), Some(Sent::Revise));

    // A run no longer waiting refuses.
    drive.set_works(&[], Some(&key(1)));
    assert_eq!(drive.approve().0, Answered::Refused);
}

#[test]
fn picking_another_issue_closes_the_review_and_the_same_keeps_it() {
    let first = waiting(1);
    let mut drive = drive(true, std::slice::from_ref(&first));
    drive.open_review(key(1), &first.work);
    assert!(!drive.pick(Some(&key(1))));
    assert!(drive.review().unwrap().is_open());
    assert!(drive.pick(Some(&key(2))));
    assert!(!drive.review().unwrap().is_open());
}

#[test]
fn what_the_work_left_is_shown_only_for_its_own_run() {
    let t = checks("a");
    let mut drive = drive(true, &[work(1, &t, Working::Running)]);
    let effects = drive.show(Path::new(ROOT), Some(&key(1)), NOW);
    let generation = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ReadLeft { generation, .. } => Some(*generation),
            Effect::ReadPr { .. }
            | Effect::ReadPrs { .. }
            | Effect::LookUp { .. }
            | Effect::ReadReviewFiles { .. } => None,
        })
        .unwrap();
    drive.handle(Input::Left {
        generation,
        left: super::super::left::left_blocking(&work(1, &t, Working::Running).work),
        now: NOW,
    });
    let own = work(1, &t, Working::Running).work;
    assert!(matches!(drive.left_seen(&own), LeftSeen::Read(_, NOW)));
    let other = work(2, &checks("b"), Working::Running).work;
    assert!(matches!(drive.left_seen(&other), LeftSeen::Unread));
}
