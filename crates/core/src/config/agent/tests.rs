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
