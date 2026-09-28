//! Unattended runs: one small issue, one session, one pull request.
//!
//! Everything about a run that can be decided without a window — which issue,
//! what branch, what the prompt says, what the issue is told afterwards — and
//! the handful of `gh` calls a run makes. The app holds the rest: the timer,
//! the session, the watching.
//!
//! **`gh` is the whole API layer.** It carries the authentication, the API
//! version and the JSON, so there is no HTTP client here and no token in any
//! config. Every call is blocking and runs in the directory of the project it
//! is about, because that is how `gh` knows which repository is meant.

use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// An issue a run can take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issue {
    pub number: u64,
    title: String,
    #[serde(default)]
    body: String,
}

impl Issue {
    /// What the issue is called.
    pub fn title_text(&self) -> &str {
        &self.title
    }
}

/// An open issue as the picker lists it: the issue, who opened it, and what it
/// is labelled.
///
/// **Who opened it is on every row**, because the body goes into the prompt
/// word for word and the agent may run `git` and `gh` with the user's
/// credentials. The automatic search only ever takes the user's own issues for
/// that reason; a person picking by hand may take anybody's, and the author is
/// what lets them see whose text they are handing over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRow {
    pub issue: Issue,
    pub author: String,
    pub labels: Vec<String>,
}

impl IssueRow {
    /// Whether it carries `label`.
    pub fn carries(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }
}

/// How many open issues the picker lists. A repository with more than this
/// open is one to narrow down on GitHub, and the list says it was cut.
pub const ISSUES_SHOWN: usize = 100;

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
        issue: Issue,
        author: Author,
        #[serde(default)]
        labels: Vec<Label>,
    }
    let rows: Vec<Row> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(rows
        .into_iter()
        .map(|row| IssueRow {
            issue: row.issue,
            author: row.author.login,
            labels: row.labels.into_iter().map(|label| label.name).collect(),
        })
        .collect())
}

/// `rows` cut to [`ISSUES_SHOWN`], and whether anything was cut.
fn bounded(mut rows: Vec<IssueRow>) -> (Vec<IssueRow>, bool) {
    let cut = rows.len() > ISSUES_SHOWN;
    rows.truncate(ISSUES_SHOWN);
    (rows, cut)
}

/// Every open issue in `root`'s repository, newest first as GitHub lists them,
/// up to [`ISSUES_SHOWN`] — and whether there were more. One past the bound is
/// asked for, which is what tells a full page from a cut one.
pub fn open_issues_blocking(root: &Path) -> Result<(Vec<IssueRow>, bool), String> {
    let limit = (ISSUES_SHOWN + 1).to_string();
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
    Ok(bounded(issue_rows(&json)?))
}

/// Take an issue picked by hand: take the trigger label off if it carries it,
/// so the automatic search does not reach for it as well, then say a run
/// started. The comment is the same one an automatic claim leaves, because it
/// has to read correctly in the same way if nothing follows it.
pub fn claim_picked_blocking(root: &Path, row: &IssueRow, label: &str) -> Result<(), String> {
    if !label.is_empty() && row.carries(label) {
        let n = row.issue.number.to_string();
        gh(root, &["issue", "edit", &n, "--remove-label", label])?;
    }
    comment_blocking(root, row.issue.number, &picked_claim_comment())
}

/// What an issue is told when a person picks it to be worked.
///
/// Not the automatic claim's sentence: that one says to re-add the trigger
/// label to retry, and a picked issue may never have carried it — or may be
/// somebody else's, which the automatic search never takes at all.
fn picked_claim_comment() -> String {
    "onehand started a run on this issue, picked by hand. If no outcome follows, the \
     run was interrupted — pick it again to retry."
        .to_string()
}

