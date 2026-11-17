use super::*;
use crate::config::{AppState, WorkspaceConfig};
use std::fs;

/// A config root and a folder beside it for what an earlier build kept
/// elsewhere.
fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("onehand-migrate-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let (config, ext) = (base.join("config"), base.join("ext"));
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&ext).unwrap();
    (config, ext)
}

/// An earlier build's workspace called `name` in `dir`, with one issue file.
fn old_workspace(dir: &Path, name: &str) {
    WorkspaceConfig {
        name: name.into(),
        ..Default::default()
    }
    .save_to(dir)
    .unwrap();
    fs::create_dir_all(dir.join("issues")).unwrap();
    fs::write(dir.join("issues").join("p-1.json"), "[]").unwrap();
}

fn old_state(config: &Path, recents: &[&Path], names: &[(&Path, &str)]) {
    let state = AppState {
        workspace_dir: recents.first().map(|p| p.to_path_buf()),
        recent_workspaces: recents.iter().map(|p| p.to_path_buf()).collect(),
        names: names
            .iter()
            .map(|(p, n)| (p.to_path_buf(), n.to_string()))
            .collect(),
        ..Default::default()
    };
    state.save_to(&config.join("state.toml")).unwrap();
}

fn state(config: &Path) -> AppState {
    AppState::load_from(&config.join("state.toml"))
}

/// Every file under `dir`, with its bytes, for comparing a tree.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_external_workspace_is_copied_and_left_in_place() {
    let (config, ext) = scratch("copy");
    let old = ext.join("ws02");
    old_workspace(&old, "Foxaiwork");
    old_state(&config, &[&old], &[]);
    let before = tree(&old);
    assert!(migrate_in(&config).is_empty());
    let new = config.join("workspaces").join(stem_for(&old));
    assert!(new.join(WorkspaceConfig::FILE).is_file());
    assert!(new.join("issues").join("p-1.json").is_file());
    assert_eq!(tree(&old), before, "the old folder is left as it was");
}

#[test]
fn the_old_name_fills_an_empty_one() {
    let (config, ext) = scratch("name");
    let old = ext.join("ws01");
    old_workspace(&old, "");
    old_state(&config, &[&old], &[(&old, "Kept name")]);
    migrate_in(&config);
    let new = config.join("workspaces").join(stem_for(&old));
    let cfg = WorkspaceConfig::load_from(&new).found().unwrap();
    assert_eq!(cfg.name, "Kept name");
}

#[test]
fn task_files_follow_the_copy() {
    let (config, ext) = scratch("tasks");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    old_state(&config, &[&old], &[]);
    let tasks = config.join("tasks");
    fs::create_dir_all(&tasks).unwrap();
    let old_file = old.join("issues").join("p-1.json");
    let moved = format!(
        "{{\"source\":{{\"tracker\":{{\"file\":{:?}}}}}}}",
        old_file.display().to_string()
    );
    fs::write(tasks.join("a.json"), &moved).unwrap();
    let other = "{\"source\":{\"tracker\":{\"file\":\"/elsewhere/issues/q.json\"}}}";
    fs::write(tasks.join("b.json"), other).unwrap();
    migrate_in(&config);
    let new_file = config
        .join("workspaces")
        .join(stem_for(&old))
        .join("issues")
        .join("p-1.json");
    let a: serde_json::Value =
        serde_json::from_slice(&fs::read(tasks.join("a.json")).unwrap()).unwrap();
    assert_eq!(
        a["source"]["tracker"]["file"],
        new_file.display().to_string()
    );
    assert_eq!(fs::read_to_string(tasks.join("b.json")).unwrap(), other);
}

