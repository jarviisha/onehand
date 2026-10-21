use super::*;
use crate::workflow::GateKind;

fn agent_step(mode: Option<&str>, options: &[(&str, &str)]) -> StepKind {
    StepKind::Agent {
        prompt: "do it".to_string(),
        gates: Vec::<GateKind>::new(),
        keep_answer: false,
        mode: mode.map(str::to_string),
        options: options
            .iter()
            .map(|(id, value)| (id.to_string(), value.to_string()))
            .collect(),
    }
}

fn settings(mode: Option<&str>, options: &[(&str, &str)]) -> Settings {
    Settings {
        mode: mode.map(str::to_string),
        options: options
            .iter()
            .map(|(id, value)| (id.to_string(), value.to_string()))
            .collect(),
    }
}

fn offer(modes: &[&str], options: &[(&str, &[&str])]) -> Offer {
    Offer {
        modes: modes.iter().map(|m| m.to_string()).collect(),
        options: options
            .iter()
            .map(|(id, values)| {
                (
                    id.to_string(),
                    values.iter().map(|v| v.to_string()).collect(),
                )
            })
            .collect(),
    }
}

fn group(s: &str) -> String {
    s.to_string()
}

#[test]
fn for_step_takes_the_steps_own_over_the_start() {
    let start = settings(Some("code"), &[("model", "cheap")]);
    let step = agent_step(Some("plan"), &[]);
    assert_eq!(
        Settings::for_step(&step, &start),
        settings(Some("plan"), &[("model", "cheap")])
    );
}

#[test]
fn a_step_naming_nothing_goes_back_to_the_start() {
    let start = settings(Some("code"), &[("model", "cheap")]);
    let step = agent_step(None, &[]);
    assert_eq!(Settings::for_step(&step, &start), start);
}

#[test]
fn the_mode_goes_first_then_the_model_then_the_rest() {
    let want = settings(Some("plan"), &[("model", "strong"), ("effort", "high")]);
    let offer = offer(
        &["plan", "code"],
        &[
            ("model", &["strong", "cheap"]),
            ("effort", &["high", "low"]),
        ],
    );
    let in_force = Settings::default();
    let mut picked = Vec::new();

    let step = next(&want, &offer, &in_force, &picked);
    assert_eq!(step, Next::Pick(Pick::Mode("plan".to_string())));
    picked.push(group(crate::chat::MODE_SELECTOR));

    let step = next(&want, &offer, &in_force, &picked);
    assert_eq!(
        step,
        Next::Pick(Pick::Option {
            id: "model".to_string(),
            value: "strong".to_string(),
        })
    );
    picked.push(group("model"));

    let step = next(&want, &offer, &in_force, &picked);
    assert_eq!(
        step,
        Next::Pick(Pick::Option {
            id: "effort".to_string(),
            value: "high".to_string(),
        })
    );
}

#[test]
fn a_value_in_force_is_not_picked() {
    let want = settings(Some("plan"), &[("effort", "high")]);
    let offer = offer(&["plan"], &[("effort", &["high", "low"])]);
    let in_force = settings(Some("plan"), &[("effort", "high")]);

    assert_eq!(next(&want, &offer, &in_force, &[]), Next::Done);
}

#[test]
fn a_value_not_offered_is_refused() {
    let want = settings(None, &[("effort", "max")]);
    let offer = offer(&[], &[("effort", &["high", "low"])]);
    let in_force = Settings::default();

    assert_eq!(
        next(&want, &offer, &in_force, &[]),
        Next::Refused("the agent offers no `max` for `effort` (it offers: high, low)".to_string())
    );
}

#[test]
fn an_option_not_offered_is_refused() {
    let want = settings(None, &[("thinking", "on")]);
    let offer = offer(&[], &[("model", &["strong"])]);
    let in_force = Settings::default();

    assert_eq!(
        next(&want, &offer, &in_force, &[]),
        Next::Refused("the agent offers no option `thinking` (it offers: model)".to_string())
    );
}

#[test]
fn a_group_already_picked_this_visit_is_not_picked_again() {
    let want = settings(None, &[("effort", "high")]);
    let offer = offer(&[], &[("effort", &["high", "low"])]);
    let in_force = Settings::default();
    let picked = vec![group("effort")];

    assert_eq!(next(&want, &offer, &in_force, &picked), Next::Done);
}

#[test]
fn nothing_named_and_nothing_offered_is_done() {
    let want = Settings::default();
    let offer = Offer::default();
    let in_force = Settings::default();

    assert_eq!(next(&want, &offer, &in_force, &[]), Next::Done);
}
