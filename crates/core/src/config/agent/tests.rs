use super::*;
use crate::config::AppConfig;

/// A fresh directory under the system temp dir, unique to `test`, holding an
/// empty file named `binary` when one is given. Core has no tempfile crate.
pub(super) fn bin_dir(test: &str, binary: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(binary) = binary {
        std::fs::write(dir.join(binary), "").unwrap();
    }
    dir
}

#[test]
fn a_preset_and_a_declared_agent_round_trip() {
    let text = r#"
[[agents]]
name = "Claude Code (acc2)"
preset = "claude-code"
env = { CLAUDE_CONFIG_DIR = "/home/me/.claude-acc2" }

[[agents]]
name = "Gemini"
command = "gemini"
args = ["--experimental-acp"]
"#;
    let cfg = AppConfig::parse(text).unwrap();
    assert_eq!(cfg.agents[0].preset, Some(Preset::ClaudeCode));
    assert!(cfg.agents[0].command.is_empty());
    assert_eq!(cfg.agents[1].preset, None);
    let written = cfg.to_toml().unwrap();
    let again = AppConfig::parse(&written).unwrap();
    assert_eq!(again.agents, cfg.agents);
    assert!(!written.contains("command = \"\""), "{written}");
    assert!(!written.contains("args = []"), "{written}");
}

#[test]
fn a_preset_launches_the_pinned_adapter_on_the_machines_binary() {
    let dir = bin_dir("launch-codex", Some("codex"));
    let spec = AgentSpec {
        name: "Codex".into(),
        preset: Some(Preset::Codex),
        ..Default::default()
    };
    let launch = spec.launch(Some(dir.as_os_str())).unwrap();
    assert_eq!(launch.command, "npx");
    assert_eq!(
        launch.args,
        [
            "--prefer-offline",
            "-y",
            "@agentclientprotocol/codex-acp@2.2.2"
        ]
    );
    assert!(launch
        .env
        .contains(&("npm_config_omit".into(), "optional".into())));
    assert!(launch
        .env
        .contains(&("CODEX_PATH".into(), dir.join("codex").display().to_string())));
}

#[test]
fn an_entrys_env_and_version_win() {
    let dir = bin_dir("launch-env", Some("claude"));
    let spec = AgentSpec {
        name: "Claude Code (acc2)".into(),
        preset: Some(Preset::ClaudeCode),
        version: Some("latest".into()),
        env: [
            ("CLAUDE_CONFIG_DIR".into(), "/x".into()),
            ("npm_config_omit".into(), "".into()),
        ]
        .into(),
        ..Default::default()
    };
    let launch = spec.launch(Some(dir.as_os_str())).unwrap();
    assert_eq!(
        launch.args[2],
        "@agentclientprotocol/claude-agent-acp@latest"
    );
    // Applied after the preset's own, so the entry's value is the last one set.
    assert_eq!(
        launch
            .env
            .iter()
            .rev()
            .find(|(k, _)| k == "npm_config_omit")
            .unwrap()
            .1,
        ""
    );
    assert!(launch
        .env
        .contains(&("CLAUDE_CONFIG_DIR".into(), "/x".into())));
    assert!(spec.outside_pin());
}

#[test]
fn a_missing_binary_is_said_and_nothing_is_launched() {
    let empty = bin_dir("launch-missing", None);
    let spec = AgentSpec {
        name: "Codex".into(),
        preset: Some(Preset::Codex),
        ..Default::default()
    };
    assert_eq!(
        spec.launch(Some(empty.as_os_str())).unwrap_err(),
        "`codex` is not on this machine — install it again, or remove Codex in Settings ▸ Agents"
    );
}

#[test]
fn a_declared_agent_launches_as_written() {
    let spec = AgentSpec {
        name: "Gemini".into(),
        command: "gemini".into(),
        args: vec!["--experimental-acp".into()],
        env: [("A".into(), "1".into())].into(),
        ..Default::default()
    };
    let launch = spec.launch(None).unwrap();
    assert_eq!(launch.command, "gemini");
    assert_eq!(launch.args, ["--experimental-acp"]);
    assert_eq!(launch.env, [("A".to_string(), "1".to_string())]);
    assert!(!spec.outside_pin());
}

use crate::config::{default_adapter_args, default_agents, run_mode};

#[test]
fn the_old_default_becomes_the_preset_where_claude_is_installed() {
    let dir = bin_dir("settle-migrates", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.agents[0].preset, Some(Preset::ClaudeCode));
    assert_eq!(cfg.agents[0].name, "Claude Code");
    assert!(cfg.agents[0].command.is_empty() && cfg.agents[0].args.is_empty());
}

#[test]
fn the_old_default_stays_declared_without_claude() {
    let empty = bin_dir("settle-no-claude", None);
    let mut cfg = AppConfig::default();
    cfg.settle(Some(empty.as_os_str()));
    assert_eq!(cfg.agents, default_agents());
}

