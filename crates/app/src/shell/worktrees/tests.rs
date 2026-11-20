use crate::shell::rig;
use crate::state::Shared;
use gpui::TestAppContext;
use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
use std::path::PathBuf;
use std::time::Duration;

/// Run the loop until `done`, letting what runs off the UI thread land.
fn until(
    cx: &mut gpui::VisualTestContext,
    mut done: impl FnMut(&mut gpui::VisualTestContext) -> bool,
) {
    for _ in 0..300 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never came to pass");
}

fn worktrees_of(id: &ProjectId, cx: &mut gpui::VisualTestContext) -> Vec<Worktree> {
    cx.update(|_, cx| {
        Shared::global(cx)
            .projects
            .get(id)
            .unwrap()
            .worktrees
            .clone()
    })
}

/// A project at a scratch repository's main checkout, kept and opened, its
/// worktree `wt` unrecorded; `edit` changes the project first.
fn opened<'a>(
    case: &str,
    seed: u64,
    cx: &'a mut TestAppContext,
    edit: impl FnOnce(&mut Project, &std::path::Path),
) -> (
    PathBuf,
    gpui::Entity<crate::shell::Shell>,
    &'a mut gpui::VisualTestContext,
    Project,
    PathBuf,
    PathBuf,
) {
    let dir = rig::folder(case);
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), vec![rig::nowhere()]);
    let (main, wt) = rig::repo_with_worktree(&dir);
    let mut project = Project::new(ProjectId::mint(seed), None, main.clone());
    edit(&mut project, &dir);
    rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    let at = main.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    (dir, shell, cx, project, main, wt)
}

#[gpui::test]
fn adding_an_offered_checkout_records_it_and_its_row_holds_it(cx: &mut TestAppContext) {
    let (dir, shell, cx, project, main, wt) = opened("wt-add", 914_001, cx, |_, _| {});
    let at = wt.clone();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let listed = onehand_core::worktree::Listed {
                path: at,
                branch: Some("w".into()),
            };
            shell.worktree_act(crate::chat::pane::WorktreeAct::Add(listed), window, cx)
        })
    });
    cx.run_until_parked();
    let recorded = worktrees_of(&project.id, cx);
    assert_eq!(
        recorded
            .iter()
            .map(|w| (w.path.clone(), w.made_by, w.branch.clone()))
            .collect::<Vec<_>>(),
        [(wt.clone(), MadeBy::Added, "w".to_string())]
    );
    cx.update(|_, cx| {
        let workspace = &shell.read(cx).window.workspace;
        assert_eq!(workspace.local_index(&wt), workspace.local_index(&main));
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn removing_a_worktree_forgets_it_and_its_folder_goes(cx: &mut TestAppContext) {
    let (dir, shell, cx, project, _, wt) = opened("wt-remove", 914_002, cx, |p, _| {
        let wt = std::fs::canonicalize(p.path.parent().unwrap().join("wt")).unwrap();
        p.worktrees.push(Worktree {
            path: wt,
            branch: "w".into(),
            from: None,
            made_by: MadeBy::Split,
            offload: None,
        });
    });
    let at = wt.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.remove_worktree_now(at, window, cx)));
    let busy = cx.update(|_, cx| Shared::global(cx).projects.removing(&wt));
    assert!(busy, "under way: a second press is not taken");
    until(cx, |cx| {
        !wt.exists() && worktrees_of(&project.id, cx).is_empty()
    });
    until(cx, |cx| {
        cx.update(|_, cx| !Shared::global(cx).projects.removing(&wt))
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn the_worktree_folder_setting_places_the_next_worktree(cx: &mut TestAppContext) {
    let (dir, shell, cx, _, main, _) = opened("wt-parent", 914_003, cx, |p, dir| {
        let trees = dir.join("trees");
        std::fs::create_dir_all(&trees).unwrap();
        p.worktree_parent = Some(trees);
    });
    let target = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.window.workspace.local_index(&main).unwrap();
            shell.begin_worktree(idx, window, cx);
            shell
                .worktree_branch
                .update(cx, |state, cx| state.set_value("feat", window, cx));
            shell.worktree_target(cx)
        })
    });
    assert_eq!(
        target,
        Some(onehand_core::worktree::worktree_dir_in(
            &dir.join("trees"),
            &main,
            "feat"
        ))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_missing_worktree_folder_is_refused_in_the_form(cx: &mut TestAppContext) {
    let (dir, shell, cx, _, main, _) = opened("wt-parent-gone", 914_004, cx, |p, dir| {
        p.worktree_parent = Some(dir.join("gone"));
    });
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.window.workspace.local_index(&main).unwrap();
            shell.begin_worktree(idx, window, cx);
            shell
                .worktree_branch
                .update(cx, |state, cx| state.set_value("feat", window, cx));
            shell.commit_worktree(cx);
        })
    });
    let gone = dir.join("gone");
    let mut said = None;
    until(cx, |cx| {
        said = cx.update(|_, cx| {
            let shell = shell.read(cx);
            shell.worktree_draft.as_ref().and_then(|d| d.error.clone())
        });
        said.is_some()
    });
    assert_eq!(
        said,
        Some(format!("{} is not there any more", gone.display()))
    );
    assert!(!gone.exists(), "nothing made");
    let _ = std::fs::remove_dir_all(dir);
}

/// A removal whose window closes before it lands still ends: nothing is left
/// under way, and the record says what is on disk.
#[gpui::test]
fn a_removal_whose_window_closes_still_ends(cx: &mut TestAppContext) {
    let (dir, shell, cx, project, _, wt) = opened("wt-remove-closed", 914_005, cx, |p, _| {
        let wt = std::fs::canonicalize(p.path.parent().unwrap().join("wt")).unwrap();
        p.worktrees.push(Worktree {
            path: wt,
            branch: "w".into(),
            from: None,
            made_by: MadeBy::Split,
            offload: None,
        });
    });
    let at = wt.clone();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.remove_worktree_now(at, window, cx));
        window.remove_window();
    });
    drop(shell);
    let ended = |cx: &mut gpui::VisualTestContext| {
        cx.cx
            .update(|cx| !Shared::global(cx).projects.removing(&wt))
    };
    for _ in 0..300 {
        cx.cx.run_until_parked();
        if ended(cx) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ended(cx), "nothing left under way");
    let recorded = cx.cx.update(|cx| {
        Shared::global(cx)
            .projects
            .get(&project.id)
            .unwrap()
            .worktrees
            .len()
    });
    assert_eq!(
        recorded == 1,
        wt.exists(),
        "the record says what is on disk"
    );
    let _ = std::fs::remove_dir_all(dir);
}