/// `"30m"`, `"2h"`, `"90s"` as a duration.
///
/// One number and one unit, nothing else. A zero is refused rather than read
/// as "continuously": an interval of nothing is a tick loop, and a timeout of
/// nothing cancels every run before its prompt goes in.
pub fn parse_every(text: &str) -> Option<Duration> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let count: u64 = text[..text.len() - unit.len_utf8()].parse().ok()?;
    let secs = match unit {
        's' => count,
        'm' => count.checked_mul(60)?,
        'h' => count.checked_mul(3600)?,
        _ => return None,
    };
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// A duration the way [`parse_every`] reads one, for a sentence about it.
fn spoken(d: Duration) -> String {
    let secs = d.as_secs();
    if secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// The branch a run on `issue` works on: `onehand/issue-<n>-<title words>`.
///
/// Built only from lowercase ASCII letters, digits and single dashes, so it is
/// a valid branch whatever the title holds — a title of nothing but
/// punctuation leaves the number alone, and a long one is cut at a word
/// boundary's worth of characters rather than carried whole into a folder name.
pub fn branch_for(issue: &Issue) -> String {
    const WORDS_MAX: usize = 40;
    let mut words = String::new();
    for ch in issue.title.chars() {
        if ch.is_ascii_alphanumeric() {
            words.push(ch.to_ascii_lowercase());
        } else if !words.is_empty() && !words.ends_with('-') {
            words.push('-');
        }
    }
    words.truncate(WORDS_MAX);
    let words = words.trim_matches('-');
    if words.is_empty() {
        format!("onehand/issue-{}", issue.number)
    } else {
        format!("onehand/issue-{}-{words}", issue.number)
    }
}

/// `branch`, or the first of `branch-2` … `branch-9` the repository at `root`
/// does not have yet.
///
/// An issue whose label was put back after a run left work on its branch gets a
/// branch of its own this time: the old one is still checked out in the
/// worktree that run left on disk, and git refuses a second checkout of it.
pub fn free_branch_blocking(root: &Path, branch: &str) -> String {
    std::iter::once(branch.to_string())
        .chain((2..=9).map(|n| format!("{branch}-{n}")))
        .find(|name| !crate::worktree::branch_exists_blocking(root, name))
        .unwrap_or_else(|| format!("{branch}-10"))
}

/// Where every run builds, shared across runs.
///
/// The worktrees are kept on disk after a run, so a build directory inside each
/// would be gigabytes per issue with nothing that ever cleans it; one shared
/// directory also turns the second run's build from cold into incremental.
/// Under the cache directory, because a build is something that can always be
/// made again.
pub fn target_dir() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("onehand").join("unattended-target"))
}

/// The one prompt a run sends.
///
/// It does not restate the repository's conventions — the commit format, the
/// test commands, the pull-request shape. Those are in the repository's own
/// instructions, which the agent reads anyway, and a second copy here is a copy
/// that goes stale without anybody noticing.
pub fn prompt_for(issue: &Issue, branch: &str) -> String {
    format!(
        "Work GitHub issue #{number} in this repository, unattended — nobody is \
         watching this session.\n\n\
         Title: {title}\n\n\
         {body}\n\n\
         ---\n\n\
         You are on branch `{branch}`, in a worktree of its own.\n\n\
         1. Read the repository's own agent instructions, and whatever they point \
         at, and follow its conventions.\n\
         2. Run the repository's checks before committing.\n\
         3. Commit, push the branch, and open the pull request yourself with \
         `gh pr create`, referencing #{number}.\n\
         4. If the issue turns out to need a decision from a person, say so in \
         one short paragraph and stop. Do not guess.\n",
        number = issue.number,
        title = issue.title,
        body = issue.body.trim(),
    )
}

/// What the issue is told when a run takes it.
///
/// Worded to stay true if nothing follows it. A crash between the claim and
/// the outcome leaves this as the last word on the issue, so it says that a run
/// *started* — the comment's own timestamp says when — and never that one is
/// happening, which is the one sentence a crash makes false.
fn claim_comment(label: &str) -> String {
    format!(
        "onehand started an unattended run on this issue. If no outcome follows, \
         the run was interrupted — re-add `{label}` to retry."
    )
}

/// How a run stopped, before anybody has looked for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The turn ended by itself. `tail` is how the agent's answer ended, which
    /// is where a question asked in prose rather than through a card would be.
    TurnEnded { tail: Option<String> },
    /// The agent parked a permission or a question nobody was there to answer.
    Asked(String),
    /// The adapter stopped answering.
    LinkLost,
    /// The session went away under the run — its window was closed — before
    /// the run had finished.
    Closed,
    /// The run outlasted its timeout.
    TimedOut(Duration),
    /// A person acted inside the run's session.
    TakenOver,
    /// The run never got as far as a prompt.
    Failed(String),
}

