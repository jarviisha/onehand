use super::Removal;
use super::*;
use crate::project::{Folder, MadeBy, Placement, Project, ProjectId, Projects, Worktree};
use crate::workspace::{BuildHost, HostName, Offloaded};
use std::path::Path;

/// A drive over one project, `a`, at `/code/a`.
fn drive() -> (ProjectsDrive, Project) {
    let p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    let d = ProjectsDrive::new(Projects::from_iter([p.clone()]), "/data/projects".into());
    (d, p)
}

fn split(path: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: "x".into(),
        from: None,
        made_by: MadeBy::Split,
        offload: None,
    }
}

fn win() -> BuildHost {
    BuildHost::parse("win", "", "").unwrap()
}

/// The write among `fx`, taken out.
fn write_of(fx: Vec<Effect>) -> Box<ProjectWrite> {
    fx.into_iter()
        .find_map(|e| match e {
            Effect::Write(w) => Some(w),
            Effect::Arm { .. } | Effect::Changed(_) | Effect::Signal(_) => None,
        })
        .expect("a write")
}

#[test]
fn placing_a_new_folder_writes_a_project_named_after_it() {
    let mut d = ProjectsDrive::new(Projects::default(), "/data/projects".into());
    let (id, fx) = d.place(
        None,
        "/code/onehand".into(),
        Placement::New,
        MadeBy::Added,
        ProjectId::mint(9),
    );
    assert_eq!(id, ProjectId::mint(9));
    assert_eq!(d.get(&id).unwrap().name, "onehand");
    assert!(matches!(&fx[..],
        [Effect::Write(w), Effect::Changed(c)]
            if w.dir == Path::new("/data/projects").join(id.as_str()) && *c == id && w.project.id == id));
}

#[test]
fn placing_a_known_folder_changes_nothing() {
    let (mut d, p) = drive();
    let (id, fx) = d.place(
        None,
        "/code/a".into(),
        Placement::Known(p.id.clone(), Folder::Home),
        MadeBy::Added,
        ProjectId::mint(9),
    );
    assert_eq!(id, p.id);
    assert!(fx.is_empty());
}

#[test]
fn a_new_folder_placed_twice_is_one_project() {
    let mut d = ProjectsDrive::new(Projects::default(), "/data/projects".into());
    let (first, _) = d.place(
        None,
        "/b".into(),
        Placement::New,
        MadeBy::Added,
        ProjectId::mint(2),
    );
    let (second, fx) = d.place(
        None,
        "/b".into(),
        Placement::New,
        MadeBy::Added,
        ProjectId::mint(3),
    );
    assert_eq!(first, second);
    assert!(fx.is_empty());
    assert_eq!(d.iter().count(), 1);
}

#[test]
fn placing_a_worktree_records_it_with_who_made_it() {
    let (mut d, p) = drive();
    let (id, fx) = d.place(
        None,
        "/code/a-w".into(),
        Placement::WorktreeOf {
            project: p.id.clone(),
            branch: "w".into(),
        },
        MadeBy::Added,
        ProjectId::mint(9),
    );
    assert_eq!(id, p.id);
    let wt = &d.get(&p.id).unwrap().worktrees[0];
    assert_eq!(
        (wt.path.as_path(), wt.branch.as_str(), wt.made_by),
        (Path::new("/code/a-w"), "w", MadeBy::Added)
    );
    assert!(matches!(fx.last(), Some(Effect::Changed(c)) if *c == p.id));
}

#[test]
fn a_worktree_placed_or_recorded_twice_is_recorded_once() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    assert!(d.record_worktree(&p.id, split("/code/a-x")).is_empty());
    assert_eq!(d.get(&p.id).unwrap().worktrees.len(), 1);
}

#[test]
fn renaming_trims_and_ignores_blank_and_unchanged() {
    let (mut d, p) = drive();
    assert!(d.rename(&p.id, "   ").is_empty());
    assert!(d.rename(&p.id, "a").is_empty());
    let fx = d.rename(&p.id, "  Onehand  ");
    assert_eq!(d.get(&p.id).unwrap().name, "Onehand");
    assert!(matches!(&fx[..], [Effect::Write(_), Effect::Changed(id)] if *id == p.id));
}

#[test]
fn a_failed_write_puts_the_name_back_and_says_so_once() {
    let (mut d, p) = drive();
    let w = write_of(d.rename(&p.id, "B"));
    let fx = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Err("disk full".into()),
    });
    assert_eq!(d.get(&p.id).unwrap().name, "a");
    let said: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Signal(Signal::SaveFailed { name, why, .. }) => {
                Some((name.as_str(), why.as_str()))
            }
            Effect::Signal(
                Signal::WorktreeRemoved { .. }
                | Signal::WorktreeStays(_)
                | Signal::WorktreeNotRemoved(_),
            )
            | Effect::Write(_)
            | Effect::Arm { .. }
            | Effect::Changed(_) => None,
        })
        .collect();
    assert_eq!(said, vec![("a", "disk full")]);
    assert!(fx
        .iter()
        .any(|e| matches!(e, Effect::Changed(id) if *id == p.id)));
}

