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

/// One thing said about an issue, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: u64,
    pub text: String,
}

impl LocalIssue {
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

    fn apply(&mut self, said: &Snapshot) {
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
        }
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
        if !keep_mine {
            // Only what was in dispute: a field nobody disagreed about keeps
            // what it says here, and the next sync sends it.
            let mut said = issue.snapshot();
            for field in in_conflict {
                field.take(&mut said, &theirs);
            }
            issue.apply(&said);
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

/// The time now, in the unit issues are stamped in.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str) -> Draft {
        Draft {
            title: title.to_string(),
            ..Draft::default()
        }
    }

    #[test]
    fn open_across_lists_open_issues_newest_changed_first_and_counts_closed() {
        let mut a = Issues::default();
        a.create(draft("a1"), 10).unwrap();
        a.create(draft("a2"), 30).unwrap();
        a.create(draft("a3"), 5).unwrap();
        a.set_open(3, false, 40).unwrap();
        let mut b = Issues::default();
        b.create(draft("b1"), 20).unwrap();
        let files = vec![(PathBuf::from("/a"), a), (PathBuf::from("/b"), b)];

        let all = open_across(files.clone(), 10);
        let seen: Vec<(&str, u64)> = all
            .rows
            .iter()
            .map(|row| (row.title.as_str(), row.updated))
            .collect();
        // One row per open issue across both files, the closed one counted.
        assert_eq!(seen, [("a2", 30), ("b1", 20), ("a1", 10)]);
        assert_eq!(all.rows[1].root, PathBuf::from("/b"));
        assert_eq!((all.closed, all.left_out), (1, 0));

        let capped = open_across(files, 2);
        assert_eq!(capped.rows.len(), 2);
        assert_eq!(capped.left_out, 1);
    }

    #[test]
    fn an_issue_is_named_by_its_forge_reference_and_a_draft_by_nothing() {
        let mut issues = Issues::default();
        let n = issues.create(draft("a"), 1).unwrap();
        assert_eq!(issues.get(n).unwrap().reference(), None);
        issues.find_mut(n).unwrap().link = Some(Link {
            connector: "GitHub".into(),
            key: "6".into(),
            reference: "#6".into(),
            base: Snapshot::default(),
            conflict: None,
        });
        // Filed here as #1, known everywhere as GitHub's #6.
        assert_eq!(issues.get(n).unwrap().reference(), Some("#6"));
    }

    #[test]
    fn issues_are_numbered_from_one_and_never_reuse_a_number() {
        let mut issues = Issues::default();
        assert_eq!(issues.create(draft("a"), 10), Ok(1));
        assert_eq!(issues.create(draft("b"), 11), Ok(2));
        // A list whose counter was lost still does not hand out a taken number.
        issues.next = 0;
        assert_eq!(issues.create(draft("c"), 12), Ok(3));
    }

    #[test]
    fn an_issue_needs_a_title_and_is_kept_trimmed() {
        let mut issues = Issues::default();
        assert!(issues.create(draft("   "), 1).is_err());
        let n = issues
            .create(
                Draft {
                    title: "  Crash on open ".into(),
                    body: "\n steps \n".into(),
                    labels: vec!["bug".into()],
                },
                1,
            )
            .unwrap();
        let issue = issues.get(n).unwrap();
        assert_eq!(
            (issue.title.as_str(), issue.body.as_str()),
            ("Crash on open", "steps")
        );
        assert!(issue.open);
        assert!(
            issues.edit(n, draft(""), 2).is_err(),
            "an edit cannot clear the title"
        );
        assert_eq!(issues.get(n).unwrap().title, "Crash on open");
    }

    #[test]
    fn an_edit_and_a_close_stamp_the_issue() {
        let mut issues = Issues::default();
        let n = issues.create(draft("a"), 1).unwrap();
        issues.edit(n, draft("b"), 5).unwrap();
        assert_eq!(issues.get(n).unwrap().updated, 5);
        issues.set_open(n, false, 9).unwrap();
        assert!(!issues.get(n).unwrap().open);
        assert_eq!(issues.get(n).unwrap().updated, 9);
        // Closing what is already closed changes nothing, the stamp included.
        issues.set_open(n, false, 20).unwrap();
        assert_eq!(issues.get(n).unwrap().updated, 9);
        assert!(issues.set_open(99, true, 1).is_err());
    }

    #[test]
    fn the_list_puts_open_work_first_and_newest_first() {
        let mut issues = Issues::default();
        for title in ["1", "2", "3", "4"] {
            issues.create(draft(title), 1).unwrap();
        }
        issues.set_open(4, false, 2).unwrap();
        issues.set_open(1, false, 2).unwrap();
        let order: Vec<u64> = issues.listed().iter().map(|i| i.number).collect();
        assert_eq!(order, [3, 2, 4, 1]);
    }

    #[test]
    fn a_note_is_added_in_order_and_stamps_the_issue() {
        let mut issues = Issues::default();
        let n = issues.create(draft("a"), 1).unwrap();
        issues.note(n, "  started  ", 4).unwrap();
        issues.note(n, "finished", 7).unwrap();
        let issue = issues.get(n).unwrap();
        let said: Vec<&str> = issue.notes.iter().map(|note| note.text.as_str()).collect();
        assert_eq!(said, ["started", "finished"]);
        assert_eq!((issue.notes[1].at, issue.updated), (7, 7));
        assert!(
            issues.note(n, "   ", 9).is_err(),
            "an empty note says nothing"
        );
        assert!(issues.note(99, "x", 9).is_err());
    }

    #[test]
    fn a_label_comes_off_once_and_only_where_it_was() {
        let mut issues = Issues::default();
        let n = issues
            .create(
                Draft {
                    title: "a".into(),
                    labels: vec!["auto".into(), "ui".into()],
                    ..Draft::default()
                },
                1,
            )
            .unwrap();
        issues.remove_label(n, "auto", 3).unwrap();
        assert_eq!(issues.get(n).unwrap().labels, ["ui"]);
        assert_eq!(issues.get(n).unwrap().updated, 3);
        issues.remove_label(n, "auto", 8).unwrap();
        assert_eq!(
            issues.get(n).unwrap().updated,
            3,
            "nothing came off, nothing changed"
        );
    }

    #[test]
    fn labels_are_split_on_commas_trimmed_and_said_once() {
        assert_eq!(parse_labels("bug, ui , bug,,  "), ["bug", "ui"]);
        assert!(parse_labels("").is_empty());
    }

    #[test]
    fn the_file_is_per_project_under_the_workspace_storage() {
        let storage = Path::new("/store");
        let a = file_for(storage, Path::new("/code/app"));
        let b = file_for(storage, Path::new("/other/app"));
        assert!(a.starts_with("/store/issues"));
        assert_ne!(a, b, "two checkouts with one folder name are two projects");
        assert_eq!(a, file_for(storage, Path::new("/code/app")));
    }

    #[test]
    fn issues_survive_a_round_trip_and_a_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("onehand-issues-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("issues").join("p.json");
        assert_eq!(load_blocking(&file), Ok(Issues::default()));

        let (kept, n) = update_blocking(&file, |issues| issues.create(draft("a"), 1)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(load_blocking(&file), Ok(kept));

        // A change that fails leaves what was there.
        assert!(update_blocking(&file, |issues| issues.edit(9, draft("x"), 2)).is_err());
        assert_eq!(load_blocking(&file).unwrap().get(1).unwrap().title, "a");

        // A file this build cannot read is refused and never written over.
        std::fs::write(&file, "not json").unwrap();
        assert!(update_blocking(&file, |issues| issues.create(draft("b"), 3)).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "not json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
