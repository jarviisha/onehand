//! Unattended runs: one small issue, one session, one pull request.
//!
//! Everything about a run that can be decided without a window — which issue,
//! what branch, what the prompt says, what the issue is told afterwards — and
//! what a run asks of the project's [`Connector`]. The app holds the rest: the
//! timer, the session, the watching.

use crate::connector::Connector;
use crate::issues;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An issue a run can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    title: String,
    body: String,
}

impl Issue {
    /// An issue as a connector read it: its number, its title and its body,
    /// which may be empty.
    pub fn new(number: u64, title: String, body: String) -> Self {
        Self {
            number,
            title,
            body,
        }
    }

    /// What the issue is called.
    pub fn title_text(&self) -> &str {
        &self.title
    }
}

/// An open issue as the picker lists it: the issue, who opened it, and what it
/// is labelled.
///
/// **Who opened it is on every row**, because the body goes into the prompt
/// word for word and the agent runs with the user's credentials — it pushes
/// and opens pull requests as them. The automatic search only ever takes the
/// user's own issues for that reason; a person picking by hand may take
/// anybody's, and the author is what lets them see whose text they are handing
/// over.
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

/// Where an issue lives, and so where a run on it is claimed, commented on and
/// found.
///
/// Kept apart from the forge a project's pull requests go to, because the two
/// come apart: an issue kept in onehand can be worked in a project on GitHub,
/// where the work still ends in a pull request, or in a project on no forge at
/// all, where it ends on a branch.
#[derive(Clone)]
pub enum Tracker {
    /// On a forge, reached through its connector.
    Forge(&'static dyn Connector),
    /// Kept by onehand, in this project's issue file.
    Local(PathBuf),
}

impl Tracker {
    /// How the prompt names issue `number`.
    fn names(&self, number: u64) -> String {
        match self {
            Self::Forge(c) => format!("{} issue #{number}", c.name()),
            Self::Local(_) => {
                format!("issue #{number}, which is kept in onehand rather than on a forge")
            }
        }
    }

    /// Open issues carrying `label` that the user wrote. Every issue kept in
    /// onehand was written by the user, so for those the label is the whole
    /// test.
    fn labelled_blocking(&self, root: &Path, label: &str) -> Result<Vec<Issue>, String> {
        match self {
            Self::Forge(c) => c.my_labelled_issues_blocking(root, label),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open && i.labels.iter().any(|l| l == label))
                .map(|i| Issue::new(i.number, i.title.clone(), i.body.clone()))
                .collect()),
        }
    }

    /// Open issues, newest first, at most `limit` of them. One kept in onehand
    /// has no author on its row: they are all the user's own.
    fn open_blocking(&self, root: &Path, limit: usize) -> Result<Vec<IssueRow>, String> {
        match self {
            Self::Forge(c) => c.open_issues_blocking(root, limit),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open)
                .take(limit)
                .map(|i| IssueRow {
                    issue: Issue::new(i.number, i.title.clone(), i.body.clone()),
                    author: String::new(),
                    labels: i.labels.clone(),
                })
                .collect()),
        }
    }

    fn remove_label_blocking(&self, root: &Path, number: u64, label: &str) -> Result<(), String> {
        match self {
            Self::Forge(c) => c.remove_label_blocking(root, number, label),
            Self::Local(file) => issues::update_blocking(file, |kept| {
                kept.remove_label(number, label, issues::now())
            })
            .map(drop),
        }
    }

    /// Leave `body` on issue `number`: a comment on a forge, a note on an issue
    /// kept in onehand.
    pub fn comment_blocking(&self, root: &Path, number: u64, body: &str) -> Result<(), String> {
        match self {
            Self::Forge(c) => c.comment_blocking(root, number, body),
            Self::Local(file) => {
                issues::update_blocking(file, |kept| kept.note(number, body, issues::now()))
                    .map(drop)
            }
        }
    }
}

/// How many open issues the picker lists. A repository with more than this
/// open is one to narrow down where it lives, and the list says it was cut.
pub const ISSUES_SHOWN: usize = 100;

/// `rows` cut to [`ISSUES_SHOWN`], and whether anything was cut.
fn bounded(mut rows: Vec<IssueRow>) -> (Vec<IssueRow>, bool) {
    let cut = rows.len() > ISSUES_SHOWN;
    rows.truncate(ISSUES_SHOWN);
    (rows, cut)
}

/// Every open issue in `root`'s repository, newest first, up to
/// [`ISSUES_SHOWN`] — and whether there were more. One past the bound is asked
/// for, which is what tells a full page from a cut one.
pub fn open_issues_blocking(
    tracker: &Tracker,
    root: &Path,
) -> Result<(Vec<IssueRow>, bool), String> {
    Ok(bounded(tracker.open_blocking(root, ISSUES_SHOWN + 1)?))
}

