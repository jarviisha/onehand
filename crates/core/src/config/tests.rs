use super::*;

#[test]
fn a_command_is_found_by_path_or_on_path_and_never_invented() {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().as_os_str().to_owned();
    let name = exe.file_name().unwrap().to_str().unwrap();
    // A path is checked as a path.
    assert_eq!(
        find_command(exe.to_str().unwrap(), None, None),
        Some(exe.clone())
    );
    // A bare name is looked for on PATH, and only there.
    assert_eq!(find_command(name, Some(&dir), None), Some(exe.clone()));
    assert_eq!(find_command(name, None, None), None);
    assert_eq!(find_command("no-such-agent-here", Some(&dir), None), None);
    assert_eq!(find_command("  ", Some(&dir), None), None);
}

/// A relative path is resolved from where the session would start it --
/// the project root -- and not from wherever the app itself was launched.
#[test]
fn a_relative_command_is_found_from_the_directory_it_would_run_in() {
    let exe = std::env::current_exe().unwrap();
    let parent = exe.parent().unwrap();
    let root = parent.parent().unwrap();
    let relative = format!(
        "{}/{}",
        parent.file_name().unwrap().to_str().unwrap(),
        exe.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(
        find_command(&relative, None, Some(root)),
        Some(root.join(&relative))
    );
    assert_eq!(find_command(&relative, None, Some(parent)), None);
}

#[test]
fn empty_file_keeps_defaults() {
    let cfg = AppConfig::parse("").unwrap();
    assert_eq!(cfg.agents, default_agents());
    assert_eq!(cfg.font.monospace, None);
}

#[test]
fn a_config_written_for_the_old_font_and_icon_tables_still_loads() {
    // `size`, `scale`, `sans` and `fallbacks` were `[font]` keys, and
    // `[icons]` was a whole table; all of them were parsed and none were
    // read. They are gone, and a file that still sets them has to keep
    // working. What allows that is serde ignoring unknown fields, which is
    // its default and which nothing here overrides — the same reason a
    // legacy agent's `kind` still parses. This test is what would fail if
    // anyone reached for `deny_unknown_fields`, because that attribute
    // would turn every one of these leftovers into a refusal to load.
    //
    // It is also the `[font]`-only case, which is the *other* tolerance:
    // `#[serde(default)]` filling in the tables this file never mentions,
    // so the default agents survive it.
    let cfg = AppConfig::parse(
        "[font]\nsize = 15.0\nscale = 2.0\nsans = \"Inter\"\nfallbacks = [\"X\"]\n\
             monospace = \"Iosevka\"\n\n[icons]\naccent = \"#ff0000\"\n",
    )
    .unwrap();
    assert_eq!(cfg.font.monospace.as_deref(), Some("Iosevka"));
    assert_eq!(cfg.agents, default_agents());
}

#[test]
fn appearance_parses_and_defaults_to_the_system() {
    assert_eq!(AppConfig::parse("").unwrap().appearance, Appearance::System);
    for (text, want) in [
        ("appearance = \"dark\"\n", Appearance::Dark),
        ("appearance = \"light\"\n", Appearance::Light),
        ("appearance = \"System\"\n", Appearance::System),
    ] {
        assert_eq!(AppConfig::parse(text).unwrap().appearance, want);
    }
}

#[test]
fn a_misspelled_appearance_does_not_take_the_config_with_it() {
    // The agent list lives in this same file. One mistyped word must not be
    // able to reset it, so an unknown value reads as "follow the desktop".
    let cfg = AppConfig::parse(
        "appearance = \"sepia\"\n\n[[agents]]\nname = \"Gemini\"\ncommand = \"gemini-acp\"\n",
    )
    .unwrap();
    assert_eq!(cfg.appearance, Appearance::System);
    assert_eq!(cfg.agents[0].name, "Gemini");
}

#[test]
fn a_config_carrying_an_appearance_still_serializes() {
    // A bare key has to be declared before the sections: TOML writes every
    // plain value above the first table, and a serializer asked to emit one
    // after a table fails outright rather than reordering.
    let cfg = AppConfig {
        appearance: Appearance::Dark,
        ..AppConfig::default()
    };
    let text = cfg
        .to_toml()
        .expect("a config with an appearance must save");
    assert_eq!(AppConfig::parse(&text).unwrap(), cfg);
}

#[test]
fn agents_parse() {
    let text = r#"
            [[agents]]
            name = "Gemini"
            command = "gemini-acp"
            args = ["--stdio"]
        "#;
    let cfg = AppConfig::parse(text).unwrap();
    assert_eq!(cfg.agents.len(), 1);
    assert_eq!(cfg.agents[0].name, "Gemini");
    assert_eq!(cfg.agents[0].command, "gemini-acp");
}

#[test]
fn legacy_kind_field_is_ignored() {
    // Old configs carried `kind = "acp"|"terminal"`; it's now an unknown key
    // and must parse without error (serde ignores it).
    let text = r#"
            [[agents]]
            name = "Old"
            kind = "terminal"
            command = "claude"
        "#;
    let cfg = AppConfig::parse(text).unwrap();
    assert_eq!(cfg.agents[0].name, "Old");
    assert_eq!(cfg.agents[0].command, "claude");
}

/// Whether a run happens is decided per project, so the config carries no
/// switch; one left in an older file is an unknown key and ignored.
#[test]
fn unattended_runs_have_no_switch_of_their_own_and_read_an_old_one_as_nothing() {
    let cfg = AppConfig::parse("").unwrap();
    assert_eq!(cfg.unattended.label, "auto");
    assert_eq!(cfg.unattended.every, "30m");
    // The switch moved onto each project; a file still carrying the old
    // global one keeps loading.
    let cfg = AppConfig::parse("[unattended]\nenabled = true\nlabel = \"x\"\n").unwrap();
    assert_eq!(cfg.unattended.label, "x");
}

/// The bridge is off unless the file asks for it, and its list starts
/// empty — an enabled bridge with nobody on the list answers nobody, which
/// is the failure that has to be the safe one.
#[test]
fn the_remote_bridge_is_off_until_asked_for() {
    let cfg = AppConfig::parse("").unwrap();
    assert!(!cfg.remote.telegram.enabled);
    assert!(cfg.remote.telegram.allowed_chats.is_empty());
    assert_eq!(cfg.remote.telegram.token_env, None);
}

#[test]
fn a_remote_section_parses_and_keeps_everything_else() {
    let cfg = AppConfig::parse(
        "[remote.telegram]\n\
             enabled = true\n\
             allowed_chats = [\"123\", \"-100456\"]\n\
             token_env = \"MY_BOT\"\n",
    )
    .unwrap();
    assert!(cfg.remote.telegram.enabled);
    assert_eq!(cfg.remote.telegram.allowed_chats, ["123", "-100456"]);
    assert_eq!(cfg.remote.telegram.token_env.as_deref(), Some("MY_BOT"));
    assert_eq!(cfg.agents, default_agents());
}

/// There is no key for the token, and there must not become one: the file
/// is rewritten whole by the settings dialog, so anything in it is printed
/// back out on a schedule nobody chose.
#[test]
fn a_token_in_the_config_file_is_not_a_key() {
    let cfg = AppConfig::parse("[remote.telegram]\nenabled = true\ntoken = \"123:secret\"\n")
        .expect("an unknown key must not fail the file");
    let text = cfg.to_toml().unwrap();
    assert!(
        !text.contains("123:secret"),
        "the config round-tripped a token back out: {text}"
    );
}

#[test]
fn legacy_profile_section_is_ignored() {
    // Older configs carried a `[profile]` section; it must parse as an
    // unknown key, not an error.
    let cfg =
        AppConfig::parse("[profile]\nname = \"Jarviis\"\navatar = \"/home/me/a.png\"\n").unwrap();
    assert_eq!(cfg, AppConfig::default());
}

#[test]
fn roundtrips_through_toml() {
    let cfg = AppConfig::default();
    let text = cfg.to_toml().unwrap();
    let back = AppConfig::parse(&text).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn workspace_config_roundtrips() {
    let cfg = WorkspaceConfig {
        name: "Mine".into(),
        roots: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        active_root: 1,
        layout: PanelLayout::default(),
        pinned: Vec::new(),
        unattended: Vec::new(),
    };
    let text = toml::to_string_pretty(&cfg).unwrap();
    let back: WorkspaceConfig = toml::from_str(&text).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn workspace_config_save_load_to_dir() {
    let dir = std::env::temp_dir().join(format!("onehand-ws-test-{}", std::process::id()));
    let cfg = WorkspaceConfig {
        name: "Persisted".into(),
        roots: vec![PathBuf::from("/x")],
        active_root: 0,
        layout: PanelLayout::default(),
        pinned: Vec::new(),
        unattended: Vec::new(),
    };
    cfg.save_to(&dir).unwrap();
    assert_eq!(WorkspaceConfig::load_from(&dir), WorkspaceLoad::Found(cfg));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The staging name has to separate two *processes*, not just two calls.
///
/// A counter alone starts at zero in every process, so two onehand
/// instances saving the same file both staged into the same temp — one
/// still writing it while the other renamed it into place, which is exactly
/// the half-written file the rename exists to prevent.
#[test]
fn temp_names_cannot_collide_across_processes() {
    let path = Path::new("/d/state.toml");
    assert_ne!(tmp_path(path, 100, 0), tmp_path(path, 101, 0));
    assert_ne!(tmp_path(path, 100, 0), tmp_path(path, 100, 1));
    // A sibling of the destination, so the rename stays on one filesystem.
    assert_eq!(tmp_path(path, 100, 0).parent(), path.parent());
}

/// Appending rather than replacing the extension: `with_extension` eats
/// everything after the last dot, so a session id carrying one lost part of
/// its name and two ids could stage into the same temp again.
#[test]
fn a_dotted_stem_keeps_its_name() {
    let name = tmp_path(Path::new("/d/a.b.json"), 7, 0);
    assert_eq!(
        name.file_name().unwrap().to_str().unwrap(),
        "a.b.json.tmp7-0"
    );
}

/// A failed write leaves the destination alone *and* leaves nothing behind:
/// a temp that outlives its write is a file nobody will ever collect.
#[test]
fn write_atomic_leaves_no_temp_behind_when_the_rename_fails() {
    let dir = std::env::temp_dir().join(format!("onehand-atomic-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The destination is a directory, so the rename cannot land.
    let dest = dir.join("taken");
    std::fs::create_dir_all(&dest).unwrap();

    assert!(write_atomic(&dest, "hello").is_err());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .filter(|n| n.to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left {leftovers:?} behind");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_non_finite_size_falls_back_instead_of_staying_nan() {
    // `f32::clamp` returns NaN for NaN, so the bound alone was no guard.
    let d = PanelLayout::default();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let got = PanelLayout {
            workbench_w: bad,
            terminal_h: bad,
            ..d
        }
        .clamped();
        assert_eq!(got.workbench_w, d.workbench_w, "for {bad}");
        assert_eq!(got.terminal_h, d.terminal_h, "for {bad}");
    }
    // Ordinary out-of-range values still clamp to the bound, not the default.
    let got = PanelLayout {
        workbench_w: 5.0,
        terminal_h: 99_999.0,
        ..d
    }
    .clamped();
    assert_eq!(got.workbench_w, PanelLayout::MIN);
    assert_eq!(got.terminal_h, PanelLayout::MAX);
}

/// The rail is clamped by its *own* range, not the docks'.
///
/// Sharing one range would let a saved rail restore at 120px, where a
/// project row is an icon and half a name, or at 2000px, where the rail is
/// the window. The docks are sized by preference; the rail is sized by what
/// its rows have to fit.
#[test]
fn the_rail_is_clamped_by_its_own_range() {
    let d = PanelLayout::default();
    assert_eq!(
        PanelLayout { rail_w: 120.0, ..d }.clamped().rail_w,
        PanelLayout::RAIL_MIN
    );
    assert_eq!(
        PanelLayout {
            rail_w: 2000.0,
            ..d
        }
        .clamped()
        .rail_w,
        PanelLayout::RAIL_MAX
    );
    assert_eq!(
        PanelLayout {
            rail_w: f32::NAN,
            ..d
        }
        .clamped()
        .rail_w,
        d.rail_w
    );
    // The default must itself be inside the range it is the fallback for.
    assert!((PanelLayout::RAIL_MIN..=PanelLayout::RAIL_MAX).contains(&d.rail_w));
}

#[test]
fn agent_args_survive_a_round_trip_through_the_form() {
    // The contract is exactly this: edit nothing, save, get the same list.
    for args in [
        vec![],
        vec!["-y".to_string(), "@scope/pkg@1.2.3".to_string()],
        vec!["--system-prompt".to_string(), "hello world".to_string()],
        vec!["--json".to_string(), r#"{"a": "b c"}"#.to_string()],
        vec![
            "a'b".to_string(),
            "c\"d".to_string(),
            "back\\slash".to_string(),
        ],
        vec!["".to_string(), "after-empty".to_string()],
    ] {
        let line = join_args(&args);
        assert_eq!(split_args(&line), args, "round trip failed for {args:?}");
    }
}

#[test]
fn split_args_reads_ordinary_lines_the_obvious_way() {
    assert_eq!(split_args("  -y   pkg  "), vec!["-y", "pkg"]);
    assert_eq!(split_args(""), Vec::<String>::new());
    assert_eq!(split_args("'a b' c"), vec!["a b", "c"]);
    // A half-typed quote keeps what has been typed rather than eating it.
    assert_eq!(split_args("a \"b c"), vec!["a", "b c"]);
}

#[test]
fn an_unreadable_workspace_config_is_not_an_empty_folder() {
    // The distinction the binding guard and the recents list both act on:
    // `Missing` frees the folder, `Unreadable` must not.
    let dir = std::env::temp_dir().join("onehand-ws-unreadable-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    assert_eq!(WorkspaceConfig::load_from(&dir), WorkspaceLoad::Missing);

    std::fs::write(dir.join(WorkspaceConfig::FILE), "name = \"unclosed").unwrap();
    assert_eq!(WorkspaceConfig::load_from(&dir), WorkspaceLoad::Unreadable);
    assert!(WorkspaceConfig::load_from(&dir).found().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn app_state_roundtrips() {
    let st = AppState {
        workspace_dir: Some(PathBuf::from("/home/me/ws")),
        recent_workspaces: vec![PathBuf::from("/home/me/ws"), PathBuf::from("/other")],
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
