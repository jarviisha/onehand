//! Unattended runs: one small issue, one session, one pull request.
//!
//! Everything about a run that can be decided without a window — which issue,
//! what branch, what the prompt says, what the issue is told afterwards — and
//! what a run asks of the project's [`Connector`]. The app holds the rest: the
//! timer, the session, the watching.

use crate::connector::Connector;
use crate::issues;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod run;
pub use run::{
    after_checks, after_turn, carry_on, load_records_blocking, new_record_file, runs_dir,
    save_record_blocking, Checked, Facts, Failure, Kept, Missing, Phase, Progress, Record, Spent,
    Start, Step, CHECKS_GRACE,
};

/// An issue a run can take.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Issue {
    pub number: u64,
    title: String,
    body: String,
    /// How the forge refers to it, for an issue kept in onehand that is kept in
    /// step with one — what its pull request references instead of `#number`.
    forge_ref: Option<String>,
}

impl Issue {
    /// An issue as a connector read it: its number, its title and its body,
    /// which may be empty.
    pub fn new(number: u64, title: String, body: String) -> Self {
        Self {
            number,
            title,
            body,
            forge_ref: None,
        }
    }

    /// How the forge refers to this issue, if it is kept in step with one.
    pub fn forge_ref(&self) -> Option<&str> {
        self.forge_ref.as_deref()
    }

    /// The same issue, known on its forge as `reference`.
    fn at(mut self, reference: String) -> Self {
        self.forge_ref = Some(reference);
        self
    }

    /// What the issue is called.
    pub fn title_text(&self) -> &str {
        &self.title
    }
}

/// An issue kept here, as a run reads it.
impl From<&issues::LocalIssue> for Issue {
    fn from(kept: &issues::LocalIssue) -> Self {
        Issue::new(kept.number, kept.title.clone(), kept.body.clone())
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
    /// Kept by onehand and kept in step with a forge: searched, claimed and
    /// told here, and every one of those reaches the forge through the sync —
    /// so a project that is synced is never searched on the forge as well, and
    /// no issue is found twice.
    Synced {
        file: PathBuf,
        forge: &'static dyn Connector,
    },
}

impl Tracker {
    /// How `issue` is shown to a person: by the forge's number where it lives
    /// on the forge or is kept in step with one, and as a draft where it is
    /// kept in onehand only — the number it is filed under there is a key,
    /// and beside a forge's own it reads as a second issue.
    pub fn shown(&self, issue: &Issue) -> String {
        match (self, issue.forge_ref()) {
            (Self::Forge(_), _) => format!("#{}", issue.number),
            (_, Some(reference)) => reference.to_string(),
            (_, None) => "Draft".to_string(),
        }
    }

    /// How the prompt names `issue`.
    fn names(&self, issue: &Issue) -> String {
        let number = issue.number;
        match (self, issue.forge_ref()) {
            (Self::Forge(c), _) => format!("{} issue #{number}", c.name()),
            (Self::Synced { forge, .. }, Some(reference)) => {
                format!("{} issue {reference}", forge.name())
            }
            (Self::Local(_) | Self::Synced { .. }, _) => {
                format!("issue #{number}, which is kept in onehand rather than on a forge")
            }
        }
    }

