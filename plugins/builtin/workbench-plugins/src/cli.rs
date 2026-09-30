//! Claude Code's plugins, as its own command line reports and changes them.
//!
//! **Every change goes through `claude plugin`, and nothing here writes Claude
//! Code's files.** Where a plugin is installed and what a marketplace offers
//! are kept in files whose layout belongs to Claude Code and has already
//! changed once (the install record carries a version number); the command line
//! is the interface it publishes and keeps.
//!
//! **One thing is read from files, because the command line does not say it:**
//! what each scope's settings file sets. Its listing reports whether a plugin
//! is enabled *in the end* — the three scopes already folded into one answer —
//! so a project that turns a global plugin off and a plugin that was never on
//! read the same. The settings files are Claude Code's documented configuration
//! rather than its internal state, and only their `enabledPlugins` key is read.
//!
//! Every call is blocking and bounded, for the reason the forge's calls are:
//! a command nobody is watching must not be able to hang its caller.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// How long a question answered from this machine may take. Listing reads the
/// marketplaces' local copies, so seconds is already stuck.
const LIST_LIMIT: Duration = Duration::from_secs(30);

/// How long a change may take. Installing clones a repository, so this is
/// sized for a slow network rather than a fast disk.
const CHANGE_LIMIT: Duration = Duration::from_secs(300);

/// Where a plugin is installed or enabled, in the words Claude Code uses on its
/// command line.
///
/// Declared from the widest reach to the narrowest, which is also the order in
/// which they are overridden: a project's setting beats the global one and the
/// machine's own copy of the project beats both. The derived order is that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    /// Every project on this machine.
    User,
    /// This project, in a settings file that is committed with it.
    Project,
    /// This project on this machine alone, in a settings file kept out of git.
    Local,
}

impl Scope {
    pub(crate) const ALL: [Scope; 3] = [Scope::User, Scope::Project, Scope::Local];

    fn arg(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Local => "local",
        }
    }

    /// The word on screen. `user` is Claude Code's name for it and reads as a
    /// person rather than a reach, so the screen says what it covers instead.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Scope::User => "Global",
            Scope::Project => "Project",
            Scope::Local => "Local",
        }
    }

    /// What choosing it means, for the tooltip.
    pub(crate) fn meaning(self) -> &'static str {
        match self {
            Scope::User => "Every project on this machine",
            Scope::Project => "This project, in .claude/settings.json — committed with it",
            Scope::Local => "This project on this machine, in .claude/settings.local.json",
        }
    }

    /// The settings file this scope writes, for `root`.
    fn settings_file(self, root: &Path) -> Option<PathBuf> {
        match self {
            // Claude Code moves its whole directory when this is set, and a
            // file read from the default place would then be somebody else's.
            Scope::User => std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
                .map(|dir| dir.join("settings.json")),
            Scope::Project => Some(root.join(".claude").join("settings.json")),
            Scope::Local => Some(root.join(".claude").join("settings.local.json")),
        }
    }
}

/// One install record, as the listing reports it. A plugin installed at two
/// scopes is two of these; what is drawn is [`Plugin`], one per plugin.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    id: String,
    #[serde(default)]
    version: Option<String>,
    scope: Scope,
    /// Whether it is enabled in the end, for the directory the listing was
    /// run in — every scope already folded in, the same on every record of one
    /// plugin.
    #[serde(default)]
    enabled: bool,
    /// The project a project or local install belongs to.
    #[serde(default)]
    project_path: Option<PathBuf>,
}

/// One installed plugin that reaches this project.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plugin {
    /// `name@marketplace`, which is what every command takes.
    pub(crate) id: String,
    pub(crate) version: Option<String>,
    /// Where it is installed, which is where it can be removed from.
    pub(crate) installed: Vec<Scope>,
    /// What each scope's settings file says, indexed as [`Scope::ALL`];
    /// `None` where the file says nothing about it.
    set: [Option<bool>; 3],
    /// What the listing says it comes to in the end, which is what a scope
    /// that sets nothing falls back to when no wider one does either.
    enabled: bool,
}

impl Plugin {
    /// Whether it is on at `scope`: what that scope sets, else what the next
    /// wider one sets, else the listing's own answer.
    ///
    /// At the narrowest scope this is what a session started here gets, and it
    /// is asserted against the listing's answer so the two cannot drift.
    pub(crate) fn in_force(&self, scope: Scope) -> bool {
        Scope::ALL
            .iter()
            .rev()
            .filter(|wider| **wider <= scope)
            .find_map(|wider| self.set[*wider as usize])
            .unwrap_or(self.enabled)
    }

