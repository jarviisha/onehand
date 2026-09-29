//! Keeping a project's own issues in step with a forge's, both ways.
//!
//! **A three-way merge, per field, against what both sides last agreed on.**
//! Each linked issue remembers the [`Snapshot`] its last settled sync ended on,
//! and a sync compares both sides to it: a field only one side changed takes
//! that side's value, one both sides changed to the same thing is settled, and
//! one both changed differently is a **conflict** — nothing about that issue
//! moves either way until a person picks a side. Labels are a set and merge
//! without conflict: what either side added is added, what either removed is
//! removed. Whole-issue "last writer wins" was the alternative, and it loses a
//! change silently whenever two people touch different fields of one issue.
//!
//! **What comes in and what goes out are not symmetric, by decision.** Every
//! open issue on the forge is imported. An issue written here is never sent
//! anywhere until somebody publishes it: what is written in onehand is private
//! until a person says otherwise, and an app that published notes on its own
//! schedule would be one nobody could write drafts in.

use super::{Issues, Link, LocalIssue, Note, Snapshot};
use crate::connector::{Connector, RemoteIssue, SyncListing};
use std::collections::HashSet;
use std::path::Path;

/// How many open issues a sync imports. A repository with more open issues
/// than this is imported up to it, and the report says so.
pub const SYNC_CAP: usize = 500;

/// What a sync did, for the line that says so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub imported: usize,
    pub pulled: usize,
    pub pushed: usize,
    /// Issues left waiting for a person, this sync's and earlier ones'.
    pub conflicts: usize,
    /// Whether the forge had more open issues than [`SYNC_CAP`], so some were
    /// not brought in.
    pub cut: bool,
    /// What could not be done, one line per issue. A failure on one issue
    /// never stops the rest.
    pub failures: Vec<String>,
}

/// A field of an issue two sides can disagree about. Labels are not one: they
/// merge as a set and never conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Title,
    Description,
    State,
}

impl Field {
    /// Set this field of `into` to what `from` says.
    pub(crate) fn take(self, into: &mut Snapshot, from: &Snapshot) {
        match self {
            Self::Title => into.title = from.title.clone(),
            Self::Description => into.body = from.body.clone(),
            Self::State => into.open = from.open,
        }
    }
}

/// The result of merging one issue's two sides against their common ancestor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    /// Every field both sides agree on or only one side changed, and this
    /// side's value for the fields in conflict.
    pub(crate) merged: Snapshot,
    /// The fields both sides changed differently.
    pub conflicts: Vec<Field>,
}

/// Merge `ours` and `theirs` against `base`, field by field.
pub fn merge(base: &Snapshot, ours: &Snapshot, theirs: &Snapshot) -> Merge {
    let mut conflicts = Vec::new();
    let title = field(
        &base.title,
        &ours.title,
        &theirs.title,
        Field::Title,
        &mut conflicts,
    );
    let body = field(
        &base.body,
        &ours.body,
        &theirs.body,
        Field::Description,
        &mut conflicts,
    );
    let open = field(
        &base.open,
        &ours.open,
        &theirs.open,
        Field::State,
        &mut conflicts,
    );
    Merge {
        merged: Snapshot {
            title,
            body,
            open,
            labels: labels(&base.labels, &ours.labels, &theirs.labels),
        },
        conflicts,
    }
}

fn field<T: PartialEq + Clone>(
    base: &T,
    ours: &T,
    theirs: &T,
    name: Field,
    conflicts: &mut Vec<Field>,
) -> T {
    if ours == base || ours == theirs {
        theirs.clone()
    } else if theirs == base {
        ours.clone()
    } else {
        conflicts.push(name);
        ours.clone()
    }
}

/// Labels as a set: what either side added is in, what either side removed is
/// out. In this side's order, then whatever the forge added.
fn labels(base: &[String], ours: &[String], theirs: &[String]) -> Vec<String> {
    let removed: HashSet<&String> = base
        .iter()
        .filter(|l| !ours.contains(l) || !theirs.contains(l))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for label in ours.iter().chain(theirs) {
        if !removed.contains(label) && !out.contains(label) {
            out.push(label.clone());
        }
    }
    out
}

