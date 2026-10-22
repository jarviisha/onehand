use super::{DraftShift, check_key, draft_after_promote, draft_shift, draft_spec};
use onehand_core::config::{AgentSpec, Preset};

/// Two agents on one launcher are two agents: a result filed for one must
/// not be shown under the other.
#[test]
fn a_test_result_belongs_to_the_whole_command_line() {
    let spec = |args: &[&str]| AgentSpec {
        name: "a".into(),
        command: "npx".into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        ..Default::default()
    };
    assert_ne!(
        check_key(&spec(&["-y", "one"])),
        check_key(&spec(&["-y", "two"]))
    );
    assert_eq!(
        check_key(&spec(&["-y", "one"])),
        check_key(&spec(&["-y", "one"]))
    );
}

/// Making an agent the default moves a form open on any agent ahead of it.
#[test]
fn making_an_agent_default_keeps_the_form_on_its_agent() {
    assert_eq!(draft_after_promote(None, 2), None);
    // The agent being edited is the one moved to the front.
    assert_eq!(draft_after_promote(Some(2), 2), Some(0));
    // Ahead of it: pushed down by one.
    assert_eq!(draft_after_promote(Some(0), 2), Some(1));
    assert_eq!(draft_after_promote(Some(1), 2), Some(2));
    // Behind it: nothing moved.
    assert_eq!(draft_after_promote(Some(3), 2), Some(3));
}

/// Deleting an agent moves a form open on another one with it.
///
/// Every case here is a save that would otherwise land on the wrong agent:
/// the list shifts under a position the form is still holding.
#[test]
fn deleting_an_agent_moves_the_form_off_the_hole() {
    assert_eq!(draft_shift(None, 0), DraftShift::Keep);
    // Deleted below the form: nothing the form points at has moved.
    assert_eq!(draft_shift(Some(0), 2), DraftShift::Keep);
    // Deleted above it: everything after the hole slid down one.
    assert_eq!(draft_shift(Some(2), 0), DraftShift::MoveTo(1));
    assert_eq!(draft_shift(Some(1), 0), DraftShift::MoveTo(0));
    // The agent being edited is the one that went.
    assert_eq!(draft_shift(Some(1), 1), DraftShift::Clear);
}

/// A built-in agent is its preset and a name: no command to give.
#[test]
fn a_preset_draft_needs_only_a_name() {
    let spec = draft_spec(Some(Preset::Codex), "Codex", "", "", "", None, &[]).unwrap();
    assert_eq!(spec.preset(), Some(Preset::Codex));
    assert!(spec.command.is_empty() && spec.args.is_empty());
    assert_eq!(
        draft_spec(Some(Preset::Codex), " ", "", "", "", None, &[]).unwrap_err(),
        "Name it first"
    );
    assert_eq!(
        draft_spec(None, "Gemini", "", "", "", None, &[]).unwrap_err(),
        "Name it and give its command first"
    );
}

/// The environment is one line of `NAME=value` words, each kept whole.
#[test]
fn an_env_word_without_equals_refuses_save() {
    assert_eq!(
        draft_spec(
            Some(Preset::ClaudeCode),
            "Claude Code (acc2)",
            "",
            "",
            "CLAUDE_CONFIG_DIR",
            None,
            &[]
        )
        .unwrap_err(),
        "Write each variable as NAME=value"
    );
    let spec = draft_spec(
        Some(Preset::ClaudeCode),
        "Claude Code (acc2)",
        "",
        "",
        "CLAUDE_CONFIG_DIR='/x y' A=b=c",
        None,
        &[],
    )
    .unwrap();
    assert_eq!(
        spec.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some("/x y")
    );
    assert_eq!(spec.env.get("A").map(String::as_str), Some("b=c"));
}

/// The form has no field for an adapter version, so saving keeps the one the
/// entry had.
#[test]
fn editing_a_preset_keeps_its_version() {
    let spec = draft_spec(
        Some(Preset::Codex),
        "Codex",
        "",
        "",
        "",
        Some("latest".into()),
        &[],
    )
    .unwrap();
    assert_eq!(spec.version.as_deref(), Some("latest"));
}

/// A declared agent keeps its command and arguments, and gains the same
/// environment line.
#[test]
fn a_declared_draft_takes_its_command_args_and_env() {
    let spec = draft_spec(
        None,
        "Gemini",
        "gemini",
        "--experimental-acp",
        "A=1",
        None,
        &[],
    )
    .unwrap();
    assert_eq!(spec.preset, None);
    assert_eq!(spec.command, "gemini");
    assert_eq!(spec.args, ["--experimental-acp"]);
    assert_eq!(spec.env.get("A").map(String::as_str), Some("1"));
}

/// A name finds one agent, so the form refuses one another entry has.
#[test]
fn a_name_another_agent_has_is_refused() {
    assert_eq!(
        draft_spec(Some(Preset::Codex), " Codex ", "", "", "", None, &["Codex"]).unwrap_err(),
        "Another agent has that name"
    );
    assert!(
        draft_spec(
            Some(Preset::Codex),
            "Codex (2)",
            "",
            "",
            "",
            None,
            &["Codex"]
        )
        .is_ok()
    );
}
