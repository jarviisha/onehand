use super::*;

#[test]
fn session_cycle_ends_when_the_remapped_modifier_is_released() {
    let modifiers = gpui::Keystroke::parse("alt-tab").unwrap().modifiers;
    let cycle = TabCycle {
        order: vec![1, 2, 3],
        pos: 0,
        modifiers,
    };
    assert!(cycle.held(modifiers));
    assert!(cycle.held(gpui::Keystroke::parse("alt-shift-tab").unwrap().modifiers));
    assert!(!cycle.held(gpui::Modifiers::default()));
    assert!(!cycle.held(gpui::Keystroke::parse("ctrl-tab").unwrap().modifiers));
    let direct = TabCycle {
        modifiers: gpui::Modifiers::default(),
        ..cycle
    };
    assert!(!direct.held(gpui::Modifiers::default()));
}

/// Give `drive` `git` as read, as a sweep would.
fn swept(
    drive: &mut onehand_core::workspace::WorkspaceDrive,
    git: Vec<(std::path::PathBuf, onehand_core::gitstat::GitStatus)>,
) {
    use onehand_core::workspace::drive::{Effect, Input};
    let Effect::Sweep { token, .. } = drive.sweep(|_| true) else {
        unreachable!("a sweep is a sweep");
    };
    let _ = drive.handle(Input::Swept { token, found: git });
}

fn status(branch: &str) -> onehand_core::gitstat::GitStatus {
    onehand_core::gitstat::GitStatus {
        branch: branch.into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    }
}

/// A project of `path` on `host`.
fn on_host(host: &str, path: &str) -> onehand_core::project::Project {
    onehand_core::project::Project::new(
        onehand_core::project::ProjectId::mint(7),
        Some(onehand_core::workspace::HostName(host.into())),
        path.into(),
    )
}

/// A folder on a host names nothing on this machine, so neither the agent
/// started ahead of a session nor the pages ever reach for it.
#[test]
fn a_host_root_is_never_given_local_work() {
    use onehand_core::workspace::{HostName, ProjectRoot, Workspace};
    use std::path::PathBuf;
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(Workspace::seeded("/tmp"));
    let _ = drive.open_host_project(&on_host("vps", "/srv/app"));
    let local = PathBuf::from("/tmp");
    // Arriving warms an agent on the local folder only.
    assert_eq!(drive.roots[0].local_path(), Some(local.as_path()));
    assert_eq!(drive.roots[1].local_path(), None);
    assert_eq!(pages::page_roots_in(&drive), vec![local]);
    // Its menus offer no worktree or check, even where a folder of
    // the same path here is a repository.
    let mut host = ProjectRoot::new("/srv/app");
    host.host = Some(HostName("vps".into()));
    host.check = Some("make test".into());
    let facts = crate::chat::pane::ProjectFacts::of(&host, true);
    assert!(!facts.is_repo && !facts.check);
    assert!(crate::chat::pane::ProjectFacts::of(&drive.roots[0], true).is_repo);
}

/// The workspace page lists a host's project with its sessions, but with
/// nothing this machine files under its path; the Tasks and Issues pages
/// leave it out.
#[test]
fn the_overview_keeps_a_host_project_and_the_other_pages_do_not() {
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut ws = Workspace::seeded("/srv/app");
    ws.storage_dir = PathBuf::from("/s");
    // The folder here is of a project, which keeps its issues.
    let here = onehand_core::project::Project::new(
        onehand_core::project::ProjectId::mint(3),
        None,
        "/srv/app".into(),
    );
    ws.roots[0] = onehand_core::workspace::ProjectRoot::of(&here);
    // A status for the local folder, which the host's of the same path must
    // never show.
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(ws);
    let _ = drive.open_host_project(&on_host("vps", "/srv/app"));
    swept(
        &mut drive,
        vec![(PathBuf::from("/srv/app"), status("main"))],
    );
    let overview = pages::page_projects_in(&drive, pages::Reach::Overview);
    assert_eq!(overview.len(), 2);
    let (here, there) = (&overview[0], &overview[1]);
    assert!(here.host.is_none() && here.git.is_some() && here.issues.is_some());
    assert_eq!(there.host, Some(HostName("vps".into())));
    assert!(there.git.is_none() && there.issues.is_none());
    let local = pages::page_projects_in(&drive, pages::Reach::Local);
    assert_eq!(local.len(), 1);
    assert!(local[0].host.is_none());
}