/// Keep the issues in `file` in step with `connector`'s, for the project at
/// `root`. Blocking — it asks the forge — and done under the issue file's
/// lock, so nothing else can write the file while a sync is between reading it
/// and writing it back.
// ponytail: the lock is held across the forge's calls, so an edit made during
// a sync waits for it; per-issue locking if a sync ever takes long enough to
// be felt.
pub fn sync_blocking(
    file: &Path,
    root: &Path,
    connector: &dyn Connector,
    now: u64,
) -> Result<(Issues, Report), String> {
    super::update_blocking(file, |issues| {
        let since = (issues.last_synced > 0).then_some(issues.last_synced);
        let SyncListing {
            mut open,
            mut changed,
        } = connector.issues_for_sync_blocking(root, since, SYNC_CAP + 1)?;
        let cut = open.len() > SYNC_CAP;
        open.truncate(SYNC_CAP);
        // Everything that changed is known only from a sync point, and only if
        // that list was not cut; otherwise each closed issue is asked about.
        let complete = since.is_some() && changed.len() <= SYNC_CAP;
        changed.truncate(SYNC_CAP);
        let mut seen = HashSet::new();
        let remote: Vec<RemoteIssue> = changed
            .into_iter()
            .chain(open)
            .filter(|r| seen.insert(r.key.clone()))
            .collect();
        let (mut report, unasked) = reconcile(issues, root, connector, &remote, complete, now);
        report.cut = cut;
        // Every linked issue was listed or asked about, so the point can move
        // on — unless one could not be asked, which the next sync must ask
        // about again rather than read as unchanged. A push that failed does
        // not hold it: that issue's `base` is held instead, and it is retried
        // from there.
        if !unasked {
            issues.last_synced = now;
        }
        Ok(report)
    })
}

/// The sync itself, over what the forge listed: every open issue, and every
/// issue changed since the last sync whatever its state.
fn reconcile(
    issues: &mut Issues,
    root: &Path,
    connector: &dyn Connector,
    remote: &[RemoteIssue],
    complete: bool,
    now: u64,
) -> (Report, bool) {
    let name = connector.name();
    let mut report = Report::default();
    let mut unasked = false;
    for issue in issues.issues.iter_mut() {
        let Some(link) = issue.link_on(name) else {
            continue;
        };
        let listed = remote.iter().find(|r| r.key == link.key);
        let theirs = match listed {
            Some(r) => r.snapshot.clone(),
            // Not listed, closed on both sides at the last look, and the forge
            // listed everything changed since then: its side is still what was
            // agreed, and only a change made here has anywhere to go.
            None if complete && !issue.open && !link.base.open => link.base.clone(),
            // Otherwise it changed there, closed, or went, or the listing
            // cannot say — and only asking does.
            None => match connector.issue_blocking(root, &link.key) {
                Ok(Some(r)) => r.snapshot,
                Ok(None) => {
                    let reference = link.reference.clone();
                    issue.link = None;
                    issue.notes.push(Note {
                        at: now,
                        text: format!(
                            "{reference} is no longer on {name}; this issue is kept here, \
                             no longer in step with it."
                        ),
                    });
                    continue;
                }
                Err(why) => {
                    report.failures.push(format!("#{}: {why}", issue.number));
                    unasked = true;
                    continue;
                }
            },
        };
        step(
            issue,
            root,
            connector,
            theirs.normalized(),
            now,
            &mut report,
        );
    }

    let linked: HashSet<String> = issues
        .issues
        .iter()
        .filter_map(|i| i.link_on(name))
        .map(|l| l.key.clone())
        .collect();
    // Only open issues are brought in: a closed one was listed because it
    // changed, and it is followed only if it was already linked.
    for r in remote
        .iter()
        .filter(|r| r.snapshot.open && !linked.contains(&r.key))
    {
        import(issues, name, r, now);
        report.imported += 1;
    }
    report.conflicts = issues
        .issues
        .iter()
        .filter(|i| i.link.as_ref().is_some_and(|l| l.conflict.is_some()))
        .count();
    (report, unasked)
}

