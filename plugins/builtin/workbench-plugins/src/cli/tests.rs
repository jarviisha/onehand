use super::*;

/// Shaped as the real listing is: `enabled` is the same on every record of
/// one plugin, because it is the folded answer rather than the record's own
/// — and for a plugin no settings file mentions, it is off.
const LISTING: &str = r#"{
      "installed": [
        {"id": "ponytail@ponytail", "version": "4.9.0", "scope": "user", "enabled": false},
        {"id": "karpathy@k", "scope": "project", "enabled": true, "projectPath": "/elsewhere"},
        {"id": "figma@official", "scope": "local", "enabled": false, "projectPath": "/work/app"},
        {"id": "figma@official", "scope": "user", "enabled": false}
      ],
      "available": [
        {"pluginId": "rare@m", "name": "rare", "marketplaceName": "m", "installCount": 3},
        {"pluginId": "popular@m", "name": "popular", "description": "Does Things", "marketplaceName": "m", "installCount": 900},
        {"pluginId": "uncounted@m", "name": "uncounted", "marketplaceName": "m"}
      ]
    }"#;

fn settings(user: &str, project: &str, local: &str) -> [HashMap<String, bool>; 3] {
    [user, project, local].map(enabled_plugins)
}

fn catalog(set: &[HashMap<String, bool>; 3]) -> Catalog {
    parse(LISTING, Path::new("/work/app"), set).unwrap()
}

fn plugin<'a>(catalog: &'a Catalog, id: &str) -> &'a Plugin {
    catalog.installed.iter().find(|p| p.id == id).unwrap()
}

#[test]
fn one_row_per_plugin_with_every_scope_it_is_installed_at() {
    let catalog = catalog(&settings("", "", ""));
    let rows: Vec<(&str, &[Scope])> = catalog
        .installed
        .iter()
        .map(|p| (p.id.as_str(), p.installed.as_slice()))
        .collect();
    // Another project's install is left out: it reaches no session here.
    assert_eq!(
        rows,
        [
            ("figma@official", &[Scope::User, Scope::Local][..]),
            ("ponytail@ponytail", &[Scope::User][..]),
        ]
    );
}

#[test]
fn a_project_turning_a_global_plugin_off_is_off_here_and_on_everywhere_else() {
    let set = settings(
        r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
        r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
        "",
    );
    let catalog = catalog(&set);
    let ponytail = plugin(&catalog, "ponytail@ponytail");
    assert!(ponytail.in_force(Scope::User));
    assert!(!ponytail.in_force(Scope::Project));
    // Local sets nothing, so it takes the project's answer.
    assert!(!ponytail.in_force(Scope::Local));
    assert_ne!(ponytail.source(Scope::Local), Some(Scope::Local));
}

