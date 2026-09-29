//! GitHub, as a connector: issues and pull requests through the GitHub CLI.
//!
//! **`gh` is the whole API layer.** It carries the authentication, the API
//! version and the JSON, so there is no HTTP client here and no token in any
//! config. Every call is blocking and runs in the directory of the project it
//! is about, because that is how `gh` knows which repository is meant.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use onehand_core::connector::Connector;
use onehand_core::unattended::{Issue, IssueRow};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The GitHub connector. It holds nothing: `gh` holds the account, and the
/// checkout says which repository.
pub struct GitHub;

impl Connector for GitHub {
    fn name(&self) -> &'static str {
        "GitHub"
    }

    fn account_blocking(&self) -> Result<String, String> {
        match account_blocking() {
            Account::SignedIn(login) => Ok(format!("signed in as {login}, through `gh`")),
            other => Err(other.problem().unwrap_or_default()),
        }
    }

    /// A repository whose `origin` is on GitHub. Read locally, before anything
    /// asks GitHub, so a project that can never be worked costs nothing per
    /// tick but this.
    fn serves_blocking(&self, root: &Path) -> Result<(), String> {
        let out = onehand_core::process::output_within(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["remote", "get-url", "origin"]),
            LOCAL_LIMIT,
        )
        .map_err(|err| format!("git {err}"))?;
        if !out.status.success() {
            return Err("it has no `origin` remote to open a pull request against".to_string());
        }
        github_remote(
            String::from_utf8_lossy(&out.stdout).trim(),
            ssh_resolve_blocking,
        )
    }

    fn open_issues_blocking(&self, root: &Path, limit: usize) -> Result<Vec<IssueRow>, String> {
        let limit = limit.to_string();
        let json = gh(
            root,
            &[
                "issue",
                "list",
                "--state",
                "open",
                "--limit",
                &limit,
                "--json",
                "number,title,body,author,labels",
            ],
        )?;
        issue_rows(&json)
    }

    fn my_labelled_issues_blocking(&self, root: &Path, label: &str) -> Result<Vec<Issue>, String> {
        let json = gh(
            root,
            &[
                "issue",
                "list",
                "--state",
                "open",
                "--author",
                "@me",
                "--label",
                label,
                "--limit",
                "50",
                "--json",
                "number,title,body",
            ],
        )?;
        issues(&json)
    }

    fn remove_label_blocking(&self, root: &Path, number: u64, label: &str) -> Result<(), String> {
        let n = number.to_string();
        gh(root, &["issue", "edit", &n, "--remove-label", label]).map(drop)
    }

    fn comment_blocking(&self, root: &Path, number: u64, body: &str) -> Result<(), String> {
        gh(
            root,
            &["issue", "comment", &number.to_string(), "--body", body],
        )
        .map(drop)
    }

    fn default_branch_blocking(&self, root: &Path) -> Result<String, String> {
        let name = gh(
            root,
            &[
                "repo",
                "view",
                "--json",
                "defaultBranchRef",
                "-q",
                ".defaultBranchRef.name",
            ],
        )?;
        if name.is_empty() {
            Err("GitHub named no default branch for this repository.".to_string())
        } else {
            Ok(name)
        }
    }

    fn pull_request_for_blocking(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<Option<String>, String> {
        // `--state all`, because a PR merged or closed before the run is
        // settled is still the answer to "did it open one".
        let url = gh(
            root,
            &[
                "pr", "list", "--head", branch, "--state", "all", "--json", "url", "-q", ".[0].url",
            ],
        )?;
        Ok((!url.is_empty()).then_some(url))
    }

    fn open_pull_request_with(&self) -> &'static str {
        "`gh pr create`"
    }
}

/// One issue as `gh --json number,title,body` prints it. The wire shape is
/// this plugin's to know; what leaves it is core's [`Issue`].
#[derive(Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
}

impl From<GhIssue> for Issue {
    fn from(gh: GhIssue) -> Self {
        Issue::new(gh.number, gh.title, gh.body)
    }
}

/// `gh issue list --json number,title,body` as issues.
fn issues(json: &str) -> Result<Vec<Issue>, String> {
    let found: Vec<GhIssue> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(found.into_iter().map(Issue::from).collect())
}

