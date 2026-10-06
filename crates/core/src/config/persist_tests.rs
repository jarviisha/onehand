use super::*;

#[test]
fn keymap_round_trips_and_other_settings_survive_edits() {
    let dir = std::env::temp_dir().join(format!("onehand-keymap-{}", std::process::id()));
    let path = dir.join("config.toml");
    let mut original = AppConfig::default();
    original.font.monospace = Some("Test Mono".into());
    original
        .keymap
        .insert("toggle_workbench".into(), vec!["alt-j".into()]);
    original.keymap.insert("restart".into(), Vec::new());
    original.save_to(&path).unwrap();
    AppConfig::update_in_place(&path, |cfg| cfg.appearance = Appearance::Dark).unwrap();
    let saved = AppConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.keymap, original.keymap);
    assert_eq!(saved.font, original.font);
    assert_eq!(saved.agents, original.agents);
    assert_eq!(saved.appearance, Appearance::Dark);
    assert!(AppConfig::parse("").unwrap().keymap.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_existing_config_is_reported_before_editing() {
    let dir =
        std::env::temp_dir().join(format!("onehand-unreadable-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let error =
        AppConfig::update_in_place(&dir, |_| panic!("must not edit unreadable data")).unwrap_err();
    assert!(error.contains("could not be read"));
    assert!(dir.is_dir());
    std::fs::remove_dir(dir).unwrap();
}

/// The whole point of `update_in_place`: an unparseable file must survive.
#[test]
fn a_broken_config_is_never_overwritten() {
    let dir = std::env::temp_dir().join("onehand-cfg-guard");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let broken = "this is not = = valid toml [[[";
    std::fs::write(&path, broken).unwrap();

    let err = AppConfig::update_in_place(&path, |cfg| cfg.agents.clear())
        .expect_err("a broken config must be reported, not rewritten");
    assert!(err.contains("won't parse"), "unexpected error: {err}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        broken,
        "the user's file must be left exactly as it was"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A missing file is a first save, not an error.
#[test]
fn a_missing_config_starts_from_defaults() {
    let dir = std::env::temp_dir().join("onehand-cfg-first-save");
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("config.toml");

    AppConfig::update_in_place(&path, |cfg| {
        cfg.agents = vec![AgentSpec {
            name: "Only".into(),
            command: "echo".into(),
            args: vec![],
            auth: Default::default(),
        }]
    })
    .expect("first save must succeed");

    let saved = AppConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.agents.len(), 1);
    assert_eq!(saved.agents[0].name, "Only");
    std::fs::remove_dir_all(&dir).ok();
}

fn installed(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_preference_that_is_not_installed_falls_through() {
    // The whole point: asking for a family that is not there renders in the
    // body face and says nothing, so a missing preference must not win.
    let have = installed(&["Liberation Mono", "Cantarell"]);
    assert_eq!(
        resolve_monospace(["DejaVu Sans Mono"], &have).as_deref(),
        Some("Liberation Mono")
    );
}

#[test]
fn preferences_are_tried_in_order_and_case_insensitively() {
    let have = installed(&["JetBrains Mono", "Menlo"]);
    assert_eq!(
        resolve_monospace(["jetbrains mono", "Menlo"], &have).as_deref(),
        Some("JetBrains Mono"),
        "the enumerated spelling is returned, not the one asked for"
    );
}

#[test]
fn anything_naming_itself_monospace_beats_giving_up() {
    let have = installed(&["Zed Mono", "Adwaita Mono", "Cantarell"]);
    assert_eq!(
        resolve_monospace([], &have).as_deref(),
        Some("Adwaita Mono"),
        "sorted, so two launches on one machine agree"
    );
}

#[test]
fn nothing_monospace_means_no_answer() {
    // Not a wrong answer dressed as a right one: the caller keeps whatever
    // default it had, which is no worse.
    assert_eq!(
        resolve_monospace(["Menlo"], &installed(&["Cantarell"])),
        None
    );
}