    /// Open issues carrying `label` that the user wrote.
    ///
    /// Kept here, that is the ones **written** here — never one brought in
    /// from a forge, linked or not, since somebody else may have written it
    /// and only the forge can say who. Such an issue is found through
    /// [`Self::Synced`], which asks, or not at all: a sync switched off, a
    /// forge unreachable for one tick, or a link lost must not turn somebody
    /// else's text into a prompt run with the user's credentials.
    fn labelled_blocking(&self, root: &Path, label: &str) -> Result<Vec<Issue>, String> {
        match self {
            Self::Forge(c) => c.my_labelled_issues_blocking(root, label),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.written_here() && i.open && i.labels.iter().any(|l| l == label))
                .map(Issue::from)
                .collect()),
            // Synced first, so a label put on at the forge is seen. An issue
            // brought in from the forge may be anybody's, so it is taken only
            // if the forge says the user wrote it — the same rule the forge's
            // own search keeps, asked of the forge because only it knows.
            Self::Synced { file, forge } => {
                let (kept, _) = issues::sync::sync_blocking(file, root, *forge, issues::now())?;
                let mine: Vec<String> = forge
                    .my_labelled_issues_blocking(root, label)?
                    .into_iter()
                    .map(|i| i.number.to_string())
                    .collect();
                Ok(kept
                    .listed()
                    .into_iter()
                    .filter(|i| i.open && i.labels.iter().any(|l| l == label))
                    .filter_map(|i| {
                        let found = Issue::from(i);
                        if i.written_here() {
                            return Some(found);
                        }
                        let link = i.link_on(forge.name()).filter(|l| mine.contains(&l.key))?;
                        Some(found.at(link.reference.clone()))
                    })
                    .collect())
            }
        }
    }

    /// Open issues, newest first, at most `limit` of them, for a person to pick
    /// from. One kept here has no author on its row. Unsynced, only the ones
    /// written here are listed — a linked one is on the forge's list already.
    /// Synced, every one is, and a linked one carries its forge reference so
    /// its pull request names it there.
    fn open_blocking(&self, root: &Path, limit: usize) -> Result<Vec<IssueRow>, String> {
        let row = |i: &issues::LocalIssue, forge_ref: Option<String>| {
            let issue = Issue::from(i);
            IssueRow {
                issue: match forge_ref {
                    Some(reference) => issue.at(reference),
                    None => issue,
                },
                author: String::new(),
                labels: i.labels.clone(),
            }
        };
        match self {
            Self::Forge(c) => c.open_issues_blocking(root, limit),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open && i.written_here())
                .take(limit)
                .map(|i| row(i, None))
                .collect()),
            Self::Synced { file, forge } => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open)
                .take(limit)
                .map(|i| row(i, i.link_on(forge.name()).map(|l| l.reference.clone())))
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
            // Off here first, which is the claim; the sync then takes it off
            // the forge. A sync that fails now is retried by the next, and the
            // issue is not found again meanwhile because only this side is
            // searched.
            Self::Synced { file, forge } => {
                issues::update_blocking(file, |kept| {
                    kept.remove_label(number, label, issues::now())
                })?;
                let _ = issues::sync::sync_blocking(file, root, *forge, issues::now());
                Ok(())
            }
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
            // A note here, and the same words on the forge's issue when it has
            // one — somebody reading it there should hear what happened too.
            Self::Synced { file, forge } => {
                let (kept, ()) =
                    issues::update_blocking(file, |kept| kept.note(number, body, issues::now()))?;
                let key = kept
                    .get(number)
                    .and_then(|i| i.link_on(forge.name()))
                    .and_then(|link| link.key.parse().ok());
                match key {
                    Some(key) => forge.comment_blocking(root, key, body),
                    None => Ok(()),
                }
            }
        }
    }
}

/// The session modes offered for the sessions that work an issue, as the
/// adapter's id and what a person calls it, least asking first.
///
/// `bypassPermissions` is among them because a person may want it and should
/// not have to edit a file to say so; what it gives away is said beside it.
pub const MODES: [(&str, &str); 4] = [
    ("auto", "Auto"),
    ("acceptEdits", "Accept edits"),
    ("default", "Ask"),
    ("bypassPermissions", "Bypass"),
];

/// [`MODES`], plus `current` at the end when it is none of them — an id
/// written into the config by hand is still the one in force, and a picker
/// that showed none of its choices pressed would say otherwise.
pub fn mode_choices(current: &str) -> Vec<(String, String)> {
    let mut choices: Vec<(String, String)> = MODES
        .iter()
        .map(|(id, name)| (id.to_string(), name.to_string()))
        .collect();
    if !choices.iter().any(|(id, _)| id == current) {
        choices.push((current.to_string(), current.to_string()));
    }
    choices
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
    branch_words(&issue.title, &issue_prefix(issue.number))
}

/// What every branch a run on issue `number` works on starts with, whatever
/// its title says now: a title edited since the last attempt still finds that
/// attempt's branch.
pub fn issue_prefix(number: u64) -> String {
    format!("onehand/issue-{number}")
}

