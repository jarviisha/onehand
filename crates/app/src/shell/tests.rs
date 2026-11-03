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

/// A folder on a host names nothing on this machine, so neither `git status`,
/// the agent started ahead of a session, nor the pages ever reach for it.
#[test]
fn a_host_root_is_never_given_local_work() {
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut ws = Workspace::seeded("/tmp");
    let local = PathBuf::from("/tmp");
    let host = ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    assert_eq!(roots::git_roots(&ws), vec![local.clone()]);
    // Arriving points the Workbench, the terminal and the warm-up at the
    // local folder, and at nothing for the host's.
    assert_eq!(roots::arrival(&ws.roots[0]), Ok(local.clone()));
    assert_eq!(roots::arrival(&ws.roots[host]), Err(roots::ON_HOST_LATER));
    assert_eq!(sessions::page_roots_in(&ws), vec![local]);
    // Its menus offer no worktree, terminal, `git status`, check or switch,
    // even where a folder of the same path here is a repository.
    ws.roots[host].check = Some("make test".into());
    let facts = crate::chat::pane::ProjectFacts::of(&ws.roots[host], true);
    assert!(!facts.local && !facts.is_repo && !facts.check);
    assert_eq!(facts.unattended, None);
    assert!(crate::chat::pane::ProjectFacts::of(&ws.roots[0], true).local);
}

/// The workspace page lists a host's project with its sessions, but with
/// nothing this machine files under its path; the Tasks and Issues pages
/// leave it out.
#[test]
fn the_overview_keeps_a_host_project_and_the_other_pages_do_not() {
    use onehand_core::gitstat::GitStatus;
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut ws = Workspace::seeded("/srv/app");
    ws.storage_dir = Some(PathBuf::from("/s"));
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    // A status for the local folder, which the host's of the same path must
    // never show.
    let status = GitStatus {
        branch: "main".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    let git = [(PathBuf::from("/srv/app"), status)].into_iter().collect();
    let overview = sessions::page_projects_in(&ws, &git, sessions::Reach::Overview);
    assert_eq!(overview.len(), 2);
    let (here, there) = (&overview[0], &overview[1]);
    assert!(here.host.is_none() && here.git.is_some() && here.issues.is_some());
    assert_eq!(there.host, Some(HostName("vps".into())));
    assert!(there.git.is_none() && there.issues.is_none());
    let local = sessions::page_projects_in(&ws, &git, sessions::Reach::Local);
    assert_eq!(local.len(), 1);
    assert!(local[0].host.is_none());
}
