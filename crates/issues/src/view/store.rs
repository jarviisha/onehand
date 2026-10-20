//! One issues file, shared by every view of it: what it holds as last read,
//! the forge that serves its project, and keeping the two in step.
//!
//! **One per file for the whole process.** The Issues tab of each window and
//! each window's Issues page draw the same file, so they hold the same entity:
//! a write through one is seen by the others at once, and a sync runs once
//! however many views are open on it. What a view picks, filters or writes
//! in its form stays with that view.

use super::SYNC_GAP;
use gpui::{App, AppContext as _, Context, Entity, Global, Task, WeakEntity};
use onehand_core::connector::{self, Connector};
use onehand_core::issues::template::{self, IssueTemplate};
use onehand_core::issues::{self, Issues, sync};
use onehand_core::saving::OrderedWriter;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every issues file a view holds, by its path. Weak, so a file no view
/// holds any more is let go.
#[derive(Default)]
struct Files(HashMap<PathBuf, WeakEntity<IssuesFile>>);

impl Global for Files {}

pub(super) struct IssuesFile {
    /// The project the file keeps the issues of.
    root: PathBuf,
    path: PathBuf,
    connectors: &'static [&'static dyn Connector],
    /// `None` until the first read lands.
    pub(super) issues: Option<Issues>,
    /// Why the last read failed, when it did.
    pub(super) failed: Option<String>,
    /// The forge that serves the project, if one does — found when the file
    /// is read, since asking reads the project's git remote.
    pub(super) forge: Option<&'static dyn Connector>,
    /// The issue templates the project offers, its own or the shipped ones,
    /// read with the file since its own are files in the project.
    pub(super) templates: Vec<IssueTemplate>,
    /// A sync is on its way; a second is not started beside it.
    pub(super) syncing: bool,
    /// Something asked for a sync while one was running — an edit saved
    /// meanwhile — so another runs as soon as it lands, rather than leaving
    /// the edit for the timer.
    again: bool,
    /// When the last sync finished, in seconds since the epoch, and what it
    /// came to: what moved, in one line, or what it could not do.
    pub(super) synced: Option<(u64, Result<String, String>)>,
    /// Something other than a view may have written the file — a run leaving
    /// a note — so it is read again the next time a view draws it.
    stale: bool,
    /// The read in flight, held so that starting another drops it.
    _load: Option<Task<()>>,
    /// Writes the file one change at a time, in the order they were made.
    writer: OrderedWriter,
    /// The changes sent to be written and those whose write has landed, and
    /// what came in meanwhile.
    pending: Pending,
}

/// The writes still on their way, and what was read meanwhile. A change is
/// shown before its write lands, on a copy whose revision is the one it was
/// made on; a read or a sync landing before the write would be no older than
/// that copy, and would put the change back until its write landed. So while
/// any write is on its way what comes in is held, and once the last one
/// lands the newest of what came in and what the writes left is shown.
#[derive(Default)]
struct Pending {
    /// The newest change sent to be written: the one whose write decides
    /// what to show should it fail.
    sent: u64,
    /// How many writes have landed, done or refused.
    landed: u64,
    /// The newest copy that came in while a write was on its way.
    held: Option<Issues>,
}

impl Pending {
    /// Take a copy read or synced from the file.
    fn incoming(&mut self, shown: &mut Option<Issues>, copy: Issues) {
        if self.landed < self.sent && shown.is_some() {
            keep_newer(&mut self.held, copy);
        } else {
            keep_newer(shown, copy);
        }
    }

    /// A change is sent to be written: which one it is.
    fn send(&mut self) -> u64 {
        self.sent += 1;
        self.sent
    }

    /// A write landed, with what it left on disk if it was done. Once none is
    /// on its way, the newest copy to come in since is what is shown.
    fn land(&mut self, shown: &mut Option<Issues>, kept: Option<Issues>) {
        self.landed += 1;
        if let Some(kept) = kept {
            keep_newer(&mut self.held, kept);
        }
        if self.landed >= self.sent
            && let Some(held) = self.held.take()
        {
            keep_newer(shown, held);
        }
    }
}

