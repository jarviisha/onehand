//! A project's own issues: kept by onehand, written by hand.
//!
//! **Per project, in the workspace's storage directory**, beside the workspace
//! file rather than inside the project. A file written into the checkout would
//! leave a clean repository dirty, show up in that project's own tree and
//! change count, and travel with every clone — and what is kept here is the
//! user's notes about the project, not part of it. The cost is the one every
//! other piece of workspace state already pays: a workspace bound to no storage
//! keeps no issues.
//!
//! **One file per project, rewritten whole**, because a project's issues are a
//! handful to a few hundred short records and a list that small is cheaper to
//! read and write in one piece than to index. Every change goes through
//! [`update_blocking`], which reads, changes and writes under one lock, so two
//! writers in this process — a person editing and a run leaving a note — cannot
//! each write back a copy missing the other's change.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub mod sync;
pub mod template;

/// Every issue a project has, and the number the next one takes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issues {
    /// The number the next issue is given. Kept rather than worked out from the
    /// list, so a number is never handed out twice even if issues are one day
    /// removed.
    #[serde(default)]
    next: u64,
    #[serde(default)]
    issues: Vec<LocalIssue>,
    /// The connector this project's issues are kept in step with, by name, or
    /// `None` while they are not. Switching it off keeps every link, so
    /// switching it back on picks up where it stopped.
    #[serde(default)]
    synced_with: Option<String>,
    /// When the last sync that reached the forge finished, in seconds since
    /// the epoch; `0` before the first. What a sync asks the forge for
    /// changes since, so a closed issue edited there is still seen.
    #[serde(default)]
    last_synced: u64,
    /// How many times this file has been written. A reader holding two copies
    /// keeps the one with the higher revision: two writes can land on the
    /// screen in the other order from the one they reached the disk in.
    #[serde(default)]
    revision: u64,
}

/// One issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalIssue {
    /// Its number within the project: `#1`, `#2`, …
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "open_by_default")]
    pub open: bool,
    #[serde(default)]
    pub labels: Vec<String>,
    /// When it was opened and last changed, in seconds since the Unix epoch.
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    /// What has been said about it since it was opened, oldest first: a run
    /// starting on it, how that run ended.
    #[serde(default)]
    pub notes: Vec<Note>,
    /// The issue on a forge this one is kept in step with, if any.
    #[serde(default)]
    pub link: Option<Link>,
    /// The connector it was brought in from, if it was brought in rather than
    /// written here. Kept after the link is gone: an issue somebody else wrote
    /// stays somebody else's, and whether a run may take it turns on that.
    #[serde(default)]
    pub(crate) imported_from: Option<String>,
}

/// What an issue says that both sides of a sync keep: everything but its
/// number, its notes and its stamps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub open: bool,
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Snapshot {
    /// Whether the two say the same thing. Labels are compared as a set: a
    /// forge is free to hand them back in another order, and an order is not
    /// something either side said.
    pub(crate) fn same_as(&self, other: &Snapshot) -> bool {
        self.title == other.title
            && self.body == other.body
            && self.open == other.open
            && self.labels.iter().all(|l| other.labels.contains(l))
            && other.labels.iter().all(|l| self.labels.contains(l))
    }

    /// The snapshot with the differences that are not differences taken out —
    /// surrounding space, and Windows line endings, which a forge hands back
    /// for text typed there. Left in, every sync would see an edit nobody made.
    pub fn normalized(mut self) -> Self {
        self.title = self.title.trim().to_string();
        self.body = self.body.replace("\r\n", "\n").trim().to_string();
        self
    }
}

/// The issue on a forge a local one is kept in step with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The connector's name, as it says it.
    pub connector: String,
    /// What the connector identifies the issue by.
    pub key: String,
    /// How a person refers to it there: `#123`.
    pub reference: String,
    /// What both sides said at the last sync that settled — the common
    /// ancestor every later change on either side is measured against.
    pub base: Snapshot,
    /// What the forge says, when it and this side both changed the same thing
    /// differently since `base`. What merged cleanly is still brought in; the
    /// fields in conflict keep this side's value, and nothing is sent to the
    /// forge until a person decides.
    #[serde(default)]
    pub conflict: Option<Snapshot>,
}

/// One thing said about an issue, and when: a run starting or ending on it, its
/// state changing, a session taking it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: u64,
    pub text: String,
    /// The conversation working the issue, by the agent's session id, for a
    /// note that says one took it up — what lets the issue name the session
    /// and open it again after onehand has restarted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl Note {
    /// An issue closed or reopened at `now`, on the forge named `on` if the
    /// change was made there rather than here.
    fn state(open: bool, on: Option<&str>, now: u64) -> Self {
        let did = if open { "Reopened" } else { "Closed" };
        Note {
            at: now,
            text: match on {
                Some(forge) => format!("{did} on {forge}"),
                None => did.to_string(),
            },
            session: None,
        }
    }
}

