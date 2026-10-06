use super::*;
use crate::task::work::issue_work;
use crate::task::work::tests::{at, branch_flow, ended, forge_flow, pr, task};
use crate::task::{Task, Working};
use crate::workflow::Outcome;

const A: &str = "/a";
const B: &str = "/b";

fn issue(number: u64, title: &str, open: bool, updated: u64) -> LocalIssue {
    LocalIssue {
        number,
        title: title.into(),
        body: String::new(),
        open,
        labels: Vec::new(),
        created: 1,
        updated,
        notes: Vec::new(),
        link: None,
        imported_from: None,
    }
}

fn key(root: &str, number: u64) -> IssueKey {
    IssueKey {
        file: PathBuf::from(format!("{root}.json")),
        number,
    }
}

/// One issue's work from its tasks, newest first by when each moved.
fn work_of(root: &str, number: u64, tasks: &[(&Task, Option<Working>)]) -> IssueWork {
    issue_work(
        key(root, number),
        tasks.iter().map(|(task, working)| (*task, *working, None)),
    )
    .unwrap()
}

/// What the test holds: the issues and their work, owned, so items can
/// borrow them.
struct Fixture {
    issues: Vec<(&'static str, LocalIssue, Option<IssueWork>)>,
}

impl Fixture {
    fn items(&self) -> Vec<Item<'_>> {
        self.issues
            .iter()
            .map(|(root, issue, work)| Item {
                root: Path::new(root),
                key: key(root, issue.number),
                issue,
                work: work.as_ref(),
            })
            .collect()
    }
}

fn running() -> Task {
    at(task("run", forge_flow(), Some("GitHub")), 2, 900)
}

fn waiting_approval() -> Task {
    at(task("appr", forge_flow(), Some("GitHub")), 1, 900)
}

fn failed() -> Task {
    ended(
        at(task("fail", branch_flow(), None), 2, 800),
        Outcome::Failed("boom".into()),
        None,
    )
}

fn done_with_pr(id: &str) -> Task {
    ended(
        at(task(id, forge_flow(), Some("GitHub")), 5, 700),
        Outcome::Done,
        None,
    )
}

fn titles(listed: &Listed<'_>) -> Vec<String> {
    listed
        .rows
        .iter()
        .map(|row| row.item.issue.title.clone())
        .collect()
}

fn with(progress: Progress) -> Filters {
    Filters {
        progress,
        ..Filters::default()
    }
}

fn none() -> HashMap<PathBuf, PrReads> {
    HashMap::new()
}

#[test]
fn twenty_issues_across_two_projects_tell_apart_what_needs_the_person() {
    let (run, appr, fail) = (running(), waiting_approval(), failed());
    let queued = task("queued", forge_flow(), Some("GitHub"));
    let mut issues = Vec::new();
    for n in 1..=20u64 {
        let root = if n % 2 == 0 { A } else { B };
        let work = match n % 5 {
            0 => Some(work_of(root, n, &[(&run, Some(Working::Running))])),
            1 => Some(work_of(root, n, &[(&appr, Some(Working::Waiting))])),
            2 => Some(work_of(root, n, &[(&fail, None)])),
            3 => Some(work_of(root, n, &[(&queued, Some(Working::Queued))])),
            _ => None,
        };
        issues.push((root, issue(n, &format!("issue {n}"), true, n), work));
    }
    let fixture = Fixture { issues };
    let items = fixture.items();
    let count = |progress| {
        list(&items, &with(progress), &none(), &Held::default(), 1_000)
            .rows
            .len()
    };
    assert_eq!(count(Progress::All), 20);
    // Waiting for approval and ended on a failure, four of each.
    assert_eq!(count(Progress::NeedsAttention), 8);
    assert_eq!(count(Progress::Running), 4);
    assert_eq!(count(Progress::Queued), 4);
    assert_eq!(count(Progress::NoRunRecorded), 4);

    let all = list(
        &items,
        &Filters::default(),
        &none(),
        &Held::default(),
        1_000,
    );
    let row = |n: u64| {
        all.rows
            .iter()
            .find(|row| row.item.issue.number == n)
            .unwrap()
    };
    assert_eq!(row(1).line.as_deref(), Some("Waiting for approval"));
    assert!(row(1).attention);
    assert_eq!(row(5).line.as_deref(), Some("Running · Implement"));
    assert!(!row(5).attention);
    assert_eq!(row(2).line.as_deref(), Some("Ended · failed"));
    assert!(row(2).attention);
    // No run recorded draws no line, so rows nobody works stay quiet.
    assert_eq!(row(4).line, None);
    assert!(!row(4).attention);
}

