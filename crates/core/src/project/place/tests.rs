use super::*;
use crate::project::testing::{repo_with_worktree, scratch};
use crate::project::{Folder, Project, ProjectId, Projects};
use crate::workspace::HostName;
use std::path::Path;

fn projects_of(project: &Project) -> Projects {
    Projects::from_iter([project.clone()])
}

#[test]
fn a_known_folder_is_known() {
    let (main, _) = repo_with_worktree("place-known");
    let p = Project::new(ProjectId::mint(1), None, main.clone());
    assert_eq!(
        place_blocking(&projects_of(&p), None, &main),
        Placement::Known(p.id, Folder::Home)
    );
}

#[test]
fn a_linked_checkout_of_a_known_project_is_its_worktree() {
    let (main, linked) = repo_with_worktree("place-linked");
    let p = Project::new(ProjectId::mint(1), None, main);
    assert_eq!(
        place_blocking(&projects_of(&p), None, &linked),
        Placement::WorktreeOf {
            project: p.id,
            branch: "w".into()
        }
    );
}

#[test]
fn the_same_sub_folder_of_a_linked_checkout_is_its_worktree() {
    let (main, linked) = repo_with_worktree("place-sub");
    let p = Project::new(ProjectId::mint(1), None, main.join("sub"));
    assert!(matches!(
        place_blocking(&projects_of(&p), None, &linked.join("sub")),
        Placement::WorktreeOf { project, .. } if project == p.id
    ));
}

#[test]
fn a_linked_checkout_at_another_sub_path_is_new() {
    let (main, linked) = repo_with_worktree("place-other-sub");
    let p = Project::new(ProjectId::mint(1), None, main.join("sub"));
    assert_eq!(
        place_blocking(&projects_of(&p), None, &linked),
        Placement::New
    );
}

#[test]
fn a_main_checkout_is_never_a_worktree() {
    let (main, linked) = repo_with_worktree("place-main-new");
    let p = Project::new(ProjectId::mint(1), None, linked);
    assert_eq!(
        place_blocking(&projects_of(&p), None, &main),
        Placement::New
    );
}

#[test]
fn a_folder_outside_git_and_a_host_folder_are_new() {
    let plain = scratch("place-plain");
    assert_eq!(
        place_blocking(&Projects::default(), None, &plain),
        Placement::New
    );
    assert_eq!(
        place_blocking(
            &Projects::default(),
            Some(&HostName("vps".into())),
            Path::new("/srv/a")
        ),
        Placement::New
    );
}

#[test]
fn a_repository_key_says_its_common_dir_sub_path_and_whether_it_is_main() {
    let (main, linked) = repo_with_worktree("place-key");
    let (common, sub, is_main) = repo_key_blocking(&main.join("sub")).unwrap();
    assert_eq!(common, main.join(".git"));
    assert_eq!(sub, Path::new("sub"));
    assert!(is_main);
    let (common, sub, is_main) = repo_key_blocking(&linked).unwrap();
    assert_eq!(common, main.join(".git"));
    assert_eq!(sub, Path::new(""));
    assert!(!is_main);
    assert!(repo_key_blocking(&scratch("place-key-plain")).is_none());
}
