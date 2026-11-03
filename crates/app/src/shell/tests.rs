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
    assert_eq!(ws.roots[host].local_path(), None);
    assert_eq!(sessions::page_roots_in(&ws), vec![local]);
    // Its menus offer no worktree, terminal, `git status`, check or switch,
    // even where a folder of the same path here is a repository.
    ws.roots[host].check = Some("make test".into());
    let facts = crate::chat::pane::ProjectFacts::of(&ws.roots[host], true);
    assert!(!facts.local && !facts.is_repo && !facts.check);
    assert_eq!(facts.unattended, None);
    assert!(crate::chat::pane::ProjectFacts::of(&ws.roots[0], true).local);
}
