use super::*;
use crate::project::testing::{git, repo_with_worktree};

fn facts(users: Vec<String>, shared_with: Vec<String>) -> OwnFacts {
    OwnFacts {
        folder: "/r-x".into(),
        branch: Some("x".into()),
        uncommitted: Ok(Vec::new()),
        users,
        shared_with,
        tasks: Vec::new(),
    }
}

#[test]
fn a_clean_unused_worktree_is_removed() {
    let (main, wt) = repo_with_worktree("own-clean");
    let facts = own_facts_blocking(&wt, Vec::new(), Vec::new(), Vec::new());
    assert_eq!(facts.branch.as_deref(), Some("w"));
    assert!(matches!(
        judge_own(&facts),
        Judged::Remove { branch: Some(b), .. } if b == "w"
    ));
    let _ = std::fs::remove_dir_all(main.parent().unwrap());
}

#[test]
fn a_worktree_in_use_is_not_removed() {
    let Judged::Refused(why) = judge_own(&facts(vec!["Session “Fix it”".into()], Vec::new()))
    else {
        panic!("refused");
    };
    assert_eq!(why, ["Session “Fix it” uses the folder."]);
    let mut dirty = facts(Vec::new(), Vec::new());
    dirty.uncommitted = Ok(vec![" M a.txt".into()]);
    assert!(matches!(judge_own(&dirty), Judged::Refused(_)));
}

#[test]
fn a_checkout_another_project_records_is_not_removed() {
    let Judged::Refused(why) = judge_own(&facts(Vec::new(), vec!["back".into()])) else {
        panic!("refused");
    };
    assert_eq!(why, ["The project back has a folder in this checkout."]);
}

#[test]
fn an_unmerged_branch_stays_when_its_worktree_goes() {
    let (main, wt) = repo_with_worktree("own-unmerged");
    std::fs::write(wt.join("b.txt"), "y").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "two"]);
    assert!(!branch_merged_blocking(&main, "w"));
    let removed = remove_own_blocking(&main, &wt, Some("w")).unwrap();
    assert!(!wt.exists());
    assert!(removed.branch_kept.is_some());
    assert!(crate::worktree::branch_exists_blocking(&main, "w"));
    let _ = std::fs::remove_dir_all(main.parent().unwrap());
}

#[test]
fn a_merged_branch_goes_with_its_worktree() {
    let (main, wt) = repo_with_worktree("own-merged");
    assert!(branch_merged_blocking(&main, "w"));
    let removed = remove_own_blocking(&main, &wt, Some("w")).unwrap();
    assert_eq!(removed.branch_kept, None);
    assert!(!crate::worktree::branch_exists_blocking(&main, "w"));
    let _ = std::fs::remove_dir_all(main.parent().unwrap());
}

#[test]
fn a_worktree_a_task_keeps_is_not_removed() {
    let mut kept = facts(Vec::new(), Vec::new());
    kept.tasks = vec!["Fix the login".into()];
    let Judged::Refused(why) = judge_own(&kept) else {
        panic!("refused");
    };
    assert_eq!(
        why,
        ["Task “Fix the login” keeps this worktree: remove it from the task."]
    );
}
