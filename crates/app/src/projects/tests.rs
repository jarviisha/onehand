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
