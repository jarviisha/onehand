use super::file::{load_all_blocking, load_from, save_to, ProjectLoad, FILE};
use super::testing::scratch;
use super::*;
use std::path::Path;

/// A worktree at `path`, as git listed it and the person adopted.
fn wt(path: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: "x".into(),
        from: None,
        made_by: MadeBy::Added,
        offload: None,
    }
}

#[test]
fn a_project_round_trips_through_its_file() {
    let dir = scratch("round-trip");
    let mut p = Project::new(ProjectId::mint(7), None, "/code/onehand".into());
    p.check = Some("make lint".into());
    p.worktrees.push(Worktree {
        path: "/code/onehand-feat".into(),
        branch: "feat".into(),
        from: Some("main".into()),
        made_by: MadeBy::Split,
        offload: None,
    });
    save_to(&p, &dir).unwrap();
    assert!(matches!(load_from(&dir), ProjectLoad::Found(back) if back == p));
}

#[test]
fn defaults_are_left_out_of_the_file() {
    let dir = scratch("defaults");
    save_to(&Project::new(ProjectId::mint(1), None, "/a".into()), &dir).unwrap();
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
    assert!(!text.contains("check"), "{text}");
    assert!(!text.contains("worktrees"), "{text}");
    assert!(!text.contains("offload"), "{text}");
}

#[test]
fn a_minted_id_is_eight_lowercase_hex() {
    let id = ProjectId::mint(42);
    assert_eq!(id.as_str().len(), 8);
    assert!(id
        .as_str()
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(ProjectId::mint(42), ProjectId::mint(43));
}

#[test]
fn a_new_project_is_named_after_its_folder() {
    let p = Project::new(ProjectId::mint(1), None, "/code/onehand".into());
    assert_eq!(p.name, "onehand");
}

#[test]
fn find_answers_the_home_folder_and_each_worktree() {
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.worktrees.push(wt("/code/a-x"));
    let projects = Projects::from_iter([p.clone()]);
    assert!(matches!(
        projects.find(None, Path::new("/code/a")),
        Some((_, Folder::Home))
    ));
    assert!(matches!(
        projects.find(None, Path::new("/code/a-x")),
        Some((_, Folder::Worktree(0)))
    ));
    assert!(projects
        .find(Some(&HostName("vps".into())), Path::new("/code/a"))
        .is_none());
    assert!(projects.find(None, Path::new("/code/b")).is_none());
}

#[test]
fn a_worktree_is_labelled_by_its_folder_and_home_by_the_name() {
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.name = "Alpha".into();
    p.worktrees.push(wt("/code/a-x"));
    assert_eq!(p.label_of(Folder::Home), "Alpha");
    assert_eq!(p.label_of(Folder::Worktree(0)), "a-x");
}

#[test]
fn offload_belongs_to_each_folder() {
    let on = Offloaded {
        host: HostName("vps".into()),
        mirror: "/m".into(),
        branch: None,
    };
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.worktrees.push(Worktree {
        offload: Some(on.clone()),
        ..wt("/code/a-x")
    });
    assert_eq!(p.offload_of(Folder::Home), None);
    assert_eq!(p.offload_of(Folder::Worktree(0)), Some(&on));
}

#[test]
fn an_unreadable_project_file_is_a_problem_and_kept() {
    let root = scratch("unreadable");
    std::fs::create_dir_all(root.join("deadbeef")).unwrap();
    std::fs::write(root.join("deadbeef").join(FILE), "id = [").unwrap();
    let (projects, problems) = load_all_blocking(&root);
    assert_eq!(projects.iter().count(), 0);
    assert_eq!(problems.len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("deadbeef").join(FILE)).unwrap(),
        "id = ["
    );
}

#[test]
fn every_project_file_under_the_folder_is_loaded() {
    let root = scratch("all");
    let a = Project::new(ProjectId::mint(1), None, "/a".into());
    let b = Project::new(ProjectId::mint(2), None, "/b".into());
    save_to(&a, &root.join(a.id.as_str())).unwrap();
    save_to(&b, &root.join(b.id.as_str())).unwrap();
    // A folder holding something else is no project, and no problem either.
    std::fs::create_dir_all(root.join("onehand-1234abcd")).unwrap();
    std::fs::write(root.join("onehand-1234abcd").join("offload.json"), "{}").unwrap();
    let (projects, problems) = load_all_blocking(&root);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(projects.get(&a.id), Some(&a));
    assert_eq!(projects.get(&b.id), Some(&b));
}
