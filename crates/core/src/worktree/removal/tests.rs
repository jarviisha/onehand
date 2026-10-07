use super::*;

fn merged() -> Facts {
    Facts {
        folder: "/w/fix-12".into(),
        branch: Some("onehand/12-fix".into()),
        uncommitted: Ok(Vec::new()),
        merged: Ok(Some(Merged {
            number: 40,
            head: "4f2c1e0aa".into(),
        })),
        past_merge: Ok(0),
        users: Vec::new(),
    }
}

fn refused(facts: &Facts) -> Vec<String> {
    match judge(facts) {
        Judged::Refused(why) => why,
        Judged::Remove { .. } => panic!("removed: {facts:?}"),
    }
}

#[test]
fn the_merged_head_removes_the_worktree_and_its_branch() {
    match judge(&merged()) {
        Judged::Remove { branch, why } => {
            assert_eq!(branch.as_deref(), Some("onehand/12-fix"));
            assert!(why.contains("#40") && why.contains("4f2c1e0"), "{why}");
        }
        Judged::Refused(why) => panic!("{why:?}"),
    }
}

/// A squash merge leaves the branch unreachable from its upstream, which
/// `git branch -d` refuses; judged by the forge's merged head, it goes. The
/// forge's branch being deleted after the merge is nothing the judgement
/// reads, so it removes all the same.
#[test]
fn a_squash_merge_and_a_deleted_remote_branch_still_remove() {
    assert!(matches!(judge(&merged()), Judged::Remove { .. }));
}

#[test]
fn commits_past_the_merged_head_refuse() {
    let facts = Facts {
        past_merge: Ok(2),
        ..merged()
    };
    let why = refused(&facts);
    assert!(why[0].contains("2 commits"), "{why:?}");
}

#[test]
fn a_forge_that_cannot_be_read_or_a_head_unknown_refuse() {
    let unread = Facts {
        merged: Err("gh is signed out".into()),
        ..merged()
    };
    assert!(refused(&unread)[0].contains("gh is signed out"));
    let not_merged = Facts {
        merged: Ok(None),
        ..merged()
    };
    assert!(refused(&not_merged)[0].contains("not merged"));
    let not_here = Facts {
        past_merge: Err("the merged commit is not in this clone".into()),
        ..merged()
    };
    assert!(refused(&not_here)[0].contains("not in this clone"));
}

#[test]
fn uncommitted_or_untracked_files_refuse_named_and_capped() {
    let files: Vec<String> = (0..8).map(|n| format!("?? new{n}.rs")).collect();
    let facts = Facts {
        uncommitted: Ok(files),
        ..merged()
    };
    let why = refused(&facts);
    assert!(
        why[0].contains("new0.rs") && why[0].contains("3 more"),
        "{why:?}"
    );
    let unread = Facts {
        uncommitted: Err("not a git repository".into()),
        ..merged()
    };
    assert!(refused(&unread)[0].contains("not a git repository"));
}

#[test]
fn each_user_of_the_folder_refuses_and_is_named() {
    let facts = Facts {
        users: vec![
            "A terminal in project fix-12, in another window".into(),
            "Task Fix the parser, running".into(),
        ],
        ..merged()
    };
    let why = refused(&facts);
    assert_eq!(why.len(), 2);
    assert!(why[0].contains("terminal") && why[1].contains("Fix the parser"));
}

#[test]
fn every_finding_is_listed() {
    let facts = Facts {
        uncommitted: Ok(vec![" M src/lib.rs".into()]),
        past_merge: Ok(1),
        users: vec!["A session in project fix-12".into()],
        ..merged()
    };
    assert_eq!(refused(&facts).len(), 3);
}

#[test]
fn a_detached_worktree_removes_the_folder_and_keeps_no_branch_to_delete() {
    let facts = Facts {
        branch: None,
        ..merged()
    };
    assert!(matches!(judge(&facts), Judged::Remove { branch: None, .. }));
}

/// A process is found by where it works now, wherever it was opened.
#[cfg(target_os = "linux")]
#[test]
fn a_process_working_inside_the_folder_is_found() {
    let here = std::env::current_dir().unwrap();
    let me = vec![Process {
        what: "A terminal in another window".into(),
        pid: std::process::id(),
    }];
    let found = working_in_blocking(&here, &me);
    assert_eq!(found, ["A terminal in another window, working in it,"]);
    assert!(working_in_blocking(Path::new("/nowhere-at-all"), &me).is_empty());
}
