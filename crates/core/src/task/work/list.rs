//! The Issues page's list: which issues it shows, in what order, and what
//! each row and the list's head say.
//!
//! One function, [`list`], decides all of it from what the page holds — the
//! issues, their work, what was read of each project's pull requests, the
//! filters, and the list as it is on screen — so the page draws what it is
//! told and decides nothing.
//!
//! **The list does not move under a person.** A row keeps its place while
//! its run changes state; a row that stops matching a filter stays, saying
//! where it went, until another issue is picked or a filter changes; a new
//! match is added below the rows already there; and an issue opened from
//! elsewhere that the filters leave out is pinned at the top.

use super::{pr_said, IssueWork, Stand};
use crate::connector::{PrState, PullRequest};
use crate::issues::{matches_query, IssueKey, LocalIssue};
use crate::task::Group;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How many rows the list draws. The rest are counted, and said.
pub(crate) const LIST_CAP: usize = 500;

/// How old a project's pull request reading may be, in seconds, before the
/// list stops trusting it to be complete.
pub const READ_AGE: u64 = 60;

/// Which issues the progress filter lets through, by where their work
/// stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Progress {
    #[default]
    All,
    /// Any of its tasks waits on a person, or ended on something nobody
    /// chose.
    NeedsAttention,
    /// Its newest task runs, a run waiting on its pull request's status
    /// checks included: that waits on the forge, not on a person.
    Running,
    Queued,
    /// Its newest task is done and its pull request is open on the forge.
    PullRequestOpen,
    NoRunRecorded,
}

impl Progress {
    /// Every choice, in the order the filter offers them.
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::NeedsAttention,
        Self::Running,
        Self::Queued,
        Self::PullRequestOpen,
        Self::NoRunRecorded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsAttention => "Needs attention",
            Self::Running => "Running",
            Self::Queued => "Queued",
            Self::PullRequestOpen => "Pull request open",
            Self::NoRunRecorded => "No run recorded",
        }
    }
}

/// What the list is narrowed to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// The closed issues rather than the open ones.
    pub closed: bool,
    pub progress: Progress,
    /// One project, by its root.
    pub project: Option<PathBuf>,
    pub label: Option<String>,
    /// The search, as [`matches_query`] reads it.
    pub query: String,
}

/// One issue the page holds, with its work as the app told it.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub root: &'a Path,
    pub key: IssueKey,
    pub issue: &'a LocalIssue,
    pub work: Option<&'a IssueWork>,
}

/// What one project's pull requests were read to be: one read of them all,
/// capped, and the branches a capped read missed, looked up one by one.
#[derive(Debug, Clone, Default)]
pub struct PrReads {
    /// When the last read finished, in seconds past the epoch; `None` before
    /// one has.
    pub at: Option<u64>,
    /// Why the last read failed, when it did.
    pub failed: Option<String>,
    /// The read reached its cap, so a branch missing from it proves nothing.
    pub capped: bool,
    /// What the read found, by the branch each was opened from.
    pub by_branch: HashMap<String, PullRequest>,
    /// Branches looked up on their own, and what each came to.
    pub looked_up: HashMap<String, Option<PullRequest>>,
}

impl PrReads {
    /// The pull request of `branch`: `Some(Some(_))` found, `Some(None)` read
    /// and none there, `None` not read. A project whose last read failed
    /// vouches for nothing: its rows are not read.
    pub(crate) fn of(&self, branch: &str) -> Option<Option<&PullRequest>> {
        if self.failed.is_some() {
            return None;
        }
        if let Some(pr) = self.by_branch.get(branch) {
            return Some(Some(pr));
        }
        if let Some(found) = self.looked_up.get(branch) {
            return Some(found.as_ref());
        }
        (self.at.is_some() && !self.capped).then_some(None)
    }

    /// Of `branches`, the ones a capped read did not find and nothing looked
    /// up yet, at most `cap` of them: what to look up one by one.
    pub fn to_look_up<'b>(
        &self,
        branches: impl IntoIterator<Item = &'b str>,
        cap: usize,
    ) -> Vec<String> {
        if !self.capped || self.failed.is_some() {
            return Vec::new();
        }
        let mut wanted: Vec<String> = Vec::new();
        for branch in branches {
            if self.of(branch).is_none() && !wanted.iter().any(|w| w == branch) {
                wanted.push(branch.to_string());
            }
        }
        wanted.truncate(cap);
        wanted
    }
}

/// The list as it is on screen, which [`list`] keeps still.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    /// The rows on screen, in order; empty for a list drawn afresh, which a
    /// change of filter is.
    pub order: Vec<IssueKey>,
    /// An issue opened from elsewhere, shown even when the filters leave it
    /// out.
    pub pinned: Option<IssueKey>,
}