#[test]
fn a_project_turning_a_plugin_off_does_not_read_back_as_off_globally() {
    // Global says nothing; the project says off. The listing's folded
    // answer is off, and it must not leak up into the Global switch.
    let set = settings("", r#"{"enabledPlugins": {"figma@official": false}}"#, "");
    let catalog = catalog(&set);
    let figma = plugin(&catalog, "figma@official");
    assert_eq!(figma.source(Scope::User), None);
    assert!(!figma.in_force(Scope::User));
    assert_eq!(figma.flip(Scope::User).verb, Verb::Enable);
}

#[test]
fn a_plugin_no_settings_file_mentions_is_off_at_every_scope() {
    let catalog = catalog(&settings("", "", ""));
    let figma = plugin(&catalog, "figma@official");
    assert!(Scope::ALL.iter().all(|s| !figma.in_force(*s)));
    assert!(Scope::ALL.iter().all(|s| figma.source(*s).is_none()));
}

#[test]
fn flipping_at_a_scope_writes_the_opposite_of_what_is_in_force_there() {
    let set = settings(
        r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
        r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
        "",
    );
    let catalog = catalog(&set);
    let ponytail = plugin(&catalog, "ponytail@ponytail");
    let verb = |scope| ponytail.flip(scope).verb;
    assert_eq!(verb(Scope::User), Verb::Disable);
    assert_eq!(verb(Scope::Project), Verb::Enable);
    assert_eq!(verb(Scope::Local), Verb::Enable);
    assert_eq!(
        ponytail.flip(Scope::Project).steps()[0],
        [
            "plugin",
            "enable",
            "ponytail@ponytail",
            "--scope",
            "project"
        ]
    );
}

#[test]
fn a_settings_file_that_cannot_be_read_sets_nothing() {
    assert!(enabled_plugins("{ not json").is_empty());
    assert!(enabled_plugins(r#"{"model": "opus"}"#).is_empty());
    assert_eq!(
        enabled_plugins(r#"{"enabledPlugins": {"a@m": false}}"#).get("a@m"),
        Some(&false)
    );
}

#[test]
fn the_marketplace_lists_the_most_installed_first_and_searches_every_field() {
    let catalog = catalog(&settings("", "", ""));
    let order: Vec<&str> = catalog.available.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(order, ["popular", "rare", "uncounted"]);
    assert!(catalog.available[0].matches("does things"));
    assert!(!catalog.available[1].matches("does things"));
}

#[test]
fn a_record_this_build_cannot_read_is_skipped_rather_than_failing_the_listing() {
    // A scope added after this was written — managed settings already
    // have one — must cost that one record, not every row in the mode.
    let listing = r#"{
          "installed": [
            {"id": "policy@corp", "scope": "managed", "enabled": true},
            {"id": "ponytail@ponytail", "scope": "user", "enabled": true}
          ],
          "available": [
            {"pluginId": "odd@m"},
            {"pluginId": "ok@m", "name": "ok", "marketplaceName": "m"}
          ]
        }"#;
    let catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
    let installed: Vec<&str> = catalog.installed.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(installed, ["ponytail@ponytail"]);
    let available: Vec<&str> = catalog.available.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(available, ["ok@m"]);
}

#[test]
fn a_listing_that_is_not_json_is_said_rather_than_read_as_empty() {
    assert!(parse("Error: not logged in", Path::new("/"), &Default::default()).is_err());
}

#[test]
fn the_scope_that_decides_is_the_narrowest_that_sets_it() {
    let set = settings(
        r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
        r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
        "",
    );
    let catalog = catalog(&set);
    let ponytail = plugin(&catalog, "ponytail@ponytail");
    assert_eq!(ponytail.source(Scope::User), Some(Scope::User));
    assert_eq!(ponytail.source(Scope::Project), Some(Scope::Project));
    assert_eq!(ponytail.source(Scope::Local), Some(Scope::Project));
    let figma = plugin(&catalog, "figma@official");
    assert_eq!(figma.source(Scope::Local), None);
}

#[test]
fn a_scope_is_offered_only_where_the_plugin_is_installed_at_it_or_wider() {
    let listing = r#"{"installed": [
            {"id": "here@m", "scope": "local", "enabled": true, "projectPath": "/work/app"},
            {"id": "shared@m", "scope": "project", "enabled": true, "projectPath": "/work/app"},
            {"id": "everywhere@m", "scope": "user", "enabled": true}
        ]}"#;
    let catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
    let reach = |id| {
        let plugin = plugin(&catalog, id);
        Scope::ALL.map(|scope| plugin.reaches(scope))
    };
    // Turning a plugin on globally where it is installed for one project
    // would write a setting that names a plugin no other project has.
    assert_eq!(reach("here@m"), [false, false, true]);
    assert_eq!(reach("shared@m"), [false, true, true]);
    assert_eq!(reach("everywhere@m"), [true, true, true]);
}

#[test]
fn a_plugin_something_unread_turns_on_is_said_to_be_decided_elsewhere() {
    // On in the end, while none of the three files mentions it: managed
    // settings or a policy did it, and the switches cannot say so.
    let listing = r#"{"installed": [
            {"id": "managed@m", "scope": "user", "enabled": true}
        ]}"#;
    let listed = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
    let managed = plugin(&listed, "managed@m");
    assert!(!managed.in_force(Scope::Local));
    assert!(managed.decided_elsewhere());
    // Where the files and the listing agree, there is nothing to say.
    let agreed = catalog(&settings("", "", ""));
    assert!(!plugin(&agreed, "figma@official").decided_elsewhere());
}