#[test]
fn an_older_task_needing_attention_puts_the_issue_under_needs_attention() {
    let older = failed();
    let mut newer = done_with_pr("newer");
    // The newer moved last, so it is the issue's work.
    newer.runs[0].history[0].at = 2_000;
    let work = work_of(A, 1, &[(&older, None), (&newer, None)]);
    let fixture = Fixture {
        issues: vec![(A, issue(1, "one", true, 1), Some(work))],
    };
    let items = fixture.items();
    let listed = list(
        &items,
        &with(Progress::NeedsAttention),
        &none(),
        &Held::default(),
        1,
    );
    assert_eq!(titles(&listed), ["one"]);
    let row = &listed.rows[0];
    assert_eq!(row.line.as_deref(), Some("Done"));
    assert!(!row.attention);
    assert!(row.earlier_attention);
}

#[test]
fn a_run_waiting_on_status_checks_is_running_not_needing_attention() {
    let checks = at(task("checks", forge_flow(), Some("GitHub")), 6, 900);
    let fixture = Fixture {
        issues: vec![(
            A,
            issue(1, "one", true, 1),
            Some(work_of(A, 1, &[(&checks, Some(Working::Running))])),
        )],
    };
    let items = fixture.items();
    let count = |progress| {
        list(&items, &with(progress), &none(), &Held::default(), 1)
            .rows
            .len()
    };
    assert_eq!(count(Progress::Running), 1);
    assert_eq!(count(Progress::NeedsAttention), 0);
}

#[test]
fn an_approved_issue_stays_saying_where_it_went_until_another_is_picked() {
    let appr = waiting_approval();
    let fixture = Fixture {
        issues: vec![
            (
                A,
                issue(1, "one", true, 2),
                Some(work_of(A, 1, &[(&appr, Some(Working::Waiting))])),
            ),
            (
                A,
                issue(2, "two", true, 1),
                Some(work_of(A, 2, &[(&appr, Some(Working::Waiting))])),
            ),
        ],
    };
    let items = fixture.items();
    let filters = with(Progress::NeedsAttention);
    let first = list(&items, &filters, &none(), &Held::default(), 1);
    assert_eq!(titles(&first), ["one", "two"]);

    // Issue one is approved: its task runs now.
    let run = running();
    let approved = Fixture {
        issues: vec![
            (
                A,
                issue(1, "one", true, 2),
                Some(work_of(A, 1, &[(&run, Some(Working::Running))])),
            ),
            fixture.issues[1].clone(),
        ],
    };
    let items = approved.items();
    let held = Held {
        order: first.order(),
        pinned: None,
    };
    let now = list(&items, &filters, &none(), &held, 1);
    assert_eq!(titles(&now), ["one", "two"]);
    assert_eq!(now.rows[0].left.as_deref(), Some("now Running · Implement"));
    assert_eq!(now.rows[1].left, None);

    // Another is picked: the row that left goes, and nothing else moves.
    // Picking the row that left keeps it, selected where it is.
    let mut held = Held {
        order: now.order(),
        pinned: None,
    };
    held.pick(&key(A, 1), &now.stopped_matching());
    let kept = list(&items, &filters, &none(), &held, 1);
    assert_eq!(titles(&kept), ["one", "two"]);
    assert_eq!(
        kept.rows[0].left.as_deref(),
        Some("now Running · Implement")
    );

    // Another is picked: the row that left goes, and nothing else moves.
    held.pick(&key(A, 2), &kept.stopped_matching());
    assert_eq!(titles(&list(&items, &filters, &none(), &held, 1)), ["two"]);
}