impl Ending {
    /// Whether a pull request is worth looking for. A run that never started
    /// cannot have opened one.
    pub fn may_have_pr(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
}

/// The comment an ending leaves on the issue, given what looking for a pull
/// request on `branch` found: one, none, or a failure to look at all.
///
/// **The pull request is the verdict, whatever the ending.** An agent can open
/// it and then time out, park or lose its adapter while tidying up, and a
/// comment saying "no pull request" beside a pull request is the worst answer
/// available — so a PR found makes the comment about the PR, with why the run
/// stopped as a note under it. For the same reason a lookup that *failed* is
/// said as a failure and never as "none": "no pull request" is a claim, and
/// one nobody checked is the worst answer by another route.
pub fn report(ending: &Ending, pr: &Result<Option<String>, String>, branch: &str) -> String {
    let head = match pr {
        Ok(None) => return no_pr(ending, branch),
        Ok(Some(url)) => format!("onehand opened {url}."),
        Err(err) => {
            format!("onehand could not tell whether a pull request was opened on `{branch}`: {err}")
        }
    };
    match stopped(ending) {
        Some(why) => format!("{head}\n\n{why}"),
        None => head,
    }
}

/// How a run ended, in one line — for the run's own transcript, where a line is
/// all a remark gets. The pull request leads when there is one, because it is
/// the verdict; the full account is the comment on the issue.
pub fn outcome_line(ending: &Ending, pr: &Result<Option<String>, String>) -> String {
    match pr {
        Ok(Some(url)) => format!("Opened {url}"),
        Err(_) => "Run over; could not tell whether a pull request was opened".to_string(),
        Ok(None) => {
            let why = match ending {
                Ending::TurnEnded { .. } => "the turn ended",
                Ending::Asked(_) => "it stopped on a decision",
                Ending::LinkLost => "the agent stopped answering",
                Ending::Closed => "its session was closed",
                Ending::TimedOut(_) => "it timed out",
                Ending::TakenOver => "it was taken over by hand",
                Ending::Failed(_) => "it could not start",
            };
            format!("Run over with no pull request: {why}; see the issue")
        }
    }
}

/// Why the run stopped, as a note under a verdict that is about something
/// else. `None` for the one ending that needs no explaining — the turn ending
/// by itself — unless it left words worth quoting.
fn stopped(ending: &Ending) -> Option<String> {
    match ending {
        Ending::TurnEnded { tail: None } => None,
        Ending::TurnEnded { tail: Some(tail) } => {
            Some(format!("The turn ended on:\n\n{}", quoted(tail)))
        }
        Ending::Asked(q) => Some(format!("It stopped on a decision:\n\n{}", quoted(q))),
        Ending::LinkLost => Some("The agent stopped answering.".to_string()),
        Ending::Closed => Some("Its session was closed before the run finished.".to_string()),
        Ending::TimedOut(d) => Some(format!("The run hit its {} timeout.", spoken(*d))),
        Ending::TakenOver => Some("It was taken over by hand.".to_string()),
        Ending::Failed(why) => Some(why.clone()),
    }
}

/// The comment when it is known that no pull request was opened.
fn no_pr(ending: &Ending, branch: &str) -> String {
    match ending {
        Ending::TurnEnded { tail: Some(tail) } => format!(
            "The turn ended with no pull request on `{branch}`. It ended on:\n\n{}",
            quoted(tail)
        ),
        Ending::TurnEnded { tail: None } => {
            format!("The turn ended with no pull request on `{branch}`.")
        }
        Ending::Asked(q) => format!("onehand stopped: it needs a decision.\n\n{}", quoted(q)),
        Ending::LinkLost => {
            format!("The agent stopped answering; there is no pull request on `{branch}`.")
        }
        Ending::Closed => format!(
            "The run's session was closed before it finished; there is no pull request on \
             `{branch}`."
        ),
        Ending::TimedOut(d) => format!(
            "No pull request after {}; the run was cancelled. Its work is on `{branch}`.",
            spoken(*d)
        ),
        Ending::TakenOver => {
            format!("Taken over by hand; the run stopped watching `{branch}`.")
        }
        Ending::Failed(why) => format!("onehand could not start the run: {why}"),
    }
}

/// `text` as a Markdown quote, every line of it.
fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How long one `gh` call may take. Each is one API request; one that has not
/// answered in this long is stuck, not slow.
const GH_LIMIT: Duration = Duration::from_secs(60);

/// How long a question answered from this machine alone may take — the
/// project's remote, what an ssh alias stands for. Seconds is already slow.
const LOCAL_LIMIT: Duration = Duration::from_secs(10);

/// Run `gh` in `root` and hand back what it printed.
fn gh(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::process::output_within(
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

/// The oldest open issue in `root`'s repository that carries `label` and was
/// opened by the user `gh` is signed in as.
///
/// **An empty label picks nothing**, without asking GitHub: the failure of
/// leaving it blank has to be "nothing runs", not "everything runs".
///
/// **Only the user's own issues.** The body goes into the prompt word for word
/// and the agent may run `git` and `gh` with the user's credentials; a label is
/// something anybody with triage rights can apply, to an issue anybody at all
/// may have written.
pub fn candidate_blocking(root: &Path, label: &str) -> Result<Option<Issue>, String> {
    if label.trim().is_empty() {
        return Ok(None);
    }
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
    oldest(&json)
}

/// The lowest-numbered issue in `gh issue list --json` output.
fn oldest(json: &str) -> Result<Option<Issue>, String> {
    let issues: Vec<Issue> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(issues.into_iter().min_by_key(|issue| issue.number))
}

/// Take `number`: remove the trigger label, then say a run started.
///
/// The label goes first because it is the whole of a run's state — once it is
/// off, nothing picks the issue again, whatever happens after.
pub fn claim_blocking(root: &Path, number: u64, label: &str) -> Result<(), String> {
    let n = number.to_string();
    gh(root, &["issue", "edit", &n, "--remove-label", label])?;
    comment_blocking(root, number, &claim_comment(label))
}

/// Leave `body` as a comment on issue `number`.
pub fn comment_blocking(root: &Path, number: u64, body: &str) -> Result<(), String> {
    gh(
        root,
        &["issue", "comment", &number.to_string(), "--body", body],
    )
    .map(drop)
}

/// Where this app can reach GitHub from, as far as a run needs to know.
///
/// **GitHub alone.** Everything a run does outside the checkout goes through
/// `gh`, so another forge is a second set of these calls rather than a setting;
/// until one exists, a project anywhere else is told so instead of failing
/// quietly on every tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHub {
    /// `gh` is signed in, as this login.
    SignedIn(String),
    /// `gh` is not installed.
    Missing,
    /// `gh` is installed and signed in to nothing.
    SignedOut,
    /// `gh` could not get an answer from GitHub.
    Unreachable(String),
}

impl GitHub {
    /// Why no run can happen, in words that say what to do about it; `None`
    /// when one can.
    pub fn problem(&self) -> Option<String> {
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
pub fn github_blocking() -> GitHub {
    // A `gh` that is not installed fails to start at all, which the bounded
    // runner reports as its own kind of failure rather than as a slow answer.
    let out = match crate::process::output_within(
        Command::new("gh")
            .args(["api", "user", "--jq", ".login"])
            .env("GH_PROMPT_DISABLED", "1"),
        GH_LIMIT,
    ) {
        Ok(out) => out,
        Err(crate::process::Failure::Missing) => return GitHub::Missing,
        Err(why) => return GitHub::Unreachable(format!("gh {why}")),
    };
    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.code() {
        Some(0) if !login.is_empty() => GitHub::SignedIn(login),
        // `gh` exits 4 when a command needs a login it does not have.
        Some(4) => GitHub::SignedOut,
        _ => GitHub::Unreachable(
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
    let refuse = |host: &str| {
        Err(format!(
            "its remote is on {host}, and unattended runs work with GitHub only"
        ))
    };
    let Some(host) = remote_host(url) else {
        return Err(
            "its remote is not on GitHub, and unattended runs work with GitHub only".to_string(),
        );
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
    let out = crate::process::output_within(Command::new("ssh").arg("-G").arg(alias), LOCAL_LIMIT)
        .ok()?;
    out.status.success().then_some(())?;
    ssh_hostname(&String::from_utf8_lossy(&out.stdout)).map(str::to_string)
}

/// Whether the project at `root` is one a run can work: a repository whose
/// `origin` is on GitHub. Read locally, before anything asks GitHub, so a
/// project that can never be worked costs nothing per tick but this.
pub fn github_project_blocking(root: &Path) -> Result<(), String> {
    let out = crate::process::output_within(
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

/// The repository's default branch, as GitHub has it.
pub fn default_branch_blocking(root: &Path) -> Result<String, String> {
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

/// The pull request opened from `branch`, if there is one.
///
/// `--state all`, because a PR merged or closed before the run is settled is
/// still the answer to "did it open one".
pub fn pr_for_blocking(root: &Path, branch: &str) -> Result<Option<String>, String> {
    let url = gh(
        root,
        &[
            "pr", "list", "--head", branch, "--state", "all", "--json", "url", "-q", ".[0].url",
        ],
    )?;
    Ok((!url.is_empty()).then_some(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str) -> Issue {
        Issue {
            number,
            title: title.to_string(),
            body: String::new(),
        }
    }

    #[test]
    fn an_interval_is_a_number_and_a_unit() {
        assert_eq!(parse_every("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_every("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_every(" 90s "), Some(Duration::from_secs(90)));
        assert_eq!(parse_every("soon"), None);
        assert_eq!(parse_every("0m"), None);
        assert_eq!(parse_every("m"), None);
        assert_eq!(parse_every(""), None);
    }

    #[test]
    fn a_duration_is_said_the_way_it_is_written() {
        for text in ["30m", "2h", "90s"] {
            assert_eq!(spoken(parse_every(text).unwrap()), text);
        }
    }

    #[test]
    fn every_title_makes_a_valid_branch() {
        let long = "word ".repeat(60);
        for title in [
            "Fix the rail",
            "!!!???",
            "",
            &long,
            "émoji 🚀 ünïcode",
            "a.lock",
        ] {
            let branch = branch_for(&issue(7, title));
            assert!(
                crate::worktree::validate_branch(&branch).is_ok(),
                "{title:?} gave {branch:?}"
            );
            assert!(branch.starts_with("onehand/issue-7"));
            assert!(branch.len() <= "onehand/issue-7-".len() + 40);
        }
        assert_eq!(
            branch_for(&issue(3, "Fix the rail!")),
            "onehand/issue-3-fix-the-rail"
        );
        assert_eq!(branch_for(&issue(3, "!!!")), "onehand/issue-3");
    }

    #[test]
    fn the_prompt_names_the_issue_and_the_branch_and_keeps_the_body_whole() {
        let body = "Steps:\n\n```rust\nfn main() {}\n```\n\nThat is all.";
        let issue = Issue {
            body: body.to_string(),
            ..issue(42, "Crash on open")
        };
        let prompt = prompt_for(&issue, "onehand/issue-42");
        assert!(prompt.contains("#42"));
        assert!(prompt.contains("`onehand/issue-42`"));
        assert!(prompt.contains(body));
    }

    #[test]
    fn an_empty_label_picks_nothing_without_asking() {
        // A root that is not a repository at all: were `gh` asked, it would fail.
        let nowhere = std::env::temp_dir();
        assert_eq!(candidate_blocking(&nowhere, ""), Ok(None));
        assert_eq!(candidate_blocking(&nowhere, "   "), Ok(None));
    }

    #[test]
    fn the_oldest_issue_is_taken_first() {
        let json = r#"[{"number":9,"title":"b","body":""},{"number":4,"title":"a","body":"x"}]"#;
        assert_eq!(oldest(json).unwrap().map(|i| i.number), Some(4));
        assert_eq!(oldest("[]"), Ok(None));
        assert!(oldest("not json").is_err());
    }

    #[test]
    fn the_claim_stays_true_if_nothing_follows_it() {
        let said = claim_comment("auto");
        assert!(said.contains("started"));
        assert!(said.contains("re-add `auto`"));
        assert!(!said.contains("is working"));
    }

    #[test]
    fn every_ending_has_a_sentence_with_and_without_a_pr() {
        let endings = [
            Ending::TurnEnded {
                tail: Some("Should I use A or B?".into()),
            },
            Ending::TurnEnded { tail: None },
            Ending::Asked("Run rm -rf target?".into()),
            Ending::LinkLost,
            Ending::Closed,
            Ending::TimedOut(Duration::from_secs(2700)),
            Ending::TakenOver,
            Ending::Failed("git refused".into()),
        ];
        for ending in &endings {
            // Exhaustive on purpose: a new ending cannot be added without being
            // listed above, and so without a sentence being checked for it.
            match ending {
                Ending::TurnEnded { .. }
                | Ending::Asked(_)
                | Ending::LinkLost
                | Ending::Closed
                | Ending::TimedOut(_)
                | Ending::TakenOver
                | Ending::Failed(_) => {}
            }
            let without = report(ending, &Ok(None), "onehand/issue-1");
            assert!(!without.is_empty());
            assert!(!without.contains("opened"), "{without}");
            let with = report(
                ending,
                &Ok(Some("https://x/pull/2".into())),
                "onehand/issue-1",
            );
            assert!(
                with.starts_with("onehand opened https://x/pull/2."),
                "{with}"
            );
            let unknown = report(ending, &Err("gh: offline".into()), "onehand/issue-1");
            assert!(unknown.contains("gh: offline"), "{unknown}");
        }
    }

    #[test]
    fn a_pr_found_after_a_timeout_is_still_the_verdict() {
        let said = report(
            &Ending::TimedOut(Duration::from_secs(2700)),
            &Ok(Some("https://x/pull/2".into())),
            "b",
        );
        assert!(said.starts_with("onehand opened https://x/pull/2."));
        assert!(said.contains("45m timeout"));
    }

    #[test]
    fn a_pr_lookup_that_failed_never_reads_as_no_pr() {
        for ending in [
            Ending::TurnEnded { tail: None },
            Ending::LinkLost,
            Ending::TimedOut(Duration::from_secs(60)),
            Ending::TakenOver,
        ] {
            let said = report(&ending, &Err("rate limited".into()), "b");
            assert!(!said.contains("no pull request"), "{said}");
            assert!(said.contains("could not tell"), "{said}");
        }
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
            why.contains("gitlab.com") && why.contains("GitHub only"),
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
        assert_eq!(GitHub::SignedIn("me".into()).problem(), None);
        assert!(GitHub::Missing.problem().unwrap().contains("not installed"));
        assert!(
            GitHub::SignedOut
                .problem()
                .unwrap()
                .contains("gh auth login"),
            "the fix is named"
        );
        assert!(GitHub::Unreachable("timeout".into())
            .problem()
            .unwrap()
            .contains("timeout"));
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

    #[test]
    fn a_listing_past_its_bound_is_cut_and_says_so() {
        let row = |n: u64| {
            format!(
                r#"{{"number":{n},"title":"t","body":"","author":{{"login":"a"}},"labels":[]}}"#
            )
        };
        let json = format!(
            "[{}]",
            (1..=ISSUES_SHOWN as u64 + 1)
                .map(row)
                .collect::<Vec<_>>()
                .join(",")
        );
        let (rows, cut) = bounded(issue_rows(&json).unwrap());
        assert_eq!(rows.len(), ISSUES_SHOWN);
        assert!(cut);
        let (rows, cut) = bounded(issue_rows("[]").unwrap());
        assert!(rows.is_empty() && !cut);
    }

    #[test]
    fn a_picked_claim_says_how_to_retry_without_a_label() {
        let said = picked_claim_comment();
        assert!(said.contains("started") && said.contains("picked by hand"));
        assert!(
            !said.contains("re-add"),
            "a picked issue may never have had the label"
        );
    }

    #[test]
    fn the_outcome_fits_on_one_line_and_leads_with_the_pr() {
        let pr = Ok(Some("https://x/pull/2".to_string()));
        let line = outcome_line(&Ending::TimedOut(Duration::from_secs(60)), &pr);
        assert!(line.starts_with("Opened https://x/pull/2"), "{line}");
        let none = outcome_line(
            &Ending::TurnEnded {
                tail: Some("long\nanswer".into()),
            },
            &Ok(None),
        );
        assert!(
            !none.contains('\n') && none.contains("no pull request"),
            "{none}"
        );
        let unknown = outcome_line(&Ending::LinkLost, &Err("offline".into()));
        assert!(unknown.contains("could not tell"), "{unknown}");
        for line in [line, none, unknown] {
            assert!(line.chars().count() <= 80, "{line}");
        }
    }

    #[test]
    fn a_question_in_prose_reaches_the_issue() {
        let said = report(
            &Ending::TurnEnded {
                tail: Some("Should I use A\nor B?".into()),
            },
            &Ok(None),
            "b",
        );
        assert!(said.contains("> Should I use A\n> or B?"));
    }
}