#[test]
fn a_scopes_place_in_the_list_is_its_index_into_the_settings() {
    assert!(
        Scope::ALL
            .iter()
            .enumerate()
            .all(|(i, scope)| *scope as usize == i)
    );
}

#[test]
fn a_release_is_compared_with_a_release_and_only_a_newer_one_is_offered() {
    let semver = serde_json::json!({"name": "p", "version": "1.3.0"});
    assert_eq!(
        update_to(Some("1.2.3"), None, &semver).as_deref(),
        Some("1.3.0")
    );
    assert_eq!(
        update_to(Some("v1.2.3"), None, &semver).as_deref(),
        Some("1.3.0")
    );
    assert_eq!(update_to(Some("1.3.0"), None, &semver), None);
    assert_eq!(update_to(Some("v1.3.0"), None, &semver), None);
    // An older release in the catalog is not an update.
    assert_eq!(update_to(Some("1.10.0"), None, &semver), None);
}

#[test]
fn a_pinned_commit_is_compared_with_the_commit_installed_not_the_version_label() {
    // As on this machine: the listing says `1.2.3`, the install record
    // says which commit that was, and the catalog pins a newer one.
    let pinned = serde_json::json!({"name": "p", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    let installed = "5b15a47f2d7150f545fbcacbfe381787fc0230dc";
    assert_eq!(
        update_to(Some("1.2.3"), Some(installed), &pinned).as_deref(),
        Some("c55ee46")
    );
    let current = "c55ee46073ed923f86ce59a5eb3b6d895095d1b7";
    assert_eq!(update_to(Some("1.2.3"), Some(current), &pinned), None);
    // No commit known for the install: no answer, rather than a guess.
    assert_eq!(update_to(Some("1.2.3"), None, &pinned), None);
    // A folder inside the marketplace's own repository names neither.
    let local = serde_json::json!({"name": "p", "source": "./plugins/p"});
    assert_eq!(
        update_to(Some("2a8ad9f74633"), Some(installed), &local),
        None
    );
}

#[test]
fn a_moved_commit_is_an_update_though_the_release_number_stayed() {
    let both = serde_json::json!({"name": "p", "version": "1.2.3", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    let old = "5b15a47f2d7150f545fbcacbfe381787fc0230dc";
    assert_eq!(
        update_to(Some("1.2.3"), Some(old), &both).as_deref(),
        Some("c55ee46")
    );
    // A newer release is named by its number even where a commit moved too.
    let newer_release = serde_json::json!({"name": "p", "version": "1.3.0", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    assert_eq!(
        update_to(Some("1.2.3"), Some(old), &newer_release).as_deref(),
        Some("1.3.0")
    );
}

#[test]
fn a_short_commit_on_either_side_matches_the_long_one_it_begins() {
    let pinned = serde_json::json!({"name": "p", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    assert_eq!(update_to(Some("1.2.3"), Some("c55ee46"), &pinned), None);
    assert_eq!(
        update_to(Some("1.2.3"), Some("C55EE46073ED"), &pinned),
        None
    );
    let short = serde_json::json!({"name": "p", "source": {"source": "url", "sha": "c55ee46"}});
    assert_eq!(
        update_to(
            Some("1.2.3"),
            Some("c55ee46073ed923f86ce59a5eb3b6d895095d1b7"),
            &short
        ),
        None
    );
}

#[test]
fn releases_compare_as_releases() {
    assert!(newer("1.3.0", "1.3.0-beta"));
    assert!(!newer("1.3.0-beta", "1.3.0"));
    assert!(!newer("1.2.0", "1.2"));
    assert!(!newer("1.2", "1.2.0"));
    assert!(newer("1.10", "1.9.9"));
    // Labels that are not numbers are not compared at all: a guess could
    // offer a downgrade.
    assert!(!newer("nightly", "stable"));
    assert!(!newer("1.3.0", "latest"));
}

#[test]
fn an_older_release_in_the_catalog_is_no_update_by_commit_either() {
    // A marketplace copy that has fallen behind: its release is older, and
    // its pin is a different commit — the older one. Not an update.
    let stale = serde_json::json!({"name": "p", "version": "1.0.0", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    let installed = "5b15a47f2d7150f545fbcacbfe381787fc0230dc";
    assert_eq!(update_to(Some("1.2.3"), Some(installed), &stale), None);
}

#[test]
fn each_install_is_judged_on_its_own_and_the_update_names_its_scope() {
    let listing = r#"{"installed": [
            {"id": "p@m", "scope": "user", "version": "1.2.3", "installPath": "/cache/u"},
            {"id": "p@m", "scope": "local", "version": "1.2.3", "installPath": "/cache/l",
             "projectPath": "/work/app"}
        ]}"#;
    let pinned = serde_json::json!({"name": "p", "source": {
        "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
    let current = "c55ee46073ed923f86ce59a5eb3b6d895095d1b7".to_string();
    let stale = "5b15a47f2d7150f545fbcacbfe381787fc0230dc".to_string();

    // The global copy is current and the local one is not: the update is
    // the local one's, and the row shows that copy.
    let mut catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
    let commits = HashMap::from([
        (PathBuf::from("/cache/u"), current.clone()),
        (PathBuf::from("/cache/l"), stale.clone()),
    ]);
    assess(&mut catalog.installed[0], Some(&pinned), &commits);
    let plugin = &catalog.installed[0];
    assert_eq!(
        plugin.update,
        Some(Update {
            to: "c55ee46".into(),
            scope: Scope::Local
        })
    );
    assert_eq!(plugin.install_path.as_deref(), Some(Path::new("/cache/l")));
    assert_eq!(plugin.commit.as_deref(), Some(stale.as_str()));

    // Both current: no update, and the row shows the narrowest copy — the
    // one a session here loads.
    let mut catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
    let commits = HashMap::from([
        (PathBuf::from("/cache/u"), current.clone()),
        (PathBuf::from("/cache/l"), current),
    ]);
    assess(&mut catalog.installed[0], Some(&pinned), &commits);
    assert_eq!(catalog.installed[0].update, None);
    assert_eq!(
        catalog.installed[0].install_path.as_deref(),
        Some(Path::new("/cache/l"))
    );
}

#[test]
fn the_install_record_is_read_for_its_commits_by_install_folder_and_nothing_else() {
    let record = r#"{"version": 2, "plugins": {
          "p@m": [
            {"scope": "user", "installPath": "/cache/p/1.2.3", "gitCommitSha": "5b15a47"},
            {"scope": "project", "installPath": "/cache/p/old"}
          ]}}"#;
    let commits = installed_commits(record);
    assert_eq!(
        commits.get(Path::new("/cache/p/1.2.3")).map(String::as_str),
        Some("5b15a47")
    );
    assert!(!commits.contains_key(Path::new("/cache/p/old")));
    // A record this build cannot read yields no commits, never an error.
    assert!(installed_commits("{ nope").is_empty());
    assert!(installed_commits(r#"{"version": 9, "plugins": []}"#).is_empty());
}

#[test]
fn what_is_installed_leads_the_marketplace() {
    let mut catalog = catalog(&settings("", "", ""));
    let markets = HashMap::from([(
        "ponytail".to_string(),
        vec![serde_json::json!({"name": "ponytail", "description": "Lazy mode"})],
    )]);
    complete_offers(&mut catalog, &markets);
    // Installed, so first — though it has no count and "popular" has 900.
    assert_eq!(catalog.available[0].id, "ponytail@ponytail");
    assert_eq!(catalog.available[1].name, "popular");
}

#[test]
fn a_plugin_is_named_by_its_name_and_marked_official_by_its_marketplace() {
    let catalog = catalog(&settings("", "", ""));
    let figma = plugin(&catalog, "figma@official");
    assert_eq!(figma.name(), "figma");
    assert_eq!(figma.marketplace(), "official");
    assert!(!figma.official());
}

#[test]
fn an_installed_plugin_the_listing_leaves_out_is_offered_again_from_its_catalog() {
    let mut catalog = catalog(&settings("", "", ""));
    let markets = HashMap::from([(
        "ponytail".to_string(),
        vec![serde_json::json!({"name": "ponytail", "description": "Lazy mode"})],
    )]);
    assert!(
        !catalog
            .available
            .iter()
            .any(|o| o.id == "ponytail@ponytail")
    );
    complete_offers(&mut catalog, &markets);
    let offer = catalog
        .available
        .iter()
        .find(|o| o.id == "ponytail@ponytail")
        .unwrap();
    assert_eq!(offer.description, "Lazy mode");
    // No count was guessed, so it sorts after every counted offer.
    assert_eq!(offer.install_count, None);
    assert!(offer.matches("lazy"));
    // An installed plugin its catalog does not name is left out, not made up.
    assert!(!catalog.available.iter().any(|o| o.id == "figma@official"));
}

#[test]
fn an_install_never_accepts_a_declared_command_unseen() {
    let change = Change {
        id: "x@m".into(),
        scope: Scope::Project,
        verb: Verb::Install,
    };
    let args = &change.steps()[0];
    assert_eq!(args, &["plugin", "install", "x@m", "--scope", "project"]);
    assert!(!args.iter().any(|a| a == "-y" || a == "--yes"));
}

#[test]
fn an_uninstall_keeps_the_plugins_data() {
    let change = Change {
        id: "x@m".into(),
        scope: Scope::User,
        verb: Verb::Uninstall,
    };
    assert_eq!(change.steps()[0][3..], ["--scope", "user", "--keep-data"]);
}

#[test]
fn a_move_installs_before_it_removes_and_keeps_the_data() {
    let change = Change {
        id: "x@m".into(),
        scope: Scope::User,
        verb: Verb::Move(Scope::Local),
    };
    assert_eq!(
        change.steps(),
        [
            vec!["plugin", "install", "x@m", "--scope", "user"],
            vec![
                "plugin",
                "uninstall",
                "x@m",
                "--scope",
                "local",
                "--keep-data"
            ],
        ]
    );
}

#[test]
fn checking_for_updates_fetches_the_plugins_own_marketplace() {
    let change = Change {
        id: "x@official".into(),
        scope: Scope::User,
        verb: Verb::CheckUpdates,
    };
    assert_eq!(
        change.steps(),
        [vec!["plugin", "marketplace", "update", "official"]]
    );
}

#[test]
fn an_update_that_changed_nothing_is_said_rather_than_passed_as_done() {
    let change = Change {
        id: "x@m".into(),
        scope: Scope::User,
        verb: Verb::Update,
    };
    assert_eq!(change.steps()[0][3..], ["--scope", "user", "--json"]);
    let up_to_date = r#"{"command":"update","outcome":"ok","updateOutcome":"up_to_date","message":"x is already at the latest version (1.2.3)."}"#;
    let why = updated(up_to_date).unwrap_err();
    assert!(why.starts_with("x is already at the latest version (1.2.3)."));
    assert!(updated(r#"{"outcome":"ok","updateOutcome":"updated"}"#).is_ok());
    assert!(updated("not json").is_ok());
}
