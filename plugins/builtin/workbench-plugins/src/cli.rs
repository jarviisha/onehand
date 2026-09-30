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
//! is enabled *in the end* — every scope already folded into one answer — so a
//! project that turns a global plugin off and a plugin that was never on read
//! the same; its `projectEnabled` speaks for the project's file alone and says
//! nothing of the other two. The settings files are Claude Code's documented configuration
//! rather than its internal state, and only their `enabledPlugins` key is read.
//!
//! Every call is blocking and bounded, for the reason the forge's calls are:
//! a command nobody is watching must not be able to hang its caller.

use crate::inventory::{self, Inventory};
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

    /// Who it reaches, as a menu row names it — by the people and places
    /// affected rather than by the file, since that is what choosing one
    /// decides.
    pub(crate) fn reach(self) -> &'static str {
        match self {
            Scope::User => "Every project",
            Scope::Project => "This project, for everyone (committed)",
            Scope::Local => "This project, on this machine",
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
    /// run in: every source of settings folded in, the same on every record
    /// of one plugin.
    #[serde(default)]
    enabled: bool,
    /// The project a project or local install belongs to.
    #[serde(default)]
    project_path: Option<PathBuf>,
    /// Where its files are, which is what its contents are read from.
    #[serde(default)]
    install_path: Option<PathBuf>,
}

/// One installed plugin that reaches this project.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plugin {
    /// `name@marketplace`, which is what every command takes.
    pub(crate) id: String,
    pub(crate) version: Option<String>,
    /// What its folder carries. Empty until read, and empty for a folder that
    /// could not be — which draws as no chips, never as a guessed count.
    pub(crate) inventory: Inventory,
    /// The version the marketplace now offers, where it can be told apart
    /// from the one installed; `None` when it cannot, which is not the same as
    /// "up to date" and is not drawn as one.
    pub(crate) update: Option<String>,
    install_path: Option<PathBuf>,
    /// Where it is installed, which is where it can be removed from.
    pub(crate) installed: Vec<Scope>,
    /// What each scope's settings file says, indexed as [`Scope::ALL`];
    /// `None` where the file says nothing about it.
    set: [Option<bool>; 3],
    /// The listing's own answer, with every source of settings folded in —
    /// including ones not read here. Never a fallback for a scope (it holds
    /// the narrower scopes too); only compared against, to notice a source
    /// the three files do not account for.
    enabled: bool,
}

/// The marketplace Anthropic runs. A plugin from it is marked *Official*;
/// nothing in the listing carries a finer verified flag, so this is the whole
/// of what the mark claims.
const OFFICIAL: &str = "claude-plugins-official";

impl Plugin {
    /// The plugin's own name, without its marketplace.
    pub(crate) fn name(&self) -> &str {
        self.id
            .split_once('@')
            .map_or(self.id.as_str(), |(name, _)| name)
    }

    /// The marketplace it came from.
    pub(crate) fn marketplace(&self) -> &str {
        self.id.split_once('@').map_or("", |(_, market)| market)
    }

    pub(crate) fn official(&self) -> bool {
        self.marketplace() == OFFICIAL
    }

    /// Whether it is on at `scope`: what that scope sets, else what the next
    /// wider one sets, else off.
    ///
    /// **Off, and not the listing's own `enabled`**, which looks like the
    /// natural fallback and is not one: that answer has every scope folded in,
    /// narrower ones included, so a project turning a plugin off would read
    /// back as the plugin being off globally. A plugin installed and mentioned
    /// by no settings file is off — which is what the listing says of one, and
    /// why the narrowest scope's answer here is what a session started in the
    /// project gets.
    pub(crate) fn in_force(&self, scope: Scope) -> bool {
        self.source(scope)
            .and_then(|source| self.set[source as usize])
            .unwrap_or(false)
    }