#[test]
fn a_held_row_keeps_its_place_and_a_new_match_lands_below() {
    let fixture = Fixture {
        issues: vec![
            (A, issue(1, "one", true, 1), None),
            (A, issue(2, "two", true, 2), None),
        ],
    };
    let items = fixture.items();
    let first = list(&items, &Filters::default(), &none(), &Held::default(), 1);
    assert_eq!(titles(&first), ["two", "one"]);

    // One changes last and a third arrives, newer than both.
    let moved = Fixture {
        issues: vec![
            (A, issue(1, "one", true, 9), None),
            (A, issue(2, "two", true, 2), None),
            (B, issue(3, "three", true, 10), None),
        ],
    };
    let items = moved.items();
    let held = Held {
        order: first.order(),
        pinned: None,
    };
    let listed = list(&items, &Filters::default(), &none(), &held, 1);
    assert_eq!(titles(&listed), ["two", "one", "three"]);
}

#[test]
fn an_issue_the_filters_leave_out_is_pinned_without_changing_them() {
    let fixture = Fixture {
        issues: vec![
            (A, issue(1, "closed in a", false, 1), None),
            (B, issue(2, "open in b", true, 2), None),
        ],
    };
    let items = fixture.items();
    let filters = Filters {
        closed: true,
        project: Some(PathBuf::from(A)),
        ..Filters::default()
    };
    let held = Held {
        order: Vec::new(),
        pinned: Some(key(B, 2)),
    };
    let listed = list(&items, &filters, &none(), &held, 1);
    assert_eq!(titles(&listed), ["open in b", "closed in a"]);
    assert!(listed.rows[0].outside);
    assert!(!listed.rows[1].outside);
    // The order never holds the pin; picking the pinned issue keeps it,
    // picking another lets it go.
    assert_eq!(listed.order(), [key(A, 1)]);
    let mut held = Held {
        order: listed.order(),
        pinned: Some(key(B, 2)),
    };
    held.pick(&key(B, 2), &listed.stopped_matching());
    assert_eq!(held.pinned, Some(key(B, 2)));
    held.pick(&key(A, 1), &listed.stopped_matching());
    assert_eq!(held.pinned, None);
}

#[test]
fn an_issue_pinned_that_matches_is_not_drawn_twice() {
    let fixture = Fixture {
        issues: vec![(A, issue(1, "one", true, 1), None)],
    };
    let items = fixture.items();
    let held = Held {
        order: Vec::new(),
        pinned: Some(key(A, 1)),
    };
    let listed = list(&items, &Filters::default(), &none(), &held, 1);
    assert_eq!(titles(&listed), ["one"]);
    assert!(!listed.rows[0].outside);
}