/// One row.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub item: &'a Item<'a>,
    /// Its line of work, in the Tasks page's words; `None` with no run
    /// recorded, which is left quiet.
    pub line: Option<String>,
    /// The line needs the person: drawn in the warning ink.
    pub attention: bool,
    /// Its newest task does not need the person but an older one does.
    pub earlier_attention: bool,
    /// It no longer matches the filters and stays until another issue is
    /// picked or a filter changes: where it went, *now Running*.
    pub left: Option<String>,
    /// Pinned at the top though the filters leave it out.
    pub outside: bool,
}

/// What a list of rows cannot vouch for, under *Pull request open*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Incomplete {
    /// Issues whose pull request was not read.
    pub not_read: usize,
    /// A reading it rests on is missing or older than [`READ_AGE`].
    pub stale: bool,
}

/// The list and what its head says.
#[derive(Debug, Clone)]
pub struct Listed<'a> {
    pub rows: Vec<Row<'a>>,
    /// How many rows [`LIST_CAP`] left out.
    pub left_out: usize,
    /// How many issues the other filters let through, open and closed: the
    /// counts the open/closed switch carries.
    pub open: usize,
    pub closed: usize,
    /// The projects whose pull request read failed.
    pub failed: Vec<PathBuf>,
    /// Under *Pull request open*, when the list may leave some out.
    pub incomplete: Option<Incomplete>,
}

impl Listed<'_> {
    /// The rows on screen, to hold the list still on the next draw.
    pub fn order(&self) -> Vec<IssueKey> {
        self.rows
            .iter()
            .filter(|row| !row.outside)
            .map(|row| row.item.key.clone())
            .collect()
    }

    /// The rows that stopped matching and stay until another issue is
    /// picked.
    pub fn stopped_matching(&self) -> Vec<IssueKey> {
        self.rows
            .iter()
            .filter(|row| !row.outside && row.left.is_some())
            .map(|row| row.item.key.clone())
            .collect()
    }
}

impl Held {
    /// `picked` was picked: the rows of `stopped` that stopped matching
    /// leave, but never the one picked, which stays selected where it is;
    /// and a pin on another issue goes.
    pub fn pick(&mut self, picked: &IssueKey, stopped: &[IssueKey]) {
        self.order
            .retain(|key| key == picked || !stopped.contains(key));
        if self.pinned.as_ref() != Some(picked) {
            self.pinned = None;
        }
    }
}

/// The list the page draws: `items` narrowed by `filters`, held still by
/// `held`, with each project's pull requests as `reads` has them.
pub fn list<'a>(
    items: &'a [Item<'a>],
    filters: &Filters,
    reads: &HashMap<PathBuf, PrReads>,
    held: &Held,
    now: u64,
) -> Listed<'a> {
    let pr = |item: &Item<'_>| pr_of(item, reads);
    let base = |item: &Item<'_>| {
        filters
            .project
            .as_deref()
            .is_none_or(|only| item.root == only)
            && filters
                .label
                .as_ref()
                .is_none_or(|label| item.issue.labels.contains(label))
            && matches_query(item.issue, &filters.query)
    };
    let progress = |item: &Item<'_>| in_progress(item, filters.progress, pr(item));
    let matches =
        |item: &Item<'_>| base(item) && progress(item) && item.issue.open != filters.closed;

    let through: Vec<&Item<'_>> = items
        .iter()
        .filter(|item| base(item) && progress(item))
        .collect();
    let open = through.iter().filter(|item| item.issue.open).count();
    let closed = through.len() - open;

    let row = |item: &'a Item<'a>| {
        let work = item.work.map(|work| &work.work);
        let attention = work.is_some_and(|w| w.group.needs_attention());
        Row {
            item,
            line: item.work.map(|work| line(work, pr(item))),
            attention,
            earlier_attention: !attention
                && item
                    .work
                    .is_some_and(|work| work.earlier.iter().any(|e| e.attention)),
            left: None,
            outside: false,
        }
    };

    let mut rows: Vec<Row<'a>> = Vec::new();
    let mut placed: HashSet<&IssueKey> = HashSet::new();
    for key in &held.order {
        let Some(item) = items.iter().find(|item| &item.key == key) else {
            continue;
        };
        if !placed.insert(&item.key) {
            continue;
        }
        let mut drawn = row(item);
        if !matches(item) {
            drawn.left = Some(format!("now {}", where_it_is(item, pr(item))));
        }
        rows.push(drawn);
    }
    let mut fresh: Vec<&'a Item<'a>> = items
        .iter()
        .filter(|item| matches(item) && !placed.contains(&item.key))
        .collect();
    fresh.sort_by(|a, b| {
        b.issue
            .updated
            .cmp(&a.issue.updated)
            .then_with(|| a.root.cmp(b.root))
            .then_with(|| a.key.number.cmp(&b.key.number))
    });
    rows.extend(fresh.into_iter().map(row));

    let pinned = held
        .pinned
        .as_ref()
        .filter(|pinned| !rows.iter().any(|row| &row.item.key == *pinned))
        .and_then(|pinned| items.iter().find(|item| &item.key == pinned));
    if let Some(item) = pinned {
        let mut drawn = row(item);
        drawn.outside = true;
        rows.insert(0, drawn);
    }

    let left_out = rows.len().saturating_sub(LIST_CAP);
    rows.truncate(LIST_CAP);

    let mut failed: Vec<PathBuf> = Vec::new();
    for item in items {
        if reads.get(item.root).is_some_and(|r| r.failed.is_some())
            && !failed.iter().any(|root| root == item.root)
        {
            failed.push(item.root.to_path_buf());
        }
    }

    let incomplete = (filters.progress == Progress::PullRequestOpen)
        .then(|| {
            let waiting: Vec<&Item<'_>> = items
                .iter()
                .filter(|item| base(item) && item.issue.open != filters.closed)
                .filter(|item| item.work.is_some_and(|w| done_with_pr(&w.work)))
                .collect();
            let not_read = waiting.iter().filter(|item| pr(item).is_none()).count();
            let stale = waiting.iter().any(|item| {
                reads
                    .get(item.root)
                    .and_then(|r| r.at)
                    .is_none_or(|at| now.saturating_sub(at) >= READ_AGE)
            });
            Incomplete { not_read, stale }
        })
        .filter(|incomplete| incomplete.not_read > 0 || incomplete.stale);

    Listed {
        rows,
        left_out,
        open,
        closed,
        failed,
        incomplete,
    }
}

