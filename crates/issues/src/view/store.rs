//! One issues file, shared by every view of it: what it holds as last read,
//! the forge that serves its project, and keeping the two in step.
//!
//! **One per file for the whole process.** The Issues tab of each window and
//! each window's Issues page draw the same file, so they hold the same entity:
//! a write through one is seen by the others at once, and a sync runs once
//! however many views are open on it. What a view picks, filters or writes
//! in its form stays with that view.

use gpui::{App, AppContext as _, Context, Entity, Global, WeakEntity};
use onehand_core::connector::Connector;
use onehand_core::issues::drive::{Effect, FileDrive, Input, read_blocking};
use onehand_core::issues::{self, Issues, sync};
use onehand_core::saving::OrderedWriter;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// Every issues file a view holds, by its path. Weak, so a file no view
/// holds any more is let go.
#[derive(Default)]
struct Files(HashMap<PathBuf, WeakEntity<IssuesFile>>);

impl Global for Files {}

/// The carrier of one file's drive: it reads, syncs, publishes and writes off
/// the UI loop as the drive asks, and tells it what came of each.
pub(super) struct IssuesFile {
    drive: FileDrive,
    /// The read in flight, held so that starting another drops it.
    _load: Option<gpui::Task<()>>,
    /// Writes the file one change at a time, in the order they were made.
    writer: OrderedWriter,
}

impl Deref for IssuesFile {
    type Target = FileDrive;
    fn deref(&self) -> &FileDrive {
        &self.drive
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
            drive: FileDrive::new(root.to_path_buf(), path.clone(), connectors),
            _load: None,
            writer: OrderedWriter::spawn("onehand-issues"),
        });
        cx.global_mut::<Files>().0.insert(path, file.downgrade());
        file
    }

    pub(super) fn mark_stale(&mut self) {
        self.drive.mark_stale();
    }

    /// Read the file if something marked it since it was last read.
    pub(super) fn load_if_stale(&mut self, cx: &mut Context<Self>) {
        let fx = self.drive.load_if_stale();
        self.carry(fx, cx);
    }

    /// Show `change` at once, before its write lands.
    pub(super) fn show<R>(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<R, String>,
        cx: &mut Context<Self>,
    ) {
        let fx = self.drive.show(change);
        self.carry(fx, cx);
    }

    /// Read, change and write the file after every change made before this
    /// one. Hands back which change it is, and what its write came to, which
    /// its caller hands to [`Self::written`].
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
        let of = self.drive.send();
        let (_, path) = self.drive.place();
        let path = path.to_path_buf();
        let written = self
            .writer
            .write(move || issues::update_blocking(&path, change));
        (of, written)
    }

    /// Change `of`'s write landed, with what it left on disk if it was done.
    /// Every write lands here, the newest or not, so what was read meanwhile
    /// is let through once none is on its way.
    pub(super) fn written(&mut self, of: u64, kept: Option<Issues>, cx: &mut Context<Self>) {
        let fx = self.drive.handle(Input::Written {
            of,
            kept,
            now: issues::now(),
        });
        self.carry(fx, cx);
    }

    /// Keep the file in step with its forge, if that is due; `asked` when
    /// something asked for it.
    pub(super) fn sync(&mut self, asked: bool, cx: &mut Context<Self>) {
        let fx = self.drive.sync(asked, issues::now());
        self.carry(fx, cx);
    }

    /// Send issue `number` to the forge: what to send to which forge, handed
    /// back for the view that asked to carry, so it says what it came to;
    /// `None` when nothing is sent.
    pub(super) fn publish(
        &mut self,
        number: u64,
        cx: &mut Context<Self>,
    ) -> Option<(u64, &'static dyn Connector)> {
        let mut send = None;
        for effect in self.drive.publish(number) {
            match effect {
                Effect::Publish { number, forge } => send = Some((number, forge)),
                other => self.carry(vec![other], cx),
            }
        }
        send
    }

    /// What sending issue `number` came to.
    pub(super) fn published(
        &mut self,
        number: u64,
        done: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let fx = self.drive.handle(Input::Published { number, done });
        self.carry(fx, cx);
    }

    /// Carry out `fx`, in order.
    fn carry(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in fx {
            match effect {
                Effect::Changed => cx.notify(),
                Effect::Read { token } => {
                    let (root, path) = self.drive.place();
                    let (root, path) = (root.to_path_buf(), path.to_path_buf());
                    let connectors = self.drive.connectors();
                    self._load = Some(cx.spawn(async move |file, cx| {
                        let (read, forge, templates) = cx
                            .background_executor()
                            .spawn(async move { read_blocking(&root, &path, connectors) })
                            .await;
                        let _ = file.update(cx, |file: &mut Self, cx| {
                            let fx = file.drive.handle(Input::Read {
                                token,
                                read,
                                forge,
                                templates,
                                now: issues::now(),
                            });
                            file.carry(fx, cx);
                        });
                    }));
                }
                Effect::Sync { forge, now } => {
                    let (root, path) = self.drive.place();
                    let (root, path) = (root.to_path_buf(), path.to_path_buf());
                    cx.spawn(async move |file, cx| {
                        let done = cx
                            .background_executor()
                            .spawn(async move { sync::sync_blocking(&path, &root, forge, now) })
                            .await;
                        let _ = file.update(cx, |file: &mut Self, cx| {
                            let fx = file.drive.handle(Input::Synced {
                                done,
                                now: issues::now(),
                            });
                            file.carry(fx, cx);
                        });
                    })
                    .detach();
                }
                // Only ever answered to the view that asked, by `publish`.
                Effect::Publish { .. } => debug_assert!(false, "a publish is the asker's to carry"),
            }
        }
    }
}

/// What a sync came to, in the line the list's sync bar carries: what moved,
/// or what it could not do.
pub(super) fn came_to(
    synced: &Result<sync::Report, String>,
    forge: &str,
) -> Result<String, String> {
    match synced {
        Ok(report) => failures(report).map_or_else(|| Ok(said(report, forge)), Err),
        Err(why) => Err(why.clone()),
    }
}

/// What a sync did, in one line.
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

#[cfg(test)]
mod tests;
