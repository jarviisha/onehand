//! What happens next for what one Issues view reads of the work on its
//! issues, and for the review it answers: when the issue on screen's pull
//! request is read, when the page reads every project's pull requests and
//! looks up the branches a capped read missed, when what the work left is
//! read, and whether a press on the review block goes to the run.
//!
//! **One per view**, the Workbench's Issues tab or the Issues page, since
//! what is on screen is that view's: which issue is shown is told it, as a
//! view fact these reads depend on. It is told the time too.
//!
//! **Never on a timer of its own.** A read is due when what it is about is
//! first shown or moves, when a person asks, or when the window comes back to
//! the front with what is shown older than [`READ_AGE`]. Every read is kept
//! to what asked for it ([`Reading`]): an answer about another issue, an
//! older request or an older run is never shown.
//!
//! **A review answers what was read.** The block draws what the run waited on
//! when it was opened; a press the run no longer waits for there reloads it
//! and says so rather than approving what was not read, and every press that
//! goes carries the visit it was drawn from, which the run checks again.

use super::left::Left;
use super::list::{PrReads, READ_AGE};
use super::{IssueWork, Reading, UnderReview, Waits, Work};
use crate::connector::{self, Connector, PullRequest, PullRequests};
use crate::issues::{self, IssueKey};
use crate::task::marks::Change;
use crate::task::Approval;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many pull requests one read of a project asks for.
pub const PR_LIST_CAP: usize = 200;

/// How many branches a capped read missed are looked up one by one, per
/// read; past it a row is not read, and the list says so.
pub(crate) const LOOKUP_CAP: usize = 20;

/// What a read of a pull request is about: a retry that keeps the task but
/// starts a new run makes every read of the old run stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrAbout {
    issue: IssueKey,
    task: String,
    run: Option<String>,
}

impl PrAbout {
    /// What `work`'s pull request read is about, once its run has reached
    /// its pull request step.
    fn of(work: &IssueWork) -> Option<Self> {
        let w = &work.work;
        w.has_pull_request().then(|| Self {
            issue: work.key.clone(),
            task: w.task.clone(),
            run: w.run.clone(),
        })
    }
}

/// What a read of what the work left is about: its task and run.
type LeftAbout = (String, Option<String>);

/// What the page last read of a work's leavings, for that work.
pub enum LeftSeen<'a> {
    Unread,
    Failed(&'a str),
    /// What it left, and when that was read.
    Read(&'a Left, u64),
}

/// What a press on the review block sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    Continue,
    Revise,
}

/// The review block's state: what it is open on and was read, and what a
/// press came to. How it is drawn (the whole answer, the acceptance, the note
/// being written) is the view's.
#[derive(Default)]
pub struct Review {
    /// The issue it is open on; `None` while closed.
    open: Option<IssueKey>,
    /// The task whose run it reviews.
    task: Option<String>,
    /// The folder its work is read in.
    dir: Option<PathBuf>,
    /// What was read when it was opened, or reloaded by a refused press.
    read: Option<UnderReview>,
    /// A press found the run no longer waiting where it was read.
    changed: bool,
    /// What a press sent, once it went.
    sent: Option<Sent>,
    /// What the step under review changed, as read off git.
    files: Reading<(String, String), Vec<Change>>,
}

