use super::*;
use crate::workspace::{HostName, Offloaded};

fn read(text: &str) -> WorkspaceConfig {
    toml::from_str(text).unwrap()
}

#[test]
fn the_old_shape_reads_as_one_record_per_project() {
    let cfg = read(
        r#"
        name = "W"
        roots = ["/a", "build01:/srv/b"]
        active_root = 1
        pinned = ["/a"]
        [checks]
        "/a" = "cargo test"
        [offloaded]
        "/a" = "build01:/m"
        [offloaded_branches]
        "/a" = "feat/x"
        "#,
    );
    assert_eq!(
        cfg.projects[0],
        ProjectConfig {
            path: "/a".into(),
            host: None,
            pinned: true,
            check: Some("cargo test".into()),
            offload: Some(Offloaded {
                host: HostName("build01".into()),
                mirror: "/m".into(),
                branch: Some("feat/x".into()),
            }),
        }
    );
    assert_eq!(cfg.projects[1].host, Some(HostName("build01".into())));
    assert_eq!(cfg.projects[1].path, PathBuf::from("/srv/b"));
    assert_eq!(cfg.active.as_deref(), Some("build01:/srv/b"));
}

#[test]
fn the_new_shape_wins_over_the_old() {
    let cfg = read(
        r#"
        roots = ["/o"]
        [[projects]]
        path = "/n"
        "#,
    );
    assert_eq!(cfg.projects.len(), 1);
    assert_eq!(cfg.projects[0].path, PathBuf::from("/n"));
}

#[test]
fn a_save_writes_only_the_new_shape() {
    let cfg = read("roots = [\"/a\"]\npinned = [\"/a\"]\n");
    let text = toml::to_string_pretty(&cfg).unwrap();
    assert!(text.contains("[[projects]]"), "{text}");
    assert!(!text.contains("roots"), "{text}");
    assert!(
        !text.contains("unattended"),
        "a default is left out: {text}"
    );
    assert_eq!(read(&text), cfg);
}

#[test]
fn an_unreadable_file_says_why() {
    let dir = std::env::temp_dir().join(format!("onehand-ws-unreadable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(WorkspaceConfig::FILE), "name = ").unwrap();
    match WorkspaceConfig::load_from(&dir) {
        WorkspaceLoad::Unreadable(why) => assert!(!why.is_empty()),
        other => panic!("{other:?}"),
    }
}
