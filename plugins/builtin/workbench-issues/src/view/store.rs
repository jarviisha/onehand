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
                        keep_newer(&mut file.issues, read);
                        file.failed = None;
                    }
                    Err(why) => file.failed = Some(why),
                }
                cx.notify();
                file.sync(false, cx);
            });
        }));
    }

    /// Take what a write left on disk.
    pub(super) fn land(&mut self, kept: Issues, cx: &mut Context<Self>) {
        keep_newer(&mut self.issues, kept);
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
                        keep_newer(&mut file.issues, kept);
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
pub(super) fn keep_newer(shown: &mut Option<Issues>, incoming: Issues) {
    if shown
        .as_ref()
        .is_none_or(|shown| incoming.revision() >= shown.revision())
    {
        *shown = Some(incoming);
    }
}
