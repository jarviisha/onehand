use super::agents::runs_line;
use super::{DraftShift, draft_after_promote, draft_shift, draft_spec};
use onehand_core::config::{AgentSpec, Preset};

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

/// A built-in agent that is its own binary says what runs, as an adapter's
/// line names its package.
#[test]
fn a_native_built_in_agent_says_what_runs() {
    let mut spec = AgentSpec::of(Preset::Copilot, &[]);
    assert_eq!(runs_line(&spec), "Built in · copilot --acp");
    spec.env.insert("COPILOT_HOME".into(), "/x".into());
    assert_eq!(
        runs_line(&spec),
        "Built in · copilot --acp · COPILOT_HOME=/x"
    );
}

mod hosts {
    use super::super::hosts::host_lines;
    use onehand_core::config::drive::HostProbe;
    use onehand_core::config::{HostOs, HostSpec};
    use onehand_core::host::drive::HostState;
    use onehand_core::host::probe::{Probed, Tool};
    use onehand_core::workspace::HostName;

    fn linux() -> HostSpec {
        HostSpec::undeclared(HostName("build01".into()))
    }

    fn windows() -> HostSpec {
        HostSpec {
            name: HostName("win".into()),
            os: HostOs::Windows,
            arch: "AMD64".into(),
        }
    }

    fn found(tools: &[(&str, &str)]) -> HostProbe {
        HostProbe::Found(Probed {
            os: HostOs::Linux,
            arch: "x86_64".into(),
            tools: tools
                .iter()
                .map(|(name, said)| Tool {
                    name: name.to_string(),
                    said: said.to_string(),
                })
                .collect(),
        })
    }

    #[test]
    fn a_row_says_the_system_alone_before_any_probe() {
        let lines = host_lines(&linux(), None, &HostState::Unknown, false, &[]);
        assert_eq!(lines.about, "Linux x86_64");
        assert_eq!((lines.note, lines.warning, lines.users), (None, None, None));
    }

    #[test]
    fn a_row_lists_what_the_probe_found_with_each_toolchain_version() {
        let probe = found(&[
            ("git", "git version 2.56.0"),
            ("tar", "tar (GNU tar) 1.35"),
            ("cargo", "cargo 1.99.1 (5f94df478 2026-08-27)"),
            ("dotnet", "10.0.113"),
        ]);
        let lines = host_lines(&linux(), Some(&probe), &HostState::Live, false, &[]);
        assert_eq!(
            lines.about,
            "Linux x86_64 · git, tar, cargo 1.99.1, dotnet 10.0.113"
        );
        assert_eq!(lines.warning, None);
    }

    #[test]
    fn a_running_probe_says_so() {
        let lines = host_lines(
            &linux(),
            Some(&HostProbe::Running),
            &HostState::Unknown,
            false,
            &[],
        );
        assert_eq!(lines.about, "Linux x86_64 · Checking…");
    }

    #[test]
    fn a_failed_probe_says_why() {
        let probe = HostProbe::Failed("ssh: connect to host build01: timed out".into());
        let lines = host_lines(&linux(), Some(&probe), &HostState::Unknown, false, &[]);
        assert_eq!(
            lines.warning.as_deref(),
            Some("ssh: connect to host build01: timed out")
        );
    }

    #[test]
    fn what_a_build_needs_and_is_missing_is_named() {
        let lines = host_lines(
            &linux(),
            Some(&found(&[("tar", "tar 1.35")])),
            &HostState::Unknown,
            false,
            &[],
        );
        assert_eq!(lines.warning.as_deref(), Some("git is not installed"));
        let lines = host_lines(&linux(), Some(&found(&[])), &HostState::Unknown, false, &[]);
        assert_eq!(
            lines.warning.as_deref(),
            Some("git and tar are not installed")
        );
    }

    #[test]
    fn a_windows_host_says_it_builds_only() {
        let lines = host_lines(&windows(), None, &HostState::Unknown, false, &[]);
        assert_eq!(lines.about, "Windows AMD64");
        assert_eq!(lines.note.as_deref(), Some("Builds only"));
    }

    #[test]
    fn a_system_this_build_does_not_know_builds_nothing_yet() {
        let odd = HostSpec {
            name: HostName("odd".into()),
            os: HostOs::Other("plan9".into()),
            arch: "x86_64".into(),
        };
        let lines = host_lines(&odd, None, &HostState::Unknown, false, &[]);
        assert_eq!(
            lines.note.as_deref(),
            Some("Builds only; nothing builds on it yet")
        );
    }

    #[test]
    fn a_linux_host_that_runs_no_sessions_builds_and_says_only_that() {
        let arm = HostSpec {
            name: HostName("arm".into()),
            os: HostOs::Linux,
            arch: "aarch64".into(),
        };
        let lines = host_lines(&arm, None, &HostState::Unknown, false, &[]);
        assert_eq!(lines.note.as_deref(), Some("Builds only"));
    }

    #[test]
    fn a_lost_or_taken_session_host_says_so() {
        let lost = HostState::Lost("timed out".into());
        let lines = host_lines(&linux(), None, &lost, true, &[]);
        assert_eq!(
            lines.warning.as_deref(),
            Some("Not connected to build01; trying again")
        );
        let lines = host_lines(&linux(), None, &HostState::Taken, false, &[]);
        assert_eq!(
            lines.warning.as_deref(),
            Some("build01 was attached from another machine")
        );
    }

    #[test]
    fn a_row_names_the_projects_that_use_it() {
        let users = ["api".to_string(), "web".to_string()];
        let lines = host_lines(&linux(), None, &HostState::Unknown, false, &users);
        assert_eq!(lines.users.as_deref(), Some("Used by api, web"));
    }
}
