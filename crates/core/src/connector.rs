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

use crate::issues::Snapshot;
use crate::unattended::{Issue, IssueRow};
use std::path::Path;

/// An issue as a forge holds it, for keeping a local one in step with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteIssue {
    /// What the connector identifies it by.
    pub key: String,
    /// How a person refers to it there: `#123`.
    pub reference: String,
    pub snapshot: Snapshot,
}

/// A pull request as a forge holds it now: where it is, whether it is still
/// open, what commit it is at, and what its status checks say about that commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub url: String,
    pub number: u64,
    pub state: PrState,
    pub draft: bool,
    /// The commit the pull request is at, as the forge last saw it pushed.
    pub head: String,
    /// The forge says it cannot be merged as it stands: it conflicts with its
    /// base.
    pub conflicting: bool,
    /// Every check reported on `head`, required or not.
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

/// One check a forge ran, or is running, on a pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub state: CheckState,
    /// Where the forge shows it, which is also how its log is found.
    pub link: Option<String>,
}

/// What a check says. Cancelled, timed out and anything else that did not
/// pass are all `Failed`: none of them is evidence the change works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Pending,
    Passed,
    Failed,
}

/// What a forge lists for a sync. Two lists because they answer different
/// questions: a cut in `open` only means fewer imports, while a cut in
/// `changed` means a change may be missing, and the sync has to ask about
/// each issue instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncListing {
    /// The open issues, by anybody — what a sync brings in.
    pub open: Vec<RemoteIssue>,
    /// At least every issue changed since the sync point, whatever its
    /// state; empty when there is no point.
    pub changed: Vec<RemoteIssue>,
}

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

    /// Bring `origin/<branch>` up to date at `root`. Plain `git fetch` unless a
    /// connector knows another way in — its own sign-in, for a remote whose
    /// credentials the app cannot reach.
    fn fetch_blocking(&self, root: &Path, branch: &str) -> Result<(), String> {
        crate::worktree::fetch_blocking(root, branch)
    }

    /// The pull request opened from `branch`, open or not, if there is one.
    fn pull_request_for_blocking(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<Option<PullRequest>, String>;

    /// Put `commit` on the forge as `branch`. Plain `git push` to `origin`
    /// unless a connector knows another way in, as with [`Self::fetch_blocking`].
    fn push_blocking(&self, root: &Path, commit: &str, branch: &str) -> Result<(), String> {
        crate::worktree::push_blocking(root, commit, branch)
    }

    /// Open a draft pull request from `branch` into the default branch.
    fn open_pull_request_blocking(
        &self,
        root: &Path,
        branch: &str,
        title: &str,
        body: &str,
    ) -> Result<(), String>;

    /// Take pull request `number` out of draft, so its reviewers are asked.
    fn mark_ready_blocking(&self, root: &Path, number: u64) -> Result<(), String>;

    /// The end of a failed check's log, for an agent repairing it.
    fn check_log_blocking(&self, _root: &Path, check: &Check) -> Result<String, String> {
        Err(format!("{} gives no log for {}.", self.name(), check.name))
    }

    /// What a sync needs to see, at most `limit` in each list; `changed` is
    /// asked for from `since`.
    fn issues_for_sync_blocking(
        &self,
        root: &Path,
        since: Option<u64>,
        limit: usize,
    ) -> Result<SyncListing, String>;

    /// One issue by its key, open or closed, or `None` if the forge no longer
    /// has it.
    fn issue_blocking(&self, root: &Path, key: &str) -> Result<Option<RemoteIssue>, String>;

    /// Open an issue saying what `said` says, labels included.
    fn create_issue_blocking(&self, root: &Path, said: &Snapshot) -> Result<RemoteIssue, String>;

    /// Change issue `key` from saying `from` to saying `to`, touching only what
    /// differs.
    fn update_issue_blocking(
        &self,
        root: &Path,
        key: &str,
        from: &Snapshot,
        to: &Snapshot,
    ) -> Result<(), String>;

    /// How an agent reads the review left on pull request `number`, as the
    /// words it is told to use.
    fn read_review_with(&self, number: u64) -> String;

    /// The web address of issue `key`, for a person to open or share.
    fn issue_url_blocking(&self, _root: &Path, _key: &str) -> Result<String, String> {
        Err(format!("{} gives no address for an issue.", self.name()))
    }
}

