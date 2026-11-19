use super::*;
use crate::config::{AgentSpec, ProjectConfig};

fn project(path: &str) -> ProjectConfig {
    ProjectConfig {
        path: path.into(),
        ..Default::default()
    }
}

fn paths(cfg: &WorkspaceConfig) -> Vec<PathBuf> {
    cfg.projects.iter().map(|p| p.path.clone()).collect()
}

/// `cfg` with a project `/new` put ahead of the others.
fn one_ahead(mut cfg: WorkspaceConfig) -> WorkspaceConfig {
    cfg.projects.insert(0, project("/new"));
    cfg
}

fn spec(name: &str) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        command: "x".into(),
        args: vec![],
        ..Default::default()
    }
}

#[test]
fn a_root_added_quietly_leaves_the_selection_alone() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.select_root(0);
    assert_eq!(ws.add_root_quietly("/run"), 2);
    assert_eq!(ws.active_root, 0);
    assert_eq!(
        ws.add_root_quietly("/b"),
        1,
        "an open project is not doubled"
    );
    assert_eq!(ws.active_root, 0);
}

#[test]
fn label_uses_last_component() {
    assert_eq!(label_for(Path::new("/home/me/proj")), "proj");
    assert_eq!(label_for(Path::new("/")), "/");
}

#[test]
fn the_check_command_is_kept_by_path() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.roots[1].check = Some("cargo test".into());
    let cfg = ws.to_config();
    assert_eq!(cfg.projects[1].check.as_deref(), Some("cargo test"));
    let back = Workspace::from_config(one_ahead(cfg), PathBuf::from("/store"), &Default::default());
    assert!(back.roots[1].check.is_none());
    assert_eq!(back.roots[2].check.as_deref(), Some("cargo test"));
}

#[test]
fn a_run_root_arrives_with_its_session_and_moves_nothing() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.select_root(0);
    let spec = AgentSpec {
        name: "x".into(),
        command: "x".into(),
        args: vec![],
        ..Default::default()
    };
    let idx = ws.add_transient_root("/run", spec.clone(), 7).unwrap();
    assert_eq!(
        ws.active_root, 0,
        "the selection stays where the user left it"
    );
    assert!(ws.roots[idx].transient);
    assert_eq!(ws.roots[idx].sessions.len(), 1);
    assert_eq!(ws.roots[idx].sessions[0].uid, 7);
    // A folder that is already a project is refused, not marked transient.
    assert!(ws.add_transient_root("/b", spec, 8).is_none());
    assert!(!ws.roots[1].transient);
}

#[test]
fn a_transient_root_is_never_written() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    let run = ws.add_root("/run");
    ws.roots[run].transient = true;
    ws.select_root(1);
    let cfg = ws.to_config();
    assert_eq!(paths(&cfg), [PathBuf::from("/a"), PathBuf::from("/b")]);
    assert_eq!(cfg.active.as_deref(), Some("/b"));
    ws.select_root(run);
    assert_eq!(ws.to_config().active.as_deref(), Some("/a"));
    ws.roots[run].transient = false;
    assert_eq!(ws.to_config().projects.len(), 3);
}

#[test]
fn remove_root_keeps_active_in_bounds() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c"); // active = 2
    ws.remove_root(2);
    assert_eq!(ws.active_root, 1);
    ws.remove_root(0);
    assert_eq!(ws.active_root, 0);
    assert_eq!(ws.roots.len(), 1);
}

#[test]
fn removing_below_active_shifts_active_down() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c");
    ws.select_root(2);
    ws.remove_root(0); // remove below active
    assert_eq!(ws.active_root, 1); // still points at /c
    assert_eq!(ws.roots[1].path, PathBuf::from("/c"));
}

#[test]
fn add_root_dedupes_an_existing_path() {
    // Adding a folder that's already a root selects it instead of creating
    // a twin (per-root state is keyed by path — twins would alias it).
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.select_root(0);
    let i = ws.add_root("/b");
    assert_eq!(ws.roots.len(), 2);
    assert_eq!(i, 1);
    assert_eq!(ws.active_root, 1);
}

