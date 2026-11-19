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