/// `gh issue list --json number,title,body,author,labels` as rows.
fn issue_rows(json: &str) -> Result<Vec<IssueRow>, String> {
    #[derive(Deserialize)]
    struct Author {
        login: String,
    }
    #[derive(Deserialize)]
    struct Label {
        name: String,
    }
    #[derive(Deserialize)]
    struct Row {
        #[serde(flatten)]
        issue: GhIssue,
        author: Author,
        #[serde(default)]
        labels: Vec<Label>,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(rows
        .into_iter()
        .map(|row| IssueRow {
            issue: row.issue.into(),
            author: row.author.login,
            labels: row.labels.into_iter().map(|label| label.name).collect(),
        })
        .collect())
}

/// How long one `gh` call may take. Each is one API request; one that has not
/// answered in this long is stuck, not slow.
const GH_LIMIT: Duration = Duration::from_secs(60);

/// How long a question answered from this machine alone may take — the
/// project's remote, what an ssh alias stands for. Seconds is already slow.
const LOCAL_LIMIT: Duration = Duration::from_secs(10);

/// Run `gh` in `root` and hand back what it printed.
fn gh(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = onehand_core::process::output_within(
        Command::new("gh")
            .args(args)
            .current_dir(root)
            .env("GH_PROMPT_DISABLED", "1"),
        GH_LIMIT,
    )
    .map_err(|err| format!("gh {}: {err}", args[0]))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if said.is_empty() {
            format!("gh {} failed and said nothing about why.", args[0])
        } else {
            said
        })
    }
}

/// What `gh` says about being signed in. Kept as its four cases rather than
/// read straight into a sentence, because each one has a different fix.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Account {
    /// `gh` is signed in, as this login.
    SignedIn(String),
    /// `gh` is not installed.
    Missing,
    /// `gh` is installed and signed in to nothing.
    SignedOut,
    /// `gh` could not get an answer from GitHub.
    Unreachable(String),
}

impl Account {
    /// Why no run can happen, in words that say what to do about it; `None`
    /// when one can.
    fn problem(&self) -> Option<String> {
        match self {
            Self::SignedIn(_) => None,
            Self::Missing => Some(
                "the GitHub CLI (`gh`) is not installed, and unattended runs reach GitHub \
                 through it"
                    .to_string(),
            ),
            Self::SignedOut => {
                Some("`gh` is not signed in to GitHub — run `gh auth login`".to_string())
            }
            Self::Unreachable(why) => Some(format!("could not reach GitHub: {why}")),
        }
    }
}

/// Ask `gh` who it is signed in as. Blocking, and bounded: one API request
/// that has not answered in a minute is stuck, not slow.
fn account_blocking() -> Account {
    // A `gh` that is not installed fails to start at all, which the bounded
    // runner reports as its own kind of failure rather than as a slow answer.
    let out = match onehand_core::process::output_within(
        Command::new("gh")
            .args(["api", "user", "--jq", ".login"])
            .env("GH_PROMPT_DISABLED", "1"),
        GH_LIMIT,
    ) {
        Ok(out) => out,
        Err(onehand_core::process::Failure::Missing) => return Account::Missing,
        Err(why) => return Account::Unreachable(format!("gh {why}")),
    };
    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.code() {
        Some(0) if !login.is_empty() => Account::SignedIn(login),
        // `gh` exits 4 when a command needs a login it does not have.
        Some(4) => Account::SignedOut,
        _ => Account::Unreachable(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("gh gave no reason")
                .trim()
                .to_string(),
        ),
    }
}

/// The host a git remote URL points at: `https://host/…`, `ssh://user@host:port/…`
/// and the scp-like `user@host:path`. `None` for a local path.
fn remote_host(url: &str) -> Option<&str> {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest,
        // scp-like: a colon before any slash, and the part before it is a host.
        None => match url.split_once(':') {
            Some((host, _)) if !host.contains('/') => host,
            _ => return None,
        },
    };
    let host = rest.split('/').next()?;
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// Whether `url` is a remote a run can work: one on github.com.
///
/// **An ssh remote is judged by the host ssh would actually reach**, which
/// `resolve` answers. The host in an ssh URL can be an alias from the user's ssh
/// configuration — `git@github-work:me/repo`, which is how one machine keeps
/// two GitHub accounts apart — and reading the word in the URL refused every
/// such project as not being on GitHub. An https remote has no alias and is
/// read as written.
fn github_remote(url: &str, resolve: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    let refuse = |host: &str| Err(format!("its remote is on {host}, not GitHub"));
    let Some(host) = remote_host(url) else {
        return Err("its remote is not on GitHub".to_string());
    };
    if host == "github.com" {
        return Ok(());
    }
    if !over_ssh(url) {
        return refuse(host);
    }
    match resolve(host) {
        Some(real) if real == "github.com" => Ok(()),
        Some(real) => refuse(&real),
        None => refuse(host),
    }
}

/// Whether `url` reaches its host over ssh: the scp-like form, or an `ssh`
/// scheme.
fn over_ssh(url: &str) -> bool {
    match url.split_once("://") {
        Some((scheme, _)) => scheme.contains("ssh"),
        None => true,
    }
}

/// The `hostname` line of what `ssh -G` printed: the host an alias stands for,
/// after the user's ssh configuration has been applied.
fn ssh_hostname(said: &str) -> Option<&str> {
    said.lines()
        .find_map(|line| line.strip_prefix("hostname "))
        .map(str::trim)
}