/// Pinning reorders the *drawing*, never the roots.
///
/// Every caller addresses a root by its index -- select it, remove it, add
/// a session to it -- so if a pin moved roots in the list, a click would
/// land on the project next to the one clicked.
#[test]
fn pinning_reorders_the_drawing_and_nothing_else() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c");
    let paths: Vec<_> = ws.roots.iter().map(|r| r.path.clone()).collect();

    ws.toggle_pin(2);
    assert_eq!(ws.display_order(), vec![2, 0, 1]);
    assert_eq!(
        ws.roots.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
        paths,
        "the roots themselves must not move"
    );
}

/// A stable partition, so unpinning puts a project back where it was rather
/// than at the end of the list.
#[test]
fn unpinning_restores_the_original_place() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c");
    ws.toggle_pin(1);
    assert_eq!(ws.display_order(), vec![1, 0, 2]);
    ws.toggle_pin(1);
    assert_eq!(ws.display_order(), vec![0, 1, 2]);
}

/// A dropped project lands where it was dropped, and `active_root` still
/// names the project the user was in.
#[test]
fn moving_a_root_reorders_the_list_and_follows_the_active_one() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c");
    ws.select_root(0);

    ws.move_root(0, 2);
    assert_eq!(
        ws.roots.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
        [
            PathBuf::from("/b"),
            PathBuf::from("/c"),
            PathBuf::from("/a")
        ]
    );
    assert_eq!(
        ws.active_root, 2,
        "the project on screen must stay on screen"
    );
}

/// Pinned roots are drawn first, so a drag out of that group is clamped to
/// its edge rather than silently doing nothing.
#[test]
fn a_drag_across_the_pin_line_stops_at_it() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.add_root("/c");
    ws.toggle_pin(0);
    ws.toggle_pin(1);
    assert_eq!(ws.display_order(), vec![0, 1, 2]);

    // Display position 0 is pinned `/a`; position 2 is unpinned `/c`.
    ws.move_root(0, 2);
    assert_eq!(
        ws.roots.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
        [
            PathBuf::from("/b"),
            PathBuf::from("/a"),
            PathBuf::from("/c")
        ],
        "it goes last among the pinned, not past them"
    );
    assert!(ws.roots[1].pinned, "and it is still pinned");
}

/// A move put back to the order last saved restores every root's place,
/// the one on screen following its own.
#[test]
fn an_order_put_back_is_the_one_saved() {
    let mut ws = Workspace::seeded("/a");
    for path in ["/b", "/c"] {
        ws.add_root(path);
    }
    let saved = ws.root_order();
    ws.move_root(0, 2);
    ws.move_root(0, 1);
    ws.add_root("/d");
    ws.active_root = ws
        .roots
        .iter()
        .position(|r| r.path == Path::new("/a"))
        .unwrap();
    ws.put_order(&saved);
    let order: Vec<_> = ws.root_order().into_iter().map(|(_, path)| path).collect();
    assert_eq!(
        order,
        ["/a", "/b", "/c", "/d"].map(PathBuf::from),
        "a root added since stays after the ones saved"
    );
    assert_eq!(ws.roots[ws.active_root].path, PathBuf::from("/a"));
}

/// A move put back after its write fails goes back from where the root
/// landed: undoing from where it was dropped would move another project.
#[test]
fn a_clamped_move_is_put_back_from_where_it_landed() {
    let mut ws = Workspace::seeded("/a");
    for path in ["/b", "/c", "/d"] {
        ws.add_root(path);
    }
    ws.toggle_pin(0);
    ws.toggle_pin(1);
    let paths = |ws: &Workspace| ws.roots.iter().map(|r| r.path.clone()).collect::<Vec<_>>();
    let before = paths(&ws);

    // Pinned `/a` dropped onto unpinned `/d` lands last among the pinned.
    let landed = ws.move_root(0, 3);
    assert_eq!(landed, Some(1));
    ws.move_root(landed.unwrap(), 0);
    assert_eq!(paths(&ws), before);

    // A drop that clamps back onto itself moves nothing, and says so.
    ws.toggle_pin(1);
    assert_eq!(ws.move_root(0, 2), None);
    assert_eq!(paths(&ws), before);
}

/// Reordering sessions keeps the one on screen on screen -- the index is
/// what says which that is.
#[test]
fn moving_a_session_carries_the_active_index_with_it() {
    let mut ws = Workspace::seeded("/a");
    ws.add_session(spec("one"), 1);
    ws.add_session(spec("two"), 2);
    ws.add_session(spec("three"), 3);
    ws.select_session(0);

    ws.move_session(0, 2, 0);
    let uids: Vec<_> = ws.roots[0].sessions.iter().map(|s| s.uid).collect();
    assert_eq!(uids, [3, 1, 2]);
    assert_eq!(ws.roots[0].active_session, 1);

    ws.move_session(0, 1, 2);
    assert_eq!(
        ws.roots[0].active_session, 2,
        "the moved one is the active one"
    );
}

