use super::*;
use onehand_core::issues::{Draft, Issues};
use onehand_core::workspace::HostName;

fn project(host: Option<HostName>, sessions: usize) -> PageProject {
    PageProject {
        label: "app".into(),
        root: PathBuf::from("/srv/app"),
        host,
        sessions: (0..sessions as u64).map(|uid| (uid, "s".into())).collect(),
        git: None,
        issues: None,
    }
}

/// A host's tile never counts the issues of a folder of the same path here,
/// nor waits on a read that was never about it.
#[test]
fn a_host_tile_says_its_sessions_alone() {
    let mut issues = Issues::default();
    let draft = Draft {
        title: "Broken".into(),
        ..Default::default()
    };
    issues.create(draft, 1).unwrap();
    let read = vec![(PathBuf::from("/srv/app"), issues)];
    let here = project(None, 1);
    let there = project(Some(HostName("vps".into())), 1);
    assert_eq!(holds(&here, Some(&read)), "1 session · 1 open issue");
    assert_eq!(holds(&there, Some(&read)), "1 session");
    assert_eq!(
        holds(&project(Some(HostName("vps".into())), 0), None),
        "No sessions"
    );
    assert_eq!(holds(&project(None, 0), None), "Looking for open issues…");
}
