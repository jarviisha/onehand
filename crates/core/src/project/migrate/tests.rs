use super::*;
use crate::config::{ProjectConfig, WorkspaceConfig, WorkspaceLoad};
use crate::project::file::load_all_blocking;
use crate::project::testing::{repo_with_worktree, scratch};
use crate::project::{MadeBy, Project, ProjectId, Projects};
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

// Issues.

/// A legacy issues file of `folder` in workspace `ws`, holding one issue of
/// onehand's own, #1, called `title`.
fn legacy_issue(root: &Path, ws: &str, folder: &Path, title: &str) -> PathBuf {
    let dir = root.join("workspaces").join(ws).join("issues");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{}.json", crate::workspace::stem_for(folder)));
    fs::write(
        &file,
        format!(r#"{{"next":2,"issues":[{{"number":1,"title":"{title}"}}]}}"#),
    )
    .unwrap();
    file
}

/// The numbers and titles of the issues in `project`'s file.
fn issues_of(root: &Path, project: &Project) -> Vec<(u64, String)> {
    let file = root
        .join("projects")
        .join(project.id.as_str())
        .join("issues.json");
    let read: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    read["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["number"].as_u64().unwrap(),
                i["title"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn merged(file: &Path) -> PathBuf {
    file.with_extension("json.merged")
}

#[test]
fn a_projects_issues_from_every_folder_and_workspace_land_in_its_file() {
    let (root, (main, linked)) = (root("issues-all"), repo_with_worktree("migrate-issues-all"));
    workspace(&root, "w1", vec![entry(&main), entry(&linked)]);
    workspace(&root, "w2", vec![entry(&main)]);
    let sources = [
        legacy_issue(&root, "w1", &main, "main in w1"),
        legacy_issue(&root, "w1", &linked, "worktree in w1"),
        legacy_issue(&root, "w2", &main, "main in w2"),
    ];
    let problems = migrate_projects_in(&root, 1);
    assert!(problems.is_empty(), "{problems:?}");
    let p = &projects(&root)[0];
    let mut got = issues_of(&root, p);
    got.sort();
    assert_eq!(
        got.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(got.len(), 3);
    for source in &sources {
        assert!(!source.exists(), "{} taken in", source.display());
        assert!(merged(source).exists(), "{} kept beside", source.display());
    }
}

#[test]
fn a_second_merge_takes_nothing_twice() {
    let (root, (main, _)) = (
        root("issues-twice"),
        repo_with_worktree("migrate-issues-twice"),
    );
    workspace(&root, "w1", vec![entry(&main)]);
    let source = legacy_issue(&root, "w1", &main, "only");
    migrate_projects_in(&root, 1);
    // A run cut short after the project's file landed, before its renames.
    fs::copy(merged(&source), &source).unwrap();
    fs::remove_file(merged(&source)).unwrap();
    migrate_projects_in(&root, 2);
    let p = &projects(&root)[0];
    assert_eq!(issues_of(&root, p), vec![(1, "only".to_string())]);
}

#[test]
fn an_unreadable_legacy_file_is_left_and_said() {
    let (root, (main, linked)) = (root("issues-bad"), repo_with_worktree("migrate-issues-bad"));
    workspace(&root, "w1", vec![entry(&main), entry(&linked)]);
    let good = legacy_issue(&root, "w1", &main, "good");
    let bad = legacy_issue(&root, "w1", &linked, "bad");
    fs::write(&bad, "{").unwrap();
    let problems = migrate_projects_in(&root, 1);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(fs::read_to_string(&bad).unwrap(), "{");
    assert!(merged(&good).exists());
    let p = &projects(&root)[0];
    assert_eq!(issues_of(&root, p), vec![(1, "good".to_string())]);
}

#[test]
fn a_host_project_keeps_no_issues_file() {
    let root = root("issues-host");
    workspace(
        &root,
        "w1",
        vec![ProjectConfig {
            host: Some(HostName("vps".into())),
            ..entry(Path::new("/srv/app"))
        }],
    );
    migrate_projects_in(&root, 1);
    let p = &projects(&root)[0];
    assert!(!root
        .join("projects")
        .join(p.id.as_str())
        .join("issues.json")
        .exists());
}

#[test]
fn a_project_whose_file_is_not_written_takes_no_issues() {
    // A project the run could not write: its folder holds no project file.
    let root = root("issues-unwritten");
    let folder = scratch("migrate-issues-unwritten-folder");
    let source = legacy_issue(&root, "w1", &folder, "stays");
    let project = Project::new(ProjectId::mint(77), None, folder.clone());
    // Other projects of the run were written, so the store is there.
    fs::create_dir_all(root.join("projects")).unwrap();
    let mut problems = Vec::new();
    super::merge_issues(
        &root,
        &Projects::from_iter([project.clone()]),
        &mut problems,
    );
    assert!(source.exists(), "the legacy file is left where it was");
    assert!(!root
        .join("projects")
        .join(project.id.as_str())
        .join("issues.json")
        .exists());
}

// Conversations.

/// A conversation `sid` kept under `root`, ran in `folder`, as an earlier
/// build wrote it: no project.
fn conversation(root: &Path, sid: &str, folder: &Path) -> PathBuf {
    let dir = root.join("conversations").join(sid);
    fs::create_dir_all(&dir).unwrap();
    let meta = serde_json::json!({
        "format": 2,
        "session_id": sid,
        "root": folder.display().to_string(),
        "agent": "Claude",
        "created": 1,
        "updated": 2,
        "items": 3,
        "title": null,
        "preview": "hello",
        "prefs": {},
    });
    let file = dir.join("meta.json");
    fs::write(&file, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    file
}

fn read_json(file: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(file).unwrap()).unwrap()
}

#[test]
fn a_conversation_of_a_projects_folder_is_stamped() {
    let (root, (main, _)) = (root("conv-home"), repo_with_worktree("migrate-conv-home"));
    workspace(&root, "w1", vec![entry(&main)]);
    let meta = conversation(&root, "s1", &main);
    let before = read_json(&meta);
    migrate_projects_in(&root, 1);
    let p = &projects(&root)[0];
    let after = read_json(&meta);
    assert_eq!(after["project"], p.id.as_str());
    for (field, value) in before.as_object().unwrap() {
        assert_eq!(&after[field], value, "{field} kept as it was");
    }
}

#[test]
fn a_conversation_of_an_unrecorded_worktree_is_stamped_through_git() {
    let (root, (main, linked)) = (root("conv-git"), repo_with_worktree("migrate-conv-git"));
    workspace(&root, "w1", vec![entry(&main)]);
    let meta = conversation(&root, "s1", &linked);
    migrate_projects_in(&root, 1);
    let p = &projects(&root)[0];
    assert!(p.worktrees.is_empty(), "history records no worktree");
    assert_eq!(read_json(&meta)["project"], p.id.as_str());
}

#[test]
fn a_conversation_of_a_removed_run_worktree_is_stamped_through_its_task() {
    let (root, (main, _)) = (root("conv-task"), repo_with_worktree("migrate-conv-task"));
    workspace(&root, "w1", vec![entry(&main)]);
    let gone = root.join("gone-run-worktree");
    let meta = conversation(&root, "s1", &gone);
    let tasks = root.join("tasks");
    fs::create_dir_all(&tasks).unwrap();
    let task = serde_json::json!({
        "id": "t1",
        "setup": { "repo": main.display().to_string(), "dir": gone.display().to_string() },
    });
    fs::write(tasks.join("t1.json"), task.to_string()).unwrap();
    migrate_projects_in(&root, 1);
    let p = &projects(&root)[0];
    assert_eq!(read_json(&meta)["project"], p.id.as_str());
}

#[test]
fn a_conversation_nothing_resolves_is_left_as_it_is() {
    let root = root("conv-none");
    let meta = conversation(&root, "s1", Path::new("/nonexistent/onehand-conv"));
    let before = fs::read(&meta).unwrap();
    migrate_projects_in(&root, 1);
    assert_eq!(fs::read(&meta).unwrap(), before);
}
