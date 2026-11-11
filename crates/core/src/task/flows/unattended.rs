//! Unattended runs: a tick claims a labelled issue, its task's run prompts
//! its session, the turn commits and ends, and the run's end reports to its
//! issue once.

use super::harness::{git, repo, Flow};
use crate::acp::{AcpEvent, AcpRequest};
use crate::config::UnattendedConfig;
use crate::task::tasks::Placed;
use crate::unattended::drive::UnattendedDrive;
use crate::workflow::{GateKind, Outcome, Place, StepKind, StepSpec, Template};
use std::collections::BTreeMap;
use std::path::Path;

/// A workflow on a worktree of one agent turn that commits.
fn plain() -> Template {
    let mut t = Template::blank("plain");
    t.id = "plain".into();
    t.place = Place::Worktree;
    t.steps = vec![StepSpec {
        id: "work".into(),
        label: "Work".into(),
        kind: StepKind::Agent {
            prompt: "go".into(),
            gates: vec![GateKind::Committed],
            keep_answer: false,
            mode: None,
            options: BTreeMap::new(),
        },
    }];
    t
}

fn notes(file: &Path, number: u64) -> Vec<String> {
    let kept = crate::issues::load_blocking(file).unwrap();
    let issue = kept.get(number).unwrap();
    issue.notes.iter().map(|note| note.text.clone()).collect()
}

#[test]
fn a_tick_claims_an_issue_whose_run_prompts_its_session_and_reports_once_done() {
    let project = repo(
        "flow-unattended",
        &[("Fix the bug", &["auto"]), ("Then this", &["auto"])],
    );
    let file = project.issues.clone().unwrap();
    let cfg = UnattendedConfig {
        workflow: "plain".into(),
        at_once: 1,
        ..Default::default()
    };
    let (unattended, _) = UnattendedDrive::new::<u8>(&cfg, &[]);
    let mut flow = Flow::new(Some(unattended), vec![plain()], 1_800_000_000);
    flow.roots = vec![project.clone()];

    // The tick claims the issue; its task asks for its place, held to the
    // cap, and its run starts off screen on the worktree the claim cut.
    flow.tick();
    assert_eq!(flow.told, vec![("t1".to_string(), Placed::Started)]);
    let task = flow.tasks.task("t1").expect("the claim kept as a task");
    assert_eq!(task.issue().unwrap().number, 1);
    let dir = task.setup.dir.clone();
    assert_ne!(dir, project.root, "on a worktree of its own");
    assert!(flow.runs.contains_key("t1"), "driven");
    let opened = &flow.opened["t1"];
    assert!(
        opened[0].starts_with("Unattended run on issue"),
        "{opened:?}"
    );
    let sent = flow.sent("t1");
    assert!(
        sent.iter().any(|r| matches!(r, AcpRequest::Prompt { .. })),
        "{sent:?}"
    );

    assert_eq!(notes(&file, 1).len(), 1, "the claim said so on its issue");

    // A second tick finds no slot free for the second issue.
    flow.tick();
    assert!(flow.tasks.task("t2").is_none(), "held to the cap of one");

    // The agent commits and ends its turn: the run is done, its project
    // dropped, and its issue told once.
    std::fs::write(dir.join("fix.txt"), "fixed").unwrap();
    git(&dir, &["add", "fix.txt"]);
    git(&dir, &["commit", "-qm", "fix"]);
    flow.agent(
        "t1",
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
    );
    assert!(flow.runs.is_empty(), "the run finished: {:?}", flow.said);
    let task = flow.tasks.task("t1").unwrap();
    assert!(
        matches!(task.outcome(), Some(Outcome::Done)),
        "{:?}",
        task.outcome()
    );
    assert_eq!(flow.torn_down, vec![(dir.clone(), false)]);
    let said = notes(&file, 1);
    assert_eq!(said.len(), 2, "the claim's, then the report once: {said:?}");
    assert!(
        said[1].contains("Every step of the workflow passed"),
        "{said:?}"
    );
    assert!(
        task.issue().unwrap().unsent.is_empty(),
        "and the report left its task"
    );

    // Its slot is free again: the next tick claims the second issue.
    flow.tick();
    let next = flow.tasks.task("t2").expect("the second issue claimed");
    assert_eq!(next.issue().unwrap().number, 2);
    let _ = std::fs::remove_dir_all(&next.setup.dir);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(project.root);
}
