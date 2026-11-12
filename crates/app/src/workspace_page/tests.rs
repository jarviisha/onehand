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
    onehand_core::issues::drive::Change::Create(draft)
        .apply(&mut issues, 1)
        .unwrap();
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

/// A host's project on the same path as one here is no filter: picking it
/// would narrow the page to the local folder's issues.
#[test]
fn only_a_project_here_is_a_filter() {
    let projects = [project(None, 0), project(Some(HostName("vps".into())), 0)];
    let options: Vec<_> = filter_options(&projects).collect();
    assert_eq!(options, vec![("app".into(), PathBuf::from("/srv/app"))]);
    assert_eq!(filter_options(&projects[1..]).count(), 0);
}

#[test]
fn a_workspace_keeps_no_issues_when_no_project_has_a_file() {
    let host = project(Some(HostName("vps".into())), 0);
    let here = PageProject {
        issues: Some(PathBuf::from("/s/issues/app.json")),
        ..project(None, 0)
    };
    assert!(keeps_no_issues(std::slice::from_ref(&host)));
    assert!(!keeps_no_issues(std::slice::from_ref(&here)));
    assert!(!keeps_no_issues(&[here, host]));
    assert!(!keeps_no_issues(&[]));
}