/// Pins survive a save/load, and follow their project rather than its slot.
#[test]
fn pins_are_stored_by_path() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.toggle_pin(1);

    let cfg = ws.to_config();
    assert!(!cfg.projects[0].pinned && cfg.projects[1].pinned);
    // A project put ahead of it in the file must not slide the pin onto the
    // project that took its place.
    let back = Workspace::from_config(one_ahead(cfg), PathBuf::from("/store"), &Default::default());
    let pinned: Vec<_> = back
        .roots
        .iter()
        .filter(|r| r.pinned)
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(pinned, vec![PathBuf::from("/b")]);
}

#[test]
fn from_config_dedupes_and_follows_active_path() {
    let cfg = WorkspaceConfig {
        name: "W".into(),
        active: Some("/a".into()),
        projects: vec![project("/b"), project("/a"), project("/a")],
        layout: PanelLayout::default(),
    };
    let ws = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert_eq!(ws.roots.len(), 2);
    assert_eq!(ws.active_root, 1); // follows /a to its surviving slot
}

#[test]
fn config_roundtrips_roots_and_active() {
    let mut ws = Workspace::seeded("/a");
    ws.name = "Mine".into();
    ws.add_root("/b");
    ws.add_root("/c");
    ws.select_root(1);
    let cfg = ws.to_config();
    assert_eq!(cfg.name, "Mine");
    assert_eq!(
        paths(&cfg),
        vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c")
        ]
    );
    assert_eq!(cfg.active.as_deref(), Some("/b"));

    let back = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert_eq!(back.name, "Mine");
    assert_eq!(back.roots.len(), 3);
    assert_eq!(back.active_root, 1);
    assert_eq!(back.storage_dir, PathBuf::from("/store"));
    assert!(back.roots[0].sessions.is_empty());
}

#[test]
fn active_names_a_project_by_key() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    ws.select_root(1);
    let back = Workspace::from_config(one_ahead(ws.to_config()), PathBuf::from("/store"), &Default::default());
    assert_eq!(back.roots[back.active_root].path, PathBuf::from("/b"));
}

#[test]
fn an_active_naming_nothing_reads_as_the_first() {
    let cfg = WorkspaceConfig {
        active: Some("/gone".into()),
        projects: vec![project("/a"), project("/b")],
        ..Default::default()
    };
    assert_eq!(
        Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default()).active_root,
        0
    );
}

/// A hand-edited or stale layout must not restore a panel that is open,
/// reachable by its shortcut, and too small to see — or one that covers the
/// window because it was last sized on a much bigger monitor.
#[test]
fn a_nonsense_layout_is_clamped_but_its_flags_are_kept() {
    let cfg = WorkspaceConfig {
        name: "W".into(),
        active: None,
        projects: vec![project("/a")],
        layout: PanelLayout {
            workbench_w: 0.0,
            workbench_open: true,
            terminal_h: 99_999.0,
            terminal_open: false,
            rail_w: 9_999.0,
        },
    };
    let ws = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert!(ws.layout.workbench_w >= 120.0);
    assert!(ws.layout.terminal_h <= 2000.0);
    // The rail has a narrower range of its own: it is sized by what its
    // rows have to say, not by preference.
    assert!(ws.layout.rail_w <= PanelLayout::RAIL_MAX);
    // Sizes are corrected; what the user chose to have open is not.
    assert!(ws.layout.workbench_open);
    assert!(!ws.layout.terminal_open);
}

#[test]
fn layout_survives_a_config_round_trip() {
    let mut ws = Workspace::seeded("/a");
    ws.layout = PanelLayout {
        workbench_w: 512.0,
        workbench_open: true,
        terminal_h: 300.0,
        terminal_open: true,
        rail_w: 280.0,
    };
    let back = Workspace::from_config(ws.to_config(), PathBuf::from("/store"), &Default::default());
    assert_eq!(back.layout, ws.layout);
}

