use super::*;

#[test]
fn state_writes_only_ids() {
    let st = AppState {
        workspaces: vec!["a-1".into()],
        ..Default::default()
    };
    let text = toml::to_string_pretty(&st).unwrap();
    assert!(text.contains("workspaces"), "{text}");
    for old in ["recent_workspaces", "names", "workspace_dir"] {
        assert!(!text.contains(old), "{old} in {text}");
    }
    assert_eq!(toml::from_str::<AppState>(&text).unwrap(), st);
}

#[test]
fn what_an_earlier_build_kept_is_read_and_kept_until_cleared() {
    let st: AppState = toml::from_str(
        "workspace_dir = \"/old\"\nrecent_workspaces = [\"/old\", \"/older\"]\n[names]\n\"/old\" = \"Old\"\n",
    )
    .unwrap();
    assert_eq!(
        st.recent_workspaces,
        [PathBuf::from("/old"), PathBuf::from("/older")]
    );
    assert_eq!(
        st.names.get(Path::new("/old")).map(String::as_str),
        Some("Old")
    );
    let text = toml::to_string_pretty(&st).unwrap();
    assert!(
        text.contains("recent_workspaces"),
        "kept until migrated: {text}"
    );
}

#[test]
fn touch_moves_an_id_to_the_front_once() {
    let mut st = AppState::default();
    st.touch("a");
    st.touch("b");
    st.touch("a");
    assert_eq!(st.workspaces, ["a", "b"]);
    st.forget("a");
    assert_eq!(st.workspaces, ["b"]);
}

#[test]
fn a_state_that_will_not_parse_is_kept_aside() {
    let dir = std::env::temp_dir().join(format!("onehand-bad-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.toml");
    std::fs::write(&path, "workspaces = [\"a\"").unwrap();
    assert_eq!(AppState::load_from(&path), AppState::default());
    let kept = std::fs::read_to_string(dir.join("state.toml.bad")).unwrap();
    assert_eq!(kept, "workspaces = [\"a\"");
    std::fs::remove_dir_all(dir).unwrap();
}