/// Take an issue picked by hand: take the trigger label off if it carries it,
/// so the automatic search does not reach for it as well, then say a run
/// started. The comment is the same one an automatic claim leaves, because it
/// has to read correctly in the same way if nothing follows it.
pub fn claim_picked_blocking(
    tracker: &Tracker,
    root: &Path,
    row: &IssueRow,
    label: &str,
) -> Result<(), String> {
    if !label.is_empty() && row.carries(label) {
        tracker.remove_label_blocking(root, row.issue.number, label)?;
    }
    tracker.comment_blocking(root, row.issue.number, &picked_claim_comment())
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
///
/// Three shapes, by where the issue lives and whether the project has a forge.
/// An issue on the forge is referenced from its pull request. An issue kept in
/// onehand is **not** — `#N` in a pull request names the forge's issue N, which
/// is some other issue. And a project on no forge is told to leave its work on
/// the branch, since there is nowhere to push it and the branch is the result.
pub fn prompt_for(
    issue: &Issue,
    branch: &str,
    tracker: &Tracker,
    forge: Option<&dyn Connector>,
) -> String {
    let finish = match (forge, tracker) {
        (Some(forge), Tracker::Forge(_)) => format!(
            "Commit, push the branch, and open the pull request yourself with {}, \
             referencing #{}.",
            forge.open_pull_request_with(),
            issue.number
        ),
        (Some(forge), Tracker::Local(_)) => format!(
            "Commit, push the branch, and open the pull request yourself with {}. Do \
             not reference #{} in it: that number is onehand's, not the forge's.",
            forge.open_pull_request_with(),
            issue.number
        ),
        (None, _) => "Commit your work on this branch. Do not push it: this project \
                      has no forge, and the branch is the result."
            .to_string(),
    };
    format!(
        "Work {named} in this repository, unattended — nobody is watching this \
         session.\n\n\
         Title: {title}\n\n\
         {body}\n\n\
         ---\n\n\
         You are on branch `{branch}`, in a worktree of its own.\n\n\
         1. Read the repository's own agent instructions, and whatever they point \
         at, and follow its conventions.\n\
         2. Run the repository's checks before committing.\n\
         3. {finish}\n\
         4. If the issue turns out to need a decision from a person, say so in \
         one short paragraph and stop. Do not guess.\n",
        named = tracker.names(issue.number),
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
    /// Whether the run's work is worth looking for. A run that never started
    /// cannot have done any.
    pub fn may_have_work(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
}

/// What a run left behind: on a project with a forge, a pull request or none;
/// on a project without one, commits on its branch or none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    PullRequest(String),
    NoPullRequest,
    Commits(u64),
    NoCommits,
}

impl Verdict {
    /// The missing verdict, as the noun phrase the sentences about it use.
    fn missing(&self) -> Option<&'static str> {
        match self {
            Self::NoPullRequest => Some("no pull request"),
            Self::NoCommits => Some("no commit"),
            Self::PullRequest(_) | Self::Commits(_) => None,
        }
    }
}

/// The comment an ending leaves on the issue, given what looking for the run's
/// work on `branch` found: a pull request or commits, none, or a failure to
/// look at all.
///
/// **The work is the verdict, whatever the ending.** An agent can open a pull
/// request and then time out, park or lose its adapter while tidying up, and a
/// comment saying "no pull request" beside a pull request is the worst answer
/// available — so work found makes the comment about the work, with why the run
/// stopped as a note under it. For the same reason a lookup that *failed* is
/// said as a failure and never as "none": "no pull request" is a claim, and
/// one nobody checked is the worst answer by another route.
pub fn report(ending: &Ending, found: &Result<Verdict, String>, branch: &str) -> String {
    let head = match found {
        Ok(Verdict::PullRequest(url)) => format!("onehand opened {url}."),
        Ok(Verdict::Commits(n)) => format!(
            "onehand left {n} commit{} on `{branch}`.",
            if *n == 1 { "" } else { "s" }
        ),
        Ok(verdict) => {
            return nothing_found(ending, branch, verdict.missing().unwrap_or("nothing"))
        }
        Err(err) => {
            format!("onehand could not tell what the run left on `{branch}`: {err}")
        }
    };
    match stopped(ending) {
        Some(why) => format!("{head}\n\n{why}"),
        None => head,
    }
}

/// How a run ended, in one line — for the run's own transcript, where a line is
/// all a remark gets. The work leads when there is some, because it is the
/// verdict; the full account is the comment on the issue.
pub fn outcome_line(ending: &Ending, found: &Result<Verdict, String>) -> String {
    match found {
        Ok(Verdict::PullRequest(url)) => format!("Opened {url}"),
        Ok(Verdict::Commits(n)) => {
            format!(
                "Left {n} commit{} on its branch",
                if *n == 1 { "" } else { "s" }
            )
        }
        Err(_) => "Run over; could not tell what it left".to_string(),
        Ok(verdict) => {
            let why = match ending {
                Ending::TurnEnded { .. } => "the turn ended",
                Ending::Asked(_) => "it stopped on a decision",
                Ending::LinkLost => "the agent stopped answering",
                Ending::Closed => "its session was closed",
                Ending::TimedOut(_) => "it timed out",
                Ending::TakenOver => "it was taken over by hand",
                Ending::Failed(_) => "it could not start",
            };
            format!(
                "Run over with {}: {why}; see the issue",
                verdict.missing().unwrap_or("nothing")
            )
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

/// The comment when it is known the run left nothing — `missing` is what it
/// did not leave, "no pull request" or "no commit".
fn nothing_found(ending: &Ending, branch: &str, missing: &str) -> String {
    match ending {
        Ending::TurnEnded { tail: Some(tail) } => format!(
            "The turn ended with {missing} on `{branch}`. It ended on:\n\n{}",
            quoted(tail)
        ),
        Ending::TurnEnded { tail: None } => {
            format!("The turn ended with {missing} on `{branch}`.")
        }
        Ending::Asked(q) => format!("onehand stopped: it needs a decision.\n\n{}", quoted(q)),
        Ending::LinkLost => {
            format!("The agent stopped answering; there is {missing} on `{branch}`.")
        }
        Ending::Closed => format!(
            "The run's session was closed before it finished; there is {missing} on \
             `{branch}`."
        ),
        Ending::TimedOut(d) => format!(
            "{} after {}; the run was cancelled. Its work is on `{branch}`.",
            capitalised(missing),
            spoken(*d)
        ),
        Ending::TakenOver => {
            format!("Taken over by hand; the run stopped watching `{branch}`.")
        }
        Ending::Failed(why) => format!("onehand could not start the run: {why}"),
    }
}

/// `text` with its first letter made a capital.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `text` as a Markdown quote, every line of it.
fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The oldest open issue in `tracker` that carries `label` and was opened by the
/// user.
///
/// **An empty label picks nothing**, without asking the connector: the failure of
/// leaving it blank has to be "nothing runs", not "everything runs".
///
/// **Only the user's own issues.** The body goes into the prompt word for word
/// and the agent runs with the user's credentials; a label is
/// something anybody with triage rights can apply, to an issue anybody at all
/// may have written.
pub fn candidate_blocking(
    tracker: &Tracker,
    root: &Path,
    label: &str,
) -> Result<Option<Issue>, String> {
    if label.trim().is_empty() {
        return Ok(None);
    }
    Ok(tracker
        .labelled_blocking(root, label)?
        .into_iter()
        .min_by_key(|issue| issue.number))
}

/// Take `number`: remove the trigger label, then say a run started.
///
/// The label goes first because it is the whole of a run's state — once it is
/// off, nothing picks the issue again, whatever happens after.
pub fn claim_blocking(
    tracker: &Tracker,
    root: &Path,
    number: u64,
    label: &str,
) -> Result<(), String> {
    tracker.remove_label_blocking(root, number, label)?;
    tracker.comment_blocking(root, number, &claim_comment(label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::fake::Fake;

    /// The test forge, as the tracker an issue on it lives in.
    fn forge() -> Tracker {
        Tracker::Forge(&Fake::SERVING)
    }

    /// A tracker over a scratch issue file of its own, holding `issues`.
    fn local(name: &str, issues: &[(&str, &[&str])]) -> (Tracker, PathBuf) {
        let dir = std::env::temp_dir().join(format!("onehand-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("issues.json");
        for (title, labels) in issues {
            crate::issues::update_blocking(&file, |kept| {
                kept.create(
                    crate::issues::Draft {
                        title: title.to_string(),
                        labels: labels.iter().map(|l| l.to_string()).collect(),
                        ..Default::default()
                    },
                    1,
                )
            })
            .unwrap();
        }
        (Tracker::Local(file), dir)
    }

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
        let prompt = prompt_for(&issue, "onehand/issue-42", &forge(), Some(&Fake::SERVING));
        assert!(prompt.contains("#42"));
        assert!(prompt.contains("`onehand/issue-42`"));
        assert!(prompt.contains(body));
        assert!(prompt.contains("Work Forge issue") && prompt.contains("`forge pr`"));
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
            let without = report(ending, &Ok(Verdict::NoPullRequest), "onehand/issue-1");
            assert!(!without.is_empty());
            assert!(!without.contains("opened"), "{without}");
            let with = report(
                ending,
                &Ok(Verdict::PullRequest("https://x/pull/2".into())),
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
            &Ok(Verdict::PullRequest("https://x/pull/2".into())),
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
        let pr = Ok(Verdict::PullRequest("https://x/pull/2".to_string()));
        let line = outcome_line(&Ending::TimedOut(Duration::from_secs(60)), &pr);
        assert!(line.starts_with("Opened https://x/pull/2"), "{line}");
        let none = outcome_line(
            &Ending::TurnEnded {
                tail: Some("long\nanswer".into()),
            },
            &Ok(Verdict::NoPullRequest),
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
            &Ok(Verdict::NoPullRequest),
            "b",
        );
        assert!(said.contains("> Should I use A\n> or B?"));
    }

    #[test]
    fn an_empty_label_picks_nothing_without_asking() {
        let nowhere = std::env::temp_dir();
        assert_eq!(candidate_blocking(&forge(), &nowhere, ""), Ok(None));
        assert_eq!(candidate_blocking(&forge(), &nowhere, "   "), Ok(None));
    }

    #[test]
    fn the_oldest_issue_is_taken_first() {
        let found = candidate_blocking(&forge(), &std::env::temp_dir(), "auto").unwrap();
        assert_eq!(found.map(|i| i.number), Some(4));
        static NONE_LABELLED: Fake = Fake {
            labelled: &[],
            ..Fake::SERVING
        };
        assert_eq!(
            candidate_blocking(
                &Tracker::Forge(&NONE_LABELLED),
                &std::env::temp_dir(),
                "auto"
            ),
            Ok(None)
        );
    }

    #[test]
    fn a_listing_past_its_bound_is_cut_and_says_so() {
        let (rows, cut) = open_issues_blocking(&forge(), &std::env::temp_dir()).unwrap();
        assert_eq!(rows.len(), ISSUES_SHOWN);
        assert!(cut);
        let (rows, cut) = bounded(Vec::new());
        assert!(rows.is_empty() && !cut);
    }

    #[test]
    fn a_local_issue_is_found_by_its_label_and_claimed_in_its_own_file() {
        let (tracker, dir) = local(
            "claim",
            &[
                ("first", &["auto"]),
                ("second", &["auto", "ui"]),
                ("third", &[]),
            ],
        );
        let root = std::env::temp_dir();
        let found = candidate_blocking(&tracker, &root, "auto")
            .unwrap()
            .unwrap();
        assert_eq!(found.number, 1, "the oldest labelled issue goes first");
        claim_blocking(&tracker, &root, 1, "auto").unwrap();
        let Tracker::Local(file) = &tracker else {
            unreachable!()
        };
        let kept = crate::issues::load_blocking(file).unwrap();
        let claimed = kept.get(1).unwrap();
        assert!(claimed.labels.is_empty(), "the label is the claim");
        assert!(claimed.notes[0].text.contains("started"));
        assert_eq!(
            candidate_blocking(&tracker, &root, "auto")
                .unwrap()
                .map(|i| i.number),
            Some(2)
        );
        let (rows, cut) = open_issues_blocking(&tracker, &root).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(!cut && rows.iter().all(|row| row.author.is_empty()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_local_issue_is_never_referenced_from_a_pull_request() {
        let (tracker, dir) = local("prompt", &[]);
        let issue = issue(3, "Fix it");
        let on_forge = prompt_for(&issue, "b", &tracker, Some(&Fake::SERVING));
        assert!(on_forge.contains("`forge pr`"), "{on_forge}");
        assert!(on_forge.contains("Do not reference #3"), "{on_forge}");
        assert!(!on_forge.contains("referencing #3"), "{on_forge}");
        let no_forge = prompt_for(&issue, "b", &tracker, None);
        assert!(no_forge.contains("Do not push"), "{no_forge}");
        assert!(!no_forge.contains("pull request"), "{no_forge}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commits_left_on_a_branch_are_the_verdict_without_a_forge() {
        let said = report(
            &Ending::TurnEnded { tail: None },
            &Ok(Verdict::Commits(2)),
            "b",
        );
        assert_eq!(said, "onehand left 2 commits on `b`.");
        let one = outcome_line(
            &Ending::TimedOut(Duration::from_secs(60)),
            &Ok(Verdict::Commits(1)),
        );
        assert_eq!(one, "Left 1 commit on its branch");
        let none = report(
            &Ending::TimedOut(Duration::from_secs(60)),
            &Ok(Verdict::NoCommits),
            "b",
        );
        assert!(none.starts_with("No commit after 1m"), "{none}");
        assert!(!none.contains("pull request"), "{none}");
    }
}