#[test]
fn from_config_clamps_out_of_bounds_active() {
    let cfg = WorkspaceConfig {
        name: String::new(),
        active: Some("/elsewhere".into()),
        projects: vec![project("/a")],
        layout: PanelLayout::default(),
    };
    let ws = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert_eq!(ws.active_root, 0);
    assert_eq!(ws.name, "Workspace"); // empty name falls back
}

#[test]
fn canon_dir_keeps_unresolvable_path() {
    let p = PathBuf::from("/definitely/not/a/real/dir-xyz");
    assert_eq!(canon_dir(p.clone()), p);
}

#[test]
fn canon_dir_collapses_dot_dot_alias() {
    let tmp = std::env::temp_dir();
    let sub = tmp.join(format!("onehand-canon-test-{}", std::process::id()));
    std::fs::create_dir_all(&sub).unwrap();
    let alias = sub.join("..").join(sub.file_name().unwrap());
    assert_eq!(canon_dir(alias), canon_dir(sub.clone()));
    let _ = std::fs::remove_dir_all(&sub);
}

#[test]
fn storage_for_is_stable_outside_the_project_and_split_by_path() {
    let a = storage_for(Path::new("/home/me/code/app"));
    assert_eq!(a, storage_for(Path::new("/home/me/code/app")));
    assert!(!a.starts_with("/home/me/code/app"));
    assert!(
        a.file_name().unwrap().to_string_lossy().starts_with("app-"),
        "the folder's own name has to be readable in {a:?}"
    );
    // Same folder name, different checkout: two workspaces, not one.
    assert_ne!(a, storage_for(Path::new("/home/me/other/app")));
}

#[test]
fn add_and_close_session_tracks_active() {
    let mut ws = Workspace::seeded("/a");
    ws.add_session(spec("one"), 1);
    ws.add_session(spec("two"), 2);
    assert_eq!(ws.active_root().unwrap().active_session, 1);
    ws.close_session(0, 1);
    assert_eq!(ws.active_root().unwrap().active_session, 0);
    ws.close_session(0, 0);
    assert!(ws.active_root().unwrap().sessions.is_empty());
    assert_eq!(ws.active_root().unwrap().active_session, 0);
}

#[test]
fn a_host_root_round_trips_through_the_workspace_file() {
    let mut ws = Workspace::seeded("/local");
    let i = ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app/"));
    ws.roots[i].pinned = true;
    ws.roots[i].check = Some("make test".into());
    let text = toml::to_string(&ws.to_config()).unwrap();
    assert!(text.contains("host = \"vps\""), "{text}");
    let back = Workspace::from_config(toml::from_str(&text).unwrap(), PathBuf::from("/s"), &Default::default());
    let root = &back.roots[1];
    assert_eq!(root.on_host(), Some(&HostName("vps".into())));
    assert_eq!(root.path, PathBuf::from("/srv/app/"));
    assert_eq!(root.label, "app");
    assert!(root.pinned);
    assert_eq!(root.check.as_deref(), Some("make test"));
    assert!(back.roots[0].on_host().is_none());
}

#[test]
fn a_local_and_a_host_root_on_one_path_both_survive_a_reload() {
    let mut ws = Workspace::seeded("/srv/app");
    let i = ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    ws.roots[i].pinned = true;
    let text = toml::to_string(&ws.to_config()).unwrap();
    let back = Workspace::from_config(toml::from_str(&text).unwrap(), PathBuf::from("/s"), &Default::default());
    assert_eq!(back.roots.len(), 2);
    assert!(back.roots[0].on_host().is_none() && !back.roots[0].pinned);
    assert!(back.roots[1].on_host().is_some() && back.roots[1].pinned);
}

#[test]
fn a_host_root_and_a_local_root_with_one_path_are_two_roots() {
    let mut ws = Workspace::seeded("/srv/app");
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    assert_eq!(ws.roots.len(), 2);
    assert_eq!(ws.add_root("/srv/app"), 0);
    assert_eq!(ws.roots.len(), 2);
    assert!(ws.roots[0].on_host().is_none());
    assert!(ws.roots[1].on_host().is_some());
}

#[test]
fn adding_the_same_host_root_twice_selects_it() {
    let mut ws = Workspace::seeded("/a");
    let first = ws.add_host_root(HostName("vps".into()), PathBuf::from("/x"));
    ws.add_root_quietly("/b");
    let second = ws.add_host_root(HostName("vps".into()), PathBuf::from("/x"));
    assert_eq!(first, second);
    assert_eq!(ws.active_root, first);
    assert_eq!(ws.roots.len(), 3);
    let other = ws.add_host_root(HostName("box".into()), PathBuf::from("/x"));
    assert_eq!(ws.roots.len(), 4);
    assert_ne!(other, first);
}

