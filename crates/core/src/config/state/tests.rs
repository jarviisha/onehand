use super::*;

#[test]
fn app_state_roundtrips() {
    let st = AppState {
        workspace_dir: Some(PathBuf::from("/home/me/ws")),
        recent_workspaces: vec![PathBuf::from("/home/me/ws"), PathBuf::from("/other")],
        ..AppState::default()
    };
    let text = toml::to_string_pretty(&st).unwrap();
    let back: AppState = toml::from_str(&text).unwrap();
    assert_eq!(st, back);
}

#[test]
fn app_state_migrates_legacy_workspace_dir() {
    let mut st: AppState = toml::from_str("workspace_dir = \"/x\"\n").unwrap();
    st.migrate();
    assert_eq!(st.recent_workspaces, vec![PathBuf::from("/x")]);
}

#[test]
fn app_state_migrate_does_not_duplicate() {
    let mut st = AppState {
        workspace_dir: Some(PathBuf::from("/x")),
        recent_workspaces: vec![PathBuf::from("/x"), PathBuf::from("/y")],
        ..AppState::default()
    };
    st.migrate();
    assert_eq!(
        st.recent_workspaces,
        vec![PathBuf::from("/x"), PathBuf::from("/y")]
    );
}

#[test]
fn app_state_touch_moves_to_front_dedupes_and_caps() {
    let mut st = AppState::default();
    for i in 0..9 {
        st.touch(PathBuf::from(format!("/ws{i}")));
    }
    assert_eq!(st.recent_workspaces.len(), AppState::MAX_RECENTS);
    assert_eq!(st.recent_workspaces[0], PathBuf::from("/ws8"));
    // Re-touching an existing entry reorders without duplicating.
    st.touch(PathBuf::from("/ws3"));
    assert_eq!(st.recent_workspaces[0], PathBuf::from("/ws3"));
    assert_eq!(st.recent_workspaces.len(), AppState::MAX_RECENTS);
    assert_eq!(st.workspace_dir, Some(PathBuf::from("/ws3")));
}

#[test]
fn app_state_forget_removes_and_remirrors() {
    let mut st = AppState::default();
    st.touch(PathBuf::from("/a"));
    st.touch(PathBuf::from("/b"));
    st.forget(Path::new("/b"));
    assert_eq!(st.recent_workspaces, vec![PathBuf::from("/a")]);
    assert_eq!(st.workspace_dir, Some(PathBuf::from("/a")));
    st.forget(Path::new("/a"));
    assert_eq!(st.workspace_dir, None);
}

/// The switcher names a workspace as the rail does, from what the recents
/// remember, never by reading the workspace's own file while it draws. An
/// entry from before names were remembered has none and reads as its folder.
#[test]
fn app_state_remembers_a_workspace_by_its_name() {
    let mut st: AppState = toml::from_str("recent_workspaces = [\"/old\"]\n").unwrap();
    st.migrate();
    assert_eq!(
        st.name_of(Path::new("/old")),
        None,
        "an old entry has no name"
    );

    st.touch(PathBuf::from("/ws"));
    assert_eq!(
        st.label_of(Path::new("/old")),
        "old",
        "it reads as its folder"
    );
    assert!(st.set_name(Path::new("/ws"), "Audit workspace"));
    assert_eq!(st.label_of(Path::new("/ws")), "Audit workspace");
    assert!(
        !st.set_name(Path::new("/ws"), "Audit workspace"),
        "nothing changed"
    );
    st.set_name(Path::new("/ws"), "Renamed");
    assert_eq!(st.name_of(Path::new("/ws")), Some("Renamed"));

    let back: AppState = toml::from_str(&toml::to_string_pretty(&st).unwrap()).unwrap();
    assert_eq!(back.name_of(Path::new("/ws")), Some("Renamed"));

    st.forget(Path::new("/ws"));
    assert_eq!(
        st.name_of(Path::new("/ws")),
        None,
        "a forgotten workspace keeps no name"
    );
    st.set_name(Path::new("/never"), "Nowhere");
    assert_eq!(
        st.name_of(Path::new("/never")),
        None,
        "only a remembered workspace is named"
    );
}

#[test]
fn a_state_that_will_not_parse_is_kept_aside() {
    let dir = std::env::temp_dir().join(format!("onehand-bad-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.toml");
    std::fs::write(&path, "recent_workspaces = [\"/a\"").unwrap();
    assert_eq!(AppState::load_from(&path), AppState::default());
    let kept = std::fs::read_to_string(dir.join("state.toml.bad")).unwrap();
    assert_eq!(kept, "recent_workspaces = [\"/a\"");
    std::fs::remove_dir_all(dir).unwrap();
}
