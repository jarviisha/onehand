//! Two tasks of one project at a step only one may be at: the second waits
//! before it until the first has moved on, and a run sent back to it while
//! another holds it waits in turn.

use super::harness::{git, repo, Flow};
use crate::acp::{AcpEvent, AcpRequest};
use crate::task::drive::Lane;
use crate::task::{Origin, Starting};
use crate::workflow::{Brief, GateKind, Outcome, Place, StepKind, StepSpec, Template, Verdict};
use std::path::Path;

const NOW: u64 = 1_800_000_000;

fn agent_step(id: &str, label: &str, limit: Option<u32>) -> StepSpec {
    StepSpec {
        limit,
        id: id.into(),
        label: label.into(),
        kind: StepKind::Agent {
            prompt: format!("{label} it."),
            gates: Vec::new(),
            keep_answer: false,
            mode: None,
            options: Default::default(),
            agent: None,
        },
    }
}

/// Implement, which one run at a time may be at and which commits, then
/// `then`, each on a worktree of its own.
fn limited(then: StepSpec) -> Template {
    let mut implement = agent_step("implement", "Implement", Some(1));
    if let StepKind::Agent { gates, .. } = &mut implement.kind {
        gates.push(GateKind::Committed);
    }
    let mut template = Template::blank("Limited");
    template.id = "limited".into();
    template.place = Place::Worktree;
    template.steps = vec![implement, then];
    template
}

/// Task `id`'s agent commits a change on its worktree, then its turn ends.
fn commits(flow: &mut Flow, id: &str) {
    let dir = flow.tasks.task(id).unwrap().setup.dir.clone();
    std::fs::write(dir.join(format!("{id}.txt")), id).unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", id]);
    turn_ends(flow, id);
}

fn starting(project: &Path, template: &Template, title: &str) -> Starting {
    Starting {
        project: project.to_path_buf(),
        template: template.clone(),
        brief: Brief {
            title: title.into(),
            body: String::new(),
            instructions: None,
            closes: None,
        },
        origin: Origin::Tool,
        grant: Default::default(),
        agent: None,
        mode: None,
        column: None,
    }
}

/// The prompts task `id`'s session was sent since last asked.
fn prompts(flow: &mut Flow, id: &str) -> Vec<String> {
    flow.sent(id)
        .into_iter()
        .filter_map(|r| match r {
            AcpRequest::Prompt { text, .. } => Some(text),
            AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. } => None,
        })
        .collect()
}

fn turn_ends(flow: &mut Flow, id: &str) {
    flow.agent(
        id,
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
    );
}

#[test]
fn two_runs_share_a_step_of_limit_one() {
    let project = repo("flow-rooms");
    let template = limited(agent_step("test", "Test", None));
    let mut flow = Flow::new(vec![template.clone()], NOW);
    let first = flow.start(starting(&project, &template, "First"));
    let second = flow.start(starting(&project, &template, "Second"));

    // The first has the step; the second waits before it, unprompted.
    assert!(prompts(&mut flow, &first)[0].starts_with("Implement it."));
    assert!(prompts(&mut flow, &second).is_empty());
    assert_eq!(flow.tasks.waits_for_room(&second), Some("implement"));
    assert!(flow.runs[&second].run.run().awaiting_room());

    // The first moves on: the second is given the step and prompted.
    commits(&mut flow, &first);
    assert!(prompts(&mut flow, &first)[0].starts_with("Test it."));
    assert_eq!(flow.tasks.waits_for_room(&second), None);
    assert!(prompts(&mut flow, &second)[0].starts_with("Implement it."));

    // Both finish.
    turn_ends(&mut flow, &first);
    commits(&mut flow, &second);
    turn_ends(&mut flow, &second);
    assert!(flow.runs.is_empty(), "{:?}", flow.said);
    for id in [&first, &second] {
        assert_eq!(flow.tasks.task(id).unwrap().outcome(), Some(Outcome::Done));
    }
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn a_run_sent_back_to_a_full_step_waits_for_it() {
    let project = repo("flow-rooms-back");
    let review = StepSpec {
        limit: None,
        id: "review".into(),
        label: "Review".into(),
        kind: StepKind::Review {
            of: "implement".into(),
            on_reject: None,
            agent: None,
            prompt: "Review it.".into(),
            mode: None,
            options: Default::default(),
        },
    };
    let template = limited(review);
    let mut flow = Flow::new(vec![template.clone()], NOW);
    let first = flow.start(starting(&project, &template, "First"));
    let second = flow.start(starting(&project, &template, "Second"));

    assert!(prompts(&mut flow, &first)[0].starts_with("Implement it."));

    // The first goes on to its review, and the second takes the step.
    commits(&mut flow, &first);
    assert!(prompts(&mut flow, &first)
        .iter()
        .any(|p| p.contains("Review it.")));
    assert!(prompts(&mut flow, &second)[0].starts_with("Implement it."));

    // The review sends the first back while the second holds the step.
    let verdict = flow.runs.get_mut(&first).unwrap().run.verdict(
        &Lane::Step("review".into()),
        Verdict::SendBack("Try again.".into()),
    );
    assert!(verdict.is_ok(), "{verdict:?}");
    turn_ends(&mut flow, &first);
    assert_eq!(flow.tasks.waits_for_room(&first), Some("implement"));
    assert!(prompts(&mut flow, &first).is_empty());

    // The second moves on: the first has the step again, with the note.
    commits(&mut flow, &second);
    assert_eq!(flow.tasks.waits_for_room(&first), None);
    let again = prompts(&mut flow, &first);
    assert!(again.iter().any(|p| p.contains("Try again.")), "{again:?}");
    let key = crate::task::rooms::RoomKey {
        repo: project.clone(),
        workflow: "limited".into(),
        step: "implement".into(),
    };
    assert_eq!(flow.tasks.room_at(&key), (1, 0));
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn a_limited_last_step_is_given_on_when_its_run_ends() {
    let project = repo("flow-rooms-last");
    let mut template = limited(agent_step("test", "Test", Some(1)));
    template.steps[0].limit = None;
    let mut flow = Flow::new(vec![template.clone()], NOW);
    let first = flow.start(starting(&project, &template, "First"));
    let second = flow.start(starting(&project, &template, "Second"));

    // Both implement at once; the first reaches the test and holds it.
    commits(&mut flow, &first);
    assert!(prompts(&mut flow, &first)
        .iter()
        .any(|p| p.starts_with("Test it.")));
    commits(&mut flow, &second);
    assert_eq!(flow.tasks.waits_for_room(&second), Some("test"));

    // The first ends: the second is given the test.
    turn_ends(&mut flow, &first);
    assert_eq!(
        flow.tasks.task(&first).unwrap().outcome(),
        Some(Outcome::Done)
    );
    assert_eq!(flow.tasks.waits_for_room(&second), None);
    assert!(prompts(&mut flow, &second)
        .iter()
        .any(|p| p.starts_with("Test it.")));
    let _ = std::fs::remove_dir_all(project);
}