#[test]
fn a_workspace_file_without_hosts_still_reads() {
    let cfg: WorkspaceConfig = toml::from_str("roots = [\"/a\"]\n").unwrap();
    let ws = Workspace::from_config(cfg, PathBuf::from("/s"), &Default::default());
    assert!(ws.roots[0].on_host().is_none());
}

#[test]
fn the_host_named_local_is_this_machine() {
    let local = HostName("local".into()).addr();
    assert!(local.local);
    assert_eq!(local.ssh, "local");
    assert!(!HostName("vps".into()).addr().local);
    assert_eq!(HostName("vps".into()).to_string(), "vps");
}

#[test]
fn only_a_folder_on_this_machine_has_a_local_path() {
    let mut ws = Workspace::seeded("/tmp");
    let here = 0;
    let there = ws.add_host_root(HostName("vps".into()), PathBuf::from("/tmp"));
    assert_eq!(ws.roots[here].local_path(), Some(Path::new("/tmp")));
    assert_eq!(ws.roots[there].local_path(), None);
}

/// A host name may hold colons of its own (an IPv6 address, bare or in
/// brackets after a user); the path is what follows the first `:/`.
#[test]
fn a_host_name_with_colons_round_trips_through_its_key() {
    for host in ["fe80::1", "user@[fe80::1]", "vps"] {
        let host = HostName(host.into());
        let key = PathBuf::from(host_path(Some(&host), Path::new("/srv/app")));
        assert_eq!(
            parse_root_key(&key),
            (Some(host.clone()), PathBuf::from("/srv/app")),
            "{key:?}"
        );
    }
}

/// A folder is named with its host as ssh names one, and bare here.
#[test]
fn a_host_folder_is_named_on_its_host() {
    let vps = HostName("vps".into());
    assert_eq!(host_path(Some(&vps), Path::new("/srv/app")), "vps:/srv/app");
    assert_eq!(host_path(None, Path::new("/srv/app")), "/srv/app");
}

#[test]
fn an_offloaded_root_is_kept_with_its_host_and_mirror() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    let on = Offloaded {
        host: HostName("box".into()),
        mirror: PathBuf::from("/srv/mirrors/l/b-1"),
        branch: None,
    };
    ws.roots[1].offload = Some(on.clone());
    let cfg = ws.to_config();
    assert_eq!(cfg.projects[1].offload.as_ref(), Some(&on));
    let back = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert!(back.roots[0].offload.is_none());
    assert_eq!(back.roots[1].offload, Some(on.clone()));
    // Its sessions run in the mirror; its folder stays this machine's.
    assert_eq!(
        back.roots[1].session_host(),
        Some((&on.host, on.mirror.as_path()))
    );
    assert_eq!(back.roots[1].local_path(), Some(Path::new("/b")));
    assert!(!back.roots[1].runs_here() && back.roots[0].runs_here());
}

/// A root on a branch keeps its branch beside its host and mirror; one kept
/// before branches reads as a mirror's.
#[test]
fn an_offloaded_root_on_a_branch_keeps_its_branch() {
    let mut ws = Workspace::seeded("/a");
    ws.add_root("/b");
    let on = Offloaded {
        host: HostName("box".into()),
        mirror: PathBuf::from("/srv/mirrors/l/b-1.branch"),
        branch: Some("onehand/box".into()),
    };
    ws.roots[1].offload = Some(on.clone());
    ws.roots[0].offload = Some(Offloaded {
        branch: None,
        mirror: PathBuf::from("/srv/mirrors/l/a-1"),
        ..on.clone()
    });
    let text = toml::to_string(&ws.to_config()).unwrap();
    let back = Workspace::from_config(toml::from_str(&text).unwrap(), PathBuf::from("/store"), &Default::default());
    assert_eq!(back.roots[1].offload, Some(on));
    assert_eq!(back.roots[0].offload.as_ref().unwrap().branch, None);
}