impl IssuesFile {
    /// The file `path`, keeping the issues of the project at `root`: the one
    /// every view holds, made and read on first use.
    pub(super) fn get(
        root: &Path,
        path: PathBuf,
        connectors: &'static [&'static dyn Connector],
        cx: &mut App,
    ) -> Entity<Self> {
        let files = cx.default_global::<Files>();
        files.0.retain(|_, file| file.upgrade().is_some());
        if let Some(file) = files.0.get(&path).and_then(WeakEntity::upgrade) {
            return file;
        }
        let file = cx.new(|_| Self {
            root: root.to_path_buf(),
            path: path.clone(),
            connectors,
            issues: None,
            failed: None,
            forge: None,
            templates: template::shipped(),
            syncing: false,
            again: false,
            synced: None,
            stale: true,
            _load: None,
            writer: OrderedWriter::spawn("onehand-issues"),
            pending: Pending::default(),
        });
        cx.global_mut::<Files>().0.insert(path, file.downgrade());
        file
    }

    pub(super) fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Read the file if something marked it since it was last read.
    pub(super) fn load_if_stale(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.stale) {
            self.load(cx);
        }
    }

    /// Read the file, then sync it if it is due.
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        self.stale = false;
        let (root, path, connectors) = (self.root.clone(), self.path.clone(), self.connectors);
        self._load = Some(cx.spawn(async move |file, cx| {
            let (read, forge, templates) = cx
                .background_executor()
                .spawn(async move {
                    let forge = connector::serving(connectors, &root)
                        .ok()
                        .map(|at| connectors[at]);
                    let templates = template::for_project_blocking(&root);
                    (issues::load_blocking(&path), forge, templates)
                })
                .await;
            let _ = file.update(cx, |file: &mut Self, cx| {
                file.forge = forge;
                file.templates = templates;
                match read {
                    Ok(read) => {
                        file.pending.incoming(&mut file.issues, read);
                        file.failed = None;
                    }
                    Err(why) => file.failed = Some(why),
                }
                cx.notify();
                file.sync(false, cx);
            });
        }));
    }

    /// Show `change` at once, before its write lands; what is read meanwhile
    /// waits for the write. A change that does not apply to what is shown is
    /// left for its write to refuse.
    pub(super) fn show<R>(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<R, String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(kept) = self.issues.as_mut()
            && change(kept).is_ok()
        {
            cx.notify();
        }
    }

    /// Read, change and write the file after every change made before this
    /// one. Hands back which change it is, and what its write came to.
    pub(super) fn write<C, R>(
        &mut self,
        change: C,
    ) -> (
        u64,
        impl std::future::Future<Output = Result<(Issues, R), String>> + use<C, R>,
    )
    where
        C: FnOnce(&mut Issues) -> Result<R, String> + Send + 'static,
        R: Send + 'static,
    {
        let of = self.pending.send();
        let path = self.path.clone();
        let written = self
            .writer
            .write(move || issues::update_blocking(&path, change));
        (of, written)
    }

    /// Whether change `of` is the newest one made.
    pub(super) fn newest(&self, of: u64) -> bool {
        of == self.pending.sent
    }

    /// A write landed, with what it left on disk if it was done. Every write
    /// lands here, the newest or not, so what was read meanwhile is let
    /// through once none is on its way.
    pub(super) fn land(&mut self, kept: Option<Issues>, cx: &mut Context<Self>) {
        self.pending.land(&mut self.issues, kept);
        cx.notify();
    }

    /// Keep the file in step with its forge, if it is kept in step with one.
    /// `now` for a sync something asked for — an edit to send, a press of
    /// *Sync now* — and not for the ones that only come around, which wait
    /// out [`SYNC_GAP`] since the last, whichever view asked for it.
    pub(super) fn sync(&mut self, now: bool, cx: &mut Context<Self>) {
        let (Some(forge), Some(kept)) = (self.forge, self.issues.as_ref()) else {
            return;
        };
        let wanted = kept.in_step_with(forge.name());
        let due = now
            || self
                .synced
                .as_ref()
                .is_none_or(|(at, _)| issues::now().saturating_sub(*at) >= SYNC_GAP.as_secs());
        if self.syncing && wanted && now {
            self.again = true;
            return;
        }
        if !wanted || !due || self.syncing {
            return;
        }
        self.syncing = true;
        cx.notify();
        let (root, path) = (self.root.clone(), self.path.clone());
        cx.spawn(async move |file, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { sync::sync_blocking(&path, &root, forge, issues::now()) })
                .await;
            let _ = file.update(cx, |file: &mut Self, cx| {
                file.syncing = false;
                let again = std::mem::take(&mut file.again);
                let came_to = match done {
                    Ok((kept, report)) => {
                        file.pending.incoming(&mut file.issues, kept);
                        failures(&report).map_or_else(|| Ok(said(&report, forge.name())), Err)
                    }
                    Err(why) => Err(why),
                };
                file.synced = Some((issues::now(), came_to));
                cx.notify();
                if again {
                    file.sync(true, cx);
                }
            });
        })
        .detach();
    }
}