#[test]
fn the_env_form_keeps_values_whole() {
    let dir = bin_dir("settle-env-form", Some("claude"));
    let mut cfg = AppConfig::default();
    let mut args = vec![
        "CLAUDE_CONFIG_DIR=/home/me/.claude acc2".to_string(),
        "X=a=b".into(),
        "npx".into(),
    ];
    args.extend(default_adapter_args());
    cfg.agents = vec![AgentSpec {
        name: "Claude Code (acc2)".into(),
        command: "env".into(),
        args,
        ..Default::default()
    }];
    cfg.settle(Some(dir.as_os_str()));
    let spec = &cfg.agents[0];
    assert_eq!(spec.preset, Some(Preset::ClaudeCode));
    assert_eq!(spec.name, "Claude Code (acc2)");
    assert!(spec.command.is_empty() && spec.args.is_empty());
    assert_eq!(
        spec.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some("/home/me/.claude acc2")
    );
    assert_eq!(spec.env.get("X").map(String::as_str), Some("a=b"));
}

#[test]
fn an_edited_entry_is_left_as_written() {
    let dir = bin_dir("settle-edited", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.agents[0].args.push("--verbose".into());
    let before = cfg.agents.clone();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.agents, before);
}

#[test]
fn duplicate_names_are_numbered_not_dropped() {
    let mut cfg = AppConfig::default();
    let one = |name: &str| AgentSpec {
        name: name.into(),
        command: "x".into(),
        ..Default::default()
    };
    cfg.agents = vec![one("A"), one("A"), one("A (2)"), one("A")];
    cfg.settle(None);
    let names: Vec<_> = cfg.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["A", "A (3)", "A (2)", "A (4)"]);
}

#[test]
fn a_nameless_preset_takes_its_shown_name() {
    let mut cfg = AppConfig {
        agents: vec![AgentSpec {
            preset: Some(Preset::Codex),
            ..Default::default()
        }],
        ..Default::default()
    };
    cfg.settle(None);
    assert_eq!(cfg.agents[0].name, "Codex");
}

#[test]
fn the_stored_mode_is_cleared_for_a_preset_and_kept_for_a_declared_agent() {
    let dir = bin_dir("settle-mode-preset", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = Some("acceptEdits".into());
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg.unattended.mode, None);

    let empty = bin_dir("settle-mode-declared", None);
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = Some("acceptEdits".into());
    cfg.settle(Some(empty.as_os_str()));
    assert_eq!(cfg.unattended.mode.as_deref(), Some("acceptEdits"));
}

#[test]
fn a_run_mode_comes_from_the_preset_when_none_is_set() {
    let claude = AgentSpec {
        name: "Claude Code".into(),
        preset: Some(Preset::ClaudeCode),
        ..Default::default()
    };
    let codex = AgentSpec {
        name: "Codex".into(),
        preset: Some(Preset::Codex),
        ..Default::default()
    };
    let both = [claude.clone(), codex.clone()];
    assert_eq!(run_mode(None, &both, None), "acceptEdits");
    assert_eq!(run_mode(None, &both, Some("Codex")), "");
    assert_eq!(run_mode(Some(""), &both, None), "");
    assert_eq!(run_mode(Some("plan"), &both, Some("Codex")), "plan");
    assert_eq!(run_mode(None, &[], None), "");
}

/// The agent runs use is read when a run starts: making another agent the
/// default changes the mode with it.
#[test]
fn a_run_mode_follows_the_agent_first_now() {
    let claude = AgentSpec {
        name: "Claude Code".into(),
        preset: Some(Preset::ClaudeCode),
        ..Default::default()
    };
    let codex = AgentSpec {
        name: "Codex".into(),
        preset: Some(Preset::Codex),
        ..Default::default()
    };
    assert_eq!(run_mode(None, &[codex.clone(), claude.clone()], None), "");
    assert_eq!(run_mode(None, &[claude, codex], None), "acceptEdits");
}

/// The old Claude default left declared, for want of `claude` on `PATH`, is
/// still Claude, and still needs the mode it always ran in.
#[test]
fn the_old_default_left_declared_runs_in_accept_edits() {
    assert_eq!(run_mode(None, &default_agents(), None), "acceptEdits");
}

#[test]
fn settling_twice_changes_nothing_more() {
    let dir = bin_dir("settle-twice", Some("claude"));
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = Some("acceptEdits".into());
    cfg.settle(Some(dir.as_os_str()));
    let once = cfg.clone();
    cfg.settle(Some(dir.as_os_str()));
    assert_eq!(cfg, once);
}

#[test]
fn an_absent_mode_is_not_written_and_an_empty_one_is() {
    let mut cfg = AppConfig::default();
    cfg.unattended.mode = None;
    assert!(!cfg.to_toml().unwrap().contains("mode ="));
    cfg.unattended.mode = Some(String::new());
    let again = AppConfig::parse(&cfg.to_toml().unwrap()).unwrap();
    assert_eq!(again.unattended.mode.as_deref(), Some(""));
}