#[test]
fn a_write_that_landed_puts_nothing_back() {
    let (mut d, p) = drive();
    let w = write_of(d.rename(&p.id, "B"));
    let fx = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Ok(()),
    });
    assert!(fx.is_empty());
    assert_eq!(d.get(&p.id).unwrap().name, "B");
}

#[test]
fn a_change_answers_changed_for_its_project() {
    let (mut d, p) = drive();
    let fx = d.set_check(&p.id, " make test ");
    assert!(matches!(&fx[..], [Effect::Arm { .. }, Effect::Changed(id)] if *id == p.id));
    assert_eq!(d.get(&p.id).unwrap().check.as_deref(), Some("make test"));
    assert!(d.set_check(&p.id, "make test").is_empty());
}

#[test]
fn a_burst_is_written_once_things_are_still() {
    let (mut d, p) = drive();
    let first = d.set_check(&p.id, "m");
    let second = d.set_check(&p.id, "make");
    let token = |fx: &[Effect]| {
        fx.iter().find_map(|e| match e {
            Effect::Arm { token, .. } => Some(*token),
            Effect::Write(_) | Effect::Changed(_) | Effect::Signal(_) => None,
        })
    };
    let (old, new) = (token(&first).unwrap(), token(&second).unwrap());
    assert!(d.handle(Input::Elapsed { token: old }).is_empty());
    let w = write_of(d.handle(Input::Elapsed { token: new }));
    assert_eq!(w.project.check.as_deref(), Some("make"));
}

#[test]
fn offload_is_the_folders_own() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let on = Offloaded {
        host: HostName("vps".into()),
        mirror: "/m".into(),
        branch: None,
    };
    let fx = d
        .set_offload(None, Path::new("/code/a-x"), Some(on.clone()))
        .unwrap();
    assert!(matches!(fx.last(), Some(Effect::Changed(id)) if *id == p.id));
    let now = d.get(&p.id).unwrap();
    assert_eq!(now.offload, None);
    assert_eq!(now.worktrees[0].offload, Some(on));
    assert!(d.set_offload(None, Path::new("/elsewhere"), None).is_none());
}

#[test]
fn build_hosts_keep_their_refusals() {
    let (mut d, p) = drive();
    let fx = d.put_build_host(&p.id, win(), None).unwrap();
    assert!(matches!(&fx[..], [Effect::Write(_), Effect::Changed(_)]));
    assert_eq!(
        d.put_build_host(&p.id, win(), None).err().unwrap(),
        "a already builds on win"
    );
    let _ = d.set_offload(
        None,
        Path::new("/code/a"),
        Some(Offloaded {
            host: HostName("vps".into()),
            mirror: "/m".into(),
            branch: None,
        }),
    );
    assert_eq!(
        d.put_build_host(&p.id, BuildHost::parse("mac", "", "").unwrap(), None)
            .err()
            .unwrap(),
        "a runs its sessions on a host; it builds there already"
    );
    let _ = d.remove_build_host(&p.id, &win());
    assert!(d.get(&p.id).unwrap().builds.is_empty());
}

fn builds_on(host: &str) -> BuildHost {
    BuildHost {
        host: HostName(host.into()),
        artifacts: vec!["target/app".into()],
        into: None,
    }
}

#[test]
fn a_build_host_is_changed_in_its_place_and_an_unchanged_one_writes_nothing() {
    let (mut d, p) = drive();
    let _ = d.put_build_host(&p.id, builds_on("build01"), None);
    let _ = d.put_build_host(&p.id, builds_on("win"), None);
    let mut changed = builds_on("other");
    changed.into = Some("out".into());
    let _ = d.put_build_host(&p.id, changed.clone(), Some(0)).unwrap();
    assert_eq!(
        d.get(&p.id).unwrap().builds,
        vec![changed.clone(), builds_on("win")]
    );
    assert!(d
        .put_build_host(&p.id, changed, Some(0))
        .unwrap()
        .is_empty());
}

#[test]
fn the_entry_being_changed_may_keep_its_host_and_no_other_may_take_it() {
    let (mut d, p) = drive();
    let _ = d.put_build_host(&p.id, builds_on("build01"), None);
    let _ = d.put_build_host(&p.id, builds_on("win"), None);
    assert_eq!(
        d.put_build_host(&p.id, builds_on("win"), Some(0)).err(),
        Some("a already builds on win".to_string())
    );
    assert!(d.put_build_host(&p.id, builds_on("win"), Some(1)).is_ok());
    assert_eq!(d.get(&p.id).unwrap().builds.len(), 2);
}

