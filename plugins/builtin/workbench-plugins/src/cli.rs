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
    /// What its folder carries. Empty until read, and empty for a folder that
    /// could not be — which draws as no chips, never as a guessed count.
    pub(crate) inventory: Inventory,
    /// What the marketplace now offers over one of its installs, and which
    /// one; `None` where no install can be told apart from what is offered,
    /// which is not the same as "up to date" and is not drawn as one.
    pub(crate) update: Option<Update>,
    /// The version, folder and commit of the install the row shows: the one
    /// the update is for, else the narrowest — the copy a session here loads.
    pub(crate) version: Option<String>,
    pub(crate) install_path: Option<PathBuf>,
    /// The commit it was installed from, where Claude Code's record says.
    pub(crate) commit: Option<String>,
    /// Each install on its own. A plugin installed at two scopes is two
    /// copies, possibly of two versions, and one can be behind while the other
    /// is not.
    installs: Vec<Install>,
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

/// One install of a plugin, at one scope.
#[derive(Clone, Debug, PartialEq)]
struct Install {
    scope: Scope,
    version: Option<String>,
    install_path: Option<PathBuf>,
}

/// An update the marketplace offers, and the scope whose copy it is for — the
/// one `claude plugin update` has to be told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Update {
    pub(crate) to: String,
    pub(crate) scope: Scope,
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

