//! A schedule comes due, its task starts off screen, the task's run prompts
//! its session, the session's turn ends, the run is done, and the schedule
//! hears its task is no longer working.

use super::harness::{git, repo, Flow};
use crate::acp::{AcpEvent, AcpRequest};
use crate::schedule::{Missed, Overlap, Schedule, Starts};
use crate::task::tasks::Placed;
use crate::task::Origin;
use crate::workflow::Outcome;
use std::collections::BTreeMap;
use std::path::Path;

const NOW: u64 = 1_800_000_000;

/// Every minute on `project`, switched on an hour ago, last fired two
/// minutes ago: due at the next look.
fn due(project: &Path) -> Schedule {
    Schedule {
        id: "s1".into(),
        name: "Tidy".into(),
        project: project.to_path_buf(),
        on: true,
        when: "* * * * *".into(),
        agent: None,
        mode: None,
        tools: vec!["list_issues".into()],
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Skip,
        on_since: NOW - 3600,
        last_fired: Some(NOW - 120),
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Prompt {
            prompt: "Tidy the readme".into(),
        },
    }
}

#[test]
fn a_due_schedule_starts_a_task_whose_run_prompts_its_session_and_finishes() {
    let project = repo("flow-schedule", &[]);
    let mut flow = Flow::new(None, Vec::new(), NOW);
    flow.schedules = vec![due(&project.root)];

    // The first look starts the task, on the checkout, off screen.
    flow.clock();
    assert_eq!(flow.told, vec![("t1".to_string(), Placed::Started)]);
    let task = flow.tasks.task("t1").expect("kept as a task");
    match &task.origin {
        Some(Origin::Schedule { id, .. }) => assert_eq!(id, "s1"),
        origin @ (Some(Origin::Tool) | None) => panic!("{origin:?}"),
    }
    assert_eq!(task.grant.tools, ["list_issues"]);
    assert_eq!(task.setup.agent.as_deref(), Some("a"));
    assert_eq!(flow.schedules[0].last_fired, Some(NOW), "written back");
    assert!(
        flow.workspace.roots.iter().any(|r| r.path == project.root),
        "its project kept in the window"
    );

    // Its run's prompt reached the session's agent.
    let sent = flow.sent("t1");
    assert!(
        sent.iter().any(|r| match r {
            AcpRequest::Prompt { .. } => true,
            AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. } => false,
        }),
        "{sent:?}"
    );

    // While its turn runs the schedule is busy, so a look starts nothing.
    flow.now += 60;
    flow.clock();
    assert_eq!(flow.told.len(), 1, "skipped while its task works");

    // The agent's turn ends; the run is done, and the task with it.
    git(
        &project.root,
        &["commit", "-q", "--allow-empty", "-m", "tidy"],
    );
    flow.agent(
        "t1",
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
    );
    assert!(flow.runs.is_empty(), "the run finished");
    let task = flow.tasks.task("t1").unwrap();
    assert_eq!(task.outcome(), Some(Outcome::Done));
    assert!(!flow.tasks.busy("t1"), "its place let go");

    // The next look finds it no longer working, and starts it again.
    flow.now += 60;
    flow.clock();
    assert_eq!(flow.told.len(), 2, "{:?}", flow.said);
    assert_eq!(flow.told[1], ("t2".to_string(), Placed::Started));
    let _ = std::fs::remove_dir_all(project.root);
}

/// A schedule starting a workflow that works on a worktree cuts it first:
/// its task works there, on its own branch, and its project is kept.
#[test]
fn a_schedule_on_a_worktree_workflow_cuts_it_first() {
    let project = repo("flow-schedule-worktree", &[]);
    let worktree = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.place == crate::workflow::Place::Worktree)
        .expect("a workflow on a worktree");
    let id = worktree.id.clone();
    let mut flow = Flow::new(None, vec![worktree], NOW);
    let mut s = due(&project.root);
    s.starts = Starts::Workflow {
        workflow: id,
        title: "Tidy".into(),
        body: String::new(),
    };
    flow.schedules = vec![s];
    flow.clock();
    assert_eq!(flow.told, vec![("t1".to_string(), Placed::Started)]);
    let task = flow.tasks.task("t1").unwrap();
    assert_ne!(task.setup.dir, project.root, "on a worktree of its own");
    assert!(task.setup.dir.is_dir());
    assert!(task.setup.branch.is_some());
    let _ = std::fs::remove_dir_all(&task.setup.dir);
    let _ = std::fs::remove_dir_all(project.root);
}