impl LocalIssue {
    /// How it came to be kept here, for the first line of its history, which
    /// `created` dates: brought in from a forge, or opened here.
    pub fn arrival(&self) -> String {
        match (&self.imported_from, &self.link) {
            (Some(forge), Some(link)) if &link.connector == forge => {
                format!("Brought in from {forge} as {}", link.reference)
            }
            (Some(forge), _) => format!("Brought in from {forge}"),
            (None, _) => "Opened here".to_string(),
        }
    }

    /// Its link to the connector named `name`, if it has one.
    pub(crate) fn link_on(&self, name: &str) -> Option<&Link> {
        self.link.as_ref().filter(|link| link.connector == name)
    }

    /// How a person names it: the forge's reference once it is on one. `None`
    /// is a draft that has not left onehand. The number it is filed under here
    /// is a key and nothing else — shown beside a forge's own, the two read as
    /// two issues, and they disagree as soon as one was opened on the forge.
    pub fn reference(&self) -> Option<&str> {
        self.link.as_ref().map(|link| link.reference.as_str())
    }

    /// Whether it was written here — never linked to anything, never brought
    /// in from anywhere. The only issues a search may take as the user's own
    /// without asking a forge.
    pub(crate) fn written_here(&self) -> bool {
        self.link.is_none() && self.imported_from.is_none()
    }

    /// What this issue says, as a sync compares it.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            title: self.title.clone(),
            body: self.body.clone(),
            open: self.open,
            labels: self.labels.clone(),
        }
    }

    /// Take what `from` says of this issue, noting a change of state, since
    /// one made on the forge is as much a part of its history as one made here.
    fn apply(&mut self, said: &Snapshot, from: &str, now: u64) {
        if self.open != said.open {
            self.notes.push(Note::state(said.open, Some(from), now));
        }
        self.title = said.title.clone();
        self.body = said.body.clone();
        self.open = said.open;
        self.labels = said.labels.clone();
    }
}

fn open_by_default() -> bool {
    true
}

/// What a person typed into the form: everything about an issue that is theirs
/// to say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}

impl Draft {
    /// The draft as it will be kept — title and body trimmed — or why it cannot
    /// be. An issue with no title is a row with nothing to click on.
    fn settled(self) -> Result<Self, String> {
        let title = self.title.trim().to_string();
        if title.is_empty() {
            return Err("An issue needs a title.".to_string());
        }
        Ok(Self {
            title,
            body: self.body.trim().to_string(),
            labels: self.labels,
        })
    }
}

/// `"bug, ui , bug"` as `["bug", "ui"]`: comma-separated, trimmed, blanks and
/// repeats dropped, in the order first written.
pub fn parse_labels(text: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for label in text.split(',').map(str::trim).filter(|l| !l.is_empty()) {
        if !labels.iter().any(|l| l == label) {
            labels.push(label.to_string());
        }
    }
    labels
}

impl Issues {
    /// Open a new issue from `draft` at `now`, and say its number.
    pub fn create(&mut self, draft: Draft, now: u64) -> Result<u64, String> {
        let draft = draft.settled()?;
        let number = self.take_number();
        self.issues.push(LocalIssue {
            number,
            title: draft.title,
            body: draft.body,
            open: true,
            labels: draft.labels,
            created: now,
            updated: now,
            notes: Vec::new(),
            link: None,
            imported_from: None,
        });
        Ok(number)
    }

    /// The number the next issue gets, taken so it is never handed out again.
    fn take_number(&mut self) -> u64 {
        let highest = self.issues.iter().map(|i| i.number).max().unwrap_or(0);
        let number = self.next.max(highest + 1).max(1);
        self.next = number + 1;
        number
    }

    /// How many times the file these came from had been written.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether these issues are kept in step with the connector named `name`.
    pub fn in_step_with(&self, name: &str) -> bool {
        self.synced_with() == Some(name)
    }

    /// Replace what issue `number` says with `draft`.
    pub fn edit(&mut self, number: u64, draft: Draft, now: u64) -> Result<(), String> {
        let draft = draft.settled()?;
        let issue = self.find_mut(number)?;
        issue.title = draft.title;
        issue.body = draft.body;
        issue.labels = draft.labels;
        issue.updated = now;
        Ok(())
    }

    /// Close or reopen issue `number`.
    pub fn set_open(&mut self, number: u64, open: bool, now: u64) -> Result<(), String> {
        let issue = self.find_mut(number)?;
        if issue.open != open {
            issue.open = open;
            issue.updated = now;
            issue.notes.push(Note::state(open, None, now));
        }
        Ok(())
    }

