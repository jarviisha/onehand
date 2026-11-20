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
                path: "/r-x".into(),
                on_disk: true
            },
            WorktreeRow::Recorded {
                path: "/r-gone".into(),
                on_disk: false
            },
        ]
    );
    assert_eq!(
        rows(&p, None, &Projects::default(), there),
        [
            WorktreeRow::Recorded {
                path: "/r-x".into(),
                on_disk: true
            },
            WorktreeRow::Recorded {
                path: "/r-gone".into(),
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

#[test]
fn projects_sharing_a_checkout_are_named() {
    let front = Project::new(ProjectId::mint(1), None, "/r/front".into());
    let mut back = Project::new(ProjectId::mint(2), None, "/r/back".into());
    back.name = "Back".into();
    back.worktrees.push(recorded("/r-y/back"));
    let projects = Projects::from_iter([front.clone(), back]);
    assert_eq!(
        projects.sharing(Path::new("/r-y"), Some(&front.id)),
        ["Back"]
    );
    assert!(projects
        .sharing(Path::new("/r-z"), Some(&front.id))
        .is_empty());
}

#[test]
fn a_project_folder_that_is_a_linked_checkout_is_never_offered_its_own() {
    // The project was added from the linked checkout `/r-x`; the main one
    // is never taken for a worktree, nor its own checkout.
    let p = Project::new(ProjectId::mint(1), None, "/r-x".into());
    let git = [
        listed("/r", "main"),
        listed("/r-x", "x"),
        listed("/r-y", "y"),
    ];
    assert_eq!(
        rows(&p, Some(&git), &Projects::default(), there),
        [WorktreeRow::Offered(listed("/r-y", "y"))]
    );
}

#[test]
fn a_detached_checkout_is_offered_on_no_branch() {
    let p = Project::new(ProjectId::mint(1), None, "/r".into());
    let git = [
        listed("/r", "main"),
        Listed {
            path: "/r-d".into(),
            branch: None,
        },
    ];
    assert_eq!(
        rows(&p, Some(&git), &Projects::default(), there),
        [WorktreeRow::Offered(Listed {
            path: "/r-d".into(),
            branch: None
        })]
    );
}

#[test]
fn a_worktree_inside_the_main_checkout_reads_by_its_own_top() {
    let mut p = Project::new(ProjectId::mint(1), None, "/r".into());
    p.worktrees.push(recorded("/r/.trees/x"));
    let git = [listed("/r", "main"), listed("/r/.trees/x", "x")];
    assert_eq!(
        rows(&p, Some(&git), &Projects::default(), there),
        [WorktreeRow::Recorded {
            path: "/r/.trees/x".into(),
            on_disk: true
        }]
    );
}
