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

/// How many of one kind of thing are kept from one plugin. Far past what any
/// plugin ships; it bounds the list, and the inventory says when it bit.
const KIND_CAP: usize = 500;

/// How many folder entries one plugin's walk may look at, across every kind.
/// The list cap alone does not bound the walk — a tree of folders holding no
/// markdown is visited in full while nothing is kept.
const VISIT_CAP: usize = 5_000;

/// How deep a command or agent folder is walked. Nested names are a folder or
/// two deep; past that it is not a command layout, it is somebody's tree.
const DEPTH_CAP: usize = 4;

/// A plugin's contents, as far as its folder says.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Inventory {
    pub(crate) description: Option<String>,
    /// Where its source is published — the manifest's `repository`, else its
    /// `homepage`.
    pub(crate) repository: Option<String>,
    /// Its changelog, where it ships one at the top of its folder.
    pub(crate) changelog: Option<PathBuf>,
    pub(crate) skills: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) agents: Vec<String>,
    pub(crate) mcp: Vec<String>,
    /// Every command a hook runs, which is what makes hooks the part of a
    /// plugin worth reading before it is trusted.
    pub(crate) hooks: Vec<Hook>,
    /// Whether a bound stopped the walk, so what is listed is not all there
    /// is — said on screen wherever the counts are, rather than printing a
    /// cut count as though it were the whole.
    pub(crate) cut: bool,
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
///
/// **Only the plugin's own folder is read.** A manifest path that is absolute
/// or climbs out with `..` is ignored, and a symlink is never walked into —
/// the manifest is somebody else's file, and followed, `"commands": "/"` walks
/// the whole disk and a link back to its own folder never ends.
pub(crate) fn read_blocking(root: &Path) -> Inventory {
    let manifest = read_json(&root.join(".claude-plugin/plugin.json"));
    let mut walk = Walk {
        visited: 0,
        cut: false,
    };
    let paths = |key: &str, default: &str| -> Vec<PathBuf> {
        let mut paths = vec![root.join(default)];
        paths.extend(listed(&manifest[key]).filter_map(|p| inside(root, p)));
        paths.dedup();
        paths
    };

    // A skill is a folder holding `SKILL.md`: the path itself, a folder in
    // it, or one a category further down — the three places a plugin puts
    // one, since a manifest may list each skill's own folder and a default
    // `skills/` may group them.
    // **A manifest that lists its skills replaces the default folder** rather
    // than adding to it, unlike commands and agents: Claude Code reads only
    // the listed ones, and a plugin can keep drafts in `skills/` it does not
    // ship.
    let skill_paths = if manifest["skills"].is_null() {
        vec![root.join("skills")]
    } else {
        listed(&manifest["skills"])
            .filter_map(|p| inside(root, p))
            .collect()
    };
    let mut skills = Vec::new();
    for base in skill_paths {
        if base.join("SKILL.md").is_file() {
            skills.extend(name_of(&base));
            continue;
        }
        for (child, is_dir) in walk.entries(&base) {
            if !is_dir {
                continue;
            }
            if child.join("SKILL.md").is_file() {
                skills.extend(name_of(&child));
                continue;
            }
            for (grandchild, is_dir) in walk.entries(&child) {
                if is_dir && grandchild.join("SKILL.md").is_file() {
                    skills.extend(name_of(&grandchild));
                }
            }
        }
    }
    let mut commands = Vec::new();
    for dir in paths("commands", "commands") {
        walk.markdown_under(&dir, &dir, 0, &mut commands);
    }
    let mut agents = Vec::new();
    for dir in paths("agents", "agents") {
        walk.markdown_under(&dir, &dir, 0, &mut agents);
    }

    // MCP servers: `.mcp.json` by default, and the manifest either names a file
    // of its own or writes the servers inline.
    let mut mcp = Vec::new();
    mcp.extend(servers(&read_json(&root.join(".mcp.json"))));
    match &manifest["mcpServers"] {
        Value::String(path) => {
            if let Some(file) = inside(root, path) {
                mcp.extend(servers(&read_json(&file)));
            }
        }
        inline @ Value::Object(_) => mcp.extend(servers(inline)),
        _ => {}
    }

    let mut hooks = Vec::new();
    let mut hook_files = vec![root.join("hooks/hooks.json")];
    match &manifest["hooks"] {
        Value::Object(_) => hooks.extend(hook_commands(&manifest["hooks"])),
        other => hook_files.extend(listed(other).filter_map(|p| inside(root, p))),
    }
    hook_files.dedup();
    for file in hook_files {
        hooks.extend(hook_commands(&read_json(&file)));
    }

    let mut cut = walk.cut;
    for list in [&mut skills, &mut commands, &mut agents, &mut mcp] {
        list.sort();
        list.dedup();
        cut |= list.len() > KIND_CAP;
        list.truncate(KIND_CAP);
    }
    cut |= hooks.len() > KIND_CAP;
    hooks.truncate(KIND_CAP);

    // `repository` is a string or an object carrying one under `url`.
    let repository = manifest["repository"]
        .as_str()
        .or_else(|| manifest["repository"]["url"].as_str())
        .or_else(|| manifest["homepage"].as_str())
        .map(str::to_string);
    let changelog = ["CHANGELOG.md", "changelog.md", "CHANGELOG"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file());

    Inventory {
        description: manifest["description"].as_str().map(str::to_string),
        repository,
        changelog,
        skills,
        commands,
        agents,
        mcp,
        hooks,
        cut,
    }
}

