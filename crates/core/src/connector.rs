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

    /// Take `label` off issue `number`.
    fn remove_label_blocking(&self, root: &Path, number: u64, label: &str) -> Result<(), String>;

    /// Leave `body` as a comment on issue `number`.
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

/// Where in `connectors` the first one that serves the project at `root` is,
/// or every reason none does.
///
/// A position rather than the connector itself, because what else is known
/// about a connector — what it said about its account — is kept beside the
/// list in the same order, and a name is not an identity two connectors are
/// barred from sharing.
pub fn serving(connectors: &[&'static dyn Connector], root: &Path) -> Result<usize, String> {
    let mut refusals = Vec::new();
    for (at, connector) in connectors.iter().enumerate() {
        match connector.serves_blocking(root) {
            Ok(()) => return Ok(at),
            Err(why) => refusals.push(why),
        }
    }
    Err(if refusals.is_empty() {
        "no connector is built into this onehand".to_string()
    } else {
        refusals.join("; ")
    })
}

/// A connector for tests: it serves or refuses as told, lists `limit` issues,
/// finds issues 9 and 4 under any label, and fails the test if asked anything
/// else.
#[cfg(test)]
pub(crate) struct Fake {
    pub(crate) name: &'static str,
    pub(crate) serves: Result<(), &'static str>,
}

#[cfg(test)]
impl Fake {
    pub(crate) const SERVING: Fake = Fake {
        name: "Forge",
        serves: Ok(()),
    };
}

#[cfg(test)]
impl Connector for Fake {
    fn name(&self) -> &'static str {
        self.name
    }
    fn account_blocking(&self) -> Result<String, String> {
        unreachable!()
    }
    fn serves_blocking(&self, _: &Path) -> Result<(), String> {
        self.serves.map_err(str::to_string)
    }
    fn open_issues_blocking(&self, _: &Path, limit: usize) -> Result<Vec<IssueRow>, String> {
        Ok((1..=limit as u64)
            .map(|n| IssueRow {
                issue: Issue::new(n, "t".into(), String::new()),
                author: "a".into(),
                labels: Vec::new(),
            })
            .collect())
    }
    fn my_labelled_issues_blocking(&self, _: &Path, label: &str) -> Result<Vec<Issue>, String> {
        assert!(
            !label.trim().is_empty(),
            "an empty label reached the connector"
        );
        Ok(vec![
            Issue::new(9, "b".into(), String::new()),
            Issue::new(4, "a".into(), String::new()),
        ])
    }
    fn remove_label_blocking(&self, _: &Path, _: u64, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn comment_blocking(&self, _: &Path, _: u64, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn default_branch_blocking(&self, _: &Path) -> Result<String, String> {
        unreachable!()
    }
    fn pull_request_for_blocking(&self, _: &Path, _: &str) -> Result<Option<String>, String> {
        unreachable!()
    }
    fn open_pull_request_with(&self) -> &'static str {
        "`forge pr`"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static REFUSES: Fake = Fake {
        name: "Elsewhere",
        serves: Err("its remote is on elsewhere.org"),
    };
    static ALSO_REFUSES: Fake = Fake {
        name: "Tracker",
        serves: Err("no board is set"),
    };
    static SERVES: Fake = Fake::SERVING;

    #[test]
    fn the_first_connector_that_serves_is_the_one_a_project_gets() {
        let root = std::env::temp_dir();
        assert_eq!(serving(&[&REFUSES, &SERVES, &SERVES], &root), Ok(1));
        assert_eq!(serving(&[&SERVES, &REFUSES], &root), Ok(0));
    }

    #[test]
    fn a_project_nobody_serves_hears_every_reason() {
        let root = std::env::temp_dir();
        assert_eq!(
            serving(&[&REFUSES], &root),
            Err("its remote is on elsewhere.org".to_string()),
            "one connector's refusal is said as it is"
        );
        assert_eq!(
            serving(&[&REFUSES, &ALSO_REFUSES], &root),
            Err("its remote is on elsewhere.org; no board is set".to_string())
        );
        assert!(serving(&[], &root).is_err());
    }
}