/// What a sync came to, in the one line the list's sync bar carries.
fn said(report: &sync::Report, forge: &str) -> String {
    let mut parts = Vec::new();
    for (n, what) in [
        (report.imported, "new"),
        (report.pulled, "updated here"),
        (report.pushed, "sent"),
        (report.conflicts, "to decide"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    let mut line = if parts.is_empty() {
        format!("In step with {forge}")
    } else {
        format!("Synced with {forge}: {}", parts.join(", "))
    };
    if report.cut {
        line.push_str(&format!(" (the newest {} open only)", sync::SYNC_CAP));
    }
    line
}

/// The standing line for what a sync could not do, or `None` when it did
/// everything: the first failure in full, and how many more there were.
fn failures(report: &sync::Report) -> Option<String> {
    let first = report.failures.first()?;
    // The cause first: the footer shows this in one line, cut to fit, and a
    // preamble there is what the cut would leave.
    Some(match report.failures.len() - 1 {
        0 => first.clone(),
        more => format!("{first} (and {more} more)"),
    })
}

/// Put `incoming` on screen unless what is there was written later. Two reads
/// or writes finish in whatever order the executor finishes them, which is not
/// always the order they reached the disk in, and the older landing second
/// would put an edit back the way it was.
fn keep_newer(shown: &mut Option<Issues>, incoming: Issues) {
    if shown
        .as_ref()
        .is_none_or(|shown| incoming.revision() >= shown.revision())
    {
        *shown = Some(incoming);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onehand_core::issues::Draft;

    /// The file as it stands after `writes` writes, each making an issue.
    fn written(name: &str, writes: usize) -> Issues {
        let path = std::env::temp_dir().join(format!(
            "onehand-issues-store-{name}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut kept = Issues::default();
        for n in 0..writes {
            let draft = Draft {
                title: format!("issue {n}"),
                ..Draft::default()
            };
            kept = issues::update_blocking(&path, move |kept| kept.create(draft, 1))
                .unwrap()
                .0;
        }
        let _ = std::fs::remove_file(&path);
        kept
    }

    fn close_first(kept: &mut Issues) -> Result<(), String> {
        kept.set_open(1, false, 2)
    }

    fn first_open(shown: &Option<Issues>) -> bool {
        shown.as_ref().unwrap().get(1).unwrap().open
    }

    #[test]
    fn a_sync_landing_before_a_shown_changes_write_keeps_the_change() {
        let mut pending = Pending::default();
        let mut shown = Some(written("sync-held", 1));
        // The change is shown, then sent to be written.
        close_first(shown.as_mut().unwrap()).unwrap();
        let of = pending.send();
        // A sync started before the change lands first, with the file as
        // it stood: no older than the copy on screen.
        let synced = written("sync-held", 1);
        pending.incoming(&mut shown, synced);
        assert!(!first_open(&shown), "the shown change stays");
        // The write lands: the file's copy, change and all, is shown.
        let mut kept = written("sync-held", 2);
        close_first(&mut kept).unwrap();
        assert_eq!(of, 1);
        pending.land(&mut shown, Some(kept.clone()));
        assert_eq!(shown, Some(kept));
    }

    #[test]
    fn a_copy_newer_than_the_write_wins_once_the_write_lands() {
        let mut pending = Pending::default();
        let mut shown = Some(written("newer", 1));
        close_first(shown.as_mut().unwrap()).unwrap();
        pending.send();
        let mut kept = written("newer", 2);
        close_first(&mut kept).unwrap();
        // A sync that ran after the write reached the disk.
        let mut synced = written("newer", 3);
        close_first(&mut synced).unwrap();
        pending.incoming(&mut shown, synced.clone());
        pending.land(&mut shown, Some(kept));
        assert_eq!(shown, Some(synced));
    }

    #[test]
    fn a_refused_write_lets_what_was_read_through() {
        let mut pending = Pending::default();
        let mut shown = Some(written("refused", 1));
        close_first(shown.as_mut().unwrap()).unwrap();
        pending.send();
        let read = written("refused", 1);
        pending.incoming(&mut shown, read.clone());
        pending.land(&mut shown, None);
        assert_eq!(shown, Some(read.clone()));
        // With nothing on its way, a read is shown as it comes.
        let later = written("refused", 2);
        pending.incoming(&mut shown, later.clone());
        assert_eq!(shown, Some(later));
    }

    #[test]
    fn an_older_write_landing_waits_for_the_newer_one() {
        let mut pending = Pending::default();
        let mut shown = Some(written("two", 1));
        close_first(shown.as_mut().unwrap()).unwrap();
        pending.send();
        shown.as_mut().unwrap().set_open(1, true, 3).unwrap();
        pending.send();
        let mut first = written("two", 2);
        close_first(&mut first).unwrap();
        pending.land(&mut shown, Some(first));
        assert!(first_open(&shown), "the newer change stays on screen");
        let second = written("two", 3);
        pending.land(&mut shown, Some(second.clone()));
        assert_eq!(shown, Some(second));
    }
}