/// No shell stands between onehand and the adapter, so a value typed as
/// `~/…` would reach the agent as a directory literally named `~`.
#[test]
fn a_leading_tilde_in_an_env_value_is_the_home_directory() {
    let home = dirs::home_dir().expect("a home directory");
    let spec = AgentSpec {
        name: "Gemini".into(),
        command: "gemini".into(),
        env: [
            ("CLAUDE_CONFIG_DIR".into(), "~/.claude-work".into()),
            ("PLAIN".into(), "a~/b".into()),
        ]
        .into(),
        ..Default::default()
    };
    let env = spec.launch(None).unwrap().env;
    assert!(env.contains(&(
        "CLAUDE_CONFIG_DIR".into(),
        home.join(".claude-work").display().to_string()
    )));
    assert!(env.contains(&("PLAIN".into(), "a~/b".into())));
}

#[test]
fn no_file_finds_the_installed_agents_claude_first() {
    let dir = bin_dir("first-both", Some("codex"));
    std::fs::write(dir.join("claude"), "").unwrap();
    let cfg = AppConfig::from_disk(None, Some(dir.as_os_str())).unwrap();
    let presets: Vec<_> = cfg.agents.iter().map(|a| a.preset).collect();
    assert_eq!(presets, [Some(Preset::ClaudeCode), Some(Preset::Codex)]);
    assert_eq!(cfg.agents[1].name, "Codex");
}

#[test]
fn no_file_and_no_agent_installed_is_an_empty_list() {
    let empty = bin_dir("first-none", None);
    assert!(AppConfig::from_disk(None, Some(empty.as_os_str()))
        .unwrap()
        .agents
        .is_empty());
}

#[test]
fn a_file_without_agents_gets_the_found_ones_and_keeps_the_rest() {
    let dir = bin_dir("first-partial", Some("codex"));
    let cfg = AppConfig::from_disk(
        Some("[font]\nmonospace = \"Test Mono\"\n"),
        Some(dir.as_os_str()),
    )
    .unwrap();
    assert_eq!(cfg.agents.len(), 1);
    assert_eq!(cfg.agents[0].preset, Some(Preset::Codex));
    assert_eq!(cfg.font.monospace.as_deref(), Some("Test Mono"));
}

#[test]
fn an_empty_list_the_person_wrote_is_kept() {
    let dir = bin_dir("first-kept-empty", Some("claude"));
    let cfg = AppConfig::from_disk(Some("agents = []\n"), Some(dir.as_os_str())).unwrap();
    assert!(cfg.agents.is_empty());
}

#[test]
fn an_added_preset_takes_a_free_name() {
    let one = |name: &str| AgentSpec {
        name: name.into(),
        command: "x".into(),
        ..Default::default()
    };
    let added = AgentSpec::of(Preset::Codex, &[one("Codex"), one("Codex (2)")]);
    assert_eq!(
        (added.name.as_str(), added.preset),
        ("Codex (3)", Some(Preset::Codex))
    );
    assert_eq!(AgentSpec::of(Preset::Codex, &[]).name, "Codex");
}

#[test]
fn what_is_on_the_machine_is_the_presets_whose_binary_is_found() {
    let dir = bin_dir("on-machine", Some("codex"));
    assert_eq!(on_machine(Some(dir.as_os_str())), [Preset::Codex]);
}

#[test]
fn the_login_hint_names_the_entrys_account() {
    let codex = AgentSpec {
        name: "Codex".into(),
        preset: Some(Preset::Codex),
        ..Default::default()
    };
    assert_eq!(
        codex.login_hint(),
        "Codex is not logged in. Run `codex login` in a terminal, then restart the agent"
    );
    let acc2 = AgentSpec {
        name: "Claude Code (acc2)".into(),
        preset: Some(Preset::ClaudeCode),
        env: [
            ("CLAUDE_CONFIG_DIR".into(), "/x/.claude-acc2".into()),
            ("OTHER".into(), "1".into()),
        ]
        .into(),
        ..Default::default()
    };
    assert_eq!(
        acc2.login_hint(),
        "Claude Code (acc2) is not logged in. Run `CLAUDE_CONFIG_DIR=/x/.claude-acc2 claude auth login` in a terminal, then restart the agent"
    );
    let other_var = AgentSpec {
        env: [("OTHER".into(), "1".into())].into(),
        ..codex.clone()
    };
    assert_eq!(other_var.login_hint(), codex.login_hint());
    let gemini = AgentSpec {
        name: "Gemini".into(),
        command: "gemini".into(),
        ..Default::default()
    };
    assert_eq!(gemini.login_hint(), "Gemini needs you to log in");
}

/// The old Claude default is left declared only where `claude` is missing,
/// so a command that needs it is no help there.
#[test]
fn the_old_default_left_declared_is_asked_to_log_in_in_plain_words() {
    let spec = default_agents().remove(0);
    assert_eq!(spec.login_hint(), "Claude Code needs you to log in");
}
