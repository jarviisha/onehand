use crate::shell::rig::{another, folder, shell};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, TestAppContext};
use onehand_core::project::{Project, ProjectId, Projects, ProjectsDrive};

/// The label of the row of `path` in `shell`'s workspace.
fn label_in(
    shell: &gpui::Entity<crate::shell::Shell>,
    path: &std::path::Path,
    cx: &mut gpui::VisualTestContext,
) -> Option<String> {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .workspace_window()
            .workspace
            .roots
            .iter()
            .find(|root| root.path == path)
            .map(|root| root.label.clone())
    })
}

#[gpui::test]
fn renaming_a_project_relabels_it_in_every_window(cx: &mut TestAppContext) {
    let dir = folder("rename-everywhere");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(77), None, root.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                ProjectsDrive::new(Projects::from_iter([project.clone()]), dir.join("projects"));
        })
    });
    // One window opens the folder; the other was made on it, its row of no
    // project yet, and places it as a window does when it opens.
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let (other_window, other) = another(cx, &root);
    cx.update(|_, cx| crate::projects::place_rows(other_window, other.clone(), cx));
    cx.run_until_parked();
    assert_eq!(label_in(&other, &root, cx).as_deref(), Some("project"));

    cx.update(|_, cx| {
        let fx = cx
            .update_global::<Shared, _>(|shared, _| shared.projects.rename(&project.id, "Renamed"));
        crate::projects::carry(fx, cx);
    });
    cx.run_until_parked();
    assert_eq!(label_in(&shell, &root, cx).as_deref(), Some("Renamed"));
    assert_eq!(label_in(&other, &root, cx).as_deref(), Some("Renamed"));
    let _ = std::fs::remove_dir_all(dir);
}

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
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never came to pass");
}

#[gpui::test]
fn a_row_carrying_its_own_facts_hands_them_to_its_project(cx: &mut TestAppContext) {
    use onehand_core::config::{WorkspaceConfig, WorkspaceLoad};
    let dir = folder("row-facts");
    let (_, _, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects = ProjectsDrive::new(Projects::default(), dir.join("projects"));
        })
    });
    // An entry an older build wrote: a folder and its check, no project.
    let storage = dir.join("ws");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(
        storage.join(WorkspaceConfig::FILE),
        format!(
            "name = \"w\"\n[[projects]]\npath = {:?}\ncheck = \"make\"\n",
            root
        ),
    )
    .unwrap();
    let WorkspaceLoad::Found(cfg) = WorkspaceConfig::load_from(&storage) else {
        panic!("the workspace reads");
    };
    let mut made = None;
    let window = cx.cx.add_window(|window, cx| {
        let ws = onehand_core::workspace::Workspace::from_config(
            cfg,
            storage.clone(),
            &Projects::default(),
        );
        let shell = gpui::AppContext::new(cx, |cx| crate::shell::Shell::new(ws, window, cx));
        made = Some(shell.clone());
        gpui_component::Root::new(shell, window, cx)
    });
    let (handle, other): (gpui::AnyWindowHandle, _) = (window.into(), made.unwrap());
    let weak = other.downgrade();
    cx.update(|_, cx| crate::windows::register(storage.clone(), handle, weak, cx));
    cx.update(|_, cx| crate::projects::place_rows(handle, other.clone(), cx));
    until(cx, |cx| {
        cx.update(|_, cx| Shared::global(cx).projects.find(None, &root).is_some())
    });
    cx.run_until_parked();
    let check = cx.update(|_, cx| {
        Shared::global(cx)
            .projects
            .find(None, &root)
            .and_then(|(project, _)| project.check.clone())
    });
    assert_eq!(
        check.as_deref(),
        Some("make"),
        "the project took the row's check"
    );
    let row = cx.update(|_, cx| {
        let shell = other.read(cx);
        let root_row = &shell.workspace_window().workspace.roots[0];
        (root_row.project.is_some(), root_row.check.clone())
    });
    assert_eq!(row, (true, Some("make".to_string())));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn renaming_a_project_saves_its_name_and_retitles_its_page(cx: &mut TestAppContext) {
    let dir = folder("rename-from-menu");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(78), None, root.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                ProjectsDrive::new(Projects::from_iter([project.clone()]), dir.join("projects"));
        })
    });
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    cx.run_until_parked();
    let idx = cx.update(|_, cx| shell.read(cx).workspace_window().workspace.active_root);
    let offered = cx.update(|_, cx| {
        let shell = shell.read(cx);
        crate::chat::pane::ProjectFacts::of(&shell.workspace_window().workspace.roots[idx], false)
            .renames
    });
    assert!(offered, "a project's own row offers the rename");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.begin_project_rename(idx, window, cx);
            let input = shell.project_rename_input().expect("the dialog's field");
            assert_eq!(
                input.read(cx).value().as_ref(),
                "project",
                "it starts on the name"
            );
            input.update(cx, |input, cx| input.set_value("  Onehand  ", window, cx));
            shell.commit_project_rename(cx);
        })
    });
    cx.run_until_parked();
    let name = cx.update(|_, cx| {
        Shared::global(cx)
            .projects
            .get(&project.id)
            .map(|p| p.name.clone())
    });
    assert_eq!(name.as_deref(), Some("Onehand"));
    assert_eq!(label_in(&shell, &root, cx).as_deref(), Some("Onehand"));
    let page = cx.update(|_, cx| {
        let pane = shell.read(cx).chat_pane().clone();
        pane.read(cx).project_page_label()
    });
    assert_eq!(
        page.as_deref(),
        Some("Onehand"),
        "the page on screen says it too"
    );
    assert!(
        cx.update(|_, cx| shell.read(cx).project_rename_input().is_none()),
        "the dialog closed"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn enter_in_the_name_field_renames_the_project(cx: &mut TestAppContext) {
    let dir = folder("rename-enter");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(79), None, root.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                ProjectsDrive::new(Projects::from_iter([project.clone()]), dir.join("projects"));
        })
    });
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let idx = shell.workspace_window().workspace.active_root;
            shell.begin_project_rename(idx, window, cx);
            let input = shell
                .project_rename_input()
                .expect("the dialog's field")
                .clone();
            input.update(cx, |input, cx| input.set_value("Typed", window, cx));
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let name = cx.update(|_, cx| {
        Shared::global(cx)
            .projects
            .get(&project.id)
            .map(|p| p.name.clone())
    });
    assert_eq!(name.as_deref(), Some("Typed"), "Enter saves what was typed");
    let _ = std::fs::remove_dir_all(dir);
}