    /// Say on issue `number` that the conversation `session` took it up, in
    /// the words `text`.
    pub fn taken_up(
        &mut self,
        number: u64,
        text: &str,
        session: String,
        now: u64,
    ) -> Result<(), String> {
        let issue = self.find_mut(number)?;
        issue.notes.push(Note {
            at: now,
            text: text.to_string(),
            session: Some(session),
        });
        issue.updated = now;
        Ok(())
    }

    /// Add `text` to what has been said about issue `number`.
    pub fn note(&mut self, number: u64, text: &str, now: u64) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("A note needs something to say.".to_string());
        }
        let issue = self.find_mut(number)?;
        issue.notes.push(Note {
            at: now,
            text: text.to_string(),
            session: None,
        });
        issue.updated = now;
        Ok(())
    }

    /// Take `label` off issue `number`, if it carries it.
    pub fn remove_label(&mut self, number: u64, label: &str, now: u64) -> Result<(), String> {
        let issue = self.find_mut(number)?;
        let before = issue.labels.len();
        issue.labels.retain(|l| l != label);
        if issue.labels.len() != before {
            issue.updated = now;
        }
        Ok(())
    }

    /// The connector these issues are kept in step with, if any.
    pub(crate) fn synced_with(&self) -> Option<&str> {
        self.synced_with.as_deref()
    }

    /// Keep these issues in step with `connector`, or stop.
    pub fn sync_with(&mut self, connector: Option<String>) {
        self.synced_with = connector;
    }

    /// Settle issue `number`'s conflict: keep what it says here, or take what
    /// the forge says.
    ///
    /// Either way the forge's side becomes the common ancestor, which is all it
    /// takes — with the ancestor equal to the forge, the next sync sees only
    /// this side as changed and pushes it, and taking the forge's makes both
    /// sides equal so there is nothing left to move.
    pub fn resolve(&mut self, number: u64, keep_mine: bool, now: u64) -> Result<(), String> {
        let issue = self.find_mut(number)?;
        let ours = issue.snapshot();
        let Some(link) = issue.link.as_mut() else {
            return Err(format!("#{number} is not kept in step with anything."));
        };
        let Some(theirs) = link.conflict.take() else {
            return Ok(());
        };
        let in_conflict = sync::merge(&link.base, &ours, &theirs).conflicts;
        link.base = theirs.clone();
        let from = link.connector.clone();
        if !keep_mine {
            // Only what was in dispute: a field nobody disagreed about keeps
            // what it says here, and the next sync sends it.
            let mut said = issue.snapshot();
            for field in in_conflict {
                field.take(&mut said, &theirs);
            }
            issue.apply(&said, &from, now);
        }
        issue.updated = now;
        Ok(())
    }

    pub fn get(&self, number: u64) -> Option<&LocalIssue> {
        self.issues.iter().find(|i| i.number == number)
    }

    /// Every issue in the order a list draws them: the open ones first, then
    /// the closed, each newest first. Open first because a closed issue is a
    /// record and an open one is work, and the list is read for the work.
    pub fn listed(&self) -> Vec<&LocalIssue> {
        let mut listed: Vec<&LocalIssue> = self.issues.iter().collect();
        listed.sort_by_key(|i| (!i.open, std::cmp::Reverse(i.number)));
        listed
    }

    fn find_mut(&mut self, number: u64) -> Result<&mut LocalIssue, String> {
        self.issues
            .iter_mut()
            .find(|i| i.number == number)
            .ok_or_else(|| format!("There is no issue #{number}."))
    }
}

/// One open issue in a list drawn across every project of a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcrossRow {
    /// The project it belongs to.
    pub root: PathBuf,
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
    /// The forge's name for it, where it is kept in step with one.
    pub reference: Option<String>,
    pub updated: u64,
}

/// The open issues of several projects, as one list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Across {
    /// Open issues, the most recently changed first, at most the cap.
    pub rows: Vec<AcrossRow>,
    /// Closed issues, counted rather than listed: the list is read for work.
    pub closed: usize,
    /// How many open issues the cap left out.
    pub left_out: usize,
}

/// Every open issue in `files` — each a project's root and what its file
/// holds — in one list, the most recently changed first and at most `cap`.
///
/// Only the projects' own files are read. An issue brought in from a forge is
/// kept in the same file as a linked issue, so reading the forge as well would
/// list it twice.
pub fn open_across(files: Vec<(PathBuf, Issues)>, cap: usize) -> Across {
    let mut across = Across::default();
    for (root, issues) in files {
        for issue in issues.issues {
            if !issue.open {
                across.closed += 1;
                continue;
            }
            across.rows.push(AcrossRow {
                root: root.clone(),
                number: issue.number,
                title: issue.title,
                labels: issue.labels,
                reference: issue.link.map(|link| link.reference),
                updated: issue.updated,
            });
        }
    }
    across
        .rows
        .sort_by_key(|row| std::cmp::Reverse(row.updated));
    across.left_out = across.rows.len().saturating_sub(cap);
    across.rows.truncate(cap);
    across
}