/// The connector in `connectors` called `name`, if there is one.
pub fn named(connectors: &[&'static dyn Connector], name: &str) -> Option<&'static dyn Connector> {
    connectors.iter().copied().find(|c| c.name() == name)
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

/// A connector for tests.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    /// It serves or refuses as told, lists `limit` issues, finds the issues
    /// numbered in `labelled` under any label, and fails the test if asked
    /// anything else.
    pub(crate) struct Fake {
        pub(crate) name: &'static str,
        pub(crate) serves: Result<(), &'static str>,
        pub(crate) labelled: &'static [u64],
    }

    impl Fake {
        pub(crate) const SERVING: Fake = Fake {
            name: "Forge",
            serves: Ok(()),
            labelled: &[9, 4],
        };
    }

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
            Ok(self
                .labelled
                .iter()
                .map(|&n| Issue::new(n, format!("#{n}"), String::new()))
                .collect())
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
        fn pull_request_for_blocking(
            &self,
            _: &Path,
            _: &str,
        ) -> Result<Option<PullRequest>, String> {
            unreachable!()
        }
        fn open_pull_request_blocking(
            &self,
            _: &Path,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<(), String> {
            unreachable!()
        }
        fn mark_ready_blocking(&self, _: &Path, _: u64) -> Result<(), String> {
            unreachable!()
        }
        fn read_review_with(&self, number: u64) -> String {
            format!("`forge review {number}`")
        }
        fn issues_for_sync_blocking(
            &self,
            _: &Path,
            _: Option<u64>,
            _: usize,
        ) -> Result<SyncListing, String> {
            unreachable!()
        }
        fn issue_blocking(&self, _: &Path, _: &str) -> Result<Option<RemoteIssue>, String> {
            unreachable!()
        }
        fn create_issue_blocking(&self, _: &Path, _: &Snapshot) -> Result<RemoteIssue, String> {
            unreachable!()
        }
        fn update_issue_blocking(
            &self,
            _: &Path,
            _: &str,
            _: &Snapshot,
            _: &Snapshot,
        ) -> Result<(), String> {
            unreachable!()
        }
    }
}

/// A forge kept in memory, for tests that have to watch a sync or a run move
/// things on the far side.
#[cfg(test)]
pub(crate) mod memory {
    use super::*;
    use std::sync::Mutex;

    /// A forge kept in memory, counting what was asked of it.
    #[derive(Default)]
    pub(crate) struct Forge {
        pub(crate) issues: Mutex<Vec<RemoteIssue>>,
        pub(crate) updates: Mutex<usize>,
        pub(crate) refuse_updates: bool,
        /// The keys of the issues the signed-in account wrote.
        pub(crate) mine: Vec<String>,
        /// Every comment left, as (key, body).
        pub(crate) comments: Mutex<Vec<(String, String)>>,
        /// Keys left out of the changed list, as a real forge leaves out an
        /// issue nothing touched since the sync point.
        pub(crate) unchanged: Mutex<Vec<String>>,
        /// How many times one issue was asked about by itself.
        pub(crate) lookups: Mutex<usize>,
    }

    impl Forge {
        pub(crate) fn with(issues: Vec<(u64, Snapshot)>) -> Self {
            Self {
                issues: Mutex::new(
                    issues
                        .into_iter()
                        .map(|(n, snapshot)| RemoteIssue {
                            key: n.to_string(),
                            reference: format!("#{n}"),
                            snapshot,
                        })
                        .collect(),
                ),
                ..Self::default()
            }
        }
        pub(crate) fn said(&self, key: &str) -> Snapshot {
            let issues = self.issues.lock().unwrap();
            issues
                .iter()
                .find(|r| r.key == key)
                .unwrap()
                .snapshot
                .clone()
        }
        pub(crate) fn set(&self, key: &str, said: Snapshot) {
            let mut issues = self.issues.lock().unwrap();
            issues.iter_mut().find(|r| r.key == key).unwrap().snapshot = said;
        }
    }

    impl Connector for Forge {
        fn name(&self) -> &'static str {
            "Forge"
        }
        fn account_blocking(&self) -> Result<String, String> {
            unreachable!()
        }
        fn serves_blocking(&self, _: &Path) -> Result<(), String> {
            unreachable!()
        }
        fn open_issues_blocking(&self, _: &Path, _: usize) -> Result<Vec<IssueRow>, String> {
            unreachable!()
        }
        fn my_labelled_issues_blocking(&self, _: &Path, label: &str) -> Result<Vec<Issue>, String> {
            let issues = self.issues.lock().unwrap();
            Ok(issues
                .iter()
                .filter(|r| r.snapshot.open && r.snapshot.labels.iter().any(|l| l == label))
                .filter(|r| self.mine.contains(&r.key))
                .map(|r| {
                    Issue::new(
                        r.key.parse().unwrap(),
                        r.snapshot.title.clone(),
                        String::new(),
                    )
                })
                .collect())
        }
        fn remove_label_blocking(&self, _: &Path, number: u64, label: &str) -> Result<(), String> {
            let mut issues = self.issues.lock().unwrap();
            if let Some(r) = issues.iter_mut().find(|r| r.key == number.to_string()) {
                r.snapshot.labels.retain(|l| l != label);
            }
            Ok(())
        }
        fn comment_blocking(&self, _: &Path, number: u64, body: &str) -> Result<(), String> {
            self.comments
                .lock()
                .unwrap()
                .push((number.to_string(), body.to_string()));
            Ok(())
        }
        fn default_branch_blocking(&self, _: &Path) -> Result<String, String> {
            unreachable!()
        }
        fn pull_request_for_blocking(
            &self,
            _: &Path,
            _: &str,
        ) -> Result<Option<PullRequest>, String> {
            unreachable!()
        }
        fn open_pull_request_blocking(
            &self,
            _: &Path,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<(), String> {
            unreachable!()
        }
        fn mark_ready_blocking(&self, _: &Path, _: u64) -> Result<(), String> {
            unreachable!()
        }
        fn read_review_with(&self, number: u64) -> String {
            format!("`forge review {number}`")
        }
        /// Everything it holds as changed once `since` is given — more than a
        /// real forge would list, which is allowed: what it has to list is at
        /// least what changed.
        fn issues_for_sync_blocking(
            &self,
            _: &Path,
            since: Option<u64>,
            limit: usize,
        ) -> Result<SyncListing, String> {
            let issues = self.issues.lock().unwrap();
            let open = issues.iter().filter(|r| r.snapshot.open);
            let unchanged = self.unchanged.lock().unwrap();
            let changed = issues
                .iter()
                .filter(|r| since.is_some() && !unchanged.contains(&r.key));
            Ok(SyncListing {
                open: open.take(limit).cloned().collect(),
                changed: changed.take(limit).cloned().collect(),
            })
        }
        fn issue_blocking(&self, _: &Path, key: &str) -> Result<Option<RemoteIssue>, String> {
            *self.lookups.lock().unwrap() += 1;
            let issues = self.issues.lock().unwrap();
            Ok(issues.iter().find(|r| r.key == key).cloned())
        }
        fn create_issue_blocking(&self, _: &Path, said: &Snapshot) -> Result<RemoteIssue, String> {
            let mut issues = self.issues.lock().unwrap();
            let n = 100 + issues.len() as u64;
            let made = RemoteIssue {
                key: n.to_string(),
                reference: format!("#{n}"),
                snapshot: said.clone(),
            };
            issues.push(made.clone());
            Ok(made)
        }
        fn update_issue_blocking(
            &self,
            _: &Path,
            key: &str,
            _: &Snapshot,
            to: &Snapshot,
        ) -> Result<(), String> {
            if self.refuse_updates {
                return Err("offline".to_string());
            }
            *self.updates.lock().unwrap() += 1;
            let mut issues = self.issues.lock().unwrap();
            issues.iter_mut().find(|r| r.key == key).unwrap().snapshot = to.clone();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    static REFUSES: Fake = Fake {
        name: "Elsewhere",
        serves: Err("its remote is on elsewhere.org"),
        labelled: &[],
    };
    static ALSO_REFUSES: Fake = Fake {
        name: "Tracker",
        serves: Err("no board is set"),
        labelled: &[],
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
