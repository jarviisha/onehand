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

/// A folder on a host names nothing on this machine, so neither the agent
/// started ahead of a session nor the pages ever reach for it.
#[test]
fn a_host_root_is_never_given_local_work() {
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut ws = Workspace::seeded("/tmp");
    let local = PathBuf::from("/tmp");
    let host = ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    // Arriving warms an agent on the local folder only.
    assert_eq!(ws.roots[0].local_path(), Some(local.as_path()));
    assert_eq!(ws.roots[host].local_path(), None);
    assert_eq!(sessions::page_roots_in(&ws), vec![local]);
    // Its menus offer no worktree, check or switch, even where a folder of
    // the same path here is a repository.
    ws.roots[host].check = Some("make test".into());
    let facts = crate::chat::pane::ProjectFacts::of(&ws.roots[host], true);
    assert!(!facts.is_repo && !facts.check);
    assert_eq!(facts.unattended, None);
    assert!(crate::chat::pane::ProjectFacts::of(&ws.roots[0], true).is_repo);
}

/// A `git status` sweep reads a host's folder over its attach when one is
/// up, and leaves it out rather than attach for it.
#[test]
fn a_git_sweep_reads_a_host_root_only_while_it_is_attached() {
    use onehand_core::disk::{Disk, HostDisk};
    use onehand_core::host::client::HostConn;
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut ws = Workspace::seeded("/tmp");
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    let read = |live: bool| {
        git_sweep::git_roots(&ws, |host| {
            live.then(|| {
                Disk::Host(HostDisk::new(
                    HostConn::closed(&host.0),
                    rt.handle().clone(),
                ))
            })
        })
        .into_iter()
        .map(|(path, disk)| (path, disk.host().map(str::to_string)))
        .collect::<Vec<_>>()
    };
    let here = (PathBuf::from("/tmp"), None);
    assert_eq!(read(false), vec![here.clone()]);
    assert_eq!(
        read(true),
        vec![here, (PathBuf::from("/srv/app"), Some("vps".into()))]
    );
}

/// A host's folder on a path a folder here also has is left out of the
/// sweep, which keeps what it reads by path alone.
#[test]
fn a_git_sweep_leaves_out_a_host_root_on_a_local_roots_path() {
    use onehand_core::disk::{Disk, HostDisk};
    use onehand_core::host::client::HostConn;
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut ws = Workspace::seeded("/srv/app");
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/other"));
    let read = git_sweep::git_roots(&ws, |host| {
        Some(Disk::Host(HostDisk::new(
            HostConn::closed(&host.0),
            rt.handle().clone(),
        )))
    })
    .into_iter()
    .map(|(path, disk)| (path, disk.host().map(str::to_string)))
    .collect::<Vec<_>>();
    assert_eq!(
        read,
        vec![
            (PathBuf::from("/srv/app"), None),
            (PathBuf::from("/srv/other"), Some("vps".into())),
        ]
    );
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

/// A host's folder on a path of its own shows the status read over its
/// attach, as a folder here does; one on a local folder's path shows none.
#[test]
fn a_host_root_reads_its_git_status_unless_a_local_root_has_its_path() {
    use onehand_core::gitstat::GitStatus;
    use onehand_core::workspace::{HostName, Workspace};
    use std::path::PathBuf;
    let mut ws = Workspace::seeded("/srv/app");
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/app"));
    ws.add_host_root(HostName("vps".into()), PathBuf::from("/srv/other"));
    let status = |branch: &str| GitStatus {
        branch: branch.into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    let git = [
        (PathBuf::from("/srv/app"), status("here")),
        (PathBuf::from("/srv/other"), status("there")),
    ]
    .into_iter()
    .collect();
    let branches: Vec<_> = ws
        .roots
        .iter()
        .map(|root| crate::state::git_of(&ws, &git, root).map(|s| s.branch.clone()))
        .collect();
    assert_eq!(
        branches,
        [Some("here".into()), None, Some("there".to_string())]
    );
    // Its status offers nothing a host project does not do.
    let facts = crate::chat::pane::ProjectFacts::of(&ws.roots[2], true);
    assert!(!facts.is_repo);
}

/// A worker's worktree, added for it alone, goes when its last session
/// closes; a project the person added stays empty.
#[test]
fn a_transient_project_is_left_behind_once_empty() {
    use onehand_core::config::AgentSpec;
    use onehand_core::workspace::Workspace;
    let mut ws = Workspace::seeded("/tmp");
    let wt = ws
        .add_transient_root("/tmp/wt", AgentSpec::default(), 7)
        .unwrap();
    assert!(!sessions::left_behind(&ws, wt), "a session is still on it");
    ws.close_session(wt, 0);
    assert!(sessions::left_behind(&ws, wt));
    assert!(
        !sessions::left_behind(&ws, 0),
        "the person's own project stays"
    );
}