    /// Which scope decides it at `scope`: `scope` itself if its file mentions
    /// it, else the nearest wider one that does, else none. The one walk every
    /// other question about a scope is answered from.
    pub(crate) fn source(&self, scope: Scope) -> Option<Scope> {
        Scope::ALL
            .into_iter()
            .rev()
            .filter(|wider| *wider <= scope)
            .find(|wider| self.set[*wider as usize].is_some())
    }

    /// Whether it is installed at `scope` or at a wider one — which is where
    /// turning it on or off can mean anything. Global is not offered for a
    /// plugin installed for this project alone: switched on there, the setting
    /// would name a plugin no other project has, and read as on everywhere.
    pub(crate) fn reaches(&self, scope: Scope) -> bool {
        self.installed.iter().any(|at| *at <= scope)
    }

    /// Whether what a session here gets is decided by something the three
    /// settings files do not show — managed settings, a policy, a settings
    /// flag. Then the switches describe the files and not the outcome, and the
    /// row has to say so rather than let them be believed.
    pub(crate) fn decided_elsewhere(&self) -> bool {
        self.in_force(Scope::Local) != self.enabled
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
///
/// **Each record is read on its own, and one this build cannot read is
/// skipped.** The listing is Claude Code's and grows: a scope this enum does
/// not name — managed settings have one — read as part of one list would fail
/// the whole listing, and the mode would show an error where every other
/// plugin should be, with *Try again* failing the same way for good. Skipped,
/// it costs that one record.
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    installed: Vec<serde_json::Value>,
    #[serde(default)]
    available: Vec<serde_json::Value>,
}

/// The records of `records` this build can read.
fn readable<T: serde::de::DeserializeOwned>(records: Vec<serde_json::Value>) -> Vec<T> {
    records
        .into_iter()
        .filter_map(|record| serde_json::from_value(record).ok())
        .collect()
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
    for record in readable::<Installed>(listing.installed) {
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
                inventory: Inventory::default(),
                update: None,
                install_path: record.install_path,
                installed: vec![record.scope],
                enabled: record.enabled,
            }),
        }
    }
    installed.sort_by(|a, b| a.id.cmp(&b.id));
    for plugin in &mut installed {
        plugin.installed.sort();
    }

    let mut available: Vec<Available> = readable(listing.available);
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
    let mut catalog = parse(&json, root, &set)?;
    let markets = marketplaces_blocking(root);
    for plugin in &mut catalog.installed {
        if let Some(path) = &plugin.install_path {
            plugin.inventory = inventory::read_blocking(path);
        }
        let entry = markets
            .get(plugin.marketplace())
            .and_then(|market| market.iter().find(|entry| entry["name"] == plugin.name()));
        plugin.update = match (&plugin.version, entry) {
            (Some(version), Some(entry)) => update_to(version, entry),
            _ => None,
        };
    }
    Ok(catalog)
}

/// Every known marketplace's catalog entries, by marketplace name.
///
/// Where each one's copy is on disk comes from the command line
/// (`marketplace list --json`); the catalog itself is the marketplace's own
/// published `marketplace.json`. A marketplace that cannot be read contributes
/// nothing, which leaves its plugins with no update known rather than failing
/// the list.
fn marketplaces_blocking(root: &Path) -> HashMap<String, Vec<serde_json::Value>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Market {
        name: String,
        install_location: Option<PathBuf>,
    }
    let Ok(json) = claude(
        root,
        &["plugin", "marketplace", "list", "--json"],
        LIST_LIMIT,
    ) else {
        return HashMap::new();
    };
    let markets: Vec<Market> = serde_json::from_str(&json).unwrap_or_default();
    markets
        .into_iter()
        .filter_map(|market| {
            let file = market
                .install_location?
                .join(".claude-plugin/marketplace.json");
            let text = std::fs::read_to_string(file).ok()?;
            let catalog: serde_json::Value = serde_json::from_str(&text).ok()?;
            let entries = catalog["plugins"].as_array()?.clone();
            Some((market.name, entries))
        })
        .collect()
}

