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
        let highest = self.issues.iter().map(|i| i.number).max().unwrap_or(0);
        let number = self.next.max(highest + 1).max(1);
        self.next = number + 1;
        self.issues.push(LocalIssue {
            number,
            title: draft.title,
            body: draft.body,
            open: true,
            labels: draft.labels,
            created: now,
            updated: now,
        });
        Ok(number)
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
