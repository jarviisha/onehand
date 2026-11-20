use super::*;
use crate::project::{MadeBy, Project, ProjectId, Projects, Worktree};
use crate::worktree::Listed;
use std::path::{Path, PathBuf};

fn recorded(path: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: "x".into(),
        from: None,
        made_by: MadeBy::Split,
        offload: None,
    }
}

fn listed(path: &str, branch: &str) -> Listed {
    Listed {
        path: path.into(),
        branch: Some(branch.into()),
    }
}

/// Every folder is there.
fn there(_: &Path) -> bool {
    true
}

#[test]
fn a_recorded_worktree_git_no_longer_lists_is_missing() {
    let mut p = Project::new(ProjectId::mint(1), None, "/r".into());
    p.worktrees.push(recorded("/r-x"));
    p.worktrees.push(recorded("/r-gone"));
    let git = [listed("/r", "main"), listed("/r-x", "x")];
    assert_eq!(
        rows(&p, Some(&git), &Projects::default(), there),
        [
            WorktreeRow::Recorded {
                at: 0,
                on_disk: true
            },
            WorktreeRow::Recorded {
                at: 1,
                on_disk: false
            },
        ]
    );
    assert_eq!(
        rows(&p, None, &Projects::default(), there),
        [
            WorktreeRow::Recorded {
                at: 0,
                on_disk: true
            },
            WorktreeRow::Recorded {
                at: 1,
                on_disk: true
            },
        ],
        "git unread: drawn as recorded"
    );
}

#[test]
fn a_listed_checkout_no_project_records_is_offered() {
    let p = Project::new(ProjectId::mint(1), None, "/r".into());
    let git = [listed("/r", "main"), listed("/r-y", "y")];
    assert_eq!(
        rows(&p, Some(&git), &Projects::default(), there),
        [WorktreeRow::Offered(listed("/r-y", "y"))]
    );
    let mut other = Project::new(ProjectId::mint(2), None, "/elsewhere".into());
    other.worktrees.push(recorded("/r-y"));
    assert!(
        rows(&p, Some(&git), &Projects::from_iter([other]), there).is_empty(),
        "another project records it"
    );
}

#[test]
fn a_shared_repository_offers_each_project_its_own() {
    let front = Project::new(ProjectId::mint(1), None, "/r/front".into());
    let back = Project::new(ProjectId::mint(2), None, "/r/back".into());
    let git = [listed("/r", "main"), listed("/r-y", "y")];
    let holds_front = |path: &Path| path == Path::new("/r-y/front");
    assert_eq!(
        rows(
            &front,
            Some(&git),
            &Projects::from_iter([back.clone()]),
            holds_front
        ),
        [WorktreeRow::Offered(Listed {
            path: PathBuf::from("/r-y/front"),
            branch: Some("y".into())
        })],
        "its own sub-folder of the checkout"
    );
    assert!(
        rows(
            &back,
            Some(&git),
            &Projects::from_iter([front]),
            holds_front
        )
        .is_empty(),
        "a checkout without the project's sub-folder is not offered"
    );
}
