//! A workflow whose review step names its own agent: a schedule starts it,
//! the implement step works in the run's session, the review in a session
//! of its own opened when the run reaches it, and the run ends done with
//! both sessions closed.

use super::harness::{repo, Flow};
use crate::acp::{AcpEvent, AcpRequest};
use crate::schedule::{Missed, Overlap, Schedule, Starts};
use crate::task::drive::Lane;
use crate::workflow::{Outcome, Place, StepKind, StepSpec, Template};
use std::collections::BTreeMap;
use std::path::Path;

const NOW: u64 = 1_800_000_000;

fn step(id: &str, label: &str, agent: Option<&str>) -> StepSpec {
    StepSpec {
        limit: None,
        id: id.into(),
        label: label.into(),
        kind: StepKind::Agent {
            prompt: format!("{label} it."),
            gates: Vec::new(),
            keep_answer: false,
            mode: None,
            options: Default::default(),
            agent: agent.map(str::to_string),
        },
    }
}

/// Implement on the run's agent, then review on `b`, in the checkout.
fn reviewed() -> Template {
    let mut template = Template::blank("Reviewed");
    template.id = "reviewed".into();
    template.place = Place::Checkout;
    template.steps = vec![
        step("implement", "Implement", None),
        step("review", "Review", Some("b")),
    ];
    template
}

/// Due at the next look, starting [`reviewed`] on `project`.
fn due(project: &Path) -> Schedule {
    Schedule {
        id: "s1".into(),
        name: "Review".into(),
        project: project.to_path_buf(),
        on: true,
        when: "* * * * *".into(),
        agent: None,
        mode: None,
        tools: Vec::new(),
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Skip,
        on_since: NOW - 3600,
        last_fired: Some(NOW - 120),
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Workflow {
            workflow: "reviewed".into(),
            title: "Tidy".into(),
            body: String::new(),
        },
    }
}

/// The prompts among `sent`.
fn prompts(sent: &[AcpRequest]) -> Vec<&str> {
    sent.iter()
        .filter_map(|r| match r {
            AcpRequest::Prompt { text, .. } => Some(text.as_str()),
            AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. } => None,
        })
        .collect()
}

fn turn_ends(flow: &mut Flow) {
    flow.agent(
        "t1",
        AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
    );
}

#[test]
fn a_two_agent_run_works_each_step_in_its_own_session() {
    let project = repo("flow-lanes");
    let mut flow = Flow::new(vec![reviewed()], NOW);
    flow.schedules = vec![due(&project)];
    flow.clock();
    let review = Lane::Step("review".into());

    // Implement's prompt went to the run's own session, the only one.
    let sent = flow.sent_in("t1", &Lane::Run);
    let first = prompts(&sent);
    assert!(
        first.len() == 1 && first[0].starts_with("Implement it."),
        "{first:?}"
    );
    assert_eq!(flow.runs["t1"].lanes.len(), 1);

    // Its turn ends: the review opens a session of its own and prompts it.
    turn_ends(&mut flow);
    let driven = &flow.runs["t1"];
    assert_eq!(driven.run.current_lane(), &review);
    assert_eq!(driven.lanes.len(), 2);
    let sent = flow.sent_in("t1", &review);
    let second = prompts(&sent);
    assert!(
        second.len() == 1 && second[0].starts_with("Review it."),
        "{second:?}"
    );
    assert!(prompts(&flow.sent_in("t1", &Lane::Run)).is_empty());
    let uids: Vec<u64> = flow.runs["t1"].lanes.values().map(|h| h.uid).collect();

    // The review's turn ends: the run is done, and both sessions close.
    turn_ends(&mut flow);
    assert!(flow.runs.is_empty(), "the run finished: {:?}", flow.said);
    let task = flow.tasks.task("t1").unwrap();
    assert_eq!(task.outcome(), Some(Outcome::Done));
    let mut closed = flow.closed.clone();
    closed.sort();
    let mut uids = uids;
    uids.sort();
    assert_eq!(closed, uids);
    let _ = std::fs::remove_dir_all(project);
}