/// `prefix`, then the first forty characters of `title` as lowercase words
/// joined by dashes.
pub(crate) fn branch_words(title: &str, prefix: &str) -> String {
    const WORDS_MAX: usize = 40;
    let mut words = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            words.push(ch.to_ascii_lowercase());
        } else if !words.is_empty() && !words.ends_with('-') {
            words.push('-');
        }
    }
    words.truncate(WORDS_MAX);
    let words = words.trim_matches('-');
    if words.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}-{words}")
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

/// The first prompt of a run's session.
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
///
/// `start` says what this session is for beyond the issue: carrying on an
/// earlier attempt, answering a review, repairing checks. A session with a
/// pull request already open pushes to it and never opens a second.
pub fn prompt_for(
    issue: &Issue,
    branch: &str,
    tracker: &Tracker,
    forge: Option<&dyn Connector>,
    start: &Start,
) -> String {
    let finish = match (forge, tracker, issue.forge_ref()) {
        (Some(_), _, _) if start.has_pull_request() => "Commit, and push the branch: its \
             pull request is already open, so do not open another."
            .to_string(),
        (Some(forge), Tracker::Forge(_), _) => format!(
            "Commit, push the branch, and open the pull request yourself with {}, \
             referencing #{}.",
            forge.open_pull_request_with(),
            issue.number
        ),
        (Some(forge), Tracker::Synced { .. }, Some(reference)) => format!(
            "Commit, push the branch, and open the pull request yourself with {}, \
             referencing {reference}.",
            forge.open_pull_request_with(),
        ),
        (Some(forge), Tracker::Local(_) | Tracker::Synced { .. }, _) => format!(
            "Commit, push the branch, and open the pull request yourself with {}. Do \
             not reference #{} in it: that number is onehand's, not the forge's.",
            forge.open_pull_request_with(),
            issue.number
        ),
        (None, _, _) => "Commit your work on this branch. Do not push it: this project \
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
         {context}\
         1. Read the repository's own agent instructions, and whatever they point \
         at, and follow its conventions.\n\
         2. Run the repository's checks before committing.\n\
         3. {finish}\n\
         4. Decide whatever the code, the tests and the documentation let you \
         infer, and list the assumptions that mattered {listed}. Only a decision \
         they cannot settle, about what the product should do, is for a person: \
         ask it with your tool for asking the user a question, not in your answer, \
         and carry on once it is answered. Do not guess at those.\n",
        named = tracker.names(issue),
        title = issue.title,
        body = issue.body.trim(),
        context = start
            .said(forge)
            .map(|said| format!("{said}\n\n"))
            .unwrap_or_default(),
        listed = match forge {
            Some(_) => "in the pull request's description",
            None => "in your last answer",
        },
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
    /// The run ended while a permission or a question the agent parked was
    /// still waiting for a person — its adapter went, or its session was
    /// closed, before anybody answered.
    Asked(String),
    /// The adapter stopped answering.
    LinkLost,
    /// The session went away under the run — its window was closed — before
    /// the run had finished.
    Closed,
    /// The run outlasted its timeout.
    TimedOut(Duration),
    /// A person put a prompt of their own into the run's session. Answering a
    /// card the run parked is not this: the run waits for that answer.
    TakenOver,
    /// The run never got as far as a prompt.
    Failed(String),
    /// The pull request's checks passed on its head, or none ran, and it is
    /// marked ready for review.
    Ready { checks_ran: bool },
    /// The run used up what it was given before its pull request was ready.
    Exhausted(Spent),
    /// The pull request was closed or merged while its checks were awaited.
    PullRequestGone,
}

impl Ending {
    /// Whether the run's work is worth looking for. A run that never started
    /// cannot have done any.
    pub fn may_have_work(&self) -> bool {
        match self {
            Self::Failed(_) => false,
            Self::TurnEnded { .. }
            | Self::Asked(_)
            | Self::LinkLost
            | Self::Closed
            | Self::TimedOut(_)
            | Self::TakenOver
            | Self::Ready { .. }
            | Self::Exhausted(_)
            | Self::PullRequestGone => true,
        }
    }
}

/// How much of its timeout a run has left, counting only the time it spent
/// working.
///
/// **Time spent waiting on a person does not count.** A run that parked a
/// question is standing still because somebody has not answered yet, and a
/// timeout that ran through the wait would cancel the run for a slow reply —
/// which is the one failure the wait exists to avoid. The clock is what the
/// timeout bounds: an agent that works without finishing.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    limit: Duration,
    spent: Duration,
    /// When the current stretch of work began; `None` while waiting.
    since: Option<Instant>,
}