/// The pull request of `item`'s newest task: `None` when it should have one
/// and it was not read, `Some(None)` when it has none or should have none.
fn pr_of<'r>(
    item: &Item<'_>,
    reads: &'r HashMap<PathBuf, PrReads>,
) -> Option<Option<&'r PullRequest>> {
    let Some(work) = item.work.map(|work| &work.work) else {
        return Some(None);
    };
    if !work.has_pull_request() {
        return Some(None);
    }
    let branch = work.branch.as_deref()?;
    reads.get(item.root)?.of(branch)
}

/// A done run that should have a pull request to read.
fn done_with_pr(work: &super::Work) -> bool {
    work.stand == Stand::Done && work.has_pull_request()
}

fn in_progress(item: &Item<'_>, progress: Progress, pr: Option<Option<&PullRequest>>) -> bool {
    let Some(issue_work) = item.work else {
        return matches!(progress, Progress::All | Progress::NoRunRecorded);
    };
    let work = &issue_work.work;
    match progress {
        Progress::All => true,
        Progress::NeedsAttention => {
            work.group.needs_attention() || issue_work.earlier.iter().any(|e| e.attention)
        }
        Progress::Running => work.group == Group::Running,
        Progress::Queued => work.group == Group::Queued,
        Progress::PullRequestOpen => {
            done_with_pr(work) && pr.flatten().is_some_and(|pr| pr.state == PrState::Open)
        }
        Progress::NoRunRecorded => false,
    }
}

/// Where an issue that stopped matching went, in a word or two.
fn where_it_is(item: &Item<'_>, pr: Option<Option<&PullRequest>>) -> String {
    if !item.issue.open {
        return "Closed".to_string();
    }
    match item.work {
        Some(work) => line(work, pr),
        None => "No run recorded".to_string(),
    }
}

/// A row's line of work: *Waiting for approval*, *Running · Verify*,
/// *Ended · failed*, *Done · pull request open*.
fn line(work: &IssueWork, pr: Option<Option<&PullRequest>>) -> String {
    let work = &work.work;
    let step = work.step.as_ref().map(|step| step.label.as_str());
    let at = |said: &str| match step {
        Some(step) => format!("{said} · {step}"),
        None => said.to_string(),
    };
    match &work.stand {
        Stand::Running | Stand::StatusChecks => at("Running"),
        Stand::Queued { .. } => "Queued".to_string(),
        Stand::Approval { .. } => super::waiting_said(true).to_string(),
        Stand::Card => super::waiting_said(false).to_string(),
        Stand::Resumable(_) => "Ended · cut off".to_string(),
        Stand::Exhausted { .. } => at("Ended · stuck"),
        Stand::TimedOut(_) => at("Ended · timed out"),
        Stand::Failed { .. } => "Ended · failed".to_string(),
        Stand::ByPerson => "Stopped".to_string(),
        Stand::Done => match pr.flatten() {
            Some(pr) if work.has_pull_request() => format!("Done · pull request {}", pr_said(pr)),
            _ => "Done".to_string(),
        },
    }
}

#[cfg(test)]
mod tests;
