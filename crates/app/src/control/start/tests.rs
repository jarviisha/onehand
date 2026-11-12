//! A start off screen, as a schedule or a tool makes one, on a real shell:
//! what blocks the launcher refuses it, and one that goes is kept as a task
//! with where it came from, its project kept without being put on screen,
//! and where it landed said to the caller.

use super::*;
use crate::shell::rig::{folder, shell};
use gpui::TestAppContext;
use onehand_core::config::AgentSpec;
use onehand_core::task::{Grant, Origin};
use onehand_core::workflow::{Place, StepKind, builtin};

/// A workflow of one agent step, on the checkout.
fn checkout() -> Template {
    let mut t = builtin::all()
        .into_iter()
        .find(|t| t.place == Place::Checkout)
        .expect("a workflow on the checkout");
    t.steps
        .retain(|step| matches!(step.kind, StepKind::Agent { .. }));
    t.steps.truncate(1);
    assert!(!t.needs_check());
    t
}

fn brief() -> Brief {
    Brief {
        title: "Tidy up".into(),
        body: String::new(),
        instructions: None,
    }
}

fn starting(project: PathBuf, origin: Origin) -> Starting {
    Starting {
        project,
        template: checkout(),
        brief: brief(),
        origin,
        grant: Grant::default(),
        agent: None,
        mode: None,
    }
}

/// With no agent configured, the preflight blocks it: refused with what
/// blocks, and no task kept.
#[gpui::test]
fn a_start_off_screen_is_refused_by_what_blocks_the_launcher(cx: &mut TestAppContext) {
    let dir = folder("refused");
    let (window, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let start = cx.update(|_, cx| {
        cx.spawn(async move |cx| start_task(window, shell, starting(root, Origin::Tool), cx).await)
    });
    cx.run_until_parked();
    let refused = match futures::executor::block_on(start) {
        Ok((_, Placed::Refused(why))) | Err(why) => why,
        Ok((_, placed)) => panic!("{placed:?}"),
    };
    assert!(!refused.is_empty());
    let kept = cx.update(|_, cx| crate::task::each(cx, |_, _| Some(())).len());
    assert_eq!(kept, 0, "nothing kept");
    let _ = std::fs::remove_dir_all(dir);
}

/// One that goes is kept as a task with its origin and the agent a new
/// session takes, its project is kept but not put on screen, and the caller
/// is told where it landed.
#[gpui::test]
fn a_start_off_screen_is_kept_quietly_and_says_where_it_landed(cx: &mut TestAppContext) {
    let dir = folder("kept");
    let agent = AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    };
    let (window, shell, cx) = shell(cx, dir.join("tasks"), vec![agent]);
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let before = cx.update(|_, cx| shell.read(cx).workspace_window().workspace.active_root);
    let at = root.clone();
    let given = starting(
        at,
        Origin::Schedule {
            id: "s1".into(),
            name: "Nightly".into(),
        },
    );
    let start = cx.update(|_, cx| {
        let shell = shell.clone();
        cx.spawn(async move |cx| start_task(window, shell, given, cx).await)
    });
    cx.run_until_parked();
    let (id, placed) = futures::executor::block_on(start).expect("started");
    assert!(matches!(placed, Placed::Started), "{placed:?}");
    let task = cx
        .update(|_, cx| crate::task::task(&id, cx))
        .expect("kept as a task");
    assert!(matches!(task.origin(), Some(Origin::Schedule { .. })));
    assert_eq!(task.setup().agent.as_deref(), Some("a"));
    assert_eq!(task.setup().dir, root);
    let (listed, after) = cx.update(|_, cx| {
        let ws = &shell.read(cx).workspace_window().workspace;
        (ws.roots.iter().any(|r| r.path == root), ws.active_root)
    });
    assert!(listed, "its project is kept");
    assert_eq!(after, before, "and not put on screen");
    let _ = std::fs::remove_dir_all(dir);
}