impl Review {
    /// Whether it is open on the issue `key`.
    pub fn open_on(&self, key: Option<&IssueKey>) -> bool {
        self.open.is_some() && self.open.as_ref() == key
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn read(&self) -> Option<&UnderReview> {
        self.read.as_ref()
    }

    pub fn changed(&self) -> bool {
        self.changed
    }

    pub fn sent(&self) -> Option<Sent> {
        self.sent
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn files(&self) -> &Reading<(String, String), Vec<Change>> {
        &self.files
    }
}

/// What a press on the review block sends the run, which the view hands to
/// whoever moves runs on.
#[derive(Debug, PartialEq, Eq)]
pub enum Press {
    /// Approve the run's step where it was read.
    Approve(Approval),
    /// Send the run's step back with `note`, where it was read.
    Revise { approval: Approval, note: String },
}

/// What a press on the review block came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Answered {
    /// It goes to the run.
    Sent(Press),
    /// The run waits somewhere else now: the block shows that instead, and
    /// says it changed.
    Reloaded,
    /// The run waits no longer, or nothing was there to answer.
    Refused,
}

/// What the Issues page reads beside what the tab does.
#[derive(Default)]
struct Page {
    /// Each project's pull requests, as last read.
    prs: HashMap<PathBuf, PrReads>,
    /// The generation of each project's read, so an answer to an older one
    /// is dropped.
    asked: HashMap<PathBuf, u64>,
    /// How many projects' reads are out.
    reading: usize,
    /// The page was just shown: its pull requests are read.
    arrived: bool,
    /// What the issue on screen left.
    left: Reading<LeftAbout, Left>,
    /// Where the work stood when it was last read, so a step moving reads it
    /// again.
    seen: Option<String>,
    review: Review,
}

/// What the front end carries out, each read off the UI loop.
pub enum Effect {
    /// Read the pull request of `branch` in `root` from `forge`
    /// ([`Connector::pull_request_for_blocking`]) and report
    /// [`Input::Pr`]. A late answer is **dropped** by its generation: a
    /// newer read, or one about another issue, task or run, supersedes it.
    ReadPr {
        generation: u64,
        root: PathBuf,
        forge: &'static dyn Connector,
        branch: String,
    },
    /// Read up to [`PR_LIST_CAP`] of `root`'s pull requests
    /// ([`Connector::pull_requests_blocking`]) and report [`Input::Prs`]. A
    /// late answer is **dropped** by its project's generation once a newer
    /// read of that project is asked, and counted down either way.
    ReadPrs {
        generation: u64,
        root: PathBuf,
        forge: &'static dyn Connector,
        branches: Vec<String>,
    },
    /// Look up `branch`'s pull request in `root` on its own and report
    /// [`Input::LookedUp`]. A late answer is **dropped** by the generation of
    /// the project read it belongs to.
    LookUp {
        generation: u64,
        root: PathBuf,
        forge: &'static dyn Connector,
        branch: String,
    },
    /// Read what `work` left ([`super::left::left_blocking`]) and report
    /// [`Input::Left`]. A late answer is **dropped** by its generation: a
    /// newer read, or one about another task or run, supersedes it.
    ReadLeft { generation: u64, work: Box<Work> },
    /// Read what the step under review changed in `dir` between the two
    /// marks ([`crate::task::marks::changes_blocking`]) and report
    /// [`Input::ReviewFiles`]. A late answer is **dropped** by its
    /// generation: the block reloaded or closed since supersedes it.
    ReadReviewFiles {
        generation: u64,
        dir: PathBuf,
        span: (String, String),
    },
}

/// What the front end reports back, each at `now` in seconds past the epoch.
pub enum Input {
    Pr {
        generation: u64,
        answer: Result<Option<PullRequest>, String>,
        now: u64,
    },
    Prs {
        generation: u64,
        root: PathBuf,
        forge: &'static dyn Connector,
        branches: Vec<String>,
        answer: Result<PullRequests, String>,
        now: u64,
    },
    LookedUp {
        generation: u64,
        root: PathBuf,
        branch: String,
        answer: Result<Option<PullRequest>, String>,
    },
    Left {
        generation: u64,
        left: Left,
        now: u64,
    },
    ReviewFiles {
        generation: u64,
        read: Result<Vec<Change>, String>,
        now: u64,
    },
}

/// One Issues view's reads of the work and its review. See the module header.
pub struct WorkDrive {
    connectors: &'static [&'static dyn Connector],
    /// The work on every issue this window's projects keep, as last told.
    works: Vec<IssueWork>,
    /// The window came back to the front since the view was last drawn.
    returned: bool,
    /// The issue whose work was last drawn, so opening one reads it.
    shown: Option<IssueKey>,
    /// The work on screen moved since its pull request was last read.
    moved: bool,
    /// The pull request of the work of the issue on screen.
    pr: Reading<PrAbout, Option<PullRequest>>,
    /// `None` for the Workbench's Issues tab.
    page: Option<Page>,
}