/// Bring one linked issue into step with what the forge says of it.
fn step(
    issue: &mut LocalIssue,
    root: &Path,
    connector: &dyn Connector,
    theirs: Snapshot,
    now: u64,
    report: &mut Report,
) {
    let Some(link) = issue.link.as_ref() else {
        return;
    };
    let (key, base) = (link.key.clone(), link.base.clone());
    let ours = issue.snapshot();
    let merge = merge(&base, &ours, &theirs);
    if !merge.conflicts.is_empty() {
        // What merged cleanly still lands here; the fields in conflict keep
        // this side's value, and the forge's side is kept to decide against.
        if !merge.merged.same_as(&ours) {
            issue.apply(&merge.merged);
            issue.updated = now;
        }
        if let Some(link) = issue.link.as_mut() {
            link.conflict = Some(theirs);
        }
        return;
    }
    if !merge.merged.same_as(&ours) {
        issue.apply(&merge.merged);
        issue.updated = now;
        report.pulled += 1;
    }
    if !merge.merged.same_as(&theirs) {
        if let Err(why) = connector.update_issue_blocking(root, &key, &theirs, &merge.merged) {
            // The ancestor is left where it was, so the next sync sees this
            // side's change as still to be sent and tries again.
            report.failures.push(format!("#{}: {why}", issue.number));
            if let Some(link) = issue.link.as_mut() {
                link.conflict = None;
            }
            return;
        }
        report.pushed += 1;
    }
    if let Some(link) = issue.link.as_mut() {
        link.base = merge.merged;
        link.conflict = None;
    }
}

/// Take a forge's issue in as a new local one, linked to it.
fn import(issues: &mut Issues, connector: &str, remote: &RemoteIssue, now: u64) {
    let said = remote.snapshot.clone().normalized();
    let number = issues.take_number();
    issues.issues.push(LocalIssue {
        number,
        title: said.title.clone(),
        body: said.body.clone(),
        open: said.open,
        labels: said.labels.clone(),
        created: now,
        updated: now,
        notes: Vec::new(),
        link: Some(Link {
            connector: connector.to_string(),
            key: remote.key.clone(),
            reference: remote.reference.clone(),
            base: said,
            conflict: None,
        }),
        imported_from: Some(connector.to_string()),
    });
}