    /// Whether `scope` sets it itself, rather than taking it from a wider one.
    pub(crate) fn set_at(&self, scope: Scope) -> bool {
        self.set[scope as usize].is_some()
    }

    /// The change that flips it at `scope`: whatever is in force there, the
    /// other way. Written at that scope, so flipping it for a project leaves
    /// every other project where it was.
    pub(crate) fn flip(&self, scope: Scope) -> Change {
        let verb = if self.in_force(scope) {
            Verb::Disable
        } else {
            Verb::Enable
        };
        Change {
            id: self.id.clone(),
            scope,
            verb,
        }
    }
}

/// One plugin a known marketplace offers.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Available {
    #[serde(rename = "pluginId")]
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default, rename = "marketplaceName")]
    pub(crate) marketplace: String,
    #[serde(default)]
    pub(crate) install_count: Option<u64>,
    /// Name, marketplace and description lowercased once, which is what a
    /// search is matched against — rather than lowercasing a few hundred
    /// descriptions on every keystroke.
    #[serde(skip)]
    haystack: String,
}

impl Available {
    /// Whether it matches `needle`, already lowercased.
    pub(crate) fn matches(&self, needle: &str) -> bool {
        self.haystack.contains(needle)
    }
}

/// What is installed where it reaches this project, and what could be.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Catalog {
    pub(crate) installed: Vec<Plugin>,
    /// Most installed first, which is the order somebody browsing a few
    /// hundred entries wants the first screen in.
    pub(crate) available: Vec<Available>,
}

impl Catalog {
    /// Whether `id` is installed at `scope`.
    pub(crate) fn has(&self, id: &str, scope: Scope) -> bool {
        self.installed
            .iter()
            .any(|plugin| plugin.id == id && plugin.installed.contains(&scope))
    }
}

/// The listing's shape with `--available`: installs and offers apart. Without
/// that flag it is a bare array, so the flag is part of what this reads.
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    installed: Vec<Installed>,
    #[serde(default)]
    available: Vec<Available>,
}

/// What a settings file's `enabledPlugins` says, by plugin.
///
/// A file that is missing says nothing, and so does one that cannot be read:
/// Claude Code refuses such a file too, so nothing it sets is in force.
pub(crate) fn enabled_plugins(json: &str) -> HashMap<String, bool> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Settings {
        #[serde(default)]
        enabled_plugins: HashMap<String, bool>,
    }
    serde_json::from_str::<Settings>(json)
        .map(|settings| settings.enabled_plugins)
        .unwrap_or_default()
}