#[test]
fn offload_data_moves_to_the_project_dir() {
    let (config, _) = scratch("offload");
    let kept = config.join("workspaces").join("x-1");
    fs::create_dir_all(kept.join("coming-back")).unwrap();
    fs::write(kept.join("offload.json"), "{}").unwrap();
    fs::write(kept.join("coming-back").join("f"), "f").unwrap();
    migrate_in(&config);
    let project = config.join("projects").join("x-1");
    assert_eq!(
        fs::read_to_string(project.join("offload.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        fs::read_to_string(project.join("coming-back").join("f")).unwrap(),
        "f"
    );
    assert!(!kept.exists(), "a folder left with nothing in it goes");
}

#[test]
fn a_half_copy_is_redone() {
    let (config, ext) = scratch("half");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    old_state(&config, &[&old], &[]);
    let staging = config
        .join("workspaces")
        .join(format!(".{}.migrating", stem_for(&old)));
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("junk"), "x").unwrap();
    migrate_in(&config);
    let new = config.join("workspaces").join(stem_for(&old));
    assert!(new.join(WorkspaceConfig::FILE).is_file());
    assert!(!new.join("junk").exists());
    assert!(!staging.exists());
}

#[test]
fn a_second_run_changes_nothing() {
    let (config, ext) = scratch("twice");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    old_state(&config, &[&old], &[]);
    migrate_in(&config);
    let once = tree(&config);
    assert!(migrate_in(&config).is_empty());
    assert_eq!(tree(&config), once);
}

#[test]
fn state_keeps_the_old_order_as_ids() {
    let (config, ext) = scratch("order");
    let (a, b) = (ext.join("a"), ext.join("b"));
    old_workspace(&a, "A");
    old_workspace(&b, "B");
    old_state(&config, &[&b, &a], &[]);
    migrate_in(&config);
    let st = state(&config);
    assert_eq!(st.workspaces, [stem_for(&b), stem_for(&a)]);
    assert!(st.recent_workspaces.is_empty() && st.workspace_dir.is_none() && st.names.is_empty());
}

#[test]
fn a_workspace_already_kept_here_keeps_its_folder() {
    let (config, _) = scratch("here");
    let here = config.join("workspaces").join("p-1");
    old_workspace(&here, "P");
    old_state(&config, &[&here], &[]);
    migrate_in(&config);
    assert_eq!(state(&config).workspaces, ["p-1"]);
}

#[test]
fn a_copy_that_cannot_land_keeps_the_old_state() {
    let (config, ext) = scratch("unlanded");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    old_state(&config, &[&old], &[]);
    // A file where the workspaces' folder goes: no copy can land.
    fs::write(config.join("workspaces"), "").unwrap();
    assert!(!migrate_in(&config).is_empty());
    assert_eq!(state(&config).recent_workspaces, [old]);
}

/// The folder a workspace made *from* the old storage folder was kept in is
/// the one its copy would take: the copy goes beside it, and a second run
/// knows it is there.
#[test]
fn a_folder_taken_by_another_workspace_is_copied_beside_it() {
    let (config, ext) = scratch("taken");
    let old = ext.join("ws02");
    old_workspace(&old, "Foxaiwork");
    let taken = config.join("workspaces").join(stem_for(&old));
    old_workspace(&taken, "ws02");
    old_state(&config, &[&old], &[]);
    migrate_in(&config);
    let beside = config
        .join("workspaces")
        .join(format!("{}-2", stem_for(&old)));
    let cfg = WorkspaceConfig::load_from(&beside).found().unwrap();
    assert_eq!(cfg.name, "Foxaiwork");
    assert_eq!(
        WorkspaceConfig::load_from(&taken).found().unwrap().name,
        "ws02",
        "the one there is left alone"
    );
    assert_eq!(
        state(&config).workspaces[0],
        format!("{}-2", stem_for(&old))
    );
    // Run again with the old fields kept, as after a failure elsewhere.
    old_state(&config, &[&old], &[]);
    migrate_in(&config);
    assert!(!config
        .join("workspaces")
        .join(format!("{}-3", stem_for(&old)))
        .exists());
}

/// A folder not there at this launch (a disk not mounted) is not taken for
/// one holding no workspace: it stays named, to be brought in later.
#[test]
fn a_missing_old_folder_is_kept_for_a_later_launch() {
    let (config, ext) = scratch("missing");
    let (here, away) = (ext.join("here"), ext.join("unmounted").join("ws"));
    old_workspace(&here, "Here");
    old_state(&config, &[&here, &away], &[(&away, "Away")]);
    migrate_in(&config);
    let st = state(&config);
    assert_eq!(st.recent_workspaces, std::slice::from_ref(&away));
    assert_eq!(st.names.get(&away).map(String::as_str), Some("Away"));
    assert_eq!(st.workspaces, [stem_for(&here)]);
}

/// One folder that cannot be brought in keeps only itself named: one that
/// landed is not copied again, even once its copy was deleted.
#[test]
fn a_landed_folder_leaves_the_old_state_though_another_failed() {
    let (config, ext) = scratch("partial");
    let (good, bad) = (ext.join("good"), ext.join("bad"));
    old_workspace(&good, "Good");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join(WorkspaceConfig::FILE), "name = ").unwrap();
    old_state(&config, &[&good, &bad], &[]);
    migrate_in(&config);
    assert_eq!(state(&config).recent_workspaces, std::slice::from_ref(&bad));
    let copy = config.join("workspaces").join(stem_for(&good));
    fs::remove_dir_all(&copy).unwrap();
    migrate_in(&config);
    assert!(!copy.exists(), "a deleted copy is not brought back");
}

/// A copy that landed before its tasks were repointed (a crash between the
/// two) has them repointed by the next run.
#[test]
fn tasks_left_behind_by_an_earlier_run_are_repointed() {
    let (config, ext) = scratch("repoint-later");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    old_state(&config, &[&old], &[]);
    migrate_in(&config);
    old_state(&config, &[&old], &[]);
    let tasks = config.join("tasks");
    fs::create_dir_all(&tasks).unwrap();
    let old_file = old.join("issues").join("p-1.json").display().to_string();
    fs::write(tasks.join("a.json"), format!("{{\"file\":{old_file:?}}}")).unwrap();
    migrate_in(&config);
    let a: serde_json::Value =
        serde_json::from_slice(&fs::read(tasks.join("a.json")).unwrap()).unwrap();
    assert!(
        !a["file"]
            .as_str()
            .unwrap()
            .starts_with(&ext.display().to_string()),
        "{a}"
    );
}

/// A link in a copied folder is not followed: a loop would never end.
#[cfg(unix)]
#[test]
fn a_link_in_an_old_folder_is_not_followed() {
    let (config, ext) = scratch("link");
    let old = ext.join("ws02");
    old_workspace(&old, "W");
    std::os::unix::fs::symlink(&old, old.join("issues").join("loop")).unwrap();
    old_state(&config, &[&old], &[]);
    migrate_in(&config);
    let new = config.join("workspaces").join(stem_for(&old));
    assert!(new.join("issues").join("p-1.json").is_file());
    assert!(!new.join("issues").join("loop").exists());
}
