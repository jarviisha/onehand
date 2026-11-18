use super::*;

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

/// What the test holds: the issues, owned, so items can borrow them.
struct Fixture {
    issues: Vec<(&'static str, LocalIssue)>,
}

impl Fixture {
    fn items(&self) -> Vec<Item<'_>> {
        self.issues
            .iter()
            .map(|(root, issue)| Item {
                root: Path::new(root),
                key: key(root, issue.number),
                issue,
            })
            .collect()
    }
}

fn titles(listed: &Listed<'_>) -> Vec<String> {
    listed
        .rows
        .iter()
        .map(|row| row.item.issue.title.clone())
        .collect()
}

/// An issue closed while the open ones are listed stays, saying where it
/// went, until another is picked.
#[test]
fn a_closed_issue_stays_saying_where_it_went_until_another_is_picked() {
    let fixture = Fixture {
        issues: vec![(A, issue(1, "one", true, 2)), (A, issue(2, "two", true, 1))],
    };
    let items = fixture.items();
    let first = list(&items, &Filters::default(), &Held::default());
    assert_eq!(titles(&first), ["one", "two"]);
    let closed = Fixture {
        issues: vec![
            (A, issue(1, "one", false, 3)),
            (A, issue(2, "two", true, 1)),
        ],
    };
    let items = closed.items();
    let mut held = Held {
        order: first.order(),
        pinned: None,
    };
    let now = list(&items, &Filters::default(), &held);
    assert_eq!(titles(&now), ["one", "two"]);
    assert_eq!(now.rows[0].left.as_deref(), Some("now closed"));
    held.pick(&key(A, 2), &now.stopped_matching());
    assert_eq!(titles(&list(&items, &Filters::default(), &held)), ["two"]);
}

#[test]
fn a_held_row_keeps_its_place_and_a_new_match_lands_below() {
    let fixture = Fixture {
        issues: vec![(A, issue(1, "one", true, 1)), (A, issue(2, "two", true, 2))],
    };
    let items = fixture.items();
    let first = list(&items, &Filters::default(), &Held::default());
    assert_eq!(titles(&first), ["two", "one"]);

    // One changes last and a third arrives, newer than both.
    let moved = Fixture {
        issues: vec![
            (A, issue(1, "one", true, 9)),
            (A, issue(2, "two", true, 2)),
            (B, issue(3, "three", true, 10)),
        ],
    };
    let items = moved.items();
    let held = Held {
        order: first.order(),
        pinned: None,
    };
    let listed = list(&items, &Filters::default(), &held);
    assert_eq!(titles(&listed), ["two", "one", "three"]);
}

#[test]
fn an_issue_the_filters_leave_out_is_pinned_without_changing_them() {
    let fixture = Fixture {
        issues: vec![
            (A, issue(1, "closed in a", false, 1)),
            (B, issue(2, "open in b", true, 2)),
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
    let listed = list(&items, &filters, &held);
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
        issues: vec![(A, issue(1, "one", true, 1))],
    };
    let items = fixture.items();
    let held = Held {
        order: Vec::new(),
        pinned: Some(key(A, 1)),
    };
    let listed = list(&items, &Filters::default(), &held);
    assert_eq!(titles(&listed), ["one"]);
    assert!(!listed.rows[0].outside);
}

#[test]
fn the_cap_says_what_it_left_out() {
    let fixture = Fixture {
        issues: (1..=LIST_CAP as u64 + 3)
            .map(|n| (A, issue(n, "x", true, n)))
            .collect(),
    };
    let items = fixture.items();
    let listed = list(&items, &Filters::default(), &Held::default());
    assert_eq!(listed.rows.len(), LIST_CAP);
    assert_eq!(listed.left_out, 3);
}

#[test]
fn the_switch_counts_open_and_closed_through_the_other_filters() {
    let mut labelled = issue(3, "bug", true, 3);
    labelled.labels = vec!["bug".into()];
    let fixture = Fixture {
        issues: vec![
            (A, issue(1, "open", true, 1)),
            (A, issue(2, "closed", false, 2)),
            (B, labelled),
        ],
    };
    let items = fixture.items();
    let listed = list(&items, &Filters::default(), &Held::default());
    assert_eq!((listed.open, listed.closed), (2, 1));
    let closed = Filters {
        closed: true,
        ..Filters::default()
    };
    assert_eq!(titles(&list(&items, &closed, &Held::default())), ["closed"]);
    let bugs = Filters {
        label: Some("bug".into()),
        ..Filters::default()
    };
    let listed = list(&items, &bugs, &Held::default());
    assert_eq!((listed.open, listed.closed), (1, 0));
    let in_a = Filters {
        project: Some(PathBuf::from(A)),
        query: "OPE".into(),
        ..Filters::default()
    };
    assert_eq!(titles(&list(&items, &in_a, &Held::default())), ["open"]);
}
