/// No dialog here names itself through the library's own title slot.
///
/// A dialog opened from a trigger is rebuilt when that trigger is pressed,
/// out of its content builder, its style and its props. Its title, header
/// and footer are elements, which cannot be cloned into a builder that runs
/// again on every open, so they do not survive the trip -- and every dialog
/// the rail opened lost its name and its buttons that way, in silence,
/// while otherwise working.
///
/// The name goes in the content instead, and on every dialog rather than
/// only the triggered ones: one shape is what stops the next dialog picking
/// the wrong one. This is here because nothing else says no -- the slot
/// exists, compiles, and does nothing.
#[test]
fn no_dialog_names_itself_through_the_library_slot() {
    // Assembled at run time so this test is not a match for itself.
    let slot = format!(".{}(", "title");
    for (file, text) in [
        ("dialogs.rs", include_str!("../dialogs.rs")),
        ("dialogs/rows.rs", include_str!("rows.rs")),
    ] {
        for (n, line) in text.lines().enumerate() {
            assert!(
                !line.contains(&slot),
                "{file}:{}: names the dialog through the library's title \
                 slot, which a triggered dialog drops on its way back open. \
                 Put the name in the content instead.\n    {}",
                n + 1,
                line.trim()
            );
        }
    }
}

/// Enter in a dialog's field does what its primary button does: the
/// library's own confirm closed the dialog, and closing it threw away what
/// was typed.
#[gpui::test]
fn enter_in_a_dialogs_field_presses_its_primary(cx: &mut gpui::TestAppContext) {
    use crate::shell::rig::{folder, shell};
    use crate::state::Shared;
    use gpui::BorrowAppContext as _;
    use onehand_core::project::{Project, ProjectId, Projects, ProjectsDrive};
    let dir = folder("dialog-enter");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(80), None, root.clone());
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
            shell.open_build_host_form(None, window, cx);
            let host = shell.build_host_draft().expect("the form").host.clone();
            host.update(cx, |input, cx| input.set_value("win", window, cx));
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let builds = cx.update(|_, cx| {
        Shared::global(cx).projects.get(&project.id).map(|p| {
            p.builds
                .iter()
                .map(|b| b.host.to_string())
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(
        builds,
        Some(vec!["win".to_string()]),
        "Enter added the host typed"
    );
    assert!(
        cx.update(|_, cx| shell.read(cx).build_host_draft().is_none()),
        "and the form closed as Add closes it"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Every dialog the shell builds says what Enter does in it. One left to
/// the library closes on Enter, and closing a shell dialog clears what was
/// typed in it.
#[test]
fn every_shell_dialog_says_what_enter_does() {
    // Assembled at run time so this test is not a match for itself.
    let (built, empty, said) = (
        format!("{}::new(cx)", "Dialog"),
        format!("return {}::new(cx)", "Dialog"),
        format!(".{}(", "on_ok"),
    );
    for (file, text) in [
        ("dialogs.rs", include_str!("../dialogs.rs")),
        ("dialogs/agent.rs", include_str!("agent.rs")),
        ("dialogs/branch.rs", include_str!("branch.rs")),
        ("dialogs/build_host.rs", include_str!("build_host.rs")),
        ("dialogs/host_project.rs", include_str!("host_project.rs")),
        ("dialogs/offload.rs", include_str!("offload.rs")),
    ] {
        let dialogs = text.matches(&built).count() - text.matches(&empty).count();
        let enters = text.matches(&said).count();
        assert_eq!(
            dialogs, enters,
            "{file}: {dialogs} dialogs built, {enters} saying what Enter does; \
             give each `.on_ok(enter_presses(..))`"
        );
    }
}
