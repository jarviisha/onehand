use super::suggested;

#[test]
fn a_folder_suggests_the_builds_its_top_names() {
    assert_eq!(
        suggested(["Cargo.toml", "src"]),
        ["cargo build", "cargo test", "cargo clippy"]
    );
    assert_eq!(suggested(["App.sln"]), ["dotnet build", "dotnet test"]);
    assert_eq!(suggested(["app.csproj"]), ["dotnet build", "dotnet test"]);
    assert!(suggested(["README.md", "cargo.toml.bak"]).is_empty());
}

/// *Add* routes what the form names; the same words again are refused in
/// the form, which stays with what was typed.
#[gpui::test]
fn the_form_adds_a_route_and_says_what_refuses_one(cx: &mut gpui::TestAppContext) {
    use crate::shell::rig::{folder, shell};
    use crate::state::Shared;
    use gpui::BorrowAppContext as _;
    use onehand_core::project::{Project, ProjectId, Projects, ProjectsDrive};
    let dir = folder("route-form");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Cargo.toml"), "").unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(82), None, root.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                ProjectsDrive::new(Projects::from_iter([project.clone()]), dir.join("projects"));
        })
    });
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    cx.run_until_parked();
    let at = root.clone();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.show_project_settings(at, window, cx))
    });
    cx.run_until_parked();
    let offered = cx.update(|_, cx| shell.read(cx).route_suggestions(&root).to_vec());
    assert_eq!(offered, ["cargo build", "cargo test", "cargo clippy"]);
    let add = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_route_form(root.clone(), None, window, cx);
                shell.pick_route_suggestion("cargo build", window, cx);
                shell.save_route(window, cx);
            })
        });
        cx.run_until_parked();
    };
    add(cx);
    let routed = cx.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell.route_draft().is_none(),
            shell
                .routes_of(&root, cx)
                .into_iter()
                .map(|route| route.words)
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(routed, (true, vec!["cargo build".to_string()]));
    add(cx);
    let (said, typed) = cx.update(|_, cx| {
        let draft = shell.read(cx).route_draft().expect("the form stays");
        (
            draft.error.clone(),
            draft.words.read(cx).value().to_string(),
        )
    });
    assert!(said.is_some_and(|why| why.contains("already routes")));
    assert_eq!(typed, "cargo build");
    // A form opened on a command removed before its *Save* says so.
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.cancel_route(cx);
            shell.open_route_form(root.clone(), Some(0), window, cx);
        })
    });
    let id = project.id.clone();
    cx.update(|_, cx| {
        let route = onehand_core::workspace::RoutedCommand::parse("cargo build", "").unwrap();
        let fx =
            cx.update_global::<Shared, _>(|shared, _| shared.projects.remove_route(&id, &route));
        crate::projects::carry(fx, cx);
    });
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.save_route(window, cx)));
    let said = cx.update(|_, cx| shell.read(cx).route_draft().and_then(|d| d.error.clone()));
    assert_eq!(said.as_deref(), Some("cargo build was removed meanwhile"));
    let _ = std::fs::remove_dir_all(dir);
}