/// `P` at `<dir>/p` recording its worktree `<dir>/p-x`, both folders made.
fn project_with_worktree(
    dir: &std::path::Path,
    seed: u64,
) -> (Project, std::path::PathBuf, std::path::PathBuf) {
    let (home, worktree) = (dir.join("p"), dir.join("p-x"));
    for folder in [&home, &worktree] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (home, worktree) = (
        std::fs::canonicalize(home).unwrap(),
        std::fs::canonicalize(worktree).unwrap(),
    );
    let mut project = Project::new(ProjectId::mint(seed), None, home.clone());
    project.worktrees.push(onehand_core::project::Worktree {
        path: worktree.clone(),
        branch: "x".into(),
        from: None,
        made_by: onehand_core::project::MadeBy::Split,
        offload: None,
    });
    (project, home, worktree)
}

#[gpui::test]
fn a_conversation_of_a_removed_worktree_shows_on_its_projects_page(cx: &mut TestAppContext) {
    let dir = folder("conv-stray");
    let (handle, _shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (project, home, _) = project_with_worktree(&dir, 911_001);
    crate::shell::rig::keep_projects(handle, &_shell, vec![project.clone()], cx);
    let gone = dir.join("p-gone");
    let sid = format!("conv-stray-{}", std::process::id());
    crate::shell::rig::kept_conversation(&sid, &gone, &project.id, None);
    let own = cx.update(|_, cx| crate::projects::scope_of(&home, cx));
    let store = onehand_core::chat::conversations_dir();
    let listed = onehand_core::chat::list_conversations(&store, &own, None)
        .into_iter()
        .any(|conv| conv.session_id == sid);
    assert!(listed, "the project's row lists it");
    let _ = std::fs::remove_dir_all(store.join(&sid));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_projects_row_lists_every_conversation_of_it(cx: &mut TestAppContext) {
    let dir = folder("conv-once");
    let (handle, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (project, home, worktree) = project_with_worktree(&dir, 911_002);
    crate::shell::rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    for folder in [&home, &worktree] {
        let at = folder.clone();
        cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    }
    let sid = format!("conv-once-{}", std::process::id());
    let stray = format!("conv-once-stray-{}", std::process::id());
    crate::shell::rig::kept_conversation(&sid, &worktree, &project.id, None);
    crate::shell::rig::kept_conversation(&stray, &dir.join("p-gone"), &project.id, None);
    crate::shell::rig::show_workspace(&shell, cx);
    let mut found = Vec::new();
    for _ in 0..100 {
        cx.run_until_parked();
        found = crate::shell::rig::workspace_recent(&shell, cx)
            .into_iter()
            .filter(|(_, id)| *id == sid || *id == stray)
            .collect();
        if found.len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    found.sort();
    let mut want = vec![(home.clone(), sid.clone()), (home.clone(), stray.clone())];
    want.sort();
    assert_eq!(
        found, want,
        "each once, on the project's one row: one that ran in its worktree, and a stray"
    );
    for id in [&sid, &stray] {
        let _ = std::fs::remove_dir_all(onehand_core::chat::conversations_dir().join(id));
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A conversation listed on a project's own row that ran in another folder
/// resumes where it ran, never in the project's folder: one whose folder is
/// gone is refused, and nothing starts.
#[gpui::test]
fn a_stray_conversation_never_resumes_in_its_projects_folder(cx: &mut TestAppContext) {
    let dir = folder("conv-stray-resume");
    let (handle, shell, cx) = shell(cx, dir.join("tasks"), vec![crate::shell::rig::nowhere()]);
    let (project, home, _) = project_with_worktree(&dir, 911_003);
    crate::shell::rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    let at = home.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let gone = dir.join("p-gone");
    let sid = format!("conv-stray-resume-{}", std::process::id());
    crate::shell::rig::kept_conversation(&sid, &gone, &project.id, None);
    let archive = onehand_core::chat::conversations_dir().join(&sid);
    let sessions = |shell: &gpui::Entity<crate::shell::Shell>, cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| {
            shell
                .read(cx)
                .workspace_window()
                .workspace
                .roots
                .iter()
                .map(|root| root.sessions.len())
                .sum::<usize>()
        })
    };
    let before = sessions(&shell, cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.on_workspace_page(
                &crate::workspace_page::WorkspacePageEvent::ResumeIn {
                    root: home.clone(),
                    agent: "nowhere".into(),
                    archive: archive.clone(),
                    ran_in: gone.clone(),
                    session: sid.clone(),
                },
                window,
                cx,
            )
        })
    });
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        sessions(&shell, cx),
        before,
        "nothing started in the project's folder"
    );
    let _ = std::fs::remove_dir_all(archive);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_list_of_worktrees_says_how_many_it_left_out() {
    assert_eq!(crate::projects::more_not_shown(20), None);
    assert_eq!(
        crate::projects::more_not_shown(25).as_deref(),
        Some("5 more not shown")
    );
}
