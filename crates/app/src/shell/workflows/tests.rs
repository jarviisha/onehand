//! A task driven on a real shell, as the tasks' drive begins it: an issue's
//! run comes up off screen, on a session of its own, in a project the
//! window lists but does not select.

use crate::shell::rig::{folder, shell};
use gpui::TestAppContext;
use onehand_core::config::AgentSpec;
use onehand_core::task::Task;
use onehand_core::unattended::{IssueSource, TrackerRef};
use onehand_core::workflow::{Brief, Setup, builtin};
use std::path::Path;
use std::time::Duration;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// An issue's task, given its place, comes up off screen: it is told it
/// started, and its project is listed in the window without moving what
/// the person has selected.
#[gpui::test]
fn an_issues_run_comes_up_off_screen(cx: &mut TestAppContext) {
    let dir = folder("issue-run");
    let agent = AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    };
    let (window, shell, cx) = shell(cx, dir.join("tasks"), vec![agent]);
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("a.txt"), "a").unwrap();
    git(&root, &["add", "a.txt"]);
    git(
        &root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "one",
        ],
    );
    let setup = Setup {
        repo: root.clone(),
        dir: root.clone(),
        branch: Some("onehand/local-1-fix".into()),
        agent: Some("a".into()),
        check: None,
        mode: None,
        forge: None,
    };
    let brief = Brief {
        title: "#1".into(),
        body: String::new(),
        instructions: None,
    };
    let issue = IssueSource {
        tracker: TrackerRef::Local {
            file: dir.join("issues.json"),
        },
        number: 1,
        forge_ref: None,
        forge: None,
        base: "main".into(),
        picked: false,
        unsent: Vec::new(),
        notes: Vec::new(),
    };
    let template = builtin::all().remove(0);
    let task = Task::of_issue("i1".into(), template, brief, setup, issue);
    let before = cx.update(|_, cx| shell.read(cx).workspace_window().workspace.active_root);
    let told = cx.update(|_, cx| crate::task::submit_told(task, window, cx));
    for _ in 0..100 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    let placed = futures::executor::block_on(told).unwrap();
    assert_eq!(placed, crate::task::Placed::Started);
    let (listed, after) = cx.update(|_, cx| {
        let ws = &shell.read(cx).workspace_window().workspace;
        let listed = ws
            .roots
            .iter()
            .any(|r| r.path == root && !r.sessions.is_empty());
        (listed, ws.active_root)
    });
    assert!(listed, "its project listed, with its session");
    assert_eq!(after, before, "and not selected");
    let _ = std::fs::remove_dir_all(dir);
}
