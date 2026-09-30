//! Claude Code's plugins, as its own command line reports and changes them.
//!
//! **Everything goes through `claude plugin`, and nothing here reads or writes
//! Claude Code's files.** Where a plugin is installed, whether it is enabled at
//! a scope and what a marketplace offers are kept in files whose layout belongs
//! to Claude Code and has already changed once (the install record carries a
//! version number). The command line is the interface it publishes and keeps,
//! so a change to its files is its problem rather than a silent break here.
//!
//! Every call is blocking and bounded, for the reason the forge's calls are:
//! a command nobody is watching must not be able to hang its caller.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// How long a question answered from this machine may take. Listing reads the
/// marketplaces' local copies, so seconds is already stuck.
const LIST_LIMIT: Duration = Duration::from_secs(30);

/// How long a change may take. Installing clones a repository and updating a
/// catalog fetches one, so this is sized for a slow network rather than a
/// fast disk.
const CHANGE_LIMIT: Duration = Duration::from_secs(300);

/// Where a plugin is installed or enabled, in the words Claude Code uses on its
/// command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
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
}

/// One install of one plugin. A plugin installed at two scopes is two of these.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Installed {
    /// `name@marketplace`, which is what every command takes.
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) version: Option<String>,
    pub(crate) scope: Scope,
    #[serde(default)]
    pub(crate) enabled: bool,
    /// The project a project or local install belongs to.
    #[serde(default)]
    project_path: Option<PathBuf>,
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
}

/// What is installed where it reaches this project, and what could be.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Catalog {
    pub(crate) installed: Vec<Installed>,
    /// Most installed first, which is the order somebody browsing a few
    /// hundred entries wants the first screen in.
    pub(crate) available: Vec<Available>,
}

#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    installed: Vec<Installed>,
    #[serde(default)]
    available: Vec<Available>,
}

/// Read `claude plugin list --json --available` as it applies to `root`.
///
/// The listing names every project's installs, since the record is one file
/// for the machine. Only the global ones and this project's reach a session
/// started here, so those are the ones kept: another project's install shown
/// here would be a switch that changes nothing about the work on screen.
pub(crate) fn parse(json: &str, root: &Path) -> Result<Catalog, String> {
    let listing: Listing = serde_json::from_str(json)
        .map_err(|err| format!("Claude Code's plugin list could not be read: {err}"))?;
    let root = canonical(root);
    let mut installed: Vec<Installed> = listing
        .installed
        .into_iter()
        .filter(|plugin| match plugin.scope {
            Scope::User => true,
            Scope::Project | Scope::Local => plugin
                .project_path
                .as_deref()
                .is_some_and(|path| canonical(path) == root),
        })
        .collect();
    installed.sort_by(|a, b| a.id.cmp(&b.id).then(a.scope.arg().cmp(b.scope.arg())));
    let mut available = listing.available;
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

/// What is installed for `root`, and what the marketplaces offer.
pub(crate) fn list_blocking(root: &Path) -> Result<Catalog, String> {
    let json = claude(
        root,
        &["plugin", "list", "--json", "--available"],
        LIST_LIMIT,
    )?;
    parse(&json, root)
}

/// A change to what is installed or enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Install(String, Scope),
    Uninstall(String, Scope),
    Enable(String, Scope),
    Disable(String, Scope),
    /// Fetch every marketplace's catalog again.
    Refresh,
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
        match self {
            Change::Install(id, scope) => {
                vec!["plugin", "install", id, "--scope", scope.arg()]
            }
            Change::Uninstall(id, scope) => {
                vec![
                    "plugin",
                    "uninstall",
                    id,
                    "--scope",
                    scope.arg(),
                    "--keep-data",
                ]
            }
            Change::Enable(id, scope) => vec!["plugin", "enable", id, "--scope", scope.arg()],
            Change::Disable(id, scope) => vec!["plugin", "disable", id, "--scope", scope.arg()],
            Change::Refresh => vec!["plugin", "marketplace", "update"],
        }
    }

    /// The plugin it is about, so its row can say it is busy.
    pub(crate) fn plugin(&self) -> Option<&str> {
        match self {
            Change::Install(id, _)
            | Change::Uninstall(id, _)
            | Change::Enable(id, _)
            | Change::Disable(id, _) => Some(id),
            Change::Refresh => None,
        }
    }

    /// What is happening, while it happens.
    pub(crate) fn doing(&self) -> String {
        match self {
            Change::Install(id, scope) => format!("Installing {id} ({})…", scope.label()),
            Change::Uninstall(id, scope) => format!("Removing {id} ({})…", scope.label()),
            Change::Enable(id, _) => format!("Enabling {id}…"),
            Change::Disable(id, _) => format!("Disabling {id}…"),
            Change::Refresh => "Updating the marketplaces…".to_string(),
        }
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

    const LISTING: &str = r#"{
      "installed": [
        {"id": "ponytail@ponytail", "version": "4.9.0", "scope": "user", "enabled": true},
        {"id": "karpathy@k", "scope": "project", "enabled": true, "projectPath": "/elsewhere"},
        {"id": "figma@official", "scope": "local", "enabled": false, "projectPath": "/work/app"},
        {"id": "figma@official", "scope": "user", "enabled": true}
      ],
      "available": [
        {"pluginId": "rare@m", "name": "rare", "marketplaceName": "m", "installCount": 3},
        {"pluginId": "popular@m", "name": "popular", "description": "d", "marketplaceName": "m", "installCount": 900},
        {"pluginId": "uncounted@m", "name": "uncounted", "marketplaceName": "m"}
      ]
    }"#;

    #[test]
    fn only_global_installs_and_this_projects_own_are_kept() {
        let catalog = parse(LISTING, Path::new("/work/app")).unwrap();
        let kept: Vec<(&str, Scope)> = catalog
            .installed
            .iter()
            .map(|p| (p.id.as_str(), p.scope))
            .collect();
        assert_eq!(
            kept,
            [
                ("figma@official", Scope::Local),
                ("figma@official", Scope::User),
                ("ponytail@ponytail", Scope::User),
            ]
        );
    }

    #[test]
    fn the_marketplace_lists_the_most_installed_first() {
        let catalog = parse(LISTING, Path::new("/work/app")).unwrap();
        let order: Vec<&str> = catalog.available.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, ["popular", "rare", "uncounted"]);
    }

    #[test]
    fn a_listing_that_is_not_json_is_said_rather_than_read_as_empty() {
        assert!(parse("Error: not logged in", Path::new("/")).is_err());
    }

    #[test]
    fn an_install_never_accepts_a_declared_command_unseen() {
        let change = Change::Install("x@m".into(), Scope::Project);
        let args = change.args();
        assert_eq!(args, ["plugin", "install", "x@m", "--scope", "project"]);
        assert!(!args.contains(&"-y") && !args.contains(&"--yes"));
    }

    #[test]
    fn an_uninstall_keeps_the_plugins_data() {
        let change = Change::Uninstall("x@m".into(), Scope::User);
        let args = change.args();
        assert!(args.contains(&"--keep-data"));
        assert!(args.ends_with(&["--scope", "user", "--keep-data"]));
    }
}
