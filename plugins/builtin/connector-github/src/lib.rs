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

use onehand_core::connector::{
    Check, CheckState, Connector, PrState, PullRequest, RemoteIssue, SyncListing,
};
use onehand_core::issues::Snapshot;
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
            Account::SignedIn(login) => Ok(format!("Signed in as {login}, through gh")),
            other => Err(other.problem().unwrap_or_default()),
        }
    }

    /// A repository whose `origin` is on GitHub. Read locally, before anything
    /// asks GitHub, so a project that can never be worked costs nothing per
    /// tick but this.
    fn serves_blocking(&self, root: &Path) -> Result<(), String> {
        github_remote(&origin_url(root)?, ssh_resolve_blocking)
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

    fn my_labelled_issues_blocking(
        &self,
        root: &Path,
        label: &str,
    ) -> Result<Vec<IssueRow>, String> {
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
                "number,title,body,author,labels",
            ],
        )?;
        issue_rows(&json)
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
    ) -> Result<Option<PullRequest>, String> {
        // `--state all`, because a PR merged or closed before the run is
        // settled is still the answer to "did it open one". The checks come in
        // the same call, so they are always the checks on the head it reports.
        let json = gh(
            root,
            &[
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                "all",
                "--limit",
                "1",
                "--json",
                "url,number,state,isDraft,headRefOid,mergeable,statusCheckRollup",
            ],
        )?;
        pull_request(&json)
    }

    fn mark_ready_blocking(&self, root: &Path, number: u64) -> Result<(), String> {
        gh(root, &["pr", "ready", &number.to_string()]).map(drop)
    }

    /// The log of an Actions job, its last lines only: the failure is at the
    /// end, and a whole log is more than any prompt should carry. Asked of
    /// the job rather than its run, because `gh run view` holds every log
    /// back until the whole run is over, and a job that failed fast is
    /// repaired while its slower siblings still run. A status check that is
    /// not an Actions job has no log `gh` can read.
    fn check_log_blocking(&self, root: &Path, check: &Check) -> Result<String, String> {
        let job = check
            .link
            .as_deref()
            .and_then(|link| link.rsplit_once("/job/"))
            .map(|(_, job)| job.split(['?', '#', '/']).next().unwrap_or(job))
            .filter(|job| !job.is_empty() && job.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| {
                format!(
                    "{} is not an Actions job, so it has no log here.",
                    check.name
                )
            })?;
        // A job's log is coloured for a terminal, and `gh` refuses to print
        // escape sequences unless asked; they are stripped here instead, so
        // none reaches a prompt.
        let log = gh(
            root,
            &[
                "api",
                "--allow-escape-sequences",
                &format!("repos/{{owner}}/{{repo}}/actions/jobs/{job}/logs"),
            ],
        )?;
        Ok(last_lines(&without_escapes(&log), LOG_LINES))
    }

    fn read_review_with(&self, number: u64) -> String {
        format!(
            "`gh pr view {number} --comments` and `gh api repos/{{owner}}/{{repo}}/pulls/{number}/comments`"
        )
    }

    fn open_pull_request_blocking(
        &self,
        root: &Path,
        branch: &str,
        title: &str,
        body: &str,
    ) -> Result<(), String> {
        gh(
            root,
            &[
                "pr", "create", "--draft", "--head", branch, "--title", title, "--body", body,
            ],
        )
        .map(drop)
    }

    /// `git push` as ever, and, when that fails on an ssh `origin`, over HTTPS
    /// with `gh`'s own sign-in, for the reason [`Self::fetch_blocking`] gives.
    /// A push the forge turned down, as one behind the branch it would
    /// replace, is its answer whichever way it went, and is not tried again.
    fn push_blocking(&self, root: &Path, commit: &str, branch: &str) -> Result<(), String> {
        onehand_core::worktree::push_blocking(root, commit, branch).or_else(|over_origin| {
            if turned_down(&over_origin) {
                return Err(over_origin);
            }
            over_https_blocking(root, over_origin, "push", |url| {
                https_args("push", url, format!("{commit}:refs/heads/{branch}"))
            })
        })
    }

    fn issue_url_blocking(&self, root: &Path, key: &str) -> Result<String, String> {
        let url = gh(root, &["issue", "view", key, "--json", "url", "-q", ".url"])?;
        if url.is_empty() {
            Err(format!("GitHub gave no address for issue {key}."))
        } else {
            Ok(url)
        }
    }

    /// `git fetch` as ever, and, when that fails on an ssh `origin`, the same
    /// repository again over HTTPS with `gh`'s own sign-in. An ssh remote fails
    /// for an app opened from the desktop far more often than for a terminal —
    /// no agent reachable, a key behind a passphrase nobody is there to type —
    /// while the person running it has already signed `gh` in, which is all
    /// HTTPS needs. An https `origin` is not tried twice: there is nothing
    /// different to try.
    fn fetch_blocking(&self, root: &Path, branch: &str) -> Result<(), String> {
        onehand_core::worktree::fetch_blocking(root, branch).or_else(|over_origin| {
            over_https_blocking(root, over_origin, "fetch", |url| {
                https_fetch_args(url, branch)
            })
        })
    }

    /// The open issues, then every issue changed since `since` — asked
    /// from a day before it, because the search takes a day and not a moment,
    /// and two clocks never quite agree. Listing more than changed costs a
    /// comparison; listing less loses an edit.
    fn issues_for_sync_blocking(
        &self,
        root: &Path,
        since: Option<u64>,
        limit: usize,
    ) -> Result<SyncListing, String> {
        let limit = limit.to_string();
        let list = |state: &str, search: Option<String>| {
            let search = search.map(|day| format!("updated:>={day}"));
            let mut args = vec!["issue", "list", "--state", state, "--limit", &limit];
            if let Some(search) = &search {
                args.extend(["--search", search.as_str()]);
            }
            args.extend(["--json", SYNC_FIELDS]);
            gh(root, &args).and_then(|json| remote_issues(&json))
        };
        // `all` and not `closed`: an issue reopened there is a change too, and
        // it may be past the end of a cut open list.
        let changed = match since {
            Some(since) => list("all", Some(day_of(since.saturating_sub(86_400))))?,
            None => Vec::new(),
        };
        Ok(SyncListing {
            open: list("open", None)?,
            changed,
        })
    }

    fn issue_blocking(&self, root: &Path, key: &str) -> Result<Option<RemoteIssue>, String> {
        match gh(root, &["issue", "view", key, "--json", SYNC_FIELDS]) {
            Ok(json) => remote_issues(&format!("[{json}]")).map(|mut found| found.pop()),
            // `gh` says an issue that is not there in words rather than with a
            // code of its own, and gone is an answer, not a failure.
            Err(why) if why.contains("Could not resolve to an issue") => Ok(None),
            Err(why) => Err(why),
        }
    }

    fn create_issue_blocking(&self, root: &Path, said: &Snapshot) -> Result<RemoteIssue, String> {
        let printed = gh(
            root,
            &[
                "issue",
                "create",
                "--title",
                &said.title,
                "--body",
                &said.body,
            ],
        )?;
        let number = created_number(&printed)
            .ok_or_else(|| format!("gh created an issue and did not say which: {printed}"))?;
        let key = number.to_string();
        // Created bare and then brought up to what was asked for, so a label
        // that does not exist on the repository yet is made rather than
        // failing the whole issue.
        let bare = Snapshot {
            title: said.title.clone(),
            body: said.body.clone(),
            open: true,
            labels: Vec::new(),
        };
        // The issue exists from here on, so it is handed back whatever happens
        // next: failing now would leave it on GitHub with nothing linked to
        // it, and the next press of Publish would make a second. What it says
        // is what it was brought up to, and a sync sends the rest.
        let snapshot = match self.update_issue_blocking(root, &key, &bare, said) {
            Ok(()) => said.clone(),
            Err(_) => bare,
        };
        Ok(RemoteIssue {
            key,
            reference: format!("#{number}"),
            snapshot,
        })
    }

    fn update_issue_blocking(
        &self,
        root: &Path,
        key: &str,
        from: &Snapshot,
        to: &Snapshot,
    ) -> Result<(), String> {
        if let Some(args) = edit_args(key, from, to) {
            for label in to.labels.iter().filter(|l| !from.labels.contains(l)) {
                // A label has to exist on the repository before an issue can
                // carry it. Made here, and "already exists" is the common,
                // harmless answer.
                let _ = gh(root, &["label", "create", label]);
            }
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            gh(root, &args)?;
        }
        if from.open != to.open {
            gh(
                root,
                &["issue", if to.open { "reopen" } else { "close" }, key],
            )?;
        }
        Ok(())
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

/// How many lines of a failed job's log an agent is handed.
const LOG_LINES: usize = 80;

/// `text` without terminal escape sequences (`ESC [ … letter`, and any other
/// `ESC` with the character after it) or other control characters but line
/// breaks and tabs.
fn without_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                if chars.next() == Some('[') {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The last `n` lines of `text`.
fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `gh pr list --json url,number,state,isDraft,headRefOid,mergeable,statusCheckRollup`
/// as the first pull request in it, if any.
///
/// A rollup holds two kinds of entry: an Actions check run, which has a
/// status and, once completed, a conclusion; and a commit status, which has a
/// state alone. Only success, neutral and skipped pass — a cancelled or timed
/// out check is no evidence the change works.
fn pull_request(json: &str) -> Result<Option<PullRequest>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        context: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        conclusion: Option<String>,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        details_url: Option<String>,
        #[serde(default)]
        target_url: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        url: String,
        number: u64,
        state: String,
        is_draft: bool,
        head_ref_oid: String,
        #[serde(default)]
        mergeable: String,
        #[serde(default)]
        status_check_rollup: Vec<Entry>,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let checks = row
        .status_check_rollup
        .into_iter()
        .map(|e| {
            let state = match (
                e.status.as_deref(),
                e.conclusion.as_deref(),
                e.state.as_deref(),
            ) {
                (Some(status), _, _) if status != "COMPLETED" => CheckState::Pending,
                (Some(_), Some("SUCCESS" | "NEUTRAL" | "SKIPPED"), _) => CheckState::Passed,
                (Some(_), _, _) => CheckState::Failed,
                (None, _, Some("SUCCESS")) => CheckState::Passed,
                (None, _, Some("PENDING" | "EXPECTED") | None) => CheckState::Pending,
                (None, _, Some(_)) => CheckState::Failed,
            };
            Check {
                name: e.name.or(e.context).unwrap_or_default(),
                state,
                link: e.details_url.or(e.target_url).filter(|l| !l.is_empty()),
            }
        })
        .collect();
    Ok(Some(PullRequest {
        url: row.url,
        number: row.number,
        state: match row.state.as_str() {
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ => PrState::Open,
        },
        draft: row.is_draft,
        head: row.head_ref_oid,
        conflicting: row.mergeable == "CONFLICTING",
        checks,
    }))
}

/// What a sync reads of an issue.
const SYNC_FIELDS: &str = "number,title,body,state,labels";

/// `gh issue list --json` with [`SYNC_FIELDS`] as issues a sync can compare.
fn remote_issues(json: &str) -> Result<Vec<RemoteIssue>, String> {
    #[derive(Deserialize)]
    struct Label {
        name: String,
    }
    #[derive(Deserialize)]
    struct Row {
        number: u64,
        title: String,
        #[serde(default)]
        body: String,
        state: String,
        #[serde(default)]
        labels: Vec<Label>,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(rows
        .into_iter()
        .map(|row| RemoteIssue {
            key: row.number.to_string(),
            reference: format!("#{}", row.number),
            snapshot: Snapshot {
                title: row.title,
                body: row.body,
                open: row.state.eq_ignore_ascii_case("open"),
                labels: row.labels.into_iter().map(|l| l.name).collect(),
            }
            .normalized(),
        })
        .collect())
}

/// The day `secs` since the epoch falls on, as `YYYY-MM-DD` in UTC — the form a
/// `gh` search's `updated:` takes. The civil-from-days count, written out
/// rather than taken from a date library for the one place it is needed.
fn day_of(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The number of the issue `gh issue create` just made: its URL is the last
/// thing it prints, and the number is the URL's last part.
fn created_number(printed: &str) -> Option<u64> {
    printed
        .lines()
        .rev()
        .find(|line| line.contains("/issues/"))?
        .trim()
        .rsplit('/')
        .next()?
        .parse()
        .ok()
}

/// The `gh issue edit` that takes issue `key` from `from` to `to`, sending only
/// the fields that differ, or `None` when nothing it carries does. Open and
/// closed are not its to change — that is `gh issue close` and `reopen`.
fn edit_args(key: &str, from: &Snapshot, to: &Snapshot) -> Option<Vec<String>> {
    let mut args: Vec<String> = ["issue", "edit", key].map(String::from).to_vec();
    if from.title != to.title {
        args.extend(["--title".into(), to.title.clone()]);
    }
    if from.body != to.body {
        args.extend(["--body".into(), to.body.clone()]);
    }
    let added: Vec<&str> = to
        .labels
        .iter()
        .filter(|l| !from.labels.contains(l))
        .map(String::as_str)
        .collect();
    let removed: Vec<&str> = from
        .labels
        .iter()
        .filter(|l| !to.labels.contains(l))
        .map(String::as_str)
        .collect();
    if !added.is_empty() {
        args.extend(["--add-label".into(), added.join(",")]);
    }
    if !removed.is_empty() {
        args.extend(["--remove-label".into(), removed.join(",")]);
    }
    (args.len() > 3).then_some(args)
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

/// The `git` arguments that fetch `branch` from `url` into `origin/<branch>`,
/// signed in by `gh` alone: the empty helper first clears any the user set,
/// so nothing else is asked and nothing waits on a prompt.
fn https_fetch_args(url: &str, branch: &str) -> Vec<String> {
    https_args(
        "fetch",
        url,
        format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
    )
}

/// Whether git's complaint about a push is the remote refusing the commit,
/// rather than never being reached.
fn turned_down(said: &str) -> bool {
    said.contains("[rejected]") || said.contains("[remote rejected]")
}

/// `git <verb>` of `refspec` with `url`, signed in by `gh` alone.
fn https_args(verb: &str, url: &str, refspec: String) -> Vec<String> {
    vec![
        "-c".into(),
        "credential.helper=".into(),
        "-c".into(),
        "credential.helper=!gh auth git-credential".into(),
        verb.into(),
        "--quiet".into(),
        url.into(),
        refspec,
    ]
}

/// After `git <verb>` failed over `origin` for `over_origin`, the same again
/// over HTTPS with `gh`'s sign-in when `origin` is an ssh remote, its
/// arguments `args` of the HTTPS address. An https `origin` is not tried
/// twice: there is nothing different to try.
fn over_https_blocking(
    root: &Path,
    over_origin: String,
    verb: &str,
    args: impl FnOnce(&str) -> Vec<String>,
) -> Result<(), String> {
    let Some(url) = origin_url(root)
        .ok()
        .and_then(|origin| https_url(&origin, ssh_resolve_blocking))
    else {
        return Err(over_origin);
    };
    let out = onehand_core::process::output_within(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args(&url))
            .env("GIT_TERMINAL_PROMPT", "0"),
        onehand_core::worktree::FETCH_LIMIT,
    );
    let why = match out {
        Ok(out) if out.status.success() => return Ok(()),
        Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
        Err(err) => format!("git {verb} {err}"),
    };
    Err(format!(
        "{over_origin} — and over HTTPS with gh's sign-in: {why}"
    ))
}

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

/// The URL of `root`'s `origin`, read locally.
fn origin_url(root: &Path) -> Result<String, String> {
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
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The HTTPS URL of the repository an ssh remote `url` names, on the host ssh
/// would actually reach — an alias such as `github-work` is `resolve`d first.
/// `None` for a remote that is not over ssh, where there is nothing different
/// to try.
fn https_url(url: &str, resolve: impl Fn(&str) -> Option<String>) -> Option<String> {
    if !over_ssh(url) {
        return None;
    }
    let host = remote_host(url)?;
    let path = match url.split_once("://") {
        // `ssh://user@host[:port]/path`: everything after the host.
        Some((_, rest)) => rest.split_once('/')?.1,
        // `user@host:path`.
        None => url.split_once(':')?.1,
    };
    let host = resolve(host).unwrap_or_else(|| host.to_string());
    Some(format!("https://{host}/{}", path.trim_start_matches('/')))
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
    fn a_job_log_loses_its_colours() {
        assert_eq!(
            without_escapes("\u{1b}[36;1mok\u{1b}[0m\r\nerror: \u{1b}[31mboom\u{1b}[0m\n"),
            "ok\nerror: boom\n"
        );
    }

    #[test]
    fn a_push_turned_down_is_not_tried_over_https() {
        assert!(turned_down(
            "! [rejected] abc -> onehand/x (non-fast-forward)\nfailed to push some refs"
        ));
        assert!(!turned_down("Permission denied (publickey)."));
    }

    #[test]
    fn a_rollup_reads_as_checks_that_pass_only_on_success() {
        let json = r#"[{"url":"u","number":7,"state":"OPEN","isDraft":true,"headRefOid":"abc",
            "mergeable":"CONFLICTING","statusCheckRollup":[
            {"__typename":"CheckRun","name":"Build","status":"COMPLETED","conclusion":"SUCCESS",
             "detailsUrl":"https://github.com/o/r/actions/runs/1/job/42"},
            {"__typename":"CheckRun","name":"Lint","status":"COMPLETED","conclusion":"CANCELLED"},
            {"__typename":"CheckRun","name":"Test","status":"IN_PROGRESS","conclusion":""},
            {"__typename":"StatusContext","context":"ci/ext","state":"PENDING","targetUrl":""},
            {"__typename":"StatusContext","context":"ci/old","state":"ERROR"}]}]"#;
        let pr = pull_request(json).unwrap().unwrap();
        assert_eq!((pr.number, pr.state, pr.draft), (7, PrState::Open, true));
        assert!(pr.conflicting);
        let states: Vec<_> = pr
            .checks
            .iter()
            .map(|c| (c.name.as_str(), c.state))
            .collect();
        assert_eq!(
            states,
            [
                ("Build", CheckState::Passed),
                ("Lint", CheckState::Failed),
                ("Test", CheckState::Pending),
                ("ci/ext", CheckState::Pending),
                ("ci/old", CheckState::Failed),
            ]
        );
        assert_eq!(pr.checks[3].link, None, "an empty link is no link");
        assert_eq!(pull_request("[]").unwrap(), None);
    }

    #[test]
    fn an_ssh_origin_is_fetched_over_https_from_the_host_ssh_would_reach() {
        let alias = |host: &str| (host == "github-work").then(|| "github.com".to_string());
        assert_eq!(
            https_url("git@github-work:me/repo.git", alias).as_deref(),
            Some("https://github.com/me/repo.git")
        );
        assert_eq!(
            https_url("ssh://git@github.com:22/me/repo.git", |_| None).as_deref(),
            Some("https://github.com/me/repo.git")
        );
        // Already https: there is nothing different to try.
        assert_eq!(https_url("https://github.com/me/repo", |_| None), None);
    }

    #[test]
    fn a_fetch_over_https_uses_only_ghs_sign_in_and_lands_where_origin_would() {
        let args = https_fetch_args("https://github.com/a/b", "main");
        assert_eq!(
            args,
            [
                "-c",
                "credential.helper=",
                "-c",
                "credential.helper=!gh auth git-credential",
                "fetch",
                "--quiet",
                "https://github.com/a/b",
                "+refs/heads/main:refs/remotes/origin/main",
            ]
        );
    }

    #[test]
    fn a_moment_is_named_by_its_day_the_way_a_search_takes_it() {
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(951_782_400), "2000-02-29");
        assert_eq!(day_of(1_700_000_000), "2023-11-14");
    }

    fn said(title: &str, body: &str, open: bool, labels: &[&str]) -> Snapshot {
        Snapshot {
            title: title.into(),
            body: body.into(),
            open,
            labels: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn issues_for_sync_are_read_with_their_state_and_labels() {
        let json = r#"[{"number":7,"title":"Crash","body":"a\r\nb","state":"OPEN","labels":[{"name":"bug"}]},
                       {"number":8,"title":"Old","state":"CLOSED","labels":[]}]"#;
        let found = remote_issues(json).unwrap();
        assert_eq!(found[0].key, "7");
        assert_eq!(found[0].reference, "#7");
        assert_eq!(found[0].snapshot, said("Crash", "a\nb", true, &["bug"]));
        assert!(!found[1].snapshot.open);
        assert!(remote_issues("nope").is_err());
    }

    #[test]
    fn a_created_issue_is_known_by_the_number_its_url_ends_in() {
        assert_eq!(
            created_number("Creating issue\nhttps://github.com/a/b/issues/123\n"),
            Some(123)
        );
        assert_eq!(created_number("nothing useful"), None);
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let from = said("a", "x", true, &["bug", "old"]);
        assert_eq!(edit_args("7", &from, &from), None);
        let to = said("b", "x", true, &["bug", "new"]);
        assert_eq!(
            edit_args("7", &from, &to).unwrap(),
            [
                "issue",
                "edit",
                "7",
                "--title",
                "b",
                "--add-label",
                "new",
                "--remove-label",
                "old"
            ]
        );
        let only_body = said("a", "y", true, &["bug", "old"]);
        assert_eq!(
            edit_args("7", &from, &only_body).unwrap(),
            ["issue", "edit", "7", "--body", "y"]
        );
    }

    #[test]
    fn labelled_issues_are_read_with_their_labels_and_nothing_unreadable_passes() {
        let json = r#"[{"number":9,"title":"b","body":"","author":{"login":"me"},
            "labels":[{"name":"auto"},{"name":"bug"}]},
            {"number":4,"title":"a","author":{"login":"me"}}]"#;
        let found = issue_rows(json).unwrap();
        let numbers: Vec<_> = found.iter().map(|row| row.issue.number).collect();
        assert_eq!(numbers, [9, 4]);
        assert_eq!(found[0].labels, ["auto", "bug"]);
        assert_eq!(found[1].issue.title_text(), "a");
        assert_eq!(issue_rows("[]"), Ok(Vec::new()));
        assert!(issue_rows("not json").is_err());
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
