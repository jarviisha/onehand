use super::*;
use crate::acp::ConfigChoice;

fn facts() -> AgentFacts {
    AgentFacts {
        modes: vec![Mode {
            id: "plan".into(),
            name: "Plan".into(),
        }],
        current_mode: Some("plan".into()),
        options: vec![ConfigOption {
            id: "model".into(),
            name: "Model".into(),
            category: Some("model".into()),
            current: Some("opus".into()),
            choices: vec![ConfigChoice {
                value: "opus".into(),
                name: "Opus 5.5".into(),
                description: None,
            }],
        }],
        commands: vec![SlashCommand {
            name: "review".into(),
            description: "Review the diff".into(),
        }],
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-facts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("agents-seen.toml")
}

#[test]
fn facts_round_trip_through_the_file() {
    let path = scratch("round");
    let mut file = FactsFile::default();
    file.agents.insert("Claude Code".into(), facts());
    file.save_to(&path).unwrap();
    assert_eq!(FactsFile::load_from(&path), file);
}

#[test]
fn a_bad_facts_file_is_kept_aside() {
    let path = scratch("bad");
    std::fs::write(&path, "agents = 3 [").unwrap();
    assert_eq!(FactsFile::load_from(&path), FactsFile::default());
    assert!(path.with_extension("toml.bad").exists());
}