/// Whether `version` reads as a commit rather than a release: seven or more
/// hex digits and nothing else.
pub(crate) fn is_hash(version: &str) -> bool {
    version.len() >= 7 && version.chars().all(|c| c.is_ascii_hexdigit())
}

/// The version a catalog entry offers over `installed`, if the two can be
/// compared at all.
///
/// Two ways, and only these: a release number the entry names, against a
/// release installed; or the commit its source is pinned to, against a commit
/// installed — shown by its first seven characters, since that is all a
/// person reads of one. An entry whose source is a folder inside the
/// marketplace's own repository names neither, and gets no answer rather than
/// a guess.
pub(crate) fn update_to(installed: &str, entry: &serde_json::Value) -> Option<String> {
    if !is_hash(installed) {
        return entry["version"]
            .as_str()
            .filter(|offered| *offered != installed)
            .map(str::to_string);
    }
    let sha = entry["source"]["sha"].as_str()?;
    (!sha.starts_with(installed) && is_hash(sha)).then(|| sha[..7].to_string())
}

/// What a change does to the plugin it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    Install,
    Update,
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
            Verb::Update => "update",
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
            Verb::Update => "Updating",
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
    /// one plugin, because it is the folded answer rather than the record's own
    /// — and for a plugin no settings file mentions, it is off.
    const LISTING: &str = r#"{
      "installed": [
        {"id": "ponytail@ponytail", "version": "4.9.0", "scope": "user", "enabled": false},
        {"id": "karpathy@k", "scope": "project", "enabled": true, "projectPath": "/elsewhere"},
        {"id": "figma@official", "scope": "local", "enabled": false, "projectPath": "/work/app"},
        {"id": "figma@official", "scope": "user", "enabled": false}
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
        // Local sets nothing, so it takes the project's answer.
        assert!(!ponytail.in_force(Scope::Local));
        assert_ne!(ponytail.source(Scope::Local), Some(Scope::Local));
    }

    #[test]
    fn a_project_turning_a_plugin_off_does_not_read_back_as_off_globally() {
        // Global says nothing; the project says off. The listing's folded
        // answer is off, and it must not leak up into the Global switch.
        let set = settings("", r#"{"enabledPlugins": {"figma@official": false}}"#, "");
        let catalog = catalog(&set);
        let figma = plugin(&catalog, "figma@official");
        assert_eq!(figma.source(Scope::User), None);
        assert!(!figma.in_force(Scope::User));
        assert_eq!(figma.flip(Scope::User).verb, Verb::Enable);
    }

    #[test]
    fn a_plugin_no_settings_file_mentions_is_off_at_every_scope() {
        let catalog = catalog(&settings("", "", ""));
        let figma = plugin(&catalog, "figma@official");
        assert!(Scope::ALL.iter().all(|s| !figma.in_force(*s)));
        assert!(Scope::ALL.iter().all(|s| figma.source(*s).is_none()));
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
    fn a_record_this_build_cannot_read_is_skipped_rather_than_failing_the_listing() {
        // A scope added after this was written — managed settings already
        // have one — must cost that one record, not every row in the mode.
        let listing = r#"{
          "installed": [
            {"id": "policy@corp", "scope": "managed", "enabled": true},
            {"id": "ponytail@ponytail", "scope": "user", "enabled": true}
          ],
          "available": [
            {"pluginId": "odd@m"},
            {"pluginId": "ok@m", "name": "ok", "marketplaceName": "m"}
          ]
        }"#;
        let catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
        let installed: Vec<&str> = catalog.installed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(installed, ["ponytail@ponytail"]);
        let available: Vec<&str> = catalog.available.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(available, ["ok@m"]);
    }

    #[test]
    fn a_listing_that_is_not_json_is_said_rather_than_read_as_empty() {
        assert!(parse("Error: not logged in", Path::new("/"), &Default::default()).is_err());
    }

    #[test]
    fn the_scope_that_decides_is_the_narrowest_that_sets_it() {
        let set = settings(
            r#"{"enabledPlugins": {"ponytail@ponytail": true}}"#,
            r#"{"enabledPlugins": {"ponytail@ponytail": false}}"#,
            "",
        );
        let catalog = catalog(&set);
        let ponytail = plugin(&catalog, "ponytail@ponytail");
        assert_eq!(ponytail.source(Scope::User), Some(Scope::User));
        assert_eq!(ponytail.source(Scope::Project), Some(Scope::Project));
        assert_eq!(ponytail.source(Scope::Local), Some(Scope::Project));
        let figma = plugin(&catalog, "figma@official");
        assert_eq!(figma.source(Scope::Local), None);
    }

    #[test]
    fn a_scope_is_offered_only_where_the_plugin_is_installed_at_it_or_wider() {
        let listing = r#"{"installed": [
            {"id": "here@m", "scope": "local", "enabled": true, "projectPath": "/work/app"},
            {"id": "shared@m", "scope": "project", "enabled": true, "projectPath": "/work/app"},
            {"id": "everywhere@m", "scope": "user", "enabled": true}
        ]}"#;
        let catalog = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
        let reach = |id| {
            let plugin = plugin(&catalog, id);
            Scope::ALL.map(|scope| plugin.reaches(scope))
        };
        // Turning a plugin on globally where it is installed for one project
        // would write a setting that names a plugin no other project has.
        assert_eq!(reach("here@m"), [false, false, true]);
        assert_eq!(reach("shared@m"), [false, true, true]);
        assert_eq!(reach("everywhere@m"), [true, true, true]);
    }

    #[test]
    fn a_plugin_something_unread_turns_on_is_said_to_be_decided_elsewhere() {
        // On in the end, while none of the three files mentions it: managed
        // settings or a policy did it, and the switches cannot say so.
        let listing = r#"{"installed": [
            {"id": "managed@m", "scope": "user", "enabled": true}
        ]}"#;
        let listed = parse(listing, Path::new("/work/app"), &Default::default()).unwrap();
        let managed = plugin(&listed, "managed@m");
        assert!(!managed.in_force(Scope::Local));
        assert!(managed.decided_elsewhere());
        // Where the files and the listing agree, there is nothing to say.
        let agreed = catalog(&settings("", "", ""));
        assert!(!plugin(&agreed, "figma@official").decided_elsewhere());
    }

    #[test]
    fn a_scopes_place_in_the_list_is_its_index_into_the_settings() {
        assert!(
            Scope::ALL
                .iter()
                .enumerate()
                .all(|(i, scope)| *scope as usize == i)
        );
    }

    #[test]
    fn a_release_is_compared_with_a_release_and_a_commit_with_a_commit() {
        let semver = serde_json::json!({"name": "p", "version": "1.3.0"});
        assert_eq!(update_to("1.2.3", &semver).as_deref(), Some("1.3.0"));
        assert_eq!(update_to("1.3.0", &semver), None);
        let pinned = serde_json::json!({"name": "p", "source": {
            "source": "url", "sha": "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"}});
        assert_eq!(
            update_to("5b15a47f2d71", &pinned).as_deref(),
            Some("c55ee46")
        );
        assert_eq!(update_to("c55ee46073ed", &pinned), None);
        // A folder inside the marketplace's own repository names no version
        // and no commit: no answer, rather than a guess.
        let local = serde_json::json!({"name": "p", "source": "./plugins/p"});
        assert_eq!(update_to("2a8ad9f74633", &local), None);
        assert_eq!(update_to("1.0.0", &local), None);
    }

    #[test]
    fn a_plugin_is_named_by_its_name_and_marked_official_by_its_marketplace() {
        let catalog = catalog(&settings("", "", ""));
        let figma = plugin(&catalog, "figma@official");
        assert_eq!(figma.name(), "figma");
        assert_eq!(figma.marketplace(), "official");
        assert!(!figma.official());
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
