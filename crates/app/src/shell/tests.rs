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

/// A folder on a host names nothing on this machine, so neither the agent
/// started ahead of a session nor the pages ever reach for it.
#[test]
fn a_host_root_is_never_given_local_work() {
    use onehand_core::workspace::{HostName, ProjectRoot, Workspace};
    use std::path::PathBuf;
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(Workspace::seeded("/tmp"));
    let _ = drive.open_host_project(HostName("vps".into()), PathBuf::from("/srv/app"));
    let local = PathBuf::from("/tmp");
    // Arriving warms an agent on the local folder only.
    assert_eq!(drive.roots[0].local_path(), Some(local.as_path()));
    assert_eq!(drive.roots[1].local_path(), None);
    assert_eq!(sessions::page_roots_in(&drive), vec![local]);
    // Its menus offer no worktree, check or switch, even where a folder of
    // the same path here is a repository.
    let mut host = ProjectRoot::new("/srv/app");
    host.host = Some(HostName("vps".into()));
    host.check = Some("make test".into());
    let facts = crate::chat::pane::ProjectFacts::of(&host, true);
    assert!(!facts.is_repo && !facts.check);
    assert_eq!(facts.unattended, None);
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
    ws.storage_dir = Some(PathBuf::from("/s"));
    // A status for the local folder, which the host's of the same path must
    // never show.
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(ws);
    let _ = drive.open_host_project(HostName("vps".into()), PathBuf::from("/srv/app"));
    swept(
        &mut drive,
        vec![(PathBuf::from("/srv/app"), status("main"))],
    );
    let overview = sessions::page_projects_in(&drive, sessions::Reach::Overview);
    assert_eq!(overview.len(), 2);
    let (here, there) = (&overview[0], &overview[1]);
    assert!(here.host.is_none() && here.git.is_some() && here.issues.is_some());
    assert_eq!(there.host, Some(HostName("vps".into())));
    assert!(there.git.is_none() && there.issues.is_none());
    let local = sessions::page_projects_in(&drive, sessions::Reach::Local);
    assert_eq!(local.len(), 1);
    assert!(local[0].host.is_none());
}

/// A host's folder on a path of its own shows the status read over its
/// attach, as a folder here does; one on a local folder's path shows none.
#[test]
fn a_host_root_reads_its_git_status_unless_a_local_root_has_its_path() {
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut drive = onehand_core::workspace::WorkspaceDrive::new(Workspace::seeded("/srv/app"));
    let _ = drive.open_host_project(HostName("vps".into()), PathBuf::from("/srv/app"));
    let _ = drive.open_host_project(HostName("vps".into()), PathBuf::from("/srv/other"));
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
