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
fn issues_survive_a_round_trip_and_a_missing_file_is_empty() {
    let dir = std::env::temp_dir().join(format!("onehand-issues-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The workspace's folder is there; its issues folder is not yet.
    std::fs::create_dir_all(&dir).unwrap();
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

#[test]
fn a_label_is_told_by_what_it_asks_for() {
    assert_eq!(LabelKind::of("ready-for-agent"), LabelKind::Action);
    assert_eq!(LabelKind::of("Needs-Triage"), LabelKind::Action);
    assert_eq!(LabelKind::of("auto"), LabelKind::Action);
    assert_eq!(LabelKind::of("enhancement"), LabelKind::Enhancement);
    assert_eq!(LabelKind::of("agent-report"), LabelKind::Report);
    assert_eq!(LabelKind::of("architecture"), LabelKind::Structure);
    assert_eq!(LabelKind::of("Refactor"), LabelKind::Structure);
    assert_eq!(LabelKind::of("documentation"), LabelKind::Docs);
    assert_eq!(LabelKind::of("bug"), LabelKind::Category);
}

/// **One project's slow write does not hold another's.** A sync holds its
/// file while it asks the forge, which can take minutes; with one lock for
/// every file, editing an issue in any other project waited for it too.
#[test]
fn a_write_to_one_file_does_not_wait_on_another() {
    let dir = std::env::temp_dir().join(format!("onehand-issues-locks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (slow, quick) = (dir.join("slow.json"), dir.join("quick.json"));
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let slow_writer = std::thread::spawn(move || {
        update_blocking(&slow, |_| {
            held.send(()).unwrap();
            // Held until the quick write has been seen to finish, or given up on.
            let _ = released.recv();
            Ok(())
        })
        .unwrap();
    });
    holding.recv().unwrap();

    // The quick write runs while the slow file is still held: with one lock
    // for both it never finishes, so it is waited for with a bound far past
    // any write, rather than timed against one.
    let (done, finished) = std::sync::mpsc::channel();
    let quick_writer = std::thread::spawn(move || {
        let _ = done.send(update_blocking(&quick, |_| Ok(())).map(|_| ()));
    });
    let quick_result = finished.recv_timeout(std::time::Duration::from_secs(10));
    release.send(()).unwrap();
    slow_writer.join().unwrap();
    quick_writer.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    quick_result
        .expect("the quick write waited on the slow one's file")
        .unwrap();
}

#[test]
fn an_open_issue_with_the_same_title_is_not_written_twice() {
    use drive::{write_blocking, Applied, Change};
    let dir = std::env::temp_dir().join(format!("onehand-issues-unless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("issues.json");
    let unless = |title: &str| Change::CreateUnlessOpen(draft(title));
    let (kept, first) = write_blocking(&file, unless("Crash on open"), 1).unwrap();
    assert_eq!(first, Applied::New(1));
    let number = 1;
    // Case and spaces aside, the same title: nothing is written.
    assert_eq!(
        write_blocking(&file, unless("  crash ON open "), 2)
            .unwrap()
            .1,
        Applied::Already(number)
    );
    assert_eq!(load_blocking(&file).unwrap().revision(), kept.revision());
    // Closed, it no longer counts.
    update_blocking(&file, |kept| {
        kept.issues.iter_mut().for_each(|i| i.open = false);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        write_blocking(&file, unless("Crash on open"), 3).unwrap().1,
        Applied::New(2)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_blank_title_is_refused_and_nothing_is_written() {
    let dir = std::env::temp_dir().join(format!("onehand-issues-blank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("issues.json");
    let blank = drive::Change::CreateUnlessOpen(draft("   "));
    assert!(drive::write_blocking(&file, blank, 1).is_err());
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A write that lands after the folder holding its file's folder is gone
/// does not put it back: it fails instead.
#[test]
fn a_write_into_a_deleted_workspace_puts_nothing_back() {
    let storage =
        std::env::temp_dir().join(format!("onehand-issues-deleted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    let file = storage.join("p").join("issues.json");
    assert!(update_blocking(&file, |_| Ok(())).is_err());
    assert!(!storage.exists());
}

/// A lead's `start_issue` brief: an open issue kept here is named as one,
/// and a closed, missing or unkept one is refused.
#[test]
fn a_supervised_brief_names_an_open_issue_kept_here() {
    assert!(supervised_brief_blocking(None, None, 1).is_err());
    let dir = std::env::temp_dir().join(format!("onehand-brief-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("issues.json");
    let mut body = draft("Fix it");
    body.body = " Do the thing. ".into();
    update_blocking(&file, |issues| issues.create(body, 1)).unwrap();
    update_blocking(&file, |issues| issues.create(draft("Old"), 1)).unwrap();
    update_blocking(&file, |issues| issues.set_open(2, false, 2)).unwrap();
    let brief = supervised_brief_blocking(Some(file.clone()), None, 1).unwrap();
    assert_eq!(brief.title, "Fix it");
    assert_eq!(brief.body, "Do the thing.");
    assert_eq!(
        brief.instructions.as_deref(),
        Some("This is issue #1, which is kept in onehand rather than on a forge.")
    );
    assert!(supervised_brief_blocking(Some(file.clone()), None, 2).is_err());
    assert!(supervised_brief_blocking(Some(file), None, 9).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

// Merging one project's files into one.

fn local(number: u64, title: &str) -> LocalIssue {
    LocalIssue {
        number,
        title: title.into(),
        body: String::new(),
        open: true,
        labels: Vec::new(),
        created: 1,
        updated: 1,
        notes: Vec::new(),
        link: None,
        imported_from: None,
    }
}

fn linked(number: u64, connector: &str, key: &str, updated: u64, title: &str) -> LocalIssue {
    LocalIssue {
        updated,
        link: Some(Link {
            connector: connector.into(),
            key: key.into(),
            reference: format!("#{key}"),
            base: Snapshot::default(),
            conflict: None,
        }),
        imported_from: Some(connector.into()),
        ..local(number, title)
    }
}

fn issues_with(issues: Vec<LocalIssue>) -> Issues {
    let next = issues.iter().map(|i| i.number).max().unwrap_or(0) + 1;
    Issues {
        next,
        issues,
        ..Default::default()
    }
}

#[test]
fn a_forge_issue_kept_in_two_files_is_kept_once_the_newer() {
    let mut into = issues_with(vec![linked(1, "gh", "42", 100, "old title")]);
    let from = issues_with(vec![linked(7, "gh", "42", 200, "new title")]);
    assert!(merge(&mut into, from, Path::new("/ws/issues/a-x.json")));
    assert_eq!(into.issues.len(), 1);
    assert_eq!(
        (into.issues[0].number, into.issues[0].title.as_str()),
        (1, "new title")
    );
    // The older copy coming second changes nothing.
    let older = issues_with(vec![linked(3, "gh", "42", 50, "stale")]);
    merge(&mut into, older, Path::new("/ws2/issues/a.json"));
    assert_eq!(into.issues[0].title, "new title");
}

#[test]
fn a_local_issue_keeps_its_number_where_free_and_takes_the_next_where_not() {
    let mut into = issues_with(vec![local(1, "a"), local(2, "b")]);
    let from = issues_with(vec![local(2, "c"), local(5, "d")]);
    merge(&mut into, from, Path::new("/ws/issues/a-x.json"));
    let numbers: Vec<_> = into
        .issues
        .iter()
        .map(|i| (i.number, i.title.clone()))
        .collect();
    assert_eq!(
        numbers,
        vec![
            (1, "a".to_string()),
            (2, "b".to_string()),
            (3, "c".to_string()),
            (5, "d".to_string())
        ]
    );
    assert_eq!(into.next, 6);
}

#[test]
fn a_source_merged_once_is_not_merged_again() {
    let mut into = Issues::default();
    let source = Path::new("/ws/issues/a-x.json");
    assert!(merge(&mut into, issues_with(vec![local(1, "a")]), source));
    assert!(!merge(&mut into, issues_with(vec![local(1, "a")]), source));
    assert_eq!(into.issues.len(), 1);
}

#[test]
fn a_merge_keeps_the_forge_it_syncs_with() {
    let mut into = Issues::default();
    let mut from = issues_with(vec![]);
    from.synced_with = Some("gh".into());
    from.last_synced = 9;
    merge(&mut into, from, Path::new("/ws/issues/a.json"));
    assert_eq!(
        (into.synced_with.as_deref(), into.last_synced),
        (Some("gh"), 9)
    );
}

#[test]
fn a_projects_issues_file_is_in_its_folder() {
    use crate::project::file::{issues_file, store_dir};
    use crate::project::ProjectId;
    let id = ProjectId::mint(5);
    assert_eq!(
        issues_file(&id),
        store_dir().join(id.as_str()).join("issues.json")
    );
}

#[test]
fn a_forge_issue_kept_twice_keeps_the_notes_of_both() {
    let note = |at: u64, text: &str| Note {
        at,
        text: text.into(),
        session: Some(format!("s{at}")),
    };
    let mut old = linked(1, "gh", "42", 100, "old");
    old.notes = vec![note(1, "taken up in the worktree")];
    let mut new = linked(4, "gh", "42", 200, "new");
    new.notes = vec![
        note(2, "taken up here"),
        note(1, "taken up in the worktree"),
    ];
    let mut into = issues_with(vec![old]);
    merge(
        &mut into,
        issues_with(vec![new]),
        Path::new("/ws/issues/a.json"),
    );
    let notes: Vec<_> = into.issues[0].notes.iter().map(|n| n.at).collect();
    assert_eq!(notes, vec![1, 2], "each note once, in order");
    assert_eq!(into.issues[0].title, "new");
}

#[test]
fn a_file_is_listed_under_the_first_folder_of_it() {
    let folders = vec![
        (PathBuf::from("/a"), PathBuf::from("/p/1.json")),
        (PathBuf::from("/b"), PathBuf::from("/p/2.json")),
        (PathBuf::from("/a-x"), PathBuf::from("/p/1.json")),
    ];
    let under = listed_under(&folders);
    assert_eq!(under[Path::new("/a")], Path::new("/a"));
    assert_eq!(
        under[Path::new("/a-x")],
        Path::new("/a"),
        "a worktree stands for its project's row"
    );
    assert_eq!(under[Path::new("/b")], Path::new("/b"));
}