/// A host's folder on a path of its own shows the status read over its
/// attach, as a folder here does; one on a local folder's path shows none.
#[test]
fn a_host_root_reads_its_git_status_unless_a_local_root_has_its_path() {
    use onehand_core::workspace::Workspace;
    use std::path::PathBuf;
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(Workspace::seeded("/srv/app"));
    let _ = drive.open_host_project(&on_host("vps", "/srv/app"));
    let _ = drive.open_host_project(&on_host("vps", "/srv/other"));
    swept(
        &mut drive,
        vec![
            (PathBuf::from("/srv/app"), status("here")),
            (PathBuf::from("/srv/other"), status("there")),
        ],
    );
    let branches: Vec<_> = drive
        .roots
        .iter()
        .map(|root| drive.git_of(root).map(|s| s.branch.clone()))
        .collect();
    assert_eq!(
        branches,
        [Some("here".into()), None, Some("there".to_string())]
    );
    // Its status offers nothing a host project does not do.
    let facts = crate::chat::pane::ProjectFacts::of(&drive.roots[2], true);
    assert!(!facts.is_repo);
}

/// Which host session a session watches is the workspace's, as the pane
/// says its host named it, on a real shell: what a restart of it watches
/// again, gone once the session goes.
#[gpui::test]
fn a_watched_host_session_is_the_workspaces_as_its_host_says_it(cx: &mut gpui::TestAppContext) {
    use crate::chat::pane::ChatPaneEvent;
    use onehand_core::host::wire::HostSession;
    use onehand_core::workspace::HostName;
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("watched-host");
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec.clone()]);
    let (host, id) = (HostName("box".into()), HostSession("h-1".into()));
    // Added but never shown, so nothing attaches to the host.
    let uid = cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let _ = shell
                .window
                .workspace
                .open_host_project(&on_host(&host.0, "/srv/app"));
            let (added, _) = shell.window.workspace.add_session(Some(spec), 41, None);
            added.expect("added")
        })
    });
    cx.update(|_, cx| {
        let chat = shell.read(cx).chat.clone();
        chat.update(cx, |_, cx| {
            cx.emit(ChatPaneEvent::Watching {
                uid,
                id: id.clone(),
            })
        });
    });
    cx.run_until_parked();
    let watched = cx.update(|_, cx| shell.read(cx).window.workspace.watched_on(uid));
    assert_eq!(watched, Some((host, id)));
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let _ = shell.window.workspace.end(uid, None);
        })
    });
    let watched = cx.update(|_, cx| shell.read(cx).window.workspace.watched_on(uid));
    assert_eq!(watched, None, "gone with its session");
    let _ = std::fs::remove_dir_all(dir);
}

/// A project's sessions in two of its folders: showing each puts the
/// Workbench and the terminal on that session's folder, and showing the
/// first again moves them back.
#[gpui::test]
fn switching_to_a_session_in_another_folder_moves_the_panels(cx: &mut gpui::TestAppContext) {
    use std::path::{Path, PathBuf};
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("follow-folder");
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec.clone()]);
    let (home, other) = (PathBuf::from("/tmp"), dir.join("a-x"));
    std::fs::create_dir_all(&other).unwrap();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            for (uid, folder) in [(51, home.clone()), (52, other.clone())] {
                let (added, fx) =
                    shell
                        .window
                        .workspace
                        .add_session_at(Some(spec.clone()), uid, folder);
                added.expect("added");
                shell.carry_workspace(fx, window, cx);
            }
        })
    });
    let mut show = |session: usize| {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_root_session(0, session, window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let shell = shell.read(cx);
            let workbench = shell.workbench.read(cx).root().map(Path::to_path_buf);
            let terminal = shell.terminal.read(cx).root().map(Path::to_path_buf);
            (workbench, terminal)
        })
    };
    assert_eq!(show(1), (Some(other.clone()), Some(other.clone())));
    assert_eq!(show(0), (Some(home.clone()), Some(home.clone())));
    assert_eq!(show(1), (Some(other.clone()), Some(other)));
    let _ = std::fs::remove_dir_all(dir);
}

