//! A connector: one outside system a project's work also lives on — a forge,
//! an issue tracker — and the calls the app makes to it.
//!
//! The trait is here and every implementation is a built-in plugin, the same
//! split the remote bridge keeps: what the app asks is decided in core, and how
//! one system answers is that system's crate. So nothing in core knows which
//! program or which API a connector drives.
//!
//! **Every call is blocking and runs in the project it is about**, because a
//! connector that shells out to a CLI learns the repository from its working
//! directory, and one that speaks HTTP can read it off the same checkout.
//!
//! One flat trait while the only connector is a forge that has both halves —
//! issues and pull requests. A system with only one of them (a tracker with no
//! code) is when the two halves split into capabilities of their own.

use crate::unattended::{Issue, IssueRow};
use std::path::Path;

pub trait Connector: Send + Sync + 'static {
    /// The system's name, as a person knows it: "GitHub".
    fn name(&self) -> &'static str;

    /// Who the connector acts as, as the rest of a sentence starting with the
    /// system's name; or why it cannot act at all, in words saying what to do.
    fn account_blocking(&self) -> Result<String, String>;

    /// Whether this connector can work the project at `root`, and why not.
    /// Read locally where it can be, since it is asked on every tick for every
    /// project switched on.
    fn serves_blocking(&self, root: &Path) -> Result<(), String>;

    /// Open issues, newest first, at most `limit` of them.
    fn open_issues_blocking(&self, root: &Path, limit: usize) -> Result<Vec<IssueRow>, String>;

    /// Open issues carrying `label` that the signed-in account opened itself.
    fn my_labelled_issues_blocking(&self, root: &Path, label: &str) -> Result<Vec<Issue>, String>;

    fn remove_label_blocking(&self, root: &Path, number: u64, label: &str) -> Result<(), String>;

    fn comment_blocking(&self, root: &Path, number: u64, body: &str) -> Result<(), String>;

    /// The repository's default branch, as the system has it.
    fn default_branch_blocking(&self, root: &Path) -> Result<String, String>;

    /// The pull request opened from `branch`, open or not, if there is one.
    fn pull_request_for_blocking(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<Option<String>, String>;

    /// How an agent opens a pull request here, as the words it is told to use:
    /// "`gh pr create`".
    fn open_pull_request_with(&self) -> &'static str;
}

/// The first of `connectors` that serves the project at `root`, or every
/// reason none does.
pub fn serving(
    connectors: &[&'static dyn Connector],
    root: &Path,
) -> Result<&'static dyn Connector, String> {
    let mut refusals = Vec::new();
    for &connector in connectors {
        match connector.serves_blocking(root) {
            Ok(()) => return Ok(connector),
            Err(why) => refusals.push(why),
        }
    }
    Err(if refusals.is_empty() {
        "no connector is built into this onehand".to_string()
    } else {
        refusals.join("; ")
    })
}