#[test]
fn a_failed_write_puts_the_build_hosts_back() {
    let (mut d, p) = drive();
    let w = write_of(d.put_build_host(&p.id, builds_on("build01"), None).unwrap());
    let _ = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Err("read-only".into()),
    });
    assert!(d.get(&p.id).unwrap().builds.is_empty());
}

// What a row carried before it was placed.

#[test]
fn a_placed_rows_own_facts_fill_what_the_project_lacks() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let on = Offloaded {
        host: HostName("vps".into()),
        mirror: "/m".into(),
        branch: None,
    };
    let fx = d.take_facts(
        &p.id,
        Path::new("/code/a-x"),
        Some("make".into()),
        vec![builds_on("win")],
        Some(on.clone()),
    );
    assert!(fx.iter().any(|e| matches!(e, Effect::Write(_))));
    assert!(matches!(fx.last(), Some(Effect::Changed(id)) if *id == p.id));
    let now = d.get(&p.id).unwrap();
    assert_eq!(now.check.as_deref(), Some("make"));
    assert_eq!(now.builds, vec![builds_on("win")]);
    assert_eq!(now.worktrees[0].offload, Some(on));
    assert_eq!(
        now.offload, None,
        "the folder's own offload, not the project's"
    );
}

#[test]
fn a_projects_own_facts_are_kept_over_a_rows() {
    let (mut d, p) = drive();
    let _ = d.set_check(&p.id, "cargo test");
    let fx = d.take_facts(
        &p.id,
        Path::new("/code/a"),
        Some("make".into()),
        Vec::new(),
        None,
    );
    assert!(fx.is_empty(), "nothing the project lacks");
    assert_eq!(d.get(&p.id).unwrap().check.as_deref(), Some("cargo test"));
}

#[test]
fn a_flush_writes_what_waits_on_a_burst_at_once() {
    let (mut d, p) = drive();
    let _ = d.set_check(&p.id, "make");
    let fx = d.flush();
    let w = write_of(fx);
    assert_eq!(w.project.check.as_deref(), Some("make"));
    assert!(d.flush().is_empty(), "nothing waits any more");
}

#[test]
fn a_worktree_parent_is_set_and_cleared_and_written() {
    let (mut d, p) = drive();
    let fx = d.set_worktree_parent(&p.id, Some("/work".into()));
    assert_eq!(
        write_of(fx).project.worktree_parent.as_deref(),
        Some(Path::new("/work"))
    );
    assert!(
        d.set_worktree_parent(&p.id, Some("/work".into()))
            .is_empty(),
        "unchanged"
    );
    let fx = d.set_worktree_parent(&p.id, None);
    assert_eq!(write_of(fx).project.worktree_parent, None);
}

#[test]
fn a_removed_worktree_is_forgotten_and_said() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let fx = d.worktree_removal(
        &p.id,
        Path::new("/code/a-x"),
        Removal::Done {
            top: "/code/a-x".into(),
            branch: Some("x".into()),
            result: Ok(crate::worktree::removal::Removed {
                branch_kept: Some("not fully merged".into()),
            }),
        },
    );
    assert!(d.get(&p.id).unwrap().worktrees.is_empty(), "forgotten");
    assert!(fx.iter().any(|e| matches!(e,
        Effect::Signal(Signal::WorktreeRemoved { name, kept: Some(why) })
            if name == "x" && why == "not fully merged")));
}

#[test]
fn a_refused_or_failed_removal_keeps_the_record() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let fx = d.worktree_removal(
        &p.id,
        Path::new("/code/a-x"),
        Removal::Refused(vec!["Uncommitted or untracked files: a.txt.".into()]),
    );
    assert!(matches!(&fx[..], [Effect::Signal(Signal::WorktreeStays(why))] if why.len() == 1));
    let fx = d.worktree_removal(
        &p.id,
        Path::new("/code/a-x"),
        Removal::Done {
            top: "/code/a-x".into(),
            branch: None,
            result: Err("locked".into()),
        },
    );
    assert!(
        matches!(&fx[..], [Effect::Signal(Signal::WorktreeNotRemoved(why))] if why == "locked")
    );
    assert_eq!(d.get(&p.id).unwrap().worktrees.len(), 1);
}

#[test]
fn a_failed_write_puts_the_worktree_folder_back() {
    let (mut d, p) = drive();
    let w = write_of(d.set_worktree_parent(&p.id, Some("/work".into())));
    let _ = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Err("disk full".into()),
    });
    assert_eq!(d.get(&p.id).unwrap().worktree_parent, None);
}
