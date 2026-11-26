use super::Removal;
use super::*;
use crate::project::{Folder, MadeBy, Placement, Project, ProjectId, Projects, Worktree};
use crate::workspace::{BuildHost, HostName, Offloaded, RoutedCommand};
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
            Effect::Arm { .. } | Effect::Changed(_) | Effect::Signal(_) | Effect::Shims(_) => None,
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
            | Effect::Changed(_)
            | Effect::Shims(_) => None,
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
            Effect::Write(_) | Effect::Changed(_) | Effect::Signal(_) | Effect::Shims(_) => None,
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
        Effect::Signal(Signal::WorktreeRemoved { folder, branch: Some(b), kept: Some(why) })
            if folder == Path::new("/code/a-x") && b == "x" && why == "not fully merged")));
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

#[test]
fn a_checkout_git_lists_is_adopted_on_its_branch_or_head() {
    let (mut d, p) = drive();
    let fx = d.adopt_worktree(
        &p.id,
        crate::worktree::Listed {
            path: "/code/a-d".into(),
            branch: None,
        },
    );
    assert!(!fx.is_empty(), "written");
    let w = &d.get(&p.id).unwrap().worktrees[0];
    assert_eq!((w.branch.as_str(), w.made_by), ("HEAD", MadeBy::Added));
}

#[test]
fn a_removal_under_way_takes_no_second_press_until_it_is_over() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let at = Path::new("/code/a-x");
    assert!(d.begin_removal(at));
    assert!(!d.begin_removal(at), "a second press");
    assert!(d.removing(at));
    d.removal_asked(at);
    assert!(!d.removing(at), "the question is up");
    assert!(d.begin_removal(at));
    let _ = d.worktree_removal(&p.id, at, Removal::Refused(vec!["x".into()]));
    assert!(!d.removing(at), "over");
}

#[test]
fn a_worktree_gone_by_a_tasks_removal_is_forgotten_in_its_project() {
    let (mut d, p) = drive();
    let _ = d.record_worktree(&p.id, split("/code/a-x"));
    let _ = d.record_worktree(&p.id, split("/code/a-x/sub"));
    let _ = d.record_worktree(&p.id, split("/code/a-x2"));
    let fx = d.worktree_gone(Path::new("/code/a-x"));
    assert!(!fx.is_empty());
    let left: Vec<_> = (d.get(&p.id).unwrap().worktrees.iter())
        .map(|w| w.path.display().to_string())
        .collect();
    assert_eq!(
        left,
        ["/code/a-x2"],
        "inside the checkout only, by component"
    );
    assert!(d.worktree_gone(Path::new("/code/a-x")).is_empty(), "once");
}

/// A project's Cards default is kept as its check command is.
#[test]
fn a_projects_cards_default_is_kept() {
    let (mut d, p) = drive();
    let fx = d.set_cards(&p.id, crate::chat::Cards::AllowEdits);
    assert!(matches!(&fx[..], [Effect::Arm { .. }, Effect::Changed(id)] if *id == p.id));
    assert_eq!(d.get(&p.id).unwrap().cards, crate::chat::Cards::AllowEdits);
    assert!(d
        .set_cards(&p.id, crate::chat::Cards::AllowEdits)
        .is_empty());
}

// Routed commands and the picked build host.

fn route(words: &str) -> RoutedCommand {
    RoutedCommand::parse(words, "").unwrap()
}

#[test]
fn put_route_refuses_a_host_project() {
    let p = Project::new(
        ProjectId::mint(2),
        Some(HostName("vps".into())),
        "/code/a".into(),
    );
    let mut d = ProjectsDrive::new(Projects::from_iter([p.clone()]), "/data/projects".into());
    assert_eq!(
        d.put_route(&p.id, route("cargo build"), None)
            .err()
            .unwrap(),
        "a runs its sessions on a host; it builds there already"
    );
}

#[test]
fn put_route_refuses_the_same_words_twice() {
    let (mut d, p) = drive();
    assert!(d.put_route(&p.id, route("cargo build"), None).is_ok());
    assert_eq!(
        d.put_route(&p.id, route("cargo build"), None)
            .err()
            .unwrap(),
        "a already routes cargo build"
    );
    assert!(d
        .put_route(&p.id, route("cargo build"), Some(&route("cargo build")))
        .is_ok());
}

/// A change is to the command the form was opened on, wherever it now
/// stands; one removed meanwhile is refused, not put in another's place.
#[test]
fn put_route_replaces_by_words() {
    let (mut d, p) = drive();
    let _ = d.put_route(&p.id, route("cargo build"), None).unwrap();
    let _ = d.put_route(&p.id, route("cargo test"), None).unwrap();
    let _ = d.remove_route(&p.id, &route("cargo build"));
    assert_eq!(
        d.put_route(&p.id, route("cargo check"), Some(&route("cargo build")))
            .err()
            .unwrap(),
        "cargo build was removed meanwhile"
    );
    let _ = d
        .put_route(&p.id, route("cargo nextest"), Some(&route("cargo test")))
        .unwrap();
    let words: Vec<_> = d
        .get(&p.id)
        .unwrap()
        .routes
        .iter()
        .map(|r| r.words.clone())
        .collect();
    assert_eq!(words, ["cargo nextest"]);
}