/// `path`, from a manifest, under `root` — or nothing, for one that is
/// absolute or climbs out.
fn inside(root: &Path, path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let escapes = path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir));
    (!escapes).then(|| root.join(path))
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

fn name_of(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// One plugin's walk, and what it has spent.
struct Walk {
    visited: usize,
    cut: bool,
}

impl Walk {
    /// The entries directly inside `dir`, each with whether it is a folder
    /// — symlinks left out, since a link is how a walk escapes or loops.
    fn entries(&mut self, dir: &Path) -> Vec<(PathBuf, bool)> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in read.flatten() {
            if self.visited >= VISIT_CAP {
                self.cut = true;
                break;
            }
            self.visited += 1;
            // `file_type` describes the entry itself and does not follow a link.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_symlink() {
                found.push((entry.path(), kind.is_dir()));
            }
        }
        found
    }

    /// Every `.md` file under `dir`, named as Claude Code names a nested one
    /// — its folders joined to its name with `:` — so `git/commit.md` is
    /// `git:commit`.
    fn markdown_under(&mut self, base: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        if depth > DEPTH_CAP {
            self.cut = true;
            return;
        }
        for (path, is_dir) in self.entries(dir) {
            if is_dir {
                self.markdown_under(base, &path, depth + 1, out);
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
            r#"{"description": "Does things", "repository": {"url": "https://example.com/p"}}"#,
        );
        plugin.write("CHANGELOG.md", "");
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
        assert_eq!(
            inventory.repository.as_deref(),
            Some("https://example.com/p")
        );
        assert_eq!(inventory.changelog, Some(plugin.0.join("CHANGELOG.md")));
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
    fn a_skill_is_found_where_the_manifest_points_and_one_category_down() {
        let plugin = Folder::new("nested");
        // As mattpocock-skills ships: skills grouped by category, and a
        // manifest listing each skill's own folder.
        plugin.write(
            ".claude-plugin/plugin.json",
            r#"{"skills": ["./skills/engineering/tdd", "./skills/misc/wizard"]}"#,
        );
        plugin.write("skills/engineering/tdd/SKILL.md", "");
        plugin.write("skills/misc/wizard/SKILL.md", "");
        plugin.write("skills/productivity/teach/SKILL.md", "");
        let inventory = read_blocking(&plugin.0);
        // The manifest's list is the plugin's skills: a skill left in the
        // default folder but not listed is not one it ships — which is how
        // mattpocock-skills comes to 25 and not the 36 on disk.
        assert_eq!(inventory.skills, ["tdd", "wizard"]);
        assert!(!inventory.cut);

        // With no list, the default folder is read, a category down.
        let bare = Folder::new("nested-bare");
        bare.write("skills/engineering/tdd/SKILL.md", "");
        bare.write("skills/productivity/teach/SKILL.md", "");
        assert_eq!(read_blocking(&bare.0).skills, ["tdd", "teach"]);
    }

    #[test]
    fn a_path_that_leaves_the_plugin_folder_is_not_followed() {
        let plugin = Folder::new("escape");
        plugin.write(
            ".claude-plugin/plugin.json",
            r#"{"commands": ["/", "../../elsewhere", "./ok"]}"#,
        );
        plugin.write("ok/fine.md", "");
        let inventory = read_blocking(&plugin.0);
        assert_eq!(inventory.commands, ["fine"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_walked_into() {
        let plugin = Folder::new("loop");
        plugin.write("commands/real.md", "");
        // A link back to the folder holding it: followed, it never ends.
        std::os::unix::fs::symlink(plugin.0.join("commands"), plugin.0.join("commands/again"))
            .unwrap();
        let inventory = read_blocking(&plugin.0);
        assert_eq!(inventory.commands, ["real"]);
    }

    #[test]
    fn a_folder_past_the_bound_says_it_was_cut() {
        let plugin = Folder::new("big");
        for n in 0..(KIND_CAP + 5) {
            plugin.write(&format!("agents/a{n}.md"), "");
        }
        let inventory = read_blocking(&plugin.0);
        assert_eq!(inventory.agents.len(), KIND_CAP);
        assert!(inventory.cut);
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_empty_inventory() {
        let inventory = read_blocking(Path::new("/nonexistent/onehand/plugin"));
        assert_eq!(inventory, Inventory::default());
    }
}