/// Send issue `number` to `connector` as a new issue there, and link the two.
/// What is written here stays here until this is asked for.
pub fn publish_blocking(
    file: &Path,
    root: &Path,
    connector: &dyn Connector,
    number: u64,
) -> Result<String, String> {
    super::update_blocking(file, |issues| {
        let issue = issues.find_mut(number)?;
        if let Some(link) = &issue.link {
            return Err(format!(
                "#{number} is already {} on {}.",
                link.reference, link.connector
            ));
        }
        let made = connector.create_issue_blocking(root, &issue.snapshot())?;
        let reference = made.reference.clone();
        issue.link = Some(Link {
            connector: connector.name().to_string(),
            key: made.key,
            reference: made.reference,
            base: made.snapshot.normalized(),
            conflict: None,
        });
        Ok(reference)
    })
    .map(|(_, reference)| reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::memory::Forge;
    use crate::issues::Draft;

    fn snap(title: &str, open: bool, labels: &[&str]) -> Snapshot {
        Snapshot {
            title: title.to_string(),
            body: String::new(),
            open,
            labels: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn a_field_only_one_side_changed_takes_that_side() {
        let base = snap("a", true, &[]);
        let ours = snap("mine", true, &[]);
        let theirs = snap("a", false, &[]);
        let m = merge(&base, &ours, &theirs);
        assert_eq!(m.merged, snap("mine", false, &[]));
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn a_field_both_sides_changed_differently_is_a_conflict_and_keeps_ours() {
        let base = snap("a", true, &[]);
        let m = merge(&base, &snap("mine", true, &[]), &snap("theirs", true, &[]));
        assert_eq!(m.conflicts, [Field::Title]);
        assert_eq!(m.merged.title, "mine");
        let same = merge(&base, &snap("x", true, &[]), &snap("x", true, &[]));
        assert!(
            same.conflicts.is_empty(),
            "the same change on both sides agrees"
        );
    }

    #[test]
    fn labels_merge_as_a_set_and_never_conflict() {
        let base = snap("a", true, &["bug", "ui", "old"]);
        let ours = snap("a", true, &["bug", "ui", "mine"]);
        let theirs = snap("a", true, &["bug", "old", "theirs"]);
        let m = merge(&base, &ours, &theirs);
        assert_eq!(m.merged.labels, ["bug", "mine", "theirs"]);
        assert!(m.conflicts.is_empty());
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("onehand-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("issues.json")
    }

    fn load(file: &Path) -> Issues {
        crate::issues::load_blocking(file).unwrap()
    }

    fn edit(file: &Path, number: u64, title: &str) {
        crate::issues::update_blocking(file, |issues| {
            let labels = issues.get(number).unwrap().labels.clone();
            issues.edit(
                number,
                Draft {
                    title: title.into(),
                    labels,
                    ..Draft::default()
                },
                5,
            )
        })
        .unwrap();
    }

    #[test]
    fn open_issues_come_in_linked_and_a_second_sync_changes_nothing() {
        let file = scratch("import");
        let root = Path::new("/");
        let forge = Forge::with(vec![
            (7, snap("Crash", true, &["bug"])),
            (8, snap("Closed long ago", false, &[])),
        ]);
        let report = sync_blocking(&file, root, &forge, 1).unwrap().1;
        assert_eq!((report.imported, report.pushed), (1, 0));
        let kept = load(&file);
        let issue = kept.get(1).unwrap();
        assert_eq!(issue.title, "Crash");
        assert_eq!(issue.link.as_ref().unwrap().reference, "#7");

        let again = sync_blocking(&file, root, &forge, 2).unwrap().1;
        assert_eq!(again, Report::default());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_change_on_either_side_crosses_to_the_other() {
        let file = scratch("cross");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();

        edit(&file, 1, "changed here");
        let report = sync_blocking(&file, root, &forge, 2).unwrap().1;
        assert_eq!(report.pushed, 1);
        assert_eq!(forge.said("7").title, "changed here");

        forge.set("7", snap("changed there", false, &[]));
        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert_eq!(report.pulled, 1);
        let kept = load(&file);
        assert_eq!(kept.get(1).unwrap().title, "changed there");
        assert!(!kept.get(1).unwrap().open, "closing there closes here");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_conflict_moves_nothing_until_a_side_is_picked() {
        let file = scratch("conflict");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();

        edit(&file, 1, "mine");
        forge.set("7", snap("theirs", true, &[]));
        let report = sync_blocking(&file, root, &forge, 2).unwrap().1;
        assert_eq!((report.conflicts, report.pushed, report.pulled), (1, 0, 0));
        assert_eq!(forge.said("7").title, "theirs");
        assert_eq!(load(&file).get(1).unwrap().title, "mine");

        // Keeping mine sends mine on the next sync.
        crate::issues::update_blocking(&file, |issues| issues.resolve(1, true, 3)).unwrap();
        let report = sync_blocking(&file, root, &forge, 4).unwrap().1;
        assert_eq!((report.conflicts, report.pushed), (0, 1));
        assert_eq!(forge.said("7").title, "mine");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn taking_theirs_settles_without_sending_anything() {
        let file = scratch("theirs");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        edit(&file, 1, "mine");
        forge.set("7", snap("theirs", true, &[]));
        sync_blocking(&file, root, &forge, 2).unwrap();

        crate::issues::update_blocking(&file, |issues| issues.resolve(1, false, 3)).unwrap();
        assert_eq!(load(&file).get(1).unwrap().title, "theirs");
        let report = sync_blocking(&file, root, &forge, 4).unwrap().1;
        assert_eq!(report, Report::default());
        assert_eq!(*forge.updates.lock().unwrap(), 0);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_push_that_fails_is_tried_again_and_never_read_as_their_change() {
        let file = scratch("retry");
        let root = Path::new("/");
        let mut forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        edit(&file, 1, "mine");

        forge.refuse_updates = true;
        let report = sync_blocking(&file, root, &forge, 2).unwrap().1;
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            load(&file).get(1).unwrap().title,
            "mine",
            "our change is kept"
        );

        forge.refuse_updates = false;
        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert_eq!(report.pushed, 1);
        assert_eq!(forge.said("7").title, "mine");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn an_issue_gone_from_the_forge_is_kept_here_unlinked_and_said() {
        let file = scratch("gone");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        forge.issues.lock().unwrap().clear();
        sync_blocking(&file, root, &forge, 2).unwrap();
        let kept = load(&file);
        let issue = kept.get(1).unwrap();
        assert!(issue.link.is_none());
        assert!(issue.notes[0].text.contains("no longer on Forge"));
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn nothing_written_here_leaves_until_it_is_published() {
        let file = scratch("publish");
        let root = Path::new("/");
        let forge = Forge::with(vec![]);
        crate::issues::update_blocking(&file, |issues| {
            issues.create(
                Draft {
                    title: "private".into(),
                    ..Draft::default()
                },
                1,
            )
        })
        .unwrap();
        sync_blocking(&file, root, &forge, 2).unwrap();
        assert!(forge.issues.lock().unwrap().is_empty());

        assert_eq!(
            publish_blocking(&file, root, &forge, 1),
            Ok("#100".to_string())
        );
        assert_eq!(forge.said("100").title, "private");
        assert!(
            publish_blocking(&file, root, &forge, 1).is_err(),
            "published once"
        );
        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert_eq!(
            report,
            Report::default(),
            "a published issue is not imported back"
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_forge_line_ending_is_not_an_edit() {
        let theirs = Snapshot {
            body: "one\r\ntwo\r\n".into(),
            ..snap(" a ", true, &[])
        }
        .normalized();
        assert_eq!(theirs.body, "one\ntwo");
        assert_eq!(theirs.title, "a");
    }

    #[test]
    fn taking_theirs_takes_only_what_was_in_conflict() {
        let file = scratch("take-part");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        // Here: a new title and a description. There: another title.
        crate::issues::update_blocking(&file, |issues| {
            issues.edit(
                1,
                Draft {
                    title: "mine".into(),
                    body: "written here".into(),
                    ..Draft::default()
                },
                2,
            )
        })
        .unwrap();
        forge.set("7", snap("theirs", true, &[]));
        sync_blocking(&file, root, &forge, 3).unwrap();

        crate::issues::update_blocking(&file, |issues| issues.resolve(1, false, 4)).unwrap();
        let kept = load(&file);
        let issue = kept.get(1).unwrap();
        assert_eq!(issue.title, "theirs");
        assert_eq!(
            issue.body, "written here",
            "a field nobody disagreed about stays"
        );
        let report = sync_blocking(&file, root, &forge, 5).unwrap().1;
        assert_eq!(report.pushed, 1);
        assert_eq!(forge.said("7").body, "written here");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn labels_in_another_order_are_not_a_change() {
        let file = scratch("order");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &["bug", "ui"]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        forge.set("7", snap("a", true, &["ui", "bug"]));
        assert_eq!(
            sync_blocking(&file, root, &forge, 2).unwrap().1,
            Report::default()
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn an_issue_closed_on_both_sides_is_still_followed() {
        let file = scratch("closed");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        forge.set("7", snap("a", false, &[]));
        sync_blocking(&file, root, &forge, 2).unwrap();
        assert!(!load(&file).get(1).unwrap().open);

        // Renamed there while closed: it still comes across.
        forge.set("7", snap("renamed there", false, &[]));
        sync_blocking(&file, root, &forge, 3).unwrap();
        assert_eq!(load(&file).get(1).unwrap().title, "renamed there");
        // Renamed here while closed: it still goes across.
        edit(&file, 1, "renamed here");
        sync_blocking(&file, root, &forge, 4).unwrap();
        assert_eq!(forge.said("7").title, "renamed here");
        // A closed issue never linked is not brought in.
        forge.issues.lock().unwrap().push(RemoteIssue {
            key: "9".into(),
            reference: "#9".into(),
            snapshot: snap("closed long ago", false, &[]),
        });
        assert_eq!(sync_blocking(&file, root, &forge, 5).unwrap().1.imported, 0);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn every_write_moves_the_revision_on() {
        let file = scratch("revision");
        let forge = Forge::with(vec![]);
        let (first, _) = crate::issues::update_blocking(&file, |i| {
            i.create(
                Draft {
                    title: "a".into(),
                    ..Draft::default()
                },
                1,
            )
        })
        .unwrap();
        let (second, _) = sync_blocking(&file, Path::new("/"), &forge, 2).unwrap();
        assert!(second.revision() > first.revision());
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    /// Issue 7, linked and then closed on both sides, with the sync that saw
    /// it close settled.
    fn closed_and_settled(file: &Path, forge: &Forge) {
        let root = Path::new("/");
        sync_blocking(file, root, forge, 1).unwrap();
        forge.set("7", snap("a", false, &[]));
        sync_blocking(file, root, forge, 2).unwrap();
        assert!(!load(file).get(1).unwrap().open);
    }

    #[test]
    fn a_file_with_no_sync_point_asks_about_a_closed_issue_before_sending_to_it() {
        let file = scratch("no-point");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        closed_and_settled(&file, &forge);
        // As a file written before the sync point existed would read.
        crate::issues::update_blocking(&file, |issues| {
            issues.last_synced = 0;
            Ok(())
        })
        .unwrap();
        forge.set("7", snap("there", false, &[]));
        edit(&file, 1, "here");

        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert_eq!((report.conflicts, report.pushed), (1, 0));
        assert_eq!(
            forge.said("7").title,
            "there",
            "the forge's edit is not written over"
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_full_open_list_neither_hides_a_closed_edit_nor_stays_cut() {
        let file = scratch("full");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        closed_and_settled(&file, &forge);
        forge
            .issues
            .lock()
            .unwrap()
            .extend((1000..1000 + SYNC_CAP as u64 + 1).map(|n| RemoteIssue {
                key: n.to_string(),
                reference: format!("#{n}"),
                snapshot: snap("x", true, &[]),
            }));
        forge.set("7", snap("renamed there", false, &[]));

        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert!(report.cut, "more open issues than the cap is said");
        assert_eq!(load(&file).get(1).unwrap().title, "renamed there");
        assert_eq!(
            load(&file).last_synced,
            3,
            "the point moves on, so the next sync is not cut too"
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn labels_that_repeat_are_not_the_same_as_labels_that_differ() {
        let a = snap("t", true, &["a", "a"]);
        let b = snap("t", true, &["a", "b"]);
        assert!(!a.same_as(&b) && !b.same_as(&a));
    }

    #[test]
    fn a_push_that_keeps_failing_does_not_hold_the_sync_point_back() {
        let file = scratch("stuck");
        let root = Path::new("/");
        let mut forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        sync_blocking(&file, root, &forge, 1).unwrap();
        edit(&file, 1, "mine");
        forge.refuse_updates = true;
        let report = sync_blocking(&file, root, &forge, 2).unwrap().1;
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            load(&file).last_synced,
            2,
            "a failed push is retried from `base`, not from the point"
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn a_complete_change_list_spares_asking_about_a_closed_issue_that_did_not_change() {
        let file = scratch("complete");
        let root = Path::new("/");
        let forge = Forge::with(vec![(7, snap("a", true, &[]))]);
        closed_and_settled(&file, &forge);
        forge.unchanged.lock().unwrap().push("7".into());
        edit(&file, 1, "renamed here");
        let asked = *forge.lookups.lock().unwrap();

        let report = sync_blocking(&file, root, &forge, 3).unwrap().1;
        assert_eq!(report.pushed, 1);
        assert_eq!(forge.said("7").title, "renamed here");
        assert_eq!(
            *forge.lookups.lock().unwrap(),
            asked,
            "nothing was asked one by one"
        );
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}