/// Whether `issue` is found by the search `query`. A query starting with `#`
/// is a forge reference and matches by its start, so `#1` finds `#12` while
/// typing; anything else is looked for in the title, case aside.
pub fn matches_query(issue: &LocalIssue, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        true
    } else if query.starts_with('#') {
        issue
            .reference()
            .is_some_and(|reference| reference.starts_with(query))
    } else {
        issue.title.to_lowercase().contains(&query.to_lowercase())
    }
}

/// Where the issues of the project at `root` are kept, in the workspace whose
/// storage directory is `storage`.
///
/// Named by the project's folder and a digest of its whole path, as a
/// workspace's own storage folder is: two checkouts of one repository are two
/// projects with one folder name.
pub fn file_for(storage: &Path, root: &Path) -> PathBuf {
    storage
        .join("issues")
        .join(format!("{}.json", crate::workspace::stem_for(root)))
}

/// An issue onehand keeps, named by the file it is kept in ([`file_for`]) and
/// its number there. Never by the project and number alone: two workspaces
/// can open one project, and each keeps an issue 12 of its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueKey {
    pub file: PathBuf,
    pub number: u64,
}

/// The issues kept in `file`. A file that is not there yet is a project with
/// no issues, not a failure.
pub fn load_blocking(file: &Path) -> Result<Issues, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|err| format!("{} could not be read: {err}", file.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Issues::default()),
        Err(err) => Err(format!("{} could not be read: {err}", file.display())),
    }
}

/// Serializes every read-change-write in this process, so no two can
/// interleave and lose one another's change.
static WRITING: Mutex<()> = Mutex::new(());

/// Read `file`, apply `change`, and write it back if the change succeeded —
/// all under one lock. Hands back what is now kept and what `change` said.
///
/// A change that fails writes nothing. A file that cannot be read is never
/// written over: an unreadable file is somebody's issues in a shape this build
/// does not understand, and replacing it with an empty list would delete them.
pub fn update_blocking<R>(
    file: &Path,
    change: impl FnOnce(&mut Issues) -> Result<R, String>,
) -> Result<(Issues, R), String> {
    let _held = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut issues = load_blocking(file)?;
    let said = change(&mut issues)?;
    issues.revision += 1;
    save_blocking(file, &issues)?;
    Ok((issues, said))
}

/// Write `issues` to `file` so a reader never sees half of it.
fn save_blocking(file: &Path, issues: &Issues) -> Result<(), String> {
    let text = serde_json::to_string_pretty(issues)
        .map_err(|err| format!("the issues could not be written out: {err}"))?;
    crate::config::write_atomic(file, &text)
        .map_err(|err| format!("{} could not be written: {err}", file.display()))
}

/// The prompt that works `issue` in the checkout the project is open on, with
/// no branch or worktree of its own, and leaves what it changed for a person
/// to read before anything is committed — the checkout is somebody's working
/// copy, with whatever else they had going in it.
pub fn work_here_prompt(issue: &LocalIssue) -> String {
    let named = match &issue.link {
        Some(link) => format!("{} issue {}", link.connector, link.reference),
        None => "this issue, which is kept in onehand rather than on a forge".to_string(),
    };
    format!(
        "Work {named} in this checkout.\n\n\
         Title: {title}\n\n\
         {body}\n\n\
         ---\n\n\
         Work on the branch that is checked out, in this directory.\n\n\
         1. Do not create a branch or a worktree, and do not switch branches.\n\
         2. Read the repository's own agent instructions, and whatever they point at, \
         and follow its conventions.\n\
         3. Run the repository's checks.\n\
         4. Leave your changes uncommitted for review: do not commit, push or open a \
         pull request.\n\
         5. If the issue needs a decision from a person, ask it with your tool for \
         asking the user a question. Do not guess.\n",
        title = issue.title,
        body = issue.body.trim(),
    )
}

/// Say on the issue `number` kept in `file` that the conversation `session`
/// took it up, in the words `text`. Blocking: one read-change-write.
pub fn taken_up_blocking(
    file: &Path,
    number: u64,
    text: &str,
    session: String,
) -> Result<(), String> {
    update_blocking(file, |kept| kept.taken_up(number, text, session, now())).map(drop)
}

/// The time now, in the unit issues are stamped in.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
