use super::*;
use crate::acp::{ConfigChoice, ConfigOption};

fn choice(value: &str) -> ConfigChoice {
    ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
    }
}

fn group(id: &str, name: &str, values: &[&str], current: Option<&str>) -> ConfigOption {
    ConfigOption {
        id: id.into(),
        name: name.into(),
        category: None,
        current: current.map(str::to_string),
        choices: values.iter().copied().map(choice).collect(),
    }
}

fn of(id: &str, name: &str, category: Option<&str>) -> ConfigOption {
    ConfigOption {
        category: category.map(str::to_string),
        ..group(id, name, &[], None)
    }
}

#[test]
fn a_group_is_found_by_id_or_by_label() {
    assert_eq!(
        group("effort", "Reasoning", &[], None).role(),
        Some(Role::Effort)
    );
    assert_eq!(group("x-9", "Effort", &[], None).role(), Some(Role::Effort));
    assert_eq!(group("x-9", "EFFORT", &[], None).role(), Some(Role::Effort));
    assert_eq!(group("model", "Model", &[], None).role(), Some(Role::Model));
    assert_eq!(group("agent", "Sub-agent", &[], None).role(), None);
}

#[test]
fn codex_names_its_effort_and_fast_groups_differently() {
    assert_eq!(
        group("reasoning_effort", "Reasoning effort", &[], None).role(),
        Some(Role::Effort)
    );
    assert_eq!(
        group("fast-mode", "Fast mode", &[], None).role(),
        Some(Role::Fast)
    );
    assert_eq!(group("fast", "Fast", &[], None).role(), Some(Role::Fast));
}

#[test]
fn the_category_wins_over_the_name() {
    assert_eq!(
        of("effort", "Effort", Some("model")).role(),
        Some(Role::Model)
    );
    assert_eq!(
        of("x", "Thinking", Some("thought_level")).role(),
        Some(Role::Effort)
    );
}

#[test]
fn an_unknown_category_falls_back_to_the_names() {
    assert_eq!(
        of("fast-mode", "Fast", Some("_vendor_x")).role(),
        Some(Role::Fast)
    );
    assert_eq!(of("agent", "Agent", Some("mode")).role(), None);
}

#[test]
fn fast_is_a_switch_only_when_its_two_values_say_which_is_on() {
    let on = group("fast", "Fast", &["on", "off"], Some("on"))
        .switch()
        .expect("a switch");
    assert!(on.on);
    assert_eq!(on.flip_to, "off");
    let off = group("fast", "Fast", &["normal", "fast"], Some("normal"))
        .switch()
        .expect("one side named is enough");
    assert!(!off.on);
    assert_eq!(off.flip_to, "fast");
    assert!(
        group("fast", "Fast", &["on", "off"], None)
            .switch()
            .is_some_and(|s| !s.on),
        "nothing in force reads as off, and a press turns it on"
    );
    assert!(
        group("fast", "Fast", &["turbo", "eco"], Some("eco"))
            .switch()
            .is_none(),
        "two words neither of which says on or off stay a picker"
    );
    assert!(
        group("fast", "Fast", &["on", "off", "auto"], Some("on"))
            .switch()
            .is_none(),
        "a third value is no switch"
    );
    assert!(
        group("fast", "Fast", &["on", "yes"], Some("on"))
            .switch()
            .is_none(),
        "two values both saying on is no switch"
    );
}

#[test]
fn a_fast_picker_is_lit_unless_its_value_reads_as_off() {
    let at = |current| {
        let option = group("fast", "Fast", &["turbo", "normal", "eco"], current);
        option.in_force().is_some_and(|choice| !choice.reads_off())
    };
    assert!(at(Some("turbo")), "a value with no off word is fast");
    assert!(at(Some("eco")), "nothing says eco is the off side");
    assert!(!at(Some("normal")), "an off word is dark");
    assert!(!at(None), "nothing in force is dark");
}

#[test]
fn a_value_the_agent_never_offered_lights_nothing() {
    let stale = group("effort", "Effort", &["low", "high"], Some("medium"));
    assert!(
        stale.in_force().is_none(),
        "guessing at the nearest choice would report a setting nobody chose"
    );
}
