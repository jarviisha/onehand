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
    // A save settles what it read, against this machine's `PATH`.
    let mut settled = original.clone();
    settled.settle(std::env::var_os("PATH").as_deref());
    assert_eq!(saved.agents, settled.agents);
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
            ..Default::default()
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

/// A file of its own under the temp dir, holding `text`, for `test`.
fn config_file(test: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_save_keeps_what_this_build_does_not_know() {
    let path = config_file(
        "save-keeps-unknown",
        "# my agents\nfuture = 1\n\n[[agents]]\nname = \"Mine\"\ncommand = \"x\"\n\n\
         [remote.telegram]\nfuture_nested = \"keep\"\n",
    );
    AppConfig::update_in_place(&path, |cfg| cfg.appearance = Appearance::Light).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for kept in [
        "# my agents",
        "future = 1",
        "future_nested = \"keep\"",
        "name = \"Mine\"",
    ] {
        assert!(text.contains(kept), "{kept} is gone from:\n{text}");
    }
    assert_eq!(
        AppConfig::parse(&text).unwrap().appearance,
        Appearance::Light
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_changed_agent_list_stays_a_list_of_tables() {
    let path = config_file(
        "agent-list-of-tables",
        "[[agents]]\nname = \"One\"\ncommand = \"x\"\n",
    );
    AppConfig::update_in_place(&path, |cfg| {
        cfg.agents.push(AgentSpec {
            name: "Two".into(),
            command: "y".into(),
            ..Default::default()
        })
    })
    .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("[[agents]]").count(), 2, "{text}");
    assert!(!text.contains("agents = ["), "{text}");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_first_save_of_anything_writes_the_found_agents() {
    let original = "appearance = \"dark\"\n";
    let path = config_file("first-save-found-agents", original);
    AppConfig::update_in_place(&path, |cfg| cfg.appearance = Appearance::Light).unwrap();
    let saved = AppConfig::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let found = AppConfig::from_disk(Some(original), std::env::var_os("PATH").as_deref())
        .unwrap()
        .agents;
    assert_eq!(saved.agents, found);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_unknown_preset_survives_a_save_of_another_agent() {
    let path = config_file(
        "unknown-preset-survives",
        "[[agents]]\npreset = \"kimi\"\nname = \"Kimi\"\nversion = \"2\"\nenv = { K = \"v\" }\n\n\
         [[agents]]\nname = \"Mine\"\ncommand = \"x\"\n",
    );
    AppConfig::update_in_place(&path, |cfg| {
        AgentEdit::Remove(cfg.agents[1].clone()).apply(&mut cfg.agents)
    })
    .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for kept in ["preset = \"kimi\"", "version = \"2\"", "K = \"v\""] {
        assert!(text.contains(kept), "{kept} is gone from:\n{text}");
    }
    assert!(!text.contains("Mine"), "{text}");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