#[test]
fn a_host_root_never_carries_an_offload() {
    let mut ws = Workspace::seeded("/a");
    ws.add_host_root(HostName("box".into()), PathBuf::from("/srv/p"));
    let mut cfg = ws.to_config();
    cfg.projects[1].offload = Some(Offloaded {
        host: HostName("box".into()),
        mirror: "/srv/m".into(),
        branch: None,
    });
    let back = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert!(back.roots[1].offload.is_none());
    assert_eq!(
        back.roots[1].session_host(),
        Some((&HostName("box".into()), Path::new("/srv/p")))
    );
    assert!(back.roots[0].session_host().is_none());
}

// The session lifecycle's edits, as the shell makes them today.

#[test]
fn a_session_needs_a_project_to_be_added_to() {
    let mut ws = Workspace::seeded("/a");
    ws.remove_root(0);
    assert_eq!(ws.add_session(spec("one"), 1), None);
}

/// A session added quietly goes only to a folder on this machine: a host's
/// folder on the same path is another project.
#[test]
fn a_session_added_quietly_finds_only_a_local_root() {
    let mut ws = Workspace::seeded("/a");
    ws.remove_root(0);
    ws.add_host_root(HostName("box".into()), PathBuf::from("/p"));
    assert_eq!(
        ws.add_session_quietly(Path::new("/p"), spec("one"), 1),
        None
    );
    let local = ws.add_root("/p");
    ws.select_root(0);
    assert_eq!(
        ws.add_session_quietly(Path::new("/p"), spec("one"), 1),
        Some(local)
    );
    assert_eq!(ws.active_root, 0, "nothing selected");
    assert_eq!(ws.roots[local].active_session, 0);
}

#[test]
fn closing_a_session_below_the_one_shown_keeps_it_shown() {
    let mut ws = Workspace::seeded("/a");
    for uid in 1..=3 {
        ws.add_session(spec("s"), uid);
    }
    ws.close_session(0, 0);
    let root = ws.active_root().unwrap();
    assert_eq!(root.active_session, 1);
    assert_eq!(root.sessions[1].uid, 3);
}

#[test]
fn a_session_out_of_range_is_neither_closed_nor_selected() {
    let mut ws = Workspace::seeded("/a");
    ws.add_session(spec("s"), 1);
    ws.close_session(0, 5);
    ws.close_session(7, 0);
    ws.select_session(5);
    let root = ws.active_root().unwrap();
    assert_eq!(root.sessions.len(), 1);
    assert_eq!(root.active_session, 0);
}

/// An order is kept by host and path, so a local and a host root on one path
/// each go back to their own place.
#[test]
fn an_order_put_back_tells_a_local_and_a_host_root_on_one_path_apart() {
    let mut ws = Workspace::seeded("/p");
    ws.add_root("/q");
    ws.add_host_root(HostName("box".into()), PathBuf::from("/p"));
    let saved = ws.root_order();
    ws.move_root(2, 0);
    ws.put_order(&saved);
    let kept: Vec<_> = ws
        .roots
        .iter()
        .map(|r| (r.host.is_some(), r.path.clone()))
        .collect();
    assert_eq!(
        kept,
        [
            (false, PathBuf::from("/p")),
            (false, "/q".into()),
            (true, "/p".into())
        ],
        "the host root goes back after the local one"
    );
}

#[test]
fn seeded_is_kept_beside_the_other_workspaces() {
    let ws = Workspace::seeded("/x/proj");
    assert_eq!(ws.storage_dir, storage_for(Path::new("/x/proj")));
    assert_eq!(
        ws.storage_dir.parent(),
        Some(crate::config::workspaces_dir().as_path())
    );
}

