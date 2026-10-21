use super::*;

fn agent(id: &str, keep_answer: bool) -> StepSpec {
    StepSpec {
        id: id.to_string(),
        label: id.to_string(),
        kind: StepKind::Agent {
            prompt: String::new(),
            gates: Vec::new(),
            keep_answer,
            mode: None,
            options: std::collections::BTreeMap::new(),
        },
    }
}

fn command(id: &str, on_fail: &str) -> StepSpec {
    StepSpec {
        id: id.to_string(),
        label: id.to_string(),
        kind: StepKind::Command {
            command: None,
            on_fail: on_fail.to_string(),
        },
    }
}

#[test]
fn facts_command_points_back() {
    let steps = [agent("plan", false), command("test", "plan")];
    assert_eq!(step_facts(1, &steps), vec!["on failure ↑ 1".to_string()]);
}

#[test]
fn facts_unknown_target_says_nothing() {
    let steps = [agent("plan", false), command("test", "nope")];
    assert!(step_facts(1, &steps).is_empty());
}

#[test]
fn facts_later_target_says_nothing() {
    let steps = [command("test", "plan"), agent("plan", false)];
    assert!(step_facts(0, &steps).is_empty());
}

#[test]
fn facts_agent_keeps_answer() {
    assert_eq!(
        step_facts(0, &[agent("plan", true)]),
        vec!["keeps answer".to_string()]
    );
}

#[test]
fn split_problems_first_per_step() {
    let problems = [
        (Some(1), "a".to_string()),
        (Some(1), "b".to_string()),
        (None, "c".to_string()),
        (Some(9), "d".to_string()),
    ];
    assert_eq!(
        split_problems(&problems, 3),
        vec![None, Some("a".to_string()), None]
    );
}

#[test]
fn follow_open_added() {
    assert_eq!(follow_open(Some(0), StepEdit::Added, 3), Some(2));
    assert_eq!(follow_open(None, StepEdit::Added, 1), Some(0));
}

#[test]
fn follow_open_moved_open_step() {
    assert_eq!(
        follow_open(Some(1), StepEdit::Moved { from: 1, to: 2 }, 3),
        Some(2)
    );
}

#[test]
fn follow_open_moved_neighbour() {
    assert_eq!(
        follow_open(Some(2), StepEdit::Moved { from: 1, to: 2 }, 3),
        Some(1)
    );
    assert_eq!(
        follow_open(Some(0), StepEdit::Moved { from: 1, to: 2 }, 3),
        Some(0)
    );
}

#[test]
fn follow_open_removed_open() {
    assert_eq!(follow_open(Some(1), StepEdit::Removed(1), 2), None);
}

#[test]
fn follow_open_removed_before() {
    assert_eq!(follow_open(Some(2), StepEdit::Removed(0), 2), Some(1));
}

#[test]
fn follow_open_removed_after() {
    assert_eq!(follow_open(Some(0), StepEdit::Removed(2), 2), Some(0));
}

#[test]
fn sentence_raises_the_first_letter() {
    assert_eq!(sentence("it has no name"), "It has no name");
    assert_eq!(sentence(""), "");
}
