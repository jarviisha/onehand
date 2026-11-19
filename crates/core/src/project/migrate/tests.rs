use super::*;
use crate::config::{ProjectConfig, WorkspaceConfig, WorkspaceLoad};
use crate::project::file::load_all_blocking;
use crate::project::testing::{repo_with_worktree, scratch};
use crate::project::{MadeBy, Project};
use crate::workspace::{HostName, Offloaded};
use std::fs;
use std::path::{Path, PathBuf};

/// A data root of its own for one test.
fn root(case: &str) -> PathBuf {
    scratch(&format!("migrate-{case}"))
}

/// A workspace `id` under `root` holding `entries` as written: no ids.
fn workspace(root: &Path, id: &str, entries: Vec<ProjectConfig>) {
    let cfg = WorkspaceConfig {
        name: id.into(),
        projects: entries,
        ..Default::default()
    };
    cfg.save_to(&root.join("workspaces").join(id)).unwrap();
}

fn entry(path: &Path) -> ProjectConfig {
    ProjectConfig {
        path: path.to_path_buf(),
        ..Default::default()
    }
}

fn load_ws(root: &Path, id: &str) -> WorkspaceConfig {
    match WorkspaceConfig::load_from(&root.join("workspaces").join(id)) {
        WorkspaceLoad::Found(cfg) => cfg,
        WorkspaceLoad::Missing | WorkspaceLoad::Unreadable(_) => panic!("workspace {id}"),
    }
}

fn projects(root: &Path) -> Vec<Project> {
    let (projects, problems) = load_all_blocking(&root.join("projects"));
    assert!(problems.is_empty(), "{problems:?}");
    projects.iter().cloned().collect()
}

fn offloaded(host: &str) -> Offloaded {
    Offloaded {
        host: HostName(host.into()),
        mirror: "/m".into(),
        branch: None,
    }
}

#[test]
fn worktree_entries_of_one_repository_become_one_project() {
    let (root, (main, linked)) = (root("one-repo"), repo_with_worktree("migrate-one-repo"));
    workspace(&root, "w1", vec![entry(&main), entry(&linked)]);
    let problems = migrate_projects_in(&root, 1);
    assert!(problems.is_empty(), "{problems:?}");
    let all = projects(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].path, main);
    assert_eq!(all[0].name, "main");
    assert_eq!(all[0].worktrees.len(), 1);
    assert_eq!(all[0].worktrees[0].path, linked);
    assert_eq!(all[0].worktrees[0].branch, "w");
    assert_eq!(all[0].worktrees[0].made_by, MadeBy::Added);
    let cfg = load_ws(&root, "w1");
    assert_eq!(cfg.projects.len(), 2, "each folder is still its own entry");
    assert!(cfg
        .projects
        .iter()
        .all(|e| e.id.as_ref() == Some(&all[0].id)));
    assert_eq!(
        cfg.projects[1].path, linked,
        "the folder is kept beside the id"
    );
}

