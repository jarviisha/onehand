//! Unattended runs: an issue worked as a task, one session each, with nobody
//! watching.
//!
//! Everything about a run that can be decided without a window — which issue,
//! what branch, what it is asked, what the issue is told afterwards — and
//! what a run asks of the project's [`Connector`]. The workflow engine runs
//! the steps; the app holds the rest: the tick, the session, the teardown.

use crate::connector::Connector;
use crate::issues;
use crate::workflow::Brief;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod report;

pub use report::{
    could_not_start, pull_request_text, report, review_closed, review_diverged, review_note,
    review_unanswerable, PendingReport, Verdict,
};

/// An issue a run can take.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// What the issue says, word for word.
    pub fn body_text(&self) -> &str {
        &self.body
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
        shown(
            lives_on_forge(self.to_ref()),
            issue.number,
            issue.forge_ref(),
        )
    }

    /// How `issue` is named in a sentence: as [`Tracker::shown`] says, and by
    /// its title where it is kept in onehand only, since *Draft* names none.
    pub fn named(&self, issue: &Issue) -> String {
        named(
            lives_on_forge(self.to_ref()),
            issue.number,
            issue.forge_ref(),
            &issue.title,
        )
    }

    /// How a run's brief names `issue`, in what every step is asked.
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
    fn labelled_blocking(&self, root: &Path, label: &str) -> Result<Vec<IssueRow>, String> {
        match self {
            Self::Forge(c) => c.my_labelled_issues_blocking(root, label),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.written_here() && i.open && i.labels.iter().any(|l| l == label))
                .map(|i| kept_row(i, None))
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
                    .map(|row| row.issue.number.to_string())
                    .collect();
                Ok(kept
                    .listed()
                    .into_iter()
                    .filter(|i| i.open && i.labels.iter().any(|l| l == label))
                    .filter_map(|i| {
                        if i.written_here() {
                            return Some(kept_row(i, None));
                        }
                        let link = i.link_on(forge.name()).filter(|l| mine.contains(&l.key))?;
                        Some(kept_row(i, Some(link.reference.clone())))
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
        let row = kept_row;
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

/// An issue kept here as a row: no author, since only a forge knows who
/// wrote one, and known on its forge as `forge_ref` when it is kept in step.
fn kept_row(i: &issues::LocalIssue, forge_ref: Option<String>) -> IssueRow {
    let issue = Issue::from(i);
    IssueRow {
        issue: match forge_ref {
            Some(reference) => issue.at(reference),
            None => issue,
        },
        author: String::new(),
        labels: i.labels.clone(),
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
/// started, to answer the review on the pull request `answering` when there
/// is one. The comment has to read correctly if nothing follows it, as an
/// automatic claim's does.
pub fn claim_picked_blocking(
    tracker: &Tracker,
    root: &Path,
    row: &IssueRow,
    label: &str,
    answering: Option<&str>,
) -> Result<(), String> {
    if !label.is_empty() && row.carries(label) {
        tracker.remove_label_blocking(root, row.issue.number, label)?;
    }
    tracker.comment_blocking(root, row.issue.number, &picked_claim_comment(answering))
}

/// What a claim says it started: a run on the issue, or one answering the
/// review on its pull request `answering`.
fn started_on(answering: Option<&str>) -> String {
    match answering {
        Some(pr) => format!("to answer the review on {pr}"),
        None => "on this issue".to_string(),
    }
}

/// What an issue is told when a person picks it to be worked.
///
/// Not the automatic claim's sentence: that one says to re-add the trigger
/// label to retry, and a picked issue may never have carried it — or may be
/// somebody else's, which the automatic search never takes at all.
fn picked_claim_comment(answering: Option<&str>) -> String {
    format!(
        "onehand started a run {}, picked by hand. If no outcome follows, the run was \
         interrupted — pick it again to retry.",
        started_on(answering)
    )
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

/// `text` as lowercase ASCII letters and digits joined by single dashes, cut
/// to `max` characters: a valid piece of a branch name whatever `text` holds.
fn slug(text: &str, max: usize) -> String {
    let mut words = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            words.push(ch.to_ascii_lowercase());
        } else if !words.is_empty() && !words.ends_with('-') {
            words.push('-');
        }
    }
    words.truncate(max);
    words.trim_matches('-').to_string()
}

/// How long the name of the forge in a branch may be.
const FORGE_MAX: usize = 20;

/// How long the title words in a branch may be.
const WORDS_MAX: usize = 40;

/// The branch a run on `issue` works on:
/// `onehand/<where it lives>-<its number>-<title words>`.
///
/// **Where the issue lives leads**, because a number alone names nothing: an
/// issue kept in onehand and one on the forge can both be 3, and two issues
/// must never share a branch, a worktree or a pull request. One kept in step
/// with a forge goes by the forge's name and number, which is what the forge
/// calls it. Built from lowercase ASCII letters, digits and single dashes, so
/// a title of nothing but punctuation leaves the number alone, and a long one
/// is cut rather than carried whole into a folder name.
pub fn branch_for(tracker: &Tracker, issue: &Issue) -> String {
    let named = match (tracker, issue.forge_ref()) {
        (Tracker::Forge(forge), _) => {
            format!("{}-{}", slug(forge.name(), FORGE_MAX), issue.number)
        }
        (Tracker::Synced { forge, .. }, Some(reference)) => format!(
            "{}-{}",
            slug(forge.name(), FORGE_MAX),
            slug(reference, FORGE_MAX)
        ),
        (Tracker::Local(_) | Tracker::Synced { .. }, _) => format!("local-{}", issue.number),
    };
    match slug(&issue.title, WORDS_MAX) {
        words if words.is_empty() => format!("onehand/{named}"),
        words => format!("onehand/{named}-{words}"),
    }
}

/// `branch`, or the first of `branch-2` … `branch-9` the repository at `root`
/// does not have yet and `taken` does not say is taken elsewhere.
///
/// An issue whose label was put back after a run left work on its branch gets a
/// branch of its own this time: the old one is still checked out in the
/// worktree that run left on disk, and git refuses a second checkout of it.
/// One the forge still has a pull request on is taken too, though its local
/// branch is gone: a new task pushing there would meet the old one's work.
/// When `taken` cannot tell, that is the error: a guess either way could cut
/// the new task on a branch the old one's pull request is on.
pub fn free_branch_blocking(
    root: &Path,
    branch: &str,
    taken: impl Fn(&str) -> Result<bool, String>,
) -> Result<String, String> {
    for name in std::iter::once(branch.to_string()).chain((2..=9).map(|n| format!("{branch}-{n}")))
    {
        if !crate::worktree::branch_exists_blocking(root, &name) && !taken(&name)? {
            return Ok(name);
        }
    }
    Ok(format!("{branch}-10"))
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

/// What a run on `issue` is asked: the issue's title and body word for word,
/// and, asked of every step, how to name the issue and what to do with a
/// decision nobody is there to make.
///
/// What a person `added` for this run, if anything, follows those
/// instructions: the issue's own text is never rewritten to carry it, and a
/// retry carries it because it is kept on the brief.
///
/// It does not restate the repository's conventions, the commit format or
/// the test commands. Those are in the repository's own instructions, which
/// every step tells the agent to read, and a second copy here would go stale
/// without anybody noticing.
pub fn brief_for(tracker: &Tracker, issue: &Issue, added: &str) -> Brief {
    let mut instructions = format!(
        "This is {}. Nobody is watching this session: if the issue turns out to need a \
         decision from a person, ask it with your tool for asking the user a question, \
         not in your answer, and carry on once it is answered. Do not guess.",
        tracker.names(issue)
    );
    if !added.trim().is_empty() {
        instructions = format!("{instructions}\n\n{}", added.trim());
    }
    Brief {
        title: issue.title.clone(),
        body: issue.body.trim().to_string(),
        instructions: Some(instructions),
    }
}

/// What the issue is told when a run takes it.
///
/// Worded to stay true if nothing follows it. A crash between the claim and
/// the outcome leaves this as the last word on the issue, so it says that a run
/// *started* — the comment's own timestamp says when — and never that one is
/// happening, which is the one sentence a crash makes false.
fn claim_comment(label: &str, answering: Option<&str>) -> String {
    format!(
        "onehand started an unattended run {}. If no outcome follows, the run was \
         interrupted — re-add `{label}` to retry.",
        started_on(answering)
    )
}

/// What a task keeps of the issue it works: enough to tell the issue how each
/// run ended, after a restart too. Where it lives is kept by name, since only
/// the app holds the connectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueSource {
    pub tracker: TrackerRef,
    pub number: u64,
    /// How the forge refers to it, for one kept in step with a forge.
    pub forge_ref: Option<String>,
    /// The connector the project's work goes to, by name; `None` on a project
    /// no forge serves, whose work stays on the branch.
    pub forge: Option<String>,
    /// What the worktree was cut from, which its commits are counted past.
    pub base: String,
    /// A person picked it, rather than the search finding it.
    pub picked: bool,
    /// Reports the issue has not been given yet, oldest first. One is dropped
    /// only once the issue has it, so a report that could not be delivered is
    /// tried again rather than lost with the run.
    #[serde(default)]
    pub unsent: Vec<PendingReport>,
    /// What the start found worth telling whoever reads why the run went
    /// wrong, when nobody was there to read it before: handed to the first
    /// report, and gone from here once it is.
    #[serde(default)]
    pub notes: Vec<String>,
}

impl IssueSource {
    /// How the issue is shown to a person, as [`Tracker::shown`] says.
    pub fn shown(&self) -> String {
        shown(
            lives_on_forge(self.tracker.clone()),
            self.number,
            self.forge_ref.as_deref(),
        )
    }

    /// How the issue titled `title` is named in a sentence, as
    /// [`Tracker::named`] says.
    pub fn named(&self, title: &str) -> String {
        named(
            lives_on_forge(self.tracker.clone()),
            self.number,
            self.forge_ref.as_deref(),
            title,
        )
    }
}

/// Whether an issue living in `tracker` lives on the forge itself.
fn lives_on_forge(tracker: TrackerRef) -> bool {
    match tracker {
        TrackerRef::Forge { .. } => true,
        TrackerRef::Local { .. } | TrackerRef::Synced { .. } => false,
    }
}

/// How an issue is shown to a person: by the forge's number where it lives on
/// the forge or is kept in step with one, and as a draft where it is kept in
/// onehand only — the number it is filed under there is a key, and beside a
/// forge's own it reads as a second issue.
fn shown(on_forge: bool, number: u64, forge_ref: Option<&str>) -> String {
    match (on_forge, forge_ref) {
        (true, _) => format!("#{number}"),
        (false, Some(reference)) => reference.to_string(),
        (false, None) => "Draft".to_string(),
    }
}

/// How an issue is named in a sentence: as [`shown`], or by its title in
/// quotes where it is a draft.
fn named(on_forge: bool, number: u64, forge_ref: Option<&str>, title: &str) -> String {
    match (on_forge, forge_ref) {
        (false, None) => format!("\u{201c}{title}\u{201d}"),
        _ => shown(on_forge, number, forge_ref),
    }
}

/// Where an issue lives, as a task keeps it: [`Tracker`] with its connector
/// named rather than held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackerRef {
    Forge { connector: String },
    Local { file: PathBuf },
    Synced { file: PathBuf, connector: String },
}

impl TrackerRef {
    /// The tracker it names, among the connectors in `all`; `None` when its
    /// connector is not one of them.
    pub fn resolve(&self, all: &[&'static dyn Connector]) -> Option<Tracker> {
        let named = |name: &str| crate::connector::named(all, name);
        Some(match self {
            Self::Forge { connector } => Tracker::Forge(named(connector)?),
            Self::Local { file } => Tracker::Local(file.clone()),
            Self::Synced { file, connector } => Tracker::Synced {
                file: file.clone(),
                forge: named(connector)?,
            },
        })
    }
}

impl Tracker {
    /// The tracker as a task keeps it.
    pub fn to_ref(&self) -> TrackerRef {
        match self {
            Self::Forge(forge) => TrackerRef::Forge {
                connector: forge.name().to_string(),
            },
            Self::Local(file) => TrackerRef::Local { file: file.clone() },
            Self::Synced { file, forge } => TrackerRef::Synced {
                file: file.clone(),
                connector: forge.name().to_string(),
            },
        }
    }
}

/// A task holding one of the slots of unattended runs, and how the slots
/// line shows it: its issue, its workflow and the step it is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub task: String,
    pub shown: String,
}

/// The slots of unattended runs across every window, as they stand: read from
/// the tasks working and starting, never stored.
///
/// A run waiting on a person gives its slot up but keeps its adapter alive,
/// so those are counted apart, against a cap of their own when one is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    /// The issue tasks queued or running, oldest first.
    pub holders: Vec<Holder>,
    /// Issue tasks kept and not yet placed, which count too.
    pub starting: usize,
    pub at_once: u32,
    /// Issue runs waiting on a person.
    pub waiting: usize,
    /// How many may wait at once; `None` for no cap.
    pub waiting_cap: Option<u32>,
}

impl Slots {
    /// The slots line: *Slots: 1 of 1 — #12 · Work an issue (Implement)*.
    pub fn said(&self) -> String {
        let taken = self.holders.len() + self.starting;
        let mut who: Vec<String> = self.holders.iter().map(|h| h.shown.clone()).collect();
        if self.starting > 0 {
            who.push(format!("{} starting", self.starting));
        }
        match who.is_empty() {
            true => format!("Slots: {taken} of {}", self.at_once),
            false => format!("Slots: {taken} of {} — {}", self.at_once, who.join(", ")),
        }
    }

    /// Why no other issue may be taken up now, if none may: every slot is
    /// taken, said with the line so a person knows whom they wait for, or as
    /// many runs wait on a person as may.
    pub fn full(&self) -> Option<String> {
        if self.holders.len() + self.starting >= self.at_once as usize {
            return Some(match self.at_once {
                0 => "Unattended runs are capped at none at once (unattended.at_once).".to_string(),
                _ => format!(
                    "Every slot of unattended runs is taken (unattended.at_once). {}",
                    self.said()
                ),
            });
        }
        let cap = self
            .waiting_cap
            .filter(|cap| self.waiting >= *cap as usize)?;
        Some(format!(
            "{} unattended runs wait on a person, the most allowed at once \
             (unattended.waiting = {cap}); answering one makes room.",
            self.waiting
        ))
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
        Self {
            limit,
            spent: Duration::ZERO,
            since: Some(now),
        }
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
        let running = self
            .since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        self.limit.saturating_sub(self.spent + running)
    }
}

/// The open issues in `tracker` that carry `label` and were opened by the
/// user, oldest first: a search takes the first no task is still working on.
///
/// **An empty label picks nothing**, without asking the connector: the failure of
/// leaving it blank has to be "nothing runs", not "everything runs".
///
/// **Only the user's own issues.** The body goes into the prompt word for word
/// and the agent runs with the user's credentials; a label is
/// something anybody with triage rights can apply, to an issue anybody at all
/// may have written.
pub fn candidates_blocking(
    tracker: &Tracker,
    root: &Path,
    label: &str,
) -> Result<Vec<IssueRow>, String> {
    if label.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut found = tracker.labelled_blocking(root, label)?;
    found.sort_by_key(|row| row.issue.number);
    Ok(found)
}

/// The id of the workflow an issue carrying `labels` is worked with: the one
/// `by_label` names for the first of its labels in that table's order, else
/// `default`.
///
/// The table's order rather than the issue's, because a forge hands labels
/// back in no order it promises, and the same issue must not get a different
/// workflow from one look to the next. **The trigger label never chooses**,
/// even written into the table by hand: every issue a search finds carries it,
/// so it would choose for all of them.
pub fn workflow_for<'a>(
    labels: &[String],
    by_label: &'a BTreeMap<String, String>,
    default: &'a str,
    trigger: &str,
) -> &'a str {
    by_label
        .iter()
        .find(|(label, _)| *label != trigger && labels.contains(label))
        .map_or(default, |(_, id)| id)
}

/// Why `label` cannot be added as a workflow label beside `by_label`, the
/// trigger label being `trigger`: it is empty, it is the trigger label, which
/// never chooses, or it already chooses a workflow.
pub fn workflow_label_refused(
    label: &str,
    trigger: &str,
    by_label: &BTreeMap<String, String>,
) -> Option<String> {
    if label.is_empty() {
        Some("Type the label an issue carries.".to_string())
    } else if label == trigger {
        Some(format!(
            "“{label}” is the trigger label; every issue a run takes carries it, so it \
             cannot choose."
        ))
    } else if by_label.contains_key(label) {
        Some(format!(
            "“{label}” already chooses a workflow; remove it first."
        ))
    } else {
        None
    }
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
    answering: Option<&str>,
) -> Result<(), String> {
    tracker.remove_label_blocking(root, number, label)?;
    tracker.comment_blocking(root, number, &claim_comment(label, answering))
}

#[cfg(test)]
mod tests;