#[test]
fn at_most_twenty_routes() {
    let (mut d, p) = drive();
    for n in 0..20 {
        d.put_route(&p.id, route(&format!("tool{n}")), None)
            .unwrap();
    }
    assert_eq!(
        d.put_route(&p.id, route("tool20"), None).err().unwrap(),
        "a routes at most 20 commands"
    );
}

#[test]
fn pick_must_be_a_build_host() {
    let (mut d, p) = drive();
    assert_eq!(
        d.pick_build_host(&p.id, Some(HostName("nope".into())))
            .err()
            .unwrap(),
        "a does not build on nope"
    );
    let _ = d.put_build_host(&p.id, builds_on("100-52"), None).unwrap();
    let fx = d
        .pick_build_host(&p.id, Some(HostName("100-52".into())))
        .unwrap();
    assert!(matches!(&fx[..], [Effect::Write(_), Effect::Changed(_)]));
    assert_eq!(
        d.get(&p.id).unwrap().builds_on,
        Some(HostName("100-52".into()))
    );
    let _ = d.pick_build_host(&p.id, None).unwrap();
    assert_eq!(d.get(&p.id).unwrap().builds_on, None);
}

#[test]
fn removing_the_picked_build_host_clears_the_pick() {
    let (mut d, p) = drive();
    let _ = d.put_build_host(&p.id, builds_on("100-52"), None).unwrap();
    let _ = d
        .pick_build_host(&p.id, Some(HostName("100-52".into())))
        .unwrap();
    let w = write_of(d.remove_build_host(&p.id, &builds_on("100-52")));
    assert_eq!(w.project.builds_on, None);
    assert!(w.project.builds.is_empty());
    assert_eq!(d.get(&p.id).unwrap().builds_on, None);
}

#[test]
fn a_failed_write_puts_the_routes_and_the_pick_back() {
    let (mut d, p) = drive();
    let _ = d.put_build_host(&p.id, builds_on("100-52"), None).unwrap();
    let w = write_of(d.put_route(&p.id, route("cargo build"), None).unwrap());
    let fx = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Err("read-only".into()),
    });
    assert!(d.get(&p.id).unwrap().routes.is_empty());
    // The shims follow the routes back.
    assert!(shims(&fx).is_some_and(|p| p.is_empty()));
}

/// The programs the last `Effect::Shims` of `fx` names.
fn shims(fx: &[Effect]) -> Option<Vec<String>> {
    fx.iter().rev().find_map(|e| match e {
        Effect::Shims(programs) => Some(programs.clone()),
        Effect::Write(_) | Effect::Arm { .. } | Effect::Changed(_) | Effect::Signal(_) => None,
    })
}

/// Every change to the routes answers the shims for every program every
/// project routes, once each, so a running agent reaches a new one at once.
#[test]
fn a_route_change_rewrites_the_shims() {
    let p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    let other = Project::new(ProjectId::mint(2), None, "/code/b".into());
    let mut d = ProjectsDrive::new(
        Projects::from_iter([p.clone(), other.clone()]),
        "/data/projects".into(),
    );
    let _ = d.put_route(&other.id, route("dotnet build"), None).unwrap();
    let fx = d.put_route(&p.id, route("cargo build"), None).unwrap();
    assert_eq!(shims(&fx), Some(vec!["cargo".into(), "dotnet".into()]));
    let fx = d.put_route(&p.id, route("cargo test"), None).unwrap();
    assert_eq!(shims(&fx), Some(vec!["cargo".into(), "dotnet".into()]));
    let _ = d.remove_route(&p.id, &route("cargo build"));
    let fx = d.remove_route(&p.id, &route("cargo test"));
    assert_eq!(shims(&fx), Some(vec!["dotnet".into()]));
    assert_eq!(shims(&d.remove_route(&p.id, &route("cargo test"))), None);
}

fn board(todo: Vec<u64>) -> crate::project::Board {
    crate::project::Board {
        workflow: "mine".into(),
        team: 3,
        todo,
    }
}

/// A drive whose project has a board holding `todo`.
fn drive_with_todo(todo: Vec<u64>) -> (ProjectsDrive, Project) {
    let (mut d, p) = drive();
    let _ = d.set_board(&p.id, Some(board(todo)));
    (d, p)
}