#[test]
fn one_folder_in_two_workspaces_is_one_project() {
    let root = root("two-workspaces");
    let plain = scratch("migrate-two-workspaces-plain");
    workspace(&root, "w1", vec![entry(&plain)]);
    workspace(&root, "w2", vec![entry(&plain)]);
    migrate_projects_in(&root, 1);
    let all = projects(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(load_ws(&root, "w1").projects[0].id, Some(all[0].id.clone()));
    assert_eq!(load_ws(&root, "w2").projects[0].id, Some(all[0].id.clone()));
}

#[test]
fn a_projects_facts_come_from_its_entries() {
    let (root, (main, linked)) = (root("facts"), repo_with_worktree("migrate-facts"));
    workspace(
        &root,
        "w1",
        vec![
            entry(&main),
            ProjectConfig {
                check: Some("make".into()),
                offload: Some(offloaded("vps")),
                pinned: true,
                ..entry(&linked)
            },
        ],
    );
    migrate_projects_in(&root, 1);
    let p = &projects(&root)[0];
    assert_eq!(
        p.check.as_deref(),
        Some("make"),
        "the first entry having one"
    );
    assert_eq!(p.offload, None, "the project folder's own offload");
    assert_eq!(p.worktrees[0].offload, Some(offloaded("vps")));
    let cfg = load_ws(&root, "w1");
    assert!(cfg.projects[1].pinned, "the pin stays the workspace's");
    assert_eq!(
        cfg.projects[1].check, None,
        "the facts left with the project"
    );
}

#[test]
fn a_second_run_after_a_cut_short_one_makes_no_new_project() {
    let root = root("cut-short");
    let plain = scratch("migrate-cut-short-plain");
    workspace(&root, "w1", vec![entry(&plain)]);
    migrate_projects_in(&root, 1);
    let first = projects(&root);
    let copy = root
        .join("workspaces")
        .join("w1")
        .join("onehand-workspace.toml.before-projects");
    let copied = fs::read_to_string(&copy).unwrap();
    // As if the run had stopped after the projects and before the workspaces.
    workspace(&root, "w1", vec![entry(&plain)]);
    migrate_projects_in(&root, 2);
    assert_eq!(projects(&root), first);
    assert_eq!(
        load_ws(&root, "w1").projects[0].id,
        Some(first[0].id.clone())
    );
    assert_eq!(
        fs::read_to_string(&copy).unwrap(),
        copied,
        "the first copy is kept"
    );
}

#[test]
fn entries_an_older_build_wrote_find_their_projects() {
    let root = root("older");
    let plain = scratch("migrate-older-plain");
    workspace(&root, "w1", vec![entry(&plain)]);
    migrate_projects_in(&root, 1);
    let first = projects(&root);
    workspace(
        &root,
        "w1",
        vec![ProjectConfig {
            check: Some("make".into()),
            ..entry(&plain)
        }],
    );
    migrate_projects_in(&root, 2);
    let now = projects(&root);
    assert_eq!(now.len(), 1);
    assert_eq!(
        load_ws(&root, "w1").projects[0].id,
        Some(first[0].id.clone())
    );
    assert_eq!(
        now[0].check.as_deref(),
        Some("make"),
        "what the older build set is taken in, the project having none"
    );
}

#[test]
fn a_run_with_nothing_to_do_writes_nothing() {
    let root = root("nothing");
    let plain = scratch("migrate-nothing-plain");
    workspace(&root, "w1", vec![entry(&plain)]);
    migrate_projects_in(&root, 1);
    let file = root
        .join("workspaces")
        .join("w1")
        .join(WorkspaceConfig::FILE);
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    migrate_projects_in(&root, 2);
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
}

#[test]
fn a_main_checkout_not_in_any_workspace_is_still_the_project_folder() {
    let (root, (main, linked)) = (
        root("main-absent"),
        repo_with_worktree("migrate-main-absent"),
    );
    workspace(&root, "w1", vec![entry(&linked)]);
    migrate_projects_in(&root, 1);
    let all = projects(&root);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].path, main);
    assert_eq!(all[0].worktrees[0].path, linked);
}

#[test]
fn a_folder_that_does_not_resolve_and_a_host_folder_are_projects_of_their_own() {
    let root = root("lone");
    workspace(
        &root,
        "w1",
        vec![
            entry(Path::new("/nonexistent/onehand-x")),
            ProjectConfig {
                host: Some(HostName("vps".into())),
                ..entry(Path::new("/srv/app"))
            },
        ],
    );
    migrate_projects_in(&root, 1);
    let mut all = projects(&root);
    all.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].path, Path::new("/nonexistent/onehand-x"));
    assert_eq!(all[1].host, Some(HostName("vps".into())));
}

#[test]
fn an_unreadable_workspace_file_is_left_untouched() {
    let root = root("unreadable");
    let dir = root.join("workspaces").join("w1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(WorkspaceConfig::FILE), "name = [").unwrap();
    let problems = migrate_projects_in(&root, 1);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        fs::read_to_string(dir.join(WorkspaceConfig::FILE)).unwrap(),
        "name = ["
    );
}

#[test]
fn offload_data_moves_to_its_folder_stem() {
    let root = root("offload");
    let old = root.join("projects").join("onehand-1234abcd");
    fs::create_dir_all(old.join("backups")).unwrap();
    fs::write(old.join("offload.json"), "{}").unwrap();
    fs::write(old.join("backups").join("b"), "b").unwrap();
    migrate_projects_in(&root, 1);
    let new = root.join("offload").join("onehand-1234abcd");
    assert_eq!(fs::read_to_string(new.join("offload.json")).unwrap(), "{}");
    assert_eq!(
        fs::read_to_string(new.join("backups").join("b")).unwrap(),
        "b"
    );
    assert!(!old.exists(), "projects/ then holds projects only");
}
