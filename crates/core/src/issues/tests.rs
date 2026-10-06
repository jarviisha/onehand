use super::*;

fn draft(title: &str) -> Draft {
    Draft {
        title: title.to_string(),
        ..Draft::default()
    }
}

#[test]
fn open_across_lists_open_issues_newest_changed_first_and_counts_closed() {
    let mut a = Issues::default();
    a.create(draft("a1"), 10).unwrap();
    a.create(draft("a2"), 30).unwrap();
    a.create(draft("a3"), 5).unwrap();
    a.set_open(3, false, 40).unwrap();
    let mut b = Issues::default();
    b.create(draft("b1"), 20).unwrap();
    let files = vec![(PathBuf::from("/a"), a), (PathBuf::from("/b"), b)];

    let all = open_across(files.clone(), 10);
    let seen: Vec<(&str, u64)> = all
        .rows
        .iter()
        .map(|row| (row.title.as_str(), row.updated))
        .collect();
    // One row per open issue across both files, the closed one counted.
    assert_eq!(seen, [("a2", 30), ("b1", 20), ("a1", 10)]);
    assert_eq!(all.rows[1].root, PathBuf::from("/b"));
    assert_eq!((all.closed, all.left_out), (1, 0));

    let capped = open_across(files, 2);
    assert_eq!(capped.rows.len(), 2);
    assert_eq!(capped.left_out, 1);
}

#[test]
fn an_issue_is_named_by_its_forge_reference_and_a_draft_by_nothing() {
    let mut issues = Issues::default();
    let n = issues.create(draft("a"), 1).unwrap();
    assert_eq!(issues.get(n).unwrap().reference(), None);
    issues.find_mut(n).unwrap().link = Some(Link {
        connector: "GitHub".into(),
        key: "6".into(),
        reference: "#6".into(),
        base: Snapshot::default(),
        conflict: None,
    });
    // Filed here as #1, known everywhere as GitHub's #6.
    assert_eq!(issues.get(n).unwrap().reference(), Some("#6"));
    // Written here and published: it arrived here, not from the forge.
    assert_eq!(issues.get(n).unwrap().arrival(), "Opened here");
    issues.find_mut(n).unwrap().imported_from = Some("GitHub".into());
    assert_eq!(
        issues.get(n).unwrap().arrival(),
        "Brought in from GitHub as #6"
    );
}

#[test]
fn a_change_of_state_is_noted_once_with_its_time() {
    let mut issues = Issues::default();
    let n = issues.create(draft("a"), 1).unwrap();
    issues.set_open(n, false, 5).unwrap();
    issues.set_open(n, false, 6).unwrap();
    issues.set_open(n, true, 9).unwrap();
    let said: Vec<(u64, &str)> = issues
        .get(n)
        .unwrap()
        .notes
        .iter()
        .map(|note| (note.at, note.text.as_str()))
        .collect();
    assert_eq!(said, [(5, "Closed"), (9, "Reopened")]);
}

#[test]
fn a_session_taking_an_issue_up_is_kept_with_it() {
    let mut issues = Issues::default();
    let n = issues.create(draft("a"), 1).unwrap();
    issues.taken_up(n, "Worked here", "s-1".into(), 4).unwrap();
    let note = issues.get(n).unwrap().notes.last().unwrap().clone();
    assert_eq!((note.at, note.session.as_deref()), (4, Some("s-1")));
    // A note with no session is written without the field, so a file from
    // before it existed reads back the same.
    let text = serde_json::to_string(&issues).unwrap();
    assert_eq!(text.matches("\"session\"").count(), 1);
}

#[test]
fn working_here_keeps_to_the_checkout_and_commits_nothing() {
    let mut issue = Issues::default();
    let n = issue.create(draft("Fix the footer"), 1).unwrap();
    let prompt = work_here_prompt(issue.get(n).unwrap());
    assert!(prompt.contains("Title: Fix the footer"));
    assert!(prompt.contains("Do not create a branch or a worktree"));
    assert!(prompt.contains("do not commit, push or open a pull request"));
}

#[test]
fn issues_are_numbered_from_one_and_never_reuse_a_number() {
    let mut issues = Issues::default();
    assert_eq!(issues.create(draft("a"), 10), Ok(1));
    assert_eq!(issues.create(draft("b"), 11), Ok(2));
    // A list whose counter was lost still does not hand out a taken number.
    issues.next = 0;
    assert_eq!(issues.create(draft("c"), 12), Ok(3));
}