impl WorkDrive {
    /// A view's drive; `page` for the Issues page, which reads more.
    pub fn new(connectors: &'static [&'static dyn Connector], page: bool) -> Self {
        Self {
            connectors,
            works: Vec::new(),
            returned: false,
            shown: None,
            moved: false,
            pr: Reading::default(),
            page: page.then(Page::default),
        }
    }

    /// The work on every issue, as last told.
    pub fn works(&self) -> &[IssueWork] {
        &self.works
    }

    /// The work on issue `key`, if a task works it.
    pub fn work_of(&self, key: &IssueKey) -> Option<&IssueWork> {
        self.works.iter().find(|work| work.key == *key)
    }

    /// Whether a read of the issue on screen, or of what it left, is out.
    pub fn reading(&self) -> bool {
        self.pr.reading() || self.page.as_ref().is_some_and(|page| page.left.reading())
    }

    /// The pull request of `work`, as last read for it: never another
    /// issue's, task's or run's.
    pub fn pr_seen(&self, work: Option<&IssueWork>) -> super::PrSeen<'_> {
        let about = work.and_then(PrAbout::of);
        if about.is_none() || self.pr.about() != about.as_ref() {
            return super::PrSeen::Unread;
        }
        match (&self.pr.failed, &self.pr.value) {
            (Some(why), _) => super::PrSeen::Failed(why),
            (None, Some((pr, _))) => super::PrSeen::Read(pr.as_ref()),
            (None, None) => super::PrSeen::Unread,
        }
    }

    /// What was last read of `work`'s pull request and when.
    pub fn pr_value(&self, work: Option<&IssueWork>) -> Option<&(Option<PullRequest>, u64)> {
        let about = work.and_then(PrAbout::of)?;
        (self.pr.about() == Some(&about))
            .then_some(self.pr.value.as_ref())
            .flatten()
    }

    /// Each project's pull requests on the page, as last read.
    pub fn prs(&self) -> Option<&HashMap<PathBuf, PrReads>> {
        self.page.as_ref().map(|page| &page.prs)
    }

    /// When the oldest project's pull requests were read.
    pub fn prs_read_at(&self) -> Option<u64> {
        self.page.as_ref()?.prs.values().filter_map(|r| r.at).min()
    }

    /// Whether a project's pull request read is out.
    pub fn prs_reading(&self) -> bool {
        self.page.as_ref().is_some_and(|page| page.reading > 0)
    }

    /// What `work` left, as the page last read it for it: never another
    /// task's or run's.
    pub fn left_seen(&self, work: &Work) -> LeftSeen<'_> {
        let Some(reading) = self.page.as_ref().map(|page| &page.left) else {
            return LeftSeen::Unread;
        };
        if reading.about() != Some(&(work.task.clone(), work.run.clone())) {
            return LeftSeen::Unread;
        }
        match (&reading.value, &reading.failed) {
            (Some((left, at)), _) => LeftSeen::Read(left, *at),
            (None, Some(why)) => LeftSeen::Failed(why),
            (None, None) => LeftSeen::Unread,
        }
    }

    /// The review block, on the page.
    pub fn review(&self) -> Option<&Review> {
        self.page.as_ref().map(|page| &page.review)
    }

    /// The work on every issue, and `shown`, the issue now selected: its
    /// work moving reads its pull request again.
    pub fn set_works(&mut self, works: &[IssueWork], shown: Option<&IssueKey>) {
        let of = |all: &[IssueWork]| {
            shown.and_then(|key| all.iter().find(|work| work.key == *key).cloned())
        };
        if of(&self.works) != of(works) {
            self.moved = true;
        }
        self.works = works.to_vec();
    }

    /// The window came back to the front.
    pub fn returned(&mut self) {
        self.returned = true;
    }

    /// The view was drawn: coming back to the front reads only what was on
    /// screen then.
    pub fn drawn(&mut self) {
        self.returned = false;
    }

    /// The issue `key` of `root` is drawn at `now` (`None` with no issue
    /// shown): read its pull request when due (never read for it, opened,
    /// its work moved, or the window came back with what is shown older
    /// than [`READ_AGE`]), and on the page what its work left when that
    /// moved.
    #[must_use = "its effects are to be carried"]
    pub fn show(&mut self, root: &Path, key: Option<&IssueKey>, now: u64) -> Vec<Effect> {
        let returned = self.returned;
        let moved = std::mem::take(&mut self.moved);
        let Some(key) = key else {
            self.shown = None;
            return Vec::new();
        };
        // Opening an issue reads it, even one read before another was shown.
        let opened = self.shown.as_ref() != Some(key);
        self.shown = Some(key.clone());
        let Some(work) = self.work_of(key).cloned() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if let Some(about) = PrAbout::of(&work) {
            let old = self
                .pr
                .value
                .as_ref()
                .is_none_or(|(_, at)| now.saturating_sub(*at) >= READ_AGE);
            if self.pr.about() != Some(&about) || opened || moved || (returned && old) {
                effects.extend(self.read_pr(root, &work, about, now));
            }
        }
        if let Some(page) = self.page.as_mut() {
            let standing = standing(&work.work);
            if page.seen.as_ref() != Some(&standing) {
                page.seen = Some(standing);
                effects.push(read_left(page, &work.work));
            }
        }
        effects
    }

    /// Read the pull request of issue `key` of `root` again, asked by a
    /// person.
    #[must_use = "its effects are to be carried"]
    pub fn refresh(&mut self, root: &Path, key: &IssueKey, now: u64) -> Vec<Effect> {
        let Some(work) = self.work_of(key).cloned() else {
            return Vec::new();
        };
        match PrAbout::of(&work) {
            Some(about) => self.read_pr(root, &work, about, now),
            None => Vec::new(),
        }
    }

    /// Read again, on the page, what issue `key`'s work left, asked by a
    /// person.
    #[must_use = "its effects are to be carried"]
    pub fn refresh_left(&mut self, key: &IssueKey) -> Vec<Effect> {
        let work = self.work_of(key).map(|work| work.work.clone());
        match (self.page.as_mut(), work) {
            (Some(page), Some(work)) => vec![read_left(page, &work)],
            (Some(_) | None, None) | (None, Some(_)) => Vec::new(),
        }
    }

    fn read_pr(&mut self, root: &Path, work: &IssueWork, about: PrAbout, now: u64) -> Vec<Effect> {
        let (Some(name), Some(branch)) = (&work.work.forge, work.work.branch.clone()) else {
            return Vec::new();
        };
        let generation = self.pr.ask(about);
        let Some(forge) = connector::named(self.connectors, name) else {
            let why = format!("{name} is not a connector this build has");
            self.pr.land(generation, Err(why), now);
            return Vec::new();
        };
        vec![Effect::ReadPr {
            generation,
            root: root.to_path_buf(),
            forge,
            branch,
        }]
    }

    /// The page was put on screen: its pull requests are read when next
    /// drawn.
    pub fn page_shown(&mut self) {
        if let Some(page) = self.page.as_mut() {
            page.arrived = true;
        }
    }

    /// The page is drawn at `now` over `projects`, each with the file its
    /// issues are kept in and the forge serving it: read every project's
    /// pull requests when due (the page just shown, or the window came back
    /// with the oldest read older than [`READ_AGE`]).
    #[must_use = "its effects are to be carried"]
    pub fn page_drawn(&mut self, projects: &[Project], now: u64) -> Vec<Effect> {
        let old = self
            .prs_read_at()
            .is_none_or(|at| now.saturating_sub(at) >= READ_AGE);
        let returned = self.returned;
        let Some(page) = self.page.as_mut() else {
            return Vec::new();
        };
        let arrived = std::mem::take(&mut page.arrived);
        match arrived || (returned && old) {
            true => self.read_prs(projects),
            false => Vec::new(),
        }
    }

    /// Read every project's pull requests now: one read per project with
    /// work that should have one, never one per row.
    #[must_use = "its effects are to be carried"]
    pub fn read_prs(&mut self, projects: &[Project]) -> Vec<Effect> {
        let wanting = self.wanting(projects);
        let Some(page) = self.page.as_mut() else {
            return Vec::new();
        };
        wanting
            .into_iter()
            .map(|(root, forge, branches)| {
                let generation = page.asked.entry(root.clone()).or_default();
                *generation += 1;
                page.reading += 1;
                Effect::ReadPrs {
                    generation: *generation,
                    root,
                    forge,
                    branches,
                }
            })
            .collect()
    }

    /// Look up, one by one, the branches the last capped reads missed.
    #[must_use = "its effects are to be carried"]
    pub fn read_missing(&mut self, projects: &[Project]) -> Vec<Effect> {
        let wanting = self.wanting(projects);
        let Some(page) = self.page.as_ref() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        for (root, forge, branches) in wanting {
            let wanted = page
                .prs
                .get(&root)
                .map(|read| read.to_look_up(branches.iter().map(String::as_str), LOOKUP_CAP))
                .unwrap_or_default();
            let generation = page.asked.get(&root).copied().unwrap_or_default();
            effects.extend(look_up(&root, forge, wanted, generation));
        }
        effects
    }

    /// The projects with work that should have a pull request and a forge
    /// to read it from, each with the branches it should be on.
    fn wanting(&self, projects: &[Project]) -> Vec<(PathBuf, &'static dyn Connector, Vec<String>)> {
        projects
            .iter()
            .filter_map(|project| {
                let forge = project.forge?;
                let branches: Vec<String> = self
                    .works
                    .iter()
                    .filter(|work| work.key.file == project.file)
                    .filter(|work| work.work.has_pull_request())
                    .filter_map(|work| work.work.branch.clone())
                    .collect();
                (!branches.is_empty()).then(|| (project.root.clone(), forge, branches))
            })
            .collect()
    }

    /// Open the review of `work`, the work of issue `key`, as it waits now.
    #[must_use = "its effects are to be carried"]
    pub fn open_review(&mut self, key: IssueKey, work: &Work) -> Vec<Effect> {
        let Some(page) = self.page.as_mut() else {
            return Vec::new();
        };
        page.review = Review {
            open: Some(key),
            task: Some(work.task.clone()),
            dir: Some(work.dir.clone()),
            read: work.review.clone(),
            ..Review::default()
        };
        read_review_files(&mut page.review).into_iter().collect()
    }

    /// Issue `key` was picked: another issue than the review block's closes
    /// it, the same keeps it. Says whether it closed.
    pub fn pick(&mut self, key: Option<&IssueKey>) -> bool {
        let Some(page) = self.page.as_mut() else {
            return false;
        };
        if page.review.open_on(key) {
            return false;
        }
        page.review = Review::default();
        true
    }

    /// Close the review block.
    pub fn close_review(&mut self) {
        if let Some(page) = self.page.as_mut() {
            page.review = Review::default();
        }
    }

    /// *Continue* on the review block: it goes if the run still waits where
    /// the block read it.
    #[must_use = "its effects are to be carried"]
    pub fn approve(&mut self) -> (Answered, Vec<Effect>) {
        self.answer(None)
    }

    /// *Revise…* with `note`: as [`Self::approve`], and an empty note is not
    /// sent.
    #[must_use = "its effects are to be carried"]
    pub fn revise(&mut self, note: &str) -> (Answered, Vec<Effect>) {
        let note = note.trim();
        if note.is_empty() {
            return (Answered::Refused, Vec::new());
        }
        self.answer(Some(note.to_string()))
    }

    fn answer(&mut self, note: Option<String>) -> (Answered, Vec<Effect>) {
        let now = self.waits_now();
        let Some(page) = self.page.as_mut() else {
            return (Answered::Refused, Vec::new());
        };
        let review = &mut page.review;
        let read = review.read.as_ref().map(|read| &read.at);
        match Waits::of(read, now) {
            Waits::There => {}
            // It waits somewhere else: the block shows that, and says so.
            Waits::Elsewhere(now) => {
                review.read = Some(*now);
                review.changed = true;
                let effects = read_review_files(review).into_iter().collect();
                return (Answered::Reloaded, effects);
            }
            Waits::Nowhere => {
                review.changed = true;
                return (Answered::Refused, Vec::new());
            }
        }
        let (Some(task), Some(read)) = (review.task.clone(), review.read.clone()) else {
            return (Answered::Refused, Vec::new());
        };
        let approval = Approval { task, at: read.at };
        let press = match note {
            None => {
                review.sent = Some(Sent::Continue);
                Press::Approve(approval)
            }
            Some(note) => {
                review.sent = Some(Sent::Revise);
                Press::Revise { approval, note }
            }
        };
        (Answered::Sent(press), Vec::new())
    }

    /// What the run of the review block's issue waits for approval on now.
    fn waits_now(&self) -> Option<UnderReview> {
        let review = &self.page.as_ref()?.review;
        let work = self.work_of(review.open.as_ref()?)?;
        (Some(&work.work.task) == review.task.as_ref())
            .then(|| work.work.review.clone())
            .flatten()
    }

    /// What the world reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Pr {
                generation,
                answer,
                now,
            } => {
                self.pr.land(generation, answer, now);
                Vec::new()
            }
            Input::Prs {
                generation,
                root,
                forge,
                branches,
                answer,
                now,
            } => {
                let Some(page) = self.page.as_mut() else {
                    return Vec::new();
                };
                // Counted down whatever the answer, a stale one included: the
                // read it stood for is no longer out.
                page.reading = page.reading.saturating_sub(1);
                if page.asked.get(&root) != Some(&generation) {
                    return Vec::new();
                }
                let read = page.prs.entry(root.clone()).or_default();
                landed(read, answer, now);
                let wanted = read.to_look_up(branches.iter().map(String::as_str), LOOKUP_CAP);
                look_up(&root, forge, wanted, generation)
            }
            Input::LookedUp {
                generation,
                root,
                branch,
                answer,
            } => {
                let Some(page) = self.page.as_mut() else {
                    return Vec::new();
                };
                if page.asked.get(&root) != Some(&generation) {
                    return Vec::new();
                }
                // A lookup that failed leaves the row not read, which the list
                // says.
                if let (Ok(found), Some(read)) = (answer, page.prs.get_mut(&root)) {
                    read.looked_up.insert(branch, found);
                }
                Vec::new()
            }
            Input::Left {
                generation,
                left,
                now,
            } => {
                if let Some(page) = self.page.as_mut() {
                    page.left.land(generation, Ok(left), now);
                }
                Vec::new()
            }
            Input::ReviewFiles {
                generation,
                read,
                now,
            } => {
                if let Some(page) = self.page.as_mut() {
                    page.review.files.land(generation, read, now);
                }
                Vec::new()
            }
        }
    }
}