/// Read `claude plugin list --json --available` as it applies to `root`, with
/// what each scope's settings file sets, indexed as [`Scope::ALL`].
///
/// The listing names every project's installs, since the record is one file
/// for the machine. Only the global ones and this project's reach a session
/// started here, so those are the ones kept: another project's install shown
/// here would be a switch that changes nothing about the work on screen.
pub(crate) fn parse(
    json: &str,
    root: &Path,
    set: &[HashMap<String, bool>; 3],
) -> Result<Catalog, String> {
    let listing: Listing = serde_json::from_str(json)
        .map_err(|err| format!("Claude Code's plugin list could not be read: {err}"))?;
    let root = canonical(root);
    let mut installed: Vec<Plugin> = Vec::new();
    for record in listing.installed {
        let reaches = match record.scope {
            Scope::User => true,
            Scope::Project | Scope::Local => record
                .project_path
                .as_deref()
                .is_some_and(|path| canonical(path) == root),
        };
        if !reaches {
            continue;
        }
        match installed.iter_mut().find(|plugin| plugin.id == record.id) {
            Some(plugin) => plugin.installed.push(record.scope),
            None => installed.push(Plugin {
                set: Scope::ALL.map(|scope| set[scope as usize].get(&record.id).copied()),
                id: record.id,
                version: record.version,
                installed: vec![record.scope],
                enabled: record.enabled,
            }),
        }
    }
    installed.sort_by(|a, b| a.id.cmp(&b.id));
    for plugin in &mut installed {
        plugin.installed.sort();
    }

    let mut available = listing.available;
    for plugin in &mut available {
        plugin.haystack = format!(
            "{}\n{}\n{}",
            plugin.name, plugin.marketplace, plugin.description
        )
        .to_lowercase();
    }
    available.sort_by(|a, b| {
        b.install_count
            .unwrap_or(0)
            .cmp(&a.install_count.unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(Catalog {
        installed,
        available,
    })
}

/// A path as the file system names it, so a symlinked checkout still matches
/// the path Claude Code recorded. A path that no longer exists is compared as
/// written.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// What is installed for `root`, what each scope sets, and what the
/// marketplaces offer.
pub(crate) fn list_blocking(root: &Path) -> Result<Catalog, String> {
    let json = claude(
        root,
        &["plugin", "list", "--json", "--available"],
        LIST_LIMIT,
    )?;
    let set = Scope::ALL.map(|scope| {
        scope
            .settings_file(root)
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map(|json| enabled_plugins(&json))
            .unwrap_or_default()
    });
    parse(&json, root, &set)
}

/// What a change does to the plugin it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    Install,
    Uninstall,
    Enable,
    Disable,
}

/// A change to what is installed or enabled: one plugin, one scope, one verb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) id: String,
    pub(crate) scope: Scope,
    pub(crate) verb: Verb,
}

impl Change {
    /// The arguments after `claude`.
    ///
    /// **An install never passes `-y`.** A marketplace can declare a command
    /// that runs to install a plugin, and `-y` is the flag that accepts it
    /// unseen; without it a non-interactive install refuses and says what the
    /// command was, which is the one outcome that leaves a person to decide.
    ///
    /// **An uninstall keeps the plugin's data** (`--keep-data`). Removing a
    /// plugin can be undone by installing it again and removing its data
    /// cannot, so the half that is final is left to be done by hand.
    fn args(&self) -> Vec<&str> {
        let Change { id, scope, verb } = self;
        let word = match verb {
            Verb::Install => "install",
            Verb::Uninstall => "uninstall",
            Verb::Enable => "enable",
            Verb::Disable => "disable",
        };
        let mut args = vec!["plugin", word, id, "--scope", scope.arg()];
        if *verb == Verb::Uninstall {
            args.push("--keep-data");
        }
        args
    }

    /// What is happening, while it happens.
    pub(crate) fn doing(&self) -> String {
        let Change { id, scope, verb } = self;
        let word = match verb {
            Verb::Install => "Installing",
            Verb::Uninstall => "Removing",
            Verb::Enable => "Enabling",
            Verb::Disable => "Disabling",
        };
        format!("{word} {id} ({})…", scope.label())
    }
}

/// Make `change`, in `root` so a project or local scope lands in that project.
pub(crate) fn change_blocking(root: &Path, change: &Change) -> Result<(), String> {
    claude(root, &change.args(), CHANGE_LIMIT).map(drop)
}

/// Run `claude` in `root` and hand back what it printed.
fn claude(root: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let out = onehand_core::process::output_within(
        Command::new("claude").args(args).current_dir(root),
        limit,
    )
    .map_err(|err| match err {
        onehand_core::process::Failure::Missing => {
            "Claude Code's command line (`claude`) was not found on PATH".to_string()
        }
        err => format!("claude {err}"),
    })?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    // A refusal is said on stderr, but some are printed as the human message
    // on stdout instead; whichever carries words is the reason.
    let said = [&out.stderr, &out.stdout]
        .into_iter()
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
        .find(|text| !text.is_empty());
    Err(said.unwrap_or_else(|| format!("claude {} failed and said nothing about why", args[1])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped as the real listing is: `enabled` is the same on every record of
    /// one plugin, because it is the folded answer rather than the record's own.
    const LISTING: &str = r#"{
      "installed": [
        {"id": "ponytail@ponytail", "version": "4.9.0", "scope": "user", "enabled": false},
        {"id": "karpathy@k", "scope": "project", "enabled": true, "projectPath": "/elsewhere"},
        {"id": "figma@official", "scope": "local", "enabled": true, "projectPath": "/work/app"},
        {"id": "figma@official", "scope": "user", "enabled": true}
      ],
      "available": [
        {"pluginId": "rare@m", "name": "rare", "marketplaceName": "m", "installCount": 3},
        {"pluginId": "popular@m", "name": "popular", "description": "Does Things", "marketplaceName": "m", "installCount": 900},
        {"pluginId": "uncounted@m", "name": "uncounted", "marketplaceName": "m"}
      ]
    }"#;

    fn settings(user: &str, project: &str, local: &str) -> [HashMap<String, bool>; 3] {
        [user, project, local].map(enabled_plugins)
    }

    fn catalog(set: &[HashMap<String, bool>; 3]) -> Catalog {
        parse(LISTING, Path::new("/work/app"), set).unwrap()
    }

    fn plugin<'a>(catalog: &'a Catalog, id: &str) -> &'a Plugin {
        catalog.installed.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn one_row_per_plugin_with_every_scope_it_is_installed_at() {
        let catalog = catalog(&settings("", "", ""));
        let rows: Vec<(&str, &[Scope])> = catalog
            .installed
            .iter()
            .map(|p| (p.id.as_str(), p.installed.as_slice()))
            .collect();
        // Another project's install is left out: it reaches no session here.
        assert_eq!(
            rows,
            [
                ("figma@official", &[Scope::User, Scope::Local][..]),
                ("ponytail@ponytail", &[Scope::User][..]),
            ]
        );
    }

    #[test]
    fn a_project_turning_a_global_plugin_off_is_off_here_and_on_everywhere_else() {
        let set = settings(
            r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
            r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
            "",
        );
        let catalog = catalog(&set);
        let ponytail = plugin(&catalog, "ponytail@ponytail");
        assert!(ponytail.in_force(Scope::User));
        assert!(!ponytail.in_force(Scope::Project));
        // Local sets nothing, so it takes the project's answer — which is the
        // listing's own, as the narrowest scope's must be.
        assert!(!ponytail.in_force(Scope::Local));
        assert!(!ponytail.set_at(Scope::Local));
        assert_eq!(ponytail.in_force(Scope::Local), ponytail.enabled);
    }

    #[test]
    fn a_scope_that_sets_nothing_anywhere_takes_the_listings_answer() {
        let catalog = catalog(&settings("", "", ""));
        let figma = plugin(&catalog, "figma@official");
        assert!(Scope::ALL.iter().all(|s| figma.in_force(*s)));
        assert!(Scope::ALL.iter().all(|s| !figma.set_at(*s)));
    }

    #[test]
    fn flipping_at_a_scope_writes_the_opposite_of_what_is_in_force_there() {
        let set = settings(
            r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
            r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
            "",
        );
        let catalog = catalog(&set);
        let ponytail = plugin(&catalog, "ponytail@ponytail");
        let verb = |scope| ponytail.flip(scope).verb;
        assert_eq!(verb(Scope::User), Verb::Disable);
        assert_eq!(verb(Scope::Project), Verb::Enable);
        assert_eq!(verb(Scope::Local), Verb::Enable);
        assert_eq!(
            ponytail.flip(Scope::Project).args(),
            [
                "plugin",
                "enable",
                "ponytail@ponytail",
                "--scope",
                "project"
            ]
        );
    }

    #[test]
    fn a_settings_file_that_cannot_be_read_sets_nothing() {
        assert!(enabled_plugins("{ not json").is_empty());
        assert!(enabled_plugins(r#"{"model": "opus"}"#).is_empty());
        assert_eq!(
            enabled_plugins(r#"{"enabledPlugins": {"a@m": false}}"#).get("a@m"),
            Some(&false)
        );
    }

    #[test]
    fn the_marketplace_lists_the_most_installed_first_and_searches_every_field() {
        let catalog = catalog(&settings("", "", ""));
        let order: Vec<&str> = catalog.available.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, ["popular", "rare", "uncounted"]);
        assert!(catalog.available[0].matches("does things"));
        assert!(!catalog.available[1].matches("does things"));
    }

    #[test]
    fn a_listing_that_is_not_json_is_said_rather_than_read_as_empty() {
        assert!(parse("Error: not logged in", Path::new("/"), &Default::default()).is_err());
    }

    #[test]
    fn an_install_never_accepts_a_declared_command_unseen() {
        let change = Change {
            id: "x@m".into(),
            scope: Scope::Project,
            verb: Verb::Install,
        };
        let args = change.args();
        assert_eq!(args, ["plugin", "install", "x@m", "--scope", "project"]);
        assert!(!args.contains(&"-y") && !args.contains(&"--yes"));
    }

    #[test]
    fn an_uninstall_keeps_the_plugins_data() {
        let change = Change {
            id: "x@m".into(),
            scope: Scope::User,
            verb: Verb::Uninstall,
        };
        assert!(change.args().ends_with(&["--scope", "user", "--keep-data"]));
    }
}