/// Ready a list of offers for drawing: each one's search text made once, and
/// the most installed first.
fn settle(available: &mut [Available]) {
    for plugin in available.iter_mut() {
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
}

/// Put back the offers the listing leaves out.
///
/// `--available` omits a plugin once it is installed and on, so the
/// marketplace list had no row for exactly the plugins somebody would look
/// for to see that they already have them — and a plugin installed but off
/// was offered as though it were not installed at all. Each installed plugin
/// missing from the offers is added from its marketplace's own catalog; the
/// listing's install count is the one thing that catalog does not carry, so
/// those rows go without one rather than with a guess.
pub(crate) fn complete_offers(
    catalog: &mut Catalog,
    markets: &HashMap<String, Vec<serde_json::Value>>,
) {
    for plugin in &catalog.installed {
        if catalog.available.iter().any(|offer| offer.id == plugin.id) {
            continue;
        }
        let Some(entry) = entry_for(markets, plugin.marketplace(), plugin.name()) else {
            continue;
        };
        catalog.available.push(Available {
            id: plugin.id.clone(),
            name: plugin.name().to_string(),
            description: entry["description"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            marketplace: plugin.marketplace().to_string(),
            install_count: None,
            haystack: String::new(),
        });
    }
    settle(&mut catalog.available);
    // What is already here leads, so the row saying so is on the first
    // screen rather than past the cut — these carry no count, and by count
    // alone they would sort last of several hundred.
    catalog
        .available
        .sort_by_key(|offer| !catalog.installed.iter().any(|p| p.id == offer.id));
}

impl Available {
    /// Whether it is from Anthropic's marketplace — the whole of what the
    /// *Official* mark claims.
    pub(crate) fn official(&self) -> bool {
        self.marketplace == OFFICIAL
    }

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
        let install = Install {
            scope: record.scope,
            version: record.version.clone(),
            install_path: record.install_path.clone(),
        };
        match installed.iter_mut().find(|plugin| plugin.id == record.id) {
            Some(plugin) => {
                plugin.installed.push(record.scope);
                plugin.installs.push(install);
            }
            None => installed.push(Plugin {
                set: Scope::ALL.map(|scope| set[scope as usize].get(&record.id).copied()),
                id: record.id,
                version: record.version,
                inventory: Inventory::default(),
                update: None,
                install_path: record.install_path,
                commit: None,
                installs: vec![install],
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
    settle(&mut available);
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
    let commits = install_record()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|record| installed_commits(&record))
        .unwrap_or_default();
    for plugin in &mut catalog.installed {
        let entry = entry_for(&markets, plugin.marketplace(), plugin.name());
        assess(plugin, entry, &commits);
        if let Some(path) = &plugin.install_path {
            plugin.inventory = inventory::read_blocking(path);
        }
    }
    complete_offers(&mut catalog, &markets);
    Ok(catalog)
}

/// Judge each of `plugin`'s installs against its catalog `entry`, and settle
/// which one the row shows: the one an update is for, else the narrowest
/// scope's — the copy a session started here loads. An update names that
/// install's scope, so *Update* goes to the copy that is behind rather than to
/// whichever scope happens to sort first.
pub(crate) fn assess(
    plugin: &mut Plugin,
    entry: Option<&serde_json::Value>,
    commits: &HashMap<PathBuf, String>,
) {
    let mut installs = plugin.installs.clone();
    // Narrowest first: the copy a session here loads is the one shown when
    // none is behind.
    installs.sort_by_key(|install| std::cmp::Reverse(install.scope));
    let judged: Vec<(&Install, Option<String>, Option<String>)> = installs
        .iter()
        .map(|install| {
            let commit = install
                .install_path
                .as_ref()
                .and_then(|path| commits.get(path))
                .cloned();
            let update = entry
                .and_then(|entry| update_to(install.version.as_deref(), commit.as_deref(), entry));
            (install, commit, update)
        })
        .collect();
    let shown = judged
        .iter()
        .find(|(_, _, update)| update.is_some())
        .or_else(|| judged.first());
    if let Some((install, commit, update)) = shown {
        plugin.version = install.version.clone();
        plugin.install_path = install.install_path.clone();
        plugin.commit = commit.clone();
        plugin.update = update.clone().map(|to| Update {
            to,
            scope: install.scope,
        });
    }
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

/// The version a catalog entry offers over what is installed, if the two can
/// be compared at all.
///
/// Two ways, and only these. A release number the entry names, against the
/// release installed — and only a *newer* one, since a catalog lagging behind
/// what is installed is not offering an update. Or the commit its source is
/// pinned to, against the commit installed — shown by its first seven
/// characters, since that is all a person reads of one. That second one needs
/// the installed commit and not the version label: a plugin fetched from a
/// repository is labelled with its manifest's release (`1.2.3`) whatever
/// commit it came from. An entry whose source is a folder inside the
/// marketplace's own repository names neither, and gets no answer rather than
/// a guess.
pub(crate) fn update_to(
    version: Option<&str>,
    commit: Option<&str>,
    entry: &serde_json::Value,
) -> Option<String> {
    // A newer release is said by its number, which is the name a person
    // knows it by.
    if let (Some(offered), Some(installed)) = (entry["version"].as_str(), version)
        && !is_hash(installed)
    {
        if newer(offered, installed) {
            return Some(offered.to_string());
        }
        // A catalog naming an *older* release has fallen behind, and the
        // commit it pins is that older one: offering it would be offering the
        // downgrade the release number just refused.
        if newer(installed, offered) {
            return None;
        }
    }
    // Otherwise the commit — checked even where the release number is the
    // same, since a repository moves on without bumping it, and that is the
    // case the installed commit is read for.
    let sha = entry["source"]["sha"].as_str().filter(|sha| is_hash(sha))?;
    let installed = commit?;
    let (sha, installed) = (sha.to_ascii_lowercase(), installed.to_ascii_lowercase());
    // Either may be the short form of the other.
    let same = sha.starts_with(&installed) || installed.starts_with(&sha);
    (!same).then(|| sha[..7].to_string())
}

/// Whether release `offered` comes after `installed`.
///
/// Numbers compared part by part after a leading `v`, trailing zero parts not
/// counting (`1.2` is `1.2.0`), and a release after its own pre-release
/// (`1.3.0` after `1.3.0-beta`). A label that is not a release number on
/// either side is not compared at all: a guess there is how a catalog lagging
/// behind is offered as an update, and a downgrade offered is worse than an
/// update missed.
fn newer(offered: &str, installed: &str) -> bool {
    fn release(label: &str) -> Option<(Vec<u64>, bool)> {
        let label = label.trim_start_matches('v');
        let core = label.split('+').next()?;
        let (numbers, pre) = match core.split_once('-') {
            Some((numbers, _)) => (numbers, true),
            None => (core, false),
        };
        let mut parts: Vec<u64> = numbers
            .split('.')
            .map(|part| part.parse().ok())
            .collect::<Option<_>>()?;
        while parts.len() > 1 && parts.last() == Some(&0) {
            parts.pop();
        }
        Some((parts, pre))
    }
    match (release(offered), release(installed)) {
        (Some((offered, offered_pre)), Some((installed, installed_pre))) => {
            offered > installed || (offered == installed && installed_pre && !offered_pre)
        }
        _ => false,
    }
}

/// The commit each install came from, by install folder, out of Claude Code's
/// install record.
///
/// **The one thing read from that record, and read only.** The listing names
/// a plugin by its release label and never by its commit, so without this a
/// plugin that moved on in its repository without bumping its release cannot
/// be told apart from one that did not move. The record is Claude Code's
/// internal state and carries a version number of its own; a shape this does
/// not recognise yields no commits, which leaves those plugins with no update
/// known rather than failing the list.
pub(crate) fn installed_commits(record: &str) -> HashMap<PathBuf, String> {
    let Ok(record) = serde_json::from_str::<serde_json::Value>(record) else {
        return HashMap::new();
    };
    let Some(plugins) = record["plugins"].as_object() else {
        return HashMap::new();
    };
    plugins
        .values()
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(|install| {
            let path = install["installPath"].as_str()?;
            let sha = install["gitCommitSha"].as_str()?;
            Some((PathBuf::from(path), sha.to_string()))
        })
        .collect()
}

/// Where Claude Code keeps its install record.
fn install_record() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .map(|dir| dir.join("plugins").join("installed_plugins.json"))
}

/// A catalog's entry for the plugin called `name`.
fn entry_for<'a>(
    markets: &'a HashMap<String, Vec<serde_json::Value>>,
    marketplace: &str,
    name: &str,
) -> Option<&'a serde_json::Value> {
    markets
        .get(marketplace)?
        .iter()
        .find(|entry| entry["name"] == name)
}

/// What a change does to the plugin it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    Install,
    Update,
    Uninstall,
    Enable,
    Disable,
    /// Install at the change's scope, then remove from this one. Two steps
    /// and not atomic — the command line has no move — so the install goes
    /// first: a failure between the two leaves the plugin at both scopes,
    /// never at neither.
    Move(Scope),
    /// Fetch the plugin's marketplace catalog again, which is the only way an
    /// update becomes known.
    CheckUpdates,
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
    fn steps(&self) -> Vec<Vec<String>> {
        let Change { id, scope, verb } = self;
        let at = |word: &str, scope: Scope| -> Vec<String> {
            let mut args = vec!["plugin", word, id, "--scope", scope.arg()];
            if word == "uninstall" {
                args.push("--keep-data");
            }
            // Its result line is what tells an update made from one refused.
            if word == "update" {
                args.push("--json");
            }
            args.into_iter().map(str::to_string).collect()
        };
        match verb {
            Verb::Install => vec![at("install", *scope)],
            Verb::Update => vec![at("update", *scope)],
            Verb::Uninstall => vec![at("uninstall", *scope)],
            Verb::Enable => vec![at("enable", *scope)],
            Verb::Disable => vec![at("disable", *scope)],
            Verb::Move(from) => vec![at("install", *scope), at("uninstall", *from)],
            Verb::CheckUpdates => {
                let market = id.split_once('@').map_or("", |(_, market)| market);
                vec![
                    ["plugin", "marketplace", "update", market]
                        .map(str::to_string)
                        .to_vec(),
                ]
            }
        }
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
            Verb::Move(from) => {
                return format!("Moving {id} from {} to {}…", from.label(), scope.label());
            }
            Verb::CheckUpdates => return format!("Checking {id} for updates…"),
        };
        format!("{word} {id} ({})…", scope.label())
    }
}

