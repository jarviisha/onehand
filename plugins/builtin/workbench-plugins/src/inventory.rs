//! What an installed plugin carries, read from its own folder.
//!
//! **The folder, not `claude plugin details`.** That command prints the same
//! inventory, but as prose for a person — no `--json` — and parsing a
//! sentence is a rule that breaks the first time its wording moves. A plugin's
//! layout is the published plugin format: a manifest at
//! `.claude-plugin/plugin.json`, and by default `skills/<name>/SKILL.md`,
//! `commands/*.md`, `agents/*.md`, `hooks/hooks.json` and `.mcp.json`, each of
//! which the manifest can add to with paths of its own.
//!
//! Read off the UI loop and bounded: a plugin is somebody else's folder, and
//! nothing about it promises to be small.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// How many of one kind of thing are read from one plugin. Far past what any
/// plugin ships; it bounds the walk, not the list.
const KIND_CAP: usize = 500;

/// A plugin's contents, as far as its folder says.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Inventory {
    pub(crate) description: Option<String>,
    pub(crate) skills: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) agents: Vec<String>,
    pub(crate) mcp: Vec<String>,
    /// Every command a hook runs, which is what makes hooks the part of a
    /// plugin worth reading before it is trusted.
    pub(crate) hooks: Vec<Hook>,
}

/// One hook: when it runs, and what it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hook {
    pub(crate) event: String,
    pub(crate) matcher: Option<String>,
    pub(crate) command: String,
}

/// Read `root`, a plugin's install folder. A folder that is missing or holds
/// nothing readable is an empty inventory, which draws as no chips at all —
/// never as a count that was guessed.
pub(crate) fn read_blocking(root: &Path) -> Inventory {
    let manifest: Value = std::fs::read_to_string(root.join(".claude-plugin/plugin.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let paths = |key: &str, default: &str| -> Vec<PathBuf> {
        let mut paths = vec![root.join(default)];
        paths.extend(listed(&manifest[key]).map(|p| root.join(p)));
        paths.dedup();
        paths
    };

    let mut skills = Vec::new();
    for dir in paths("skills", "skills") {
        for entry in entries(&dir) {
            if entry.join("SKILL.md").is_file() {
                skills.extend(name_of(&entry));
            }
        }
    }
    let mut commands = Vec::new();
    for dir in paths("commands", "commands") {
        markdown_under(&dir, &dir, &mut commands);
    }
    let mut agents = Vec::new();
    for dir in paths("agents", "agents") {
        markdown_under(&dir, &dir, &mut agents);
    }

    // MCP servers: `.mcp.json` by default, and the manifest either names a file
    // of its own or writes the servers inline.
    let mut mcp = Vec::new();
    mcp.extend(servers(&read_json(&root.join(".mcp.json"))));
    match &manifest["mcpServers"] {
        Value::String(path) => mcp.extend(servers(&read_json(&root.join(path)))),
        inline @ Value::Object(_) => mcp.extend(servers(inline)),
        _ => {}
    }

    let mut hooks = Vec::new();
    let default_hooks = root.join("hooks/hooks.json");
    let mut hook_files = vec![default_hooks.clone()];
    match &manifest["hooks"] {
        Value::Object(_) => hooks.extend(hook_commands(&manifest["hooks"])),
        other => hook_files.extend(listed(other).map(|p| root.join(p))),
    }
    hook_files.dedup();
    for file in hook_files {
        hooks.extend(hook_commands(&read_json(&file)));
    }

    for list in [&mut skills, &mut commands, &mut agents, &mut mcp] {
        list.sort();
        list.dedup();
        list.truncate(KIND_CAP);
    }
    hooks.truncate(KIND_CAP);

    Inventory {
        description: manifest["description"].as_str().map(str::to_string),
        skills,
        commands,
        agents,
        mcp,
        hooks,
    }
}

/// A manifest path field, which may be one string or a list of them.
fn listed(value: &Value) -> impl Iterator<Item = &str> {
    let many: Vec<&str> = match value {
        Value::String(one) => vec![one.as_str()],
        Value::Array(all) => all.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    many.into_iter()
}

fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// The directories and files directly inside `dir`, in no promised order.
fn entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .map(|entry| entry.path())
                .take(KIND_CAP)
                .collect()
        })
        .unwrap_or_default()
}