/// A worktree split off a project is recorded in it, and a session starts in
/// it on the project's own row, shown.
#[gpui::test]
fn a_split_starts_a_session_in_the_new_worktree(cx: &mut gpui::TestAppContext) {
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("split-starts");
    let (handle, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec]);
    let (home, worktree) = (dir.join("p"), dir.join("p-feat"));
    for folder in [&home, &worktree] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (home, worktree) = (
        std::fs::canonicalize(home).unwrap(),
        std::fs::canonicalize(worktree).unwrap(),
    );
    let project = onehand_core::project::Project::new(
        onehand_core::project::ProjectId::mint(912_001),
        None,
        home.clone(),
    );
    crate::shell::rig::keep_projects(handle, &shell, vec![project], cx);
    let at = home.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let wt = worktree.clone();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_split(&home, wt, "feat".into(), None, window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let workspace = &shell.read(cx).window.workspace;
        let rows: Vec<_> = workspace.roots.iter().map(|r| r.path.clone()).collect();
        assert_eq!(rows.iter().filter(|r| **r == home).count(), 1, "{rows:?}");
        assert!(!rows.contains(&worktree), "no row of its own: {rows:?}");
        let uid = workspace.active_uid().expect("a session shown");
        assert_eq!(workspace.folder_of(uid), Some(worktree.clone()));
        assert_eq!(
            workspace.active_root().map(|r| r.path.clone()),
            Some(home.clone())
        );
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// The rail's toggle shows the rail whenever it is off screen, hidden or
/// under a panel filling the window, and hides it only while it shows.
#[test]
fn the_rail_toggle_shows_a_rail_a_maximized_panel_hides() {
    use docks::{RailToggle, rail_toggle};
    assert_eq!(rail_toggle(false, false), RailToggle::Hide);
    assert_eq!(rail_toggle(true, false), RailToggle::Show);
    assert_eq!(rail_toggle(false, true), RailToggle::Show);
    assert_eq!(rail_toggle(true, true), RailToggle::Show);
}

/// A project's row shows its page over sessions that stay open; the pane
/// then views no session, and its page is up.
#[gpui::test]
fn a_project_row_shows_its_page_over_open_sessions(cx: &mut gpui::TestAppContext) {
    use std::path::PathBuf;
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("page-over-sessions");
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec.clone()]);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let (added, fx) = shell.window.workspace.add_session_at(
                Some(spec.clone()),
                61,
                PathBuf::from("/tmp"),
            );
            added.expect("added");
            shell.carry_workspace(fx, window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).active_session_uid()),
        Some(61)
    );
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_project_page(0, window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.active_session_uid(), None);
        assert_eq!(
            shell.window.workspace.roots[0].sessions.len(),
            1,
            "still open"
        );
        let pane = shell.chat.read(cx);
        assert_eq!(pane.viewing(), None);
        assert!(pane.project_page_shown());
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// Ctrl+Tab from a project's page lands on its most recent session, even
/// when it is the only one: the page is not one of the sessions cycled.
#[gpui::test]
fn cycling_from_the_page_lands_on_the_latest_session(cx: &mut gpui::TestAppContext) {
    use std::path::PathBuf;
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("cycle-from-page");
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec.clone()]);
    let add = |uid: u64, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let (added, fx) = shell.window.workspace.add_session_at(
                    Some(spec.clone()),
                    uid,
                    PathBuf::from("/tmp"),
                );
                added.expect("added");
                shell.carry_workspace(fx, window, cx);
            })
        });
        cx.run_until_parked();
    };
    let page_then_cycle = |forward: bool, cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.show_project_page(0, window, cx);
                shell.cycle_session(forward, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| shell.read(cx).active_session_uid())
    };
    add(71, cx);
    assert_eq!(page_then_cycle(true, cx), Some(71), "the only session");
    add(72, cx);
    assert_eq!(page_then_cycle(true, cx), Some(72), "forward: the latest");
    assert_eq!(page_then_cycle(false, cx), Some(71), "backward: the oldest");
    let _ = std::fs::remove_dir_all(dir);
}

/// Stepping through the rail from a project's page starts at that project:
/// forward to its first session, not the rail's.
#[gpui::test]
fn stepping_from_a_page_starts_at_its_project(cx: &mut gpui::TestAppContext) {
    let spec = crate::shell::rig::nowhere();
    let dir = crate::shell::rig::folder("step-from-page");
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec.clone()]);
    let other = dir.join("b");
    std::fs::create_dir_all(&other).unwrap();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let (added, fx) =
                shell
                    .window
                    .workspace
                    .add_session_at(Some(spec.clone()), 91, "/tmp".into());
            added.expect("added");
            shell.carry_workspace(fx, window, cx);
            let fx = shell.window.workspace.open_project(other.clone(), None);
            shell.carry_workspace(fx, window, cx);
            let (added, fx) =
                shell
                    .window
                    .workspace
                    .add_session_at(Some(spec.clone()), 92, other.clone());
            added.expect("added");
            shell.carry_workspace(fx, window, cx);
        })
    });
    cx.run_until_parked();
    let b = cx.update(|_, cx| shell.read(cx).window.workspace.active_root);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show_project_page(b, window, cx);
            shell.rail_step(true, window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).active_session_uid()),
        Some(92)
    );
    let _ = std::fs::remove_dir_all(dir);
}