#[test]
fn an_open_pull_request_is_its_own_filter_and_a_partial_read_is_said() {
    let tasks: Vec<Task> = (1..=3)
        .map(|n| {
            let mut done = done_with_pr(&format!("t{n}"));
            done.setup.branch = Some(format!("onehand/b{n}"));
            done.runs[0].setup.branch = done.setup.branch.clone();
            done
        })
        .collect();
    let fixture = Fixture {
        issues: tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let n = i as u64 + 1;
                let root = if n == 3 { B } else { A };
                (
                    root,
                    issue(n, &format!("issue {n}"), true, n),
                    Some(work_of(root, n, &[(t, None)])),
                )
            })
            .collect(),
    };
    let items = fixture.items();
    let mut reads = HashMap::new();
    // A's read reached its cap: it found b1, and b2 is missing from it.
    reads.insert(
        PathBuf::from(A),
        PrReads {
            at: Some(1_000),
            capped: true,
            by_branch: HashMap::from([("onehand/b1".to_string(), pr(PrState::Open, false))]),
            ..PrReads::default()
        },
    );
    // B's read failed, after an earlier one had found b3 open.
    reads.insert(
        PathBuf::from(B),
        PrReads {
            at: Some(1_000),
            failed: Some("gh is not signed in".into()),
            by_branch: HashMap::from([("onehand/b3".to_string(), pr(PrState::Open, false))]),
            ..PrReads::default()
        },
    );
    let listed = list(
        &items,
        &with(Progress::PullRequestOpen),
        &reads,
        &Held::default(),
        1_010,
    );
    assert_eq!(titles(&listed), ["issue 1"]);
    assert_eq!(
        listed.incomplete,
        Some(Incomplete {
            not_read: 2,
            stale: false
        })
    );
    assert_eq!(listed.failed, [PathBuf::from(B)]);
    // The row read draws its pull request; one not read draws none.
    let all = list(&items, &Filters::default(), &reads, &Held::default(), 1_010);
    let line = |n: u64| {
        all.rows
            .iter()
            .find(|row| row.item.issue.number == n)
            .unwrap()
            .line
            .clone()
    };
    assert_eq!(line(1).as_deref(), Some("Done · pull request open"));
    assert_eq!(line(2).as_deref(), Some("Done"));
    // A failed read draws no pull request line, even one read before.
    assert_eq!(line(3).as_deref(), Some("Done"));
    // An open pull request waits on the forge, not on the person.
    assert_eq!(
        list(
            &items,
            &with(Progress::NeedsAttention),
            &reads,
            &Held::default(),
            1_010
        )
        .rows
        .len(),
        0
    );

    // Looked up past the cap, b2 is read; the lookups have a cap of their own.
    let wanted = reads[Path::new(A)].to_look_up(["onehand/b1", "onehand/b2", "onehand/b2"], 5);
    assert_eq!(wanted, ["onehand/b2"]);
    assert!(
        reads[Path::new(A)]
            .to_look_up(["onehand/b2", "onehand/x"], 1)
            .len()
            == 1
    );
    reads
        .get_mut(Path::new(A))
        .unwrap()
        .looked_up
        .insert("onehand/b2".into(), Some(pr(PrState::Open, true)));
    reads.get_mut(Path::new(B)).unwrap().failed = None;
    let listed = list(
        &items,
        &with(Progress::PullRequestOpen),
        &reads,
        &Held::default(),
        1_010,
    );
    assert_eq!(titles(&listed), ["issue 3", "issue 2", "issue 1"]);
    assert_eq!(listed.incomplete, None);

    // A reading older than a minute is not trusted to be complete.
    let listed = list(
        &items,
        &with(Progress::PullRequestOpen),
        &reads,
        &Held::default(),
        1_000 + READ_AGE,
    );
    assert_eq!(
        listed.incomplete,
        Some(Incomplete {
            not_read: 0,
            stale: true
        })
    );
}

#[test]
fn the_cap_says_what_it_left_out() {
    let fixture = Fixture {
        issues: (1..=LIST_CAP as u64 + 3)
            .map(|n| (A, issue(n, "x", true, n), None))
            .collect(),
    };
    let items = fixture.items();
    let listed = list(&items, &Filters::default(), &none(), &Held::default(), 1);
    assert_eq!(listed.rows.len(), LIST_CAP);
    assert_eq!(listed.left_out, 3);
}

#[test]
fn the_switch_counts_open_and_closed_through_the_other_filters() {
    let mut labelled = issue(3, "bug", true, 3);
    labelled.labels = vec!["bug".into()];
    let fixture = Fixture {
        issues: vec![
            (A, issue(1, "open", true, 1), None),
            (A, issue(2, "closed", false, 2), None),
            (B, labelled, None),
        ],
    };
    let items = fixture.items();
    let listed = list(&items, &Filters::default(), &none(), &Held::default(), 1);
    assert_eq!((listed.open, listed.closed), (2, 1));
    let closed = Filters {
        closed: true,
        ..Filters::default()
    };
    assert_eq!(
        titles(&list(&items, &closed, &none(), &Held::default(), 1)),
        ["closed"]
    );
    let bugs = Filters {
        label: Some("bug".into()),
        ..Filters::default()
    };
    let listed = list(&items, &bugs, &none(), &Held::default(), 1);
    assert_eq!((listed.open, listed.closed), (1, 0));
    let in_a = Filters {
        project: Some(PathBuf::from(A)),
        query: "OPE".into(),
        ..Filters::default()
    };
    assert_eq!(
        titles(&list(&items, &in_a, &none(), &Held::default(), 1)),
        ["open"]
    );
}
