use super::*;
use crate::config::WorkspaceConfig;
use crate::workspace::{launch_blocking, Launch, Workspace};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-list-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A workspace called `name` kept in `root/id`.
fn kept(root: &Path, id: &str, name: &str) -> PathBuf {
    let dir = root.join(id);
    WorkspaceConfig {
        name: name.into(),
        ..Default::default()
    }
    .save_to(&dir)
    .unwrap();
    dir
}

fn state(ids: &[&str]) -> AppState {
    AppState {
        workspaces: ids.iter().map(|id| id.to_string()).collect(),
        ..Default::default()
    }
}

fn ids(drive: &RecentsDrive) -> Vec<String> {
    drive
        .listed()
        .iter()
        .map(|l| l.dir.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn written(fx: &[Effect]) -> &AppState {
    match fx {
        [Effect::Write { state, .. }] => state,
        _ => panic!("one write"),
    }
}

#[test]
fn every_workspace_folder_is_listed_in_order() {
    let root = scratch("order");
    kept(&root, "a", "A");
    kept(&root, "b", "B");
    kept(&root, "c", "C");
    std::fs::create_dir_all(root.join("d")).unwrap();
    std::fs::create_dir_all(root.join(".e.migrating")).unwrap();
    let drive = RecentsDrive::new(
        state(&["c", "gone"]),
        root.join("state.toml"),
        list_blocking(&root),
    );
    assert_eq!(ids(&drive), ["c", "a", "b"]);
    assert_eq!(drive.listed()[0].name, "C");
    assert_eq!(drive.workspaces, ["c"], "an id with no folder is dropped");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_unreadable_workspace_is_listed_with_why() {
    let root = scratch("unreadable");
    std::fs::create_dir_all(root.join("x-1")).unwrap();
    std::fs::write(root.join("x-1").join(WorkspaceConfig::FILE), "name = ").unwrap();
    let listed = list_blocking(&root);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "x-1");
    assert!(listed[0]
        .unreadable
        .as_deref()
        .is_some_and(|why| !why.is_empty()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opened_moves_to_the_front_and_writes() {
    let root = scratch("opened");
    let a = kept(&root, "a", "A");
    let b = kept(&root, "b", "B");
    let mut drive = RecentsDrive::new(state(&["a"]), root.join("state.toml"), list_blocking(&root));
    let fx = drive.opened(b.clone(), "Bee");
    assert_eq!(written(&fx).workspaces, ["b", "a"]);
    assert_eq!(ids(&drive), ["b", "a"]);
    assert_eq!(drive.listed()[0].name, "Bee");
    // One made since the list was read joins it.
    let c = root.join("c");
    let fx = drive.opened(c, "C");
    assert_eq!(written(&fx).workspaces, ["c", "b", "a"]);
    assert_eq!(ids(&drive), ["c", "b", "a"]);
    let _ = a;
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_rename_is_listed_and_writes_nothing() {
    let root = scratch("named");
    let a = kept(&root, "a", "A");
    let mut drive = RecentsDrive::new(state(&[]), root.join("state.toml"), list_blocking(&root));
    assert!(drive.named(&a, "Renamed").is_empty());
    assert_eq!(drive.listed()[0].name, "Renamed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dropped_leaves_the_list_and_writes() {
    let root = scratch("dropped");
    let a = kept(&root, "a", "A");
    kept(&root, "b", "B");
    let mut drive = RecentsDrive::new(
        state(&["a", "b"]),
        root.join("state.toml"),
        list_blocking(&root),
    );
    let fx = drive.dropped(&a);
    assert_eq!(written(&fx).workspaces, ["b"]);
    assert_eq!(ids(&drive), ["b"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_write_is_only_logged() {
    let mut drive = RecentsDrive::new(state(&[]), PathBuf::from("/nowhere/state.toml"), Vec::new());
    let fx = drive.handle(Input::Written {
        pending: 1,
        result: Err("no".into()),
    });
    assert!(matches!(fx.as_slice(), [Effect::Signal(Signal::WriteFailed(why))] if why == "no"));
}

#[test]
fn a_launch_opens_the_first_workspace_that_loads() {
    let root = scratch("launch");
    std::fs::create_dir_all(root.join("bad")).unwrap();
    std::fs::write(root.join("bad").join(WorkspaceConfig::FILE), "name = ").unwrap();
    let good = kept(&root, "good", "Good");
    let drive = RecentsDrive::new(
        state(&["bad", "good"]),
        root.join("state.toml"),
        list_blocking(&root),
    );
    match launch_blocking(drive.listed(), Workspace::seeded("/x")) {
        Launch::Found(ws) => assert_eq!((ws.storage_dir, ws.name.as_str()), (good, "Good")),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn with_none_a_launch_writes_the_seed() {
    let drive = RecentsDrive::new(state(&[]), PathBuf::from("/nowhere/state.toml"), Vec::new());
    let seed = Workspace::seeded("/x/never-made");
    let at = seed.storage_dir.clone();
    match launch_blocking(drive.listed(), seed) {
        Launch::Seeded(ws) => assert_eq!(ws.storage_dir, at),
        other => panic!("{other:?}"),
    }
}

#[test]
fn seed_over_an_unreadable_file_never_writes_it() {
    let root = scratch("seed-unreadable");
    let mut seed = Workspace::seeded("/x/proj");
    seed.storage_dir = root.join("proj-1");
    std::fs::create_dir_all(&seed.storage_dir).unwrap();
    let file = seed.storage_dir.join(WorkspaceConfig::FILE);
    std::fs::write(&file, "name = ").unwrap();
    let drive = RecentsDrive::new(state(&[]), root.join("state.toml"), Vec::new());
    match launch_blocking(drive.listed(), seed) {
        Launch::Seeded(ws) => {
            assert_eq!(
                ws.storage_dir,
                root.join("proj-1-2"),
                "kept beside it instead"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "name = ");
    let _ = std::fs::remove_dir_all(root);
}