/// Ask ssh which host `alias` stands for. `ssh -G` only prints the settled
/// configuration; it connects to nothing.
fn ssh_resolve_blocking(alias: &str) -> Option<String> {
    let out =
        onehand_core::process::output_within(Command::new("ssh").arg("-G").arg(alias), LOCAL_LIMIT)
            .ok()?;
    out.status.success().then_some(())?;
    ssh_hostname(&String::from_utf8_lossy(&out.stdout)).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labelled_issues_are_read_and_nothing_unreadable_passes() {
        let json = r#"[{"number":9,"title":"b","body":""},{"number":4,"title":"a"}]"#;
        let found = issues(json).unwrap();
        assert_eq!(found.iter().map(|i| i.number).collect::<Vec<_>>(), [9, 4]);
        assert_eq!(found[1].title_text(), "a");
        assert_eq!(issues("[]"), Ok(Vec::new()));
        assert!(issues("not json").is_err());
    }

    #[test]
    fn a_remote_names_its_host_in_every_form_git_accepts() {
        for url in [
            "https://github.com/jarviisha/onehand.git",
            "https://user:token@github.com/jarviisha/onehand",
            "git@github.com:jarviisha/onehand.git",
            "ssh://git@github.com/jarviisha/onehand.git",
            "ssh://git@github.com:22/jarviisha/onehand.git",
        ] {
            assert_eq!(remote_host(url), Some("github.com"), "{url}");
        }
        assert_eq!(remote_host("git@gitlab.com:a/b.git"), Some("gitlab.com"));
        assert_eq!(remote_host("/srv/git/local.git"), None);
    }

    #[test]
    fn only_a_github_remote_can_be_worked() {
        let none = |_: &str| None;
        assert_eq!(github_remote("https://github.com/a/b", none), Ok(()));
        let why = github_remote("git@gitlab.com:a/b.git", none).unwrap_err();
        assert!(
            why.contains("gitlab.com") && why.contains("not GitHub"),
            "{why}"
        );
        assert!(github_remote("/srv/git/local.git", none).is_err());
    }

    /// An ssh host alias — how one machine keeps two GitHub accounts apart — is
    /// whatever ssh's own configuration says it is, not the word in the URL.
    #[test]
    fn an_ssh_alias_is_judged_by_the_host_it_stands_for() {
        let config = |host: &str| (host == "github-work").then(|| "github.com".to_string());
        assert_eq!(github_remote("git@github-work:me/repo.git", config), Ok(()));
        assert_eq!(
            github_remote("ssh://git@github-work/me/repo.git", config),
            Ok(())
        );
        // An alias for somewhere else is still somewhere else, and says where.
        let elsewhere = |_: &str| Some("gitlab.com".to_string());
        let why = github_remote("git@work:me/repo.git", elsewhere).unwrap_err();
        assert!(why.contains("gitlab.com"), "{why}");
        // https has no alias to resolve.
        let never = |_: &str| -> Option<String> { panic!("https is not resolved through ssh") };
        assert!(github_remote("https://example.com/a/b", never).is_err());
    }

    #[test]
    fn ssh_names_the_host_an_alias_resolves_to() {
        let said = "user git\nhostname github.com\nport 22\n";
        assert_eq!(ssh_hostname(said), Some("github.com"));
        assert_eq!(ssh_hostname("port 22\n"), None);
    }

    #[test]
    fn a_github_account_that_cannot_be_used_says_what_to_do() {
        assert_eq!(Account::SignedIn("me".into()).problem(), None);
        assert!(
            Account::Missing
                .problem()
                .unwrap()
                .contains("not installed")
        );
        assert!(
            Account::SignedOut
                .problem()
                .unwrap()
                .contains("gh auth login"),
            "the fix is named"
        );
        assert!(
            Account::Unreachable("timeout".into())
                .problem()
                .unwrap()
                .contains("timeout")
        );
    }

    #[test]
    fn the_open_issues_are_read_with_who_wrote_them_and_what_they_carry() {
        let json = r#"[
            {"number":7,"title":"Crash","body":"x","author":{"login":"stranger"},
             "labels":[{"name":"bug"},{"name":"auto"}]},
            {"number":3,"title":"Typo","body":"","author":{"login":"me"},"labels":[]}
        ]"#;
        let rows = issue_rows(json).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].issue.number, 7);
        assert_eq!(rows[0].issue.title_text(), "Crash");
        assert_eq!(rows[0].author, "stranger");
        assert_eq!(rows[0].labels, ["bug", "auto"]);
        assert!(rows[0].carries("auto") && !rows[1].carries("auto"));
        assert!(issue_rows("nope").is_err());
    }
}