fn name_of(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// Every `.md` file under `dir`, named as Claude Code names a nested one —
/// its folders joined to its name with `:` — so `git/commit.md` is
/// `git:commit`.
fn markdown_under(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for path in entries(dir) {
        if out.len() >= KIND_CAP {
            return;
        }
        if path.is_dir() {
            markdown_under(base, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let relative = path.strip_prefix(base).unwrap_or(&path).with_extension("");
            let name = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(":");
            out.push(name);
        }
    }
}

/// The server names in an MCP config — wrapped in `mcpServers` in a file of
/// its own, or bare when a manifest writes them inline.
fn servers(config: &Value) -> Vec<String> {
    let map = config.get("mcpServers").unwrap_or(config);
    map.as_object()
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default()
}

/// Every command in a hooks config: `{"hooks": {Event: [{matcher, hooks:
/// [{type: "command", command}]}]}}`, or the same map without the outer
/// `hooks` when a manifest writes it inline.
fn hook_commands(config: &Value) -> Vec<Hook> {
    let events = config.get("hooks").unwrap_or(config);
    let Some(events) = events.as_object() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group["matcher"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(str::to_string);
            for hook in group["hooks"].as_array().into_iter().flatten() {
                if let Some(command) = hook["command"].as_str() {
                    found.push(Hook {
                        event: event.clone(),
                        matcher: matcher.clone(),
                        command: command.to_string(),
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin folder built for one test and removed after it.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "onehand-plugin-inventory-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Folder(dir)
        }

        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_default_folders_are_read_and_named_as_claude_code_names_them() {
        let plugin = Folder::new("defaults");
        plugin.write(
            ".claude-plugin/plugin.json",
            r#"{"description": "Does things"}"#,
        );
        plugin.write("skills/tdd/SKILL.md", "");
        plugin.write("skills/notes/README.md", ""); // not a skill: no SKILL.md
        plugin.write("commands/review.md", "");
        plugin.write("commands/git/commit.md", "");
        plugin.write("agents/planner.md", "");
        plugin.write(".mcp.json", r#"{"mcpServers": {"docs": {}}}"#);
        plugin.write(
            "hooks/hooks.json",
            r#"{"hooks": {"PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "fmt.sh"}]}]}}"#,
        );
        let inventory = read_blocking(&plugin.0);
        assert_eq!(inventory.description.as_deref(), Some("Does things"));
        assert_eq!(inventory.skills, ["tdd"]);
        assert_eq!(inventory.commands, ["git:commit", "review"]);
        assert_eq!(inventory.agents, ["planner"]);
        assert_eq!(inventory.mcp, ["docs"]);
        assert_eq!(
            inventory.hooks,
            [Hook {
                event: "PostToolUse".into(),
                matcher: Some("Edit".into()),
                command: "fmt.sh".into(),
            }]
        );
    }

    #[test]
    fn a_manifest_adds_paths_and_writes_servers_and_hooks_of_its_own() {
        let plugin = Folder::new("manifest");
        plugin.write(
            ".claude-plugin/plugin.json",
            r#"{
              "commands": ["./extra"],
              "hooks": "./hooks/custom.json",
              "mcpServers": {"inline": {}}
            }"#,
        );
        plugin.write("extra/deploy.md", "");
        plugin.write(
            "hooks/custom.json",
            r#"{"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "node start.js"}]}]}}"#,
        );
        let inventory = read_blocking(&plugin.0);
        assert_eq!(inventory.commands, ["deploy"]);
        assert_eq!(inventory.mcp, ["inline"]);
        assert_eq!(inventory.hooks.len(), 1);
        assert_eq!(inventory.hooks[0].event, "SessionStart");
        assert_eq!(inventory.hooks[0].matcher, None);
        assert_eq!(inventory.hooks[0].command, "node start.js");
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_empty_inventory() {
        let inventory = read_blocking(Path::new("/nonexistent/onehand/plugin"));
        assert_eq!(inventory, Inventory::default());
    }
}