#[test]
fn an_issue_needs_a_title_and_is_kept_trimmed() {
    let mut issues = Issues::default();
    assert!(issues.create(draft("   "), 1).is_err());
    let n = issues
        .create(
            Draft {
                title: "  Crash on open ".into(),
                body: "\n steps \n".into(),
                labels: vec!["bug".into()],
            },
            1,
        )
        .unwrap();
    let issue = issues.get(n).unwrap();
    assert_eq!(
        (issue.title.as_str(), issue.body.as_str()),
        ("Crash on open", "steps")
    );
    assert!(issue.open);
    assert!(
        issues.edit(n, draft(""), 2).is_err(),
        "an edit cannot clear the title"
    );
    assert_eq!(issues.get(n).unwrap().title, "Crash on open");
}

#[test]
fn an_edit_and_a_close_stamp_the_issue() {
    let mut issues = Issues::default();
    let n = issues.create(draft("a"), 1).unwrap();
    issues.edit(n, draft("b"), 5).unwrap();
    assert_eq!(issues.get(n).unwrap().updated, 5);
    issues.set_open(n, false, 9).unwrap();
    assert!(!issues.get(n).unwrap().open);
    assert_eq!(issues.get(n).unwrap().updated, 9);
    // Closing what is already closed changes nothing, the stamp included.
    issues.set_open(n, false, 20).unwrap();
    assert_eq!(issues.get(n).unwrap().updated, 9);
    assert!(issues.set_open(99, true, 1).is_err());
}

#[test]
fn the_list_puts_open_work_first_and_newest_first() {
    let mut issues = Issues::default();
    for title in ["1", "2", "3", "4"] {
        issues.create(draft(title), 1).unwrap();
    }
    issues.set_open(4, false, 2).unwrap();
    issues.set_open(1, false, 2).unwrap();
    let order: Vec<u64> = issues.listed().iter().map(|i| i.number).collect();
    assert_eq!(order, [3, 2, 4, 1]);
}

#[test]
fn a_note_is_added_in_order_and_stamps_the_issue() {
    let mut issues = Issues::default();
    let n = issues.create(draft("a"), 1).unwrap();
    issues.note(n, "  started  ", 4).unwrap();
    issues.note(n, "finished", 7).unwrap();
    let issue = issues.get(n).unwrap();
    let said: Vec<&str> = issue.notes.iter().map(|note| note.text.as_str()).collect();
    assert_eq!(said, ["started", "finished"]);
    assert_eq!((issue.notes[1].at, issue.updated), (7, 7));
    assert!(
        issues.note(n, "   ", 9).is_err(),
        "an empty note says nothing"
    );
    assert!(issues.note(99, "x", 9).is_err());
}

#[test]
fn a_label_comes_off_once_and_only_where_it_was() {
    let mut issues = Issues::default();
    let n = issues
        .create(
            Draft {
                title: "a".into(),
                labels: vec!["auto".into(), "ui".into()],
                ..Draft::default()
            },
            1,
        )
        .unwrap();
    issues.remove_label(n, "auto", 3).unwrap();
    assert_eq!(issues.get(n).unwrap().labels, ["ui"]);
    assert_eq!(issues.get(n).unwrap().updated, 3);
    issues.remove_label(n, "auto", 8).unwrap();
    assert_eq!(
        issues.get(n).unwrap().updated,
        3,
        "nothing came off, nothing changed"
    );
}

#[test]
fn labels_are_split_on_commas_trimmed_and_said_once() {
    assert_eq!(parse_labels("bug, ui , bug,,  "), ["bug", "ui"]);
    assert!(parse_labels("").is_empty());
}

#[test]
fn the_file_is_per_project_under_the_workspace_storage() {
    let storage = Path::new("/store");
    let a = file_for(storage, Path::new("/code/app"));
    let b = file_for(storage, Path::new("/other/app"));
    assert!(a.starts_with("/store/issues"));
    assert_ne!(a, b, "two checkouts with one folder name are two projects");
    assert_eq!(a, file_for(storage, Path::new("/code/app")));
}

#[test]
fn issues_survive_a_round_trip_and_a_missing_file_is_empty() {
    let dir = std::env::temp_dir().join(format!("onehand-issues-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("issues").join("p.json");
    assert_eq!(load_blocking(&file), Ok(Issues::default()));

    let (kept, n) = update_blocking(&file, |issues| issues.create(draft("a"), 1)).unwrap();
    assert_eq!(n, 1);
    assert_eq!(load_blocking(&file), Ok(kept));

    // A change that fails leaves what was there.
    assert!(update_blocking(&file, |issues| issues.edit(9, draft("x"), 2)).is_err());
    assert_eq!(load_blocking(&file).unwrap().get(1).unwrap().title, "a");

    // A file this build cannot read is refused and never written over.
    std::fs::write(&file, "not json").unwrap();
    assert!(update_blocking(&file, |issues| issues.create(draft("b"), 3)).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not json");
    let _ = std::fs::remove_dir_all(&dir);
}
