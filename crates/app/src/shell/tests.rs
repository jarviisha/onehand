use super::*;

#[test]
fn session_cycle_ends_when_the_remapped_modifier_is_released() {
    let modifiers = gpui::Keystroke::parse("alt-tab").unwrap().modifiers;
    let cycle = TabCycle {
        order: vec![1, 2, 3],
        pos: 0,
        modifiers,
    };
    assert!(cycle.held(modifiers));
    assert!(cycle.held(gpui::Keystroke::parse("alt-shift-tab").unwrap().modifiers));
    assert!(!cycle.held(gpui::Modifiers::default()));
    assert!(!cycle.held(gpui::Keystroke::parse("ctrl-tab").unwrap().modifiers));
    let direct = TabCycle {
        modifiers: gpui::Modifiers::default(),
        ..cycle
    };
    assert!(!direct.held(gpui::Modifiers::default()));
}