impl Budget {
    /// A budget of `limit`, running from `now`.
    pub fn start(limit: Duration, now: Instant) -> Self {
        Self::resumed(limit, Duration::ZERO, now)
    }

    /// A budget of `limit` with `spent` of it already gone, running from `now`:
    /// a later session of the same attempt.
    pub fn resumed(limit: Duration, spent: Duration, now: Instant) -> Self {
        Self {
            limit,
            spent,
            since: Some(now),
        }
    }

    /// How much of the limit is spent at `now`.
    pub fn spent(&self, now: Instant) -> Duration {
        let running = self
            .since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        self.spent + running
    }

    /// Stop counting: the run is waiting on a person. Pausing twice is once.
    pub fn pause(&mut self, now: Instant) {
        if let Some(since) = self.since.take() {
            self.spent += now.saturating_duration_since(since);
        }
    }

    /// Count again from `now`. Resuming a running budget changes nothing.
    pub fn resume(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    /// The whole timeout, however much of it is spent.
    pub fn limit(&self) -> Duration {
        self.limit
    }

    /// What is left of the limit at `now`.
    pub fn left(&self, now: Instant) -> Duration {
        self.limit.saturating_sub(self.spent(now))
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
                Ending::Asked(_) => "it ended waiting on a decision",
                Ending::LinkLost => "the agent stopped answering",
                Ending::Closed => "its session was closed",
                Ending::TimedOut(_) => "it timed out",
                Ending::TakenOver => "it was taken over by hand",
                Ending::Failed(_) => "it could not start",
                Ending::Ready { .. } => "its checks passed",
                Ending::Exhausted(_) => "it ran out of what it was given",
                Ending::PullRequestGone => "its pull request was closed",
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
        Ending::Asked(q) => Some(format!(
            "It ended waiting on a decision nobody answered:\n\n{}",
            quoted(q)
        )),
        Ending::LinkLost => Some("The agent stopped answering.".to_string()),
        Ending::Closed => Some("Its session was closed before the run finished.".to_string()),
        Ending::TimedOut(d) => Some(format!("The run hit its {} timeout.", spoken(*d))),
        Ending::TakenOver => Some("It was taken over by hand.".to_string()),
        Ending::Failed(why) => Some(why.clone()),
        Ending::Ready { checks_ran: true } => Some(
            "Its checks passed on its latest commit, and it is marked ready for review."
                .to_string(),
        ),
        Ending::Ready { checks_ran: false } => {
            Some("No checks ran on its latest commit; it is marked ready for review.".to_string())
        }
        Ending::Exhausted(spent) => Some(spent.said()),
        Ending::PullRequestGone => Some(
            "It was closed or merged before its checks settled, so the run stopped.".to_string(),
        ),
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
        Ending::Asked(q) => format!(
            "The run ended waiting on a decision nobody answered; there is {missing} on \
             `{branch}`.\n\n{}",
            quoted(q)
        ),
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
        Ending::Ready { .. } | Ending::PullRequestGone => {
            format!("The run ended with {missing} on `{branch}`.")
        }
        Ending::Exhausted(spent) => format!(
            "{} There is {missing} on `{branch}`; its work stays there.",
            spent.said()
        ),
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
///
/// **An issue in `busy` is passed over**: a run on it has not ended, and a
/// second would work the same branch. Its label stays on, so it is taken once
/// that run ends.
pub fn candidate_blocking(
    tracker: &Tracker,
    root: &Path,
    label: &str,
    busy: &[u64],
) -> Result<Option<Issue>, String> {
    if label.trim().is_empty() {
        return Ok(None);
    }
    Ok(tracker
        .labelled_blocking(root, label)?
        .into_iter()
        .filter(|issue| !busy.contains(&issue.number))
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
mod tests;
