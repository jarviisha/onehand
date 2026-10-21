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

fn row(id: &str, value: &str) -> (String, String) {
    (id.to_string(), value.to_string())
}

fn map(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(id, value)| (id.to_string(), value.to_string()))
        .collect()
}

#[test]
fn a_drafts_settings_round_trip() {
    let mode = Some("plan".to_string());
    let options = map(&[("effort", "high"), ("model", "opus"), ("fast", "on")]);
    // Nothing known of the agent: every option is a row.
    let (text, picked, rows) = settings_texts(&mode, &options, &[]);
    assert!(picked.is_empty());
    assert_eq!(
        settings_of(&text, &picked, &rows),
        (mode.clone(), options.clone())
    );
    // The agent offers model and effort: those are picks, fast a row.
    let offered = ["model".to_string(), "effort".to_string()];
    let (text, picked, rows) = settings_texts(&mode, &options, &offered);
    assert_eq!(picked, map(&[("effort", "high"), ("model", "opus")]));
    assert_eq!(rows, vec![row("fast", "on")]);
    assert_eq!(settings_of(&text, &picked, &rows), (mode, options));
}

#[test]
fn blank_option_rows_are_dropped() {
    let rows = [row(" model ", " opus "), row("  ", ""), row("effort", "")];
    let (mode, options) = settings_of("  ", &map(&[("fast", "on")]), &rows);
    assert_eq!(mode, None);
    // A row with one side blank stays, for validation to name it.
    assert_eq!(
        options,
        map(&[("effort", ""), ("fast", "on"), ("model", "opus")])
    );
}

#[test]
fn clearing_a_pick_leaves_it_to_the_run() {
    let mut picked = map(&[("model", "opus")]);
    set_pick(&mut picked, "effort", Some("high"));
    set_pick(&mut picked, "model", Some("haiku"));
    assert_eq!(picked, map(&[("effort", "high"), ("model", "haiku")]));
    set_pick(&mut picked, "model", None);
    assert_eq!(settings_of("", &picked, &[]).1, map(&[("effort", "high")]));
}

#[test]
fn an_option_set_twice_is_found() {
    let none = std::collections::BTreeMap::new();
    let rows = [
        row("model", "opus"),
        row(" model ", "haiku"),
        row("effort", "high"),
        row("model", "sonnet"),
        row("", "x"),
        row(" ", "y"),
    ];
    assert_eq!(options_set_twice(&none, &rows), vec!["model".to_string()]);
    assert!(options_set_twice(&none, &[row("model", "a"), row("fast", "on")]).is_empty());
    // A pick and a row on one id is set twice too.
    let picked = map(&[("model", "opus"), ("effort", "high")]);
    assert_eq!(
        options_set_twice(&picked, &[row(" model ", "haiku"), row("fast", "on")]),
        vec!["model".to_string()]
    );
}

#[test]
fn typed_rows_the_agent_offers_become_picks() {
    let offer = onehand_core::workflow::Offer {
        modes: Vec::new(),
        options: vec![
            (
                "model".to_string(),
                vec!["strong".to_string(), "cheap".to_string()],
            ),
            ("effort".to_string(), vec!["low".to_string()]),
            ("fast".to_string(), vec!["on".to_string()]),
        ],
    };
    let rows = [
        row(" model ", " cheap "), // offered, a value it offers: adopted
        row("effort", "high"),     // a value not offered: stays typed
        row("other", "x"),         // an id not offered: stays typed
        row("fast", "on"),         // already picked: stays, set twice
        row("", ""),
    ];
    assert_eq!(adoptable(&rows, &map(&[("fast", "on")]), &offer), vec![0]);
    // Two rows on one id are left for the person to settle.
    let twice = [row("model", "cheap"), row("model", "strong")];
    assert!(adoptable(&twice, &map(&[]), &offer).is_empty());
}