/// A project on the Issues page, as its pull request reads need it.
pub struct Project {
    pub root: PathBuf,
    /// The file its issues are kept in.
    pub file: PathBuf,
    /// The forge serving it, once its file is read.
    pub forge: Option<&'static dyn Connector>,
}

impl Project {
    /// `root`, its issues kept under `storage`.
    pub fn new(root: &Path, storage: &Path, forge: Option<&'static dyn Connector>) -> Self {
        Self {
            root: root.to_path_buf(),
            file: issues::file_for(storage, root),
            forge,
        }
    }
}

/// Where `work` stands, in one string a move changes.
fn standing(work: &Work) -> String {
    format!(
        "{}|{:?}|{:?}|{}|{:?}",
        work.task,
        work.run,
        work.step.as_ref().map(|step| step.at),
        work.said,
        work.span
    )
}

fn read_left(page: &mut Page, work: &Work) -> Effect {
    let generation = page.left.ask((work.task.clone(), work.run.clone()));
    Effect::ReadLeft {
        generation,
        work: Box::new(work.clone()),
    }
}

/// Read what the step under review changed, when it is pinned at both ends.
fn read_review_files(review: &mut Review) -> Option<Effect> {
    let span = review.read.as_ref()?.span.clone()?;
    let dir = review.dir.clone()?;
    let generation = review.files.ask(span.clone());
    Some(Effect::ReadReviewFiles {
        generation,
        dir,
        span,
    })
}

fn look_up(
    root: &Path,
    forge: &'static dyn Connector,
    branches: Vec<String>,
    generation: u64,
) -> Vec<Effect> {
    branches
        .into_iter()
        .map(|branch| Effect::LookUp {
            generation,
            root: root.to_path_buf(),
            forge,
            branch,
        })
        .collect()
}

/// Take a read of a project's pull requests. A failure is marked with why
/// and leaves `at` where the last good read put it, so the reading is not
/// taken for fresh and *Read them* reads it again.
fn landed(read: &mut PrReads, answer: Result<PullRequests, String>, now: u64) {
    match answer {
        Ok(found) => {
            read.by_branch = found.by_branch.into_iter().collect();
            read.capped = found.capped;
            read.looked_up.clear();
            read.failed = None;
            read.at = Some(now);
        }
        Err(why) => read.failed = Some(why),
    }
}

#[cfg(test)]
mod tests;