/// Deleting a workspace removes what is its own and nothing else: a
/// project's offload kept in the same folder by an earlier build stays.
#[test]
fn deleting_a_workspace_leaves_what_is_not_its_own() {
    let dir = std::env::temp_dir().join(format!("onehand-delete-ws-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    WorkspaceConfig::default().save_to(&dir).unwrap();
    std::fs::create_dir_all(dir.join("issues")).unwrap();
    std::fs::write(dir.join("issues").join("p-1.json"), "[]").unwrap();
    std::fs::create_dir_all(dir.join("coming-back")).unwrap();
    std::fs::write(dir.join("coming-back").join("f"), "f").unwrap();
    assert!(
        remove_workspace_blocking(&dir).is_err(),
        "says what it left"
    );
    assert!(!dir.join(WorkspaceConfig::FILE).exists());
    assert!(!dir.join("issues").exists());
    assert!(dir.join("coming-back").join("f").is_file());
    std::fs::remove_dir_all(dir.join("coming-back")).unwrap();
    WorkspaceConfig::default().save_to(&dir).unwrap();
    assert!(remove_workspace_blocking(&dir).is_ok());
    assert!(!dir.exists());
}

fn builds_on(host: &str, artifacts: &[&str], into: Option<&str>) -> BuildHost {
    BuildHost {
        host: HostName(host.into()),
        artifacts: artifacts.iter().map(PathBuf::from).collect(),
        into: into.map(PathBuf::from),
    }
}

#[test]
fn build_hosts_round_trip_through_the_workspace_file() {
    let mut ws = Workspace::seeded("/a");
    ws.roots[0].builds = vec![
        builds_on("build01", &["target/release/app", "dist"], None),
        builds_on("win", &["bin/Release"], Some("out/windows")),
    ];
    let text = toml::to_string(&ws.to_config()).expect("serializes");
    assert!(text.contains("builds"), "{text}");
    let cfg: WorkspaceConfig = toml::from_str(&text).expect("parses");
    let back = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert_eq!(back.roots[0].builds, ws.roots[0].builds);
}

#[test]
fn a_project_with_no_build_hosts_writes_none() {
    let text = toml::to_string(&Workspace::seeded("/a").to_config()).expect("serializes");
    assert!(!text.contains("builds"), "{text}");
}

#[test]
fn parse_reads_one_artifact_a_line() {
    let entry = BuildHost::parse(" build01 ", "a/b\n\n c \n", "").expect("reads");
    assert_eq!(entry, builds_on("build01", &["a/b", "c"], None));
    let entry = BuildHost::parse("build01", "", " out/linux ").expect("reads");
    assert_eq!(entry, builds_on("build01", &[], Some("out/linux")));
}

#[test]
fn parse_refuses_what_is_not_a_path_inside_the_project() {
    let refused = |host: &str, artifacts: &str, into: &str| {
        BuildHost::parse(host, artifacts, into).expect_err("refused")
    };
    assert_eq!(refused("  ", "a", ""), "Name the host");
    assert_eq!(
        refused("h", "/etc/passwd", ""),
        "/etc/passwd must be inside the project"
    );
    assert_eq!(
        refused("h", "a/../../b", ""),
        "a/../../b must be inside the project"
    );
    assert_eq!(
        refused("h", "bin/*.dll", ""),
        "bin/*.dll is a pattern; name a file or a folder"
    );
    assert_eq!(
        refused("h", "a", "../out"),
        "../out must be inside the project"
    );
    assert_eq!(
        refused("h", "a", "out/[x]"),
        "out/[x] is a pattern; name a file or a folder"
    );
}

#[test]
fn the_project_itself_and_gits_own_folder_are_never_an_artifact() {
    for path in [".", "./", "a/..", ""] {
        assert!(
            inside_refusal(path).is_some(),
            "{path:?} names no file or folder inside"
        );
    }
    assert_eq!(
        inside_refusal(".git").as_deref(),
        Some(".git is git's own folder; it cannot be brought back")
    );
    assert_eq!(
        inside_refusal("sub/.git/hooks").as_deref(),
        Some("sub/.git/hooks is git's own folder; it cannot be brought back")
    );
    assert_eq!(inside_refusal(".github/workflows"), None);
    assert_eq!(inside_refusal("./target/app"), None);
}

#[test]
fn a_project_whose_sessions_run_on_a_host_has_no_build_hosts_but_keeps_them() {
    let mut ws = Workspace::seeded("/a");
    ws.roots[0].builds = vec![builds_on("build01", &["dist"], None)];
    assert_eq!(ws.roots[0].build_hosts().len(), 1);
    ws.roots[0].offload = Some(Offloaded {
        host: HostName("box".into()),
        mirror: "/m".into(),
        branch: None,
    });
    assert!(ws.roots[0].build_hosts().is_empty());
    // Kept in the file all the same, for when it runs here again.
    let text = toml::to_string(&ws.to_config()).expect("serializes");
    let cfg: WorkspaceConfig = toml::from_str(&text).expect("parses");
    let mut back = Workspace::from_config(cfg, PathBuf::from("/store"), &Default::default());
    assert!(back.roots[0].build_hosts().is_empty());
    back.roots[0].offload = None;
    assert_eq!(back.roots[0].build_hosts(), ws.roots[0].builds);
}

#[test]
fn an_artifact_is_written_with_forward_slashes() {
    assert_eq!(
        inside_refusal(r"bin\Release").as_deref(),
        Some(r"bin\Release is written with \; write it with /")
    );
    assert_eq!(inside_refusal("bin/Release"), None);
}

#[test]
fn a_project_uses_the_host_its_sessions_run_on_and_the_hosts_it_builds_on() {
    let mut ws = Workspace::seeded("/a");
    ws.roots[0].builds = vec![builds_on("build01", &[], None), builds_on("win", &[], None)];
    let used = |root: &ProjectRoot| -> Vec<String> {
        root.hosts_used()
            .into_iter()
            .map(|host| host.0.clone())
            .collect()
    };
    assert_eq!(used(&ws.roots[0]), ["build01", "win"]);
    // Offloaded, its sessions' host is the one it uses; it builds there.
    ws.roots[0].offload = Some(Offloaded {
        host: HostName("box".into()),
        mirror: "/m".into(),
        branch: None,
    });
    assert_eq!(used(&ws.roots[0]), ["box"]);
    assert!(used(&Workspace::seeded("/b").roots[0]).is_empty());
}

// Rows and their projects.

fn worktree(path: &str) -> crate::project::Worktree {
    crate::project::Worktree {
        path: path.into(),
        branch: "x".into(),
        from: None,
        made_by: crate::project::MadeBy::Split,
        offload: None,
    }
}

fn entry(id: Option<&crate::project::ProjectId>, path: &str) -> ProjectConfig {
    ProjectConfig {
        id: id.cloned(),
        ..project(path)
    }
}

#[test]
fn rows_take_their_facts_from_their_project() {
    use crate::project::{Project, ProjectId, Projects};
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.name = "Alpha".into();
    p.check = Some("make".into());
    p.worktrees.push(worktree("/code/a-x"));
    let projects = Projects::from_iter([p.clone()]);
    let cfg = WorkspaceConfig {
        projects: vec![entry(Some(&p.id), "/code/a"), entry(Some(&p.id), "/code/a-x")],
        ..Default::default()
    };
    let ws = Workspace::from_config(cfg, "/ws".into(), &projects);
    assert_eq!(ws.roots[0].label, "Alpha");
    assert_eq!(ws.roots[1].label, "a-x");
    assert_eq!(ws.roots[1].check.as_deref(), Some("make"));
    assert_eq!(ws.roots[1].project.as_ref(), Some(&p.id));
}

#[test]
fn an_entry_without_an_id_finds_its_project_by_path() {
    use crate::project::{Project, ProjectId, Projects};
    let p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    let cfg = WorkspaceConfig {
        projects: vec![entry(None, "/code/a")],
        ..Default::default()
    };
    let ws = Workspace::from_config(cfg, "/ws".into(), &Projects::from_iter([p.clone()]));
    assert_eq!(ws.roots[0].project, Some(p.id));
}

#[test]
fn an_entry_no_project_holds_keeps_and_writes_its_own_facts() {
    let cfg = WorkspaceConfig {
        projects: vec![ProjectConfig {
            check: Some("make".into()),
            ..project("/code/lone")
        }],
        ..Default::default()
    };
    let ws = Workspace::from_config(cfg, "/ws".into(), &Default::default());
    assert_eq!(ws.roots[0].project, None);
    assert_eq!(ws.roots[0].check.as_deref(), Some("make"));
    assert_eq!(ws.to_config().projects[0].check.as_deref(), Some("make"));
}

#[test]
fn a_placed_row_writes_its_id_and_leaves_its_facts_to_its_project() {
    use crate::project::{Folder, Project, ProjectId};
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.check = Some("make".into());
    let mut ws = Workspace::seeded_on(&p);
    let mut transient = ProjectRoot::new("/worker");
    transient.transient = true;
    ws.roots.push(transient);
    let cfg = ws.to_config();
    assert_eq!(cfg.projects.len(), 1, "a transient row is left out");
    assert_eq!(cfg.projects[0].id, Some(p.id.clone()));
    assert_eq!(cfg.projects[0].check, None);
    assert_eq!(ProjectRoot::of(&p, Folder::Home).check.as_deref(), Some("make"));
}

#[test]
fn offload_data_is_kept_by_folder() {
    let dir = offload_dir(Path::new("/code/onehand"));
    assert_eq!(
        dir,
        crate::config::config_dir()
            .join("offload")
            .join(stem_for(Path::new("/code/onehand")))
    );
}