/// Make `change`, in `root` so a project or local scope lands in that project.
pub(crate) fn change_blocking(root: &Path, change: &Change) -> Result<(), String> {
    for step in change.steps() {
        let args: Vec<&str> = step.iter().map(String::as_str).collect();
        let said = claude(root, &args, CHANGE_LIMIT)?;
        if change.verb == Verb::Update {
            updated(&said)?;
        }
    }
    Ok(())
}

/// Whether `claude plugin update --json` changed anything.
///
/// **It exits 0 when it did nothing.** It decides "latest" by release number,
/// so a plugin whose repository moved to a new commit without a new release
/// reads to it as up to date — while the commit compared here says otherwise.
/// Passed as success, the press changed nothing and said nothing; it is said
/// as a refusal instead, with the way that does fetch the commit.
pub(crate) fn updated(said: &str) -> Result<(), String> {
    let Ok(result) = serde_json::from_str::<serde_json::Value>(said.trim()) else {
        return Ok(());
    };
    if result["updateOutcome"] != "up_to_date" {
        return Ok(());
    }
    let message = result["message"]
        .as_str()
        .unwrap_or("Claude Code reports it already at the latest version.");
    Err(format!(
        "{message} Claude Code updates by release number, and this plugin moved to a \
         new commit without a new one; remove and install it again to get that commit."
    ))
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
mod tests;