fn todo_of(d: &ProjectsDrive, p: &Project) -> Vec<u64> {
    d.get(&p.id).unwrap().board().unwrap().todo.clone()
}

#[test]
fn board_round_trips_through_toml() {
    let dir = std::env::temp_dir().join(format!("onehand-board-{}", std::process::id()));
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    crate::project::file::save_to(&p, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join(crate::project::file::FILE)).unwrap();
    assert!(!text.contains("board"));
    p.board = Some(board(vec![4, 9]));
    crate::project::file::save_to(&p, &dir).unwrap();
    match crate::project::file::load_from(&dir) {
        crate::project::file::ProjectLoad::Found(loaded) => assert_eq!(*loaded, p),
        _ => panic!("the project loads"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_todo_appends_once() {
    let (mut d, p) = drive_with_todo(Vec::new());
    let _ = d.add_todo(&p.id, 4);
    assert!(d.add_todo(&p.id, 4).is_empty());
    assert_eq!(todo_of(&d, &p), vec![4]);
}

#[test]
fn move_todo_swaps_neighbours() {
    let (mut d, p) = drive_with_todo(vec![4, 9, 2]);
    let _ = d.move_todo(&p.id, 2, crate::project::Toward::Up);
    assert_eq!(todo_of(&d, &p), vec![4, 2, 9]);
    assert!(d.move_todo(&p.id, 4, crate::project::Toward::Up).is_empty());
    assert_eq!(todo_of(&d, &p), vec![4, 2, 9]);
}

#[test]
fn place_todo_inserts_a_new_issue_at_the_index() {
    let (mut d, p) = drive_with_todo(vec![4, 9]);
    let _ = d.place_todo(&p.id, 7, 1);
    assert_eq!(todo_of(&d, &p), vec![4, 7, 9]);
}

#[test]
fn place_todo_moves_an_issue_within_to_do() {
    let (mut d, p) = drive_with_todo(vec![4, 9, 2]);
    let _ = d.place_todo(&p.id, 2, 0);
    assert_eq!(todo_of(&d, &p), vec![2, 4, 9]);
}

#[test]
fn place_todo_clamps_past_the_end() {
    let (mut d, p) = drive_with_todo(vec![4, 9]);
    let _ = d.place_todo(&p.id, 4, 50);
    assert_eq!(todo_of(&d, &p), vec![9, 4]);
}

#[test]
fn place_todo_to_its_own_place_writes_nothing() {
    let (mut d, p) = drive_with_todo(vec![4, 9, 2]);
    assert!(d.place_todo(&p.id, 9, 1).is_empty());
}

#[test]
fn take_todo_removes_only_named() {
    let (mut d, p) = drive_with_todo(vec![4, 9, 2]);
    let _ = d.take_todo(&p.id, &[9]);
    assert_eq!(todo_of(&d, &p), vec![4, 2]);
}

#[test]
fn start_todo_counts_starts_until_they_land() {
    let (mut d, p) = drive_with_todo(vec![4, 9, 2, 7]);
    let w = write_of(d.start_todo(&p.id, &[4, 9]));
    assert_eq!(
        w.project.board().unwrap().todo,
        vec![2, 7],
        "written without them"
    );
    let issues: crate::issues::Issues = serde_json::from_str(
        r#"{"next":10,"issues":[{"number":2,"title":"b"},{"number":7,"title":"c"}]}"#,
    )
    .unwrap();
    let taken = std::collections::HashSet::new();
    let flow = worktree_flow(crate::workflow::Place::Worktree);
    let pull = d
        .pull(&p.id, &flow, 0, 0, None, &issues, None, &taken)
        .unwrap();
    assert_eq!(pull.start, [2], "a team of 3 with 2 on their way");
    let room = crate::task::rooms::Room {
        limit: 2,
        holding: 0,
        waiting: 0,
    };
    let first = d.pull(&p.id, &flow, 0, 0, Some(room), &issues, None, &taken);
    assert!(
        first.unwrap().start.is_empty(),
        "the 2 on their way fill a first step of 2"
    );
    d.landed(&p.id, 4);
    assert_eq!(
        d.pull(&p.id, &flow, 1, 0, None, &issues, None, &taken)
            .unwrap()
            .start,
        [2]
    );
    assert!(
        d.pull(&p.id, &flow, 1, 1, None, &issues, None, &taken)
            .unwrap()
            .start
            .is_empty(),
        "one running, one on its way and an extend waiting fill a team of 3"
    );
    assert_eq!(
        d.extends_given(&p.id, 1, &["a".to_string(), "b".to_string()]),
        ["a"],
        "a team of 3, one running and one on its way, has one place for an extend"
    );
    d.landed(&p.id, 9);
    assert_eq!(
        d.pull(&p.id, &flow, 0, 0, None, &issues, None, &taken)
            .unwrap()
            .start,
        [2, 7]
    );
}

#[test]
fn set_team_clamps() {
    let (mut d, p) = drive_with_todo(Vec::new());
    let _ = d.set_team(&p.id, 0);
    assert_eq!(d.get(&p.id).unwrap().board().unwrap().team, 1);
    let _ = d.set_team(&p.id, 12);
    assert_eq!(d.get(&p.id).unwrap().board().unwrap().team, 8);
}

#[test]
fn a_project_without_a_board_ignores_todo_changes() {
    let (mut d, p) = drive();
    assert!(d.add_todo(&p.id, 4).is_empty());
    assert!(d.set_team(&p.id, 3).is_empty());
    assert!(d.get(&p.id).unwrap().board().is_none());
}

#[test]
fn failed_write_puts_board_back() {
    let (mut d, p) = drive();
    let w = write_of(d.set_board(&p.id, Some(board(vec![4]))));
    let _ = d.handle(Input::Written {
        id: p.id.clone(),
        pending: w.pending,
        result: Err("disk full".into()),
    });
    assert!(d.get(&p.id).unwrap().board().is_none());
}

fn worktree_flow(place: crate::workflow::Place) -> crate::workflow::Template {
    let mut template = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    template.place = place;
    template
}

#[test]
fn bind_board_starts_small_and_keeps_what_a_rebind_had() {
    let (mut d, p) = drive();
    let flow = worktree_flow(crate::workflow::Place::Worktree);
    assert!(d.bind_board(&p.id, Some(&flow)).is_ok());
    let bound = d.get(&p.id).unwrap().board().unwrap().clone();
    assert_eq!((bound.workflow.as_str(), bound.team), ("builtin:branch", 2));
    assert!(bound.todo.is_empty());
    let _ = d.set_board(&p.id, Some(board(vec![4])));
    let mut copy = flow.clone();
    copy.id = "copy".into();
    assert!(d.bind_copy(&p.id, &copy).is_some_and(|bound| bound.is_ok()));
    let kept = d.get(&p.id).unwrap().board().unwrap().clone();
    assert_eq!((kept.workflow.as_str(), kept.team), ("copy", 3));
    assert_eq!(kept.todo, [4]);
}

#[test]
fn bind_board_refuses_what_a_board_cannot_run() {
    let (mut d, p) = drive();
    assert!(d
        .bind_board(&p.id, None)
        .is_err_and(|why| why.contains("gone")));
    let checkout = worktree_flow(crate::workflow::Place::Checkout);
    assert!(d.bind_board(&p.id, Some(&checkout)).is_err());
    assert!(d.get(&p.id).unwrap().board().is_none());
    assert!(
        d.bind_copy(&p.id, &checkout).is_none(),
        "no board to copy for"
    );
}

#[test]
fn unbinding_asks_only_over_a_to_do() {
    let (d, p) = drive();
    assert!(!d.unbinding_asks(&p.id));
    let (d, p) = drive_with_todo(Vec::new());
    assert!(!d.unbinding_asks(&p.id));
    let (d, p) = drive_with_todo(vec![4]);
    assert!(d.unbinding_asks(&p.id));
}

#[test]
fn pull_counts_the_first_steps_room_and_refuses_an_unusable_workflow() {
    let (d, p) = drive_with_todo(vec![2, 7]);
    let issues: crate::issues::Issues = serde_json::from_str(
        r#"{"next":10,"issues":[{"number":2,"title":"b"},{"number":7,"title":"c"}]}"#,
    )
    .unwrap();
    let taken = std::collections::HashSet::new();
    let flow = worktree_flow(crate::workflow::Place::Worktree);
    let room = crate::task::rooms::Room {
        limit: 3,
        holding: 1,
        waiting: 1,
    };
    let pull = d.pull(&p.id, &flow, 0, 0, Some(room), &issues, None, &taken);
    assert_eq!(
        pull.unwrap().start,
        [2],
        "one left of three, held and waited for"
    );
    let checkout = worktree_flow(crate::workflow::Place::Checkout);
    assert!(d
        .pull(&p.id, &checkout, 0, 0, None, &issues, None, &taken)
        .is_none());
}

#[test]
fn place_todo_before_lands_just_before_its_target() {
    let (mut d, p) = drive_with_todo(vec![1, 2, 3, 4]);
    let _ = d.place_todo_before(&p.id, 4, 2);
    assert_eq!(todo_of(&d, &p), vec![1, 4, 2, 3]);
    let _ = d.place_todo_before(&p.id, 9, 1);
    assert_eq!(todo_of(&d, &p), vec![9, 1, 4, 2, 3], "from the Backlog");
    assert!(d.place_todo_before(&p.id, 2, 2).is_empty());
    assert!(
        d.place_todo_before(&p.id, 2, 7).is_empty(),
        "no such target"
    );
}
