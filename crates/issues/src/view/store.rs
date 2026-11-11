//! One issues file, shared by every view of it: what it holds as last read,
//! the forge that serves its project, and keeping the two in step.
//!
//! **One per file for the whole process.** The Issues tab of each window and
//! each window's Issues page draw the same file, so they hold the same entity:
//! a write through one is seen by the others at once, and a sync runs once
//! however many views are open on it. What a view picks, filters or writes
//! in its form stays with that view.

use gpui::{App, AppContext as _, Context, Entity, Global, WeakEntity};
use onehand_core::chat::Now;
use onehand_core::connector::Connector;
use onehand_core::issues::drive::{Change, Effect, FileDrive, Input, Signal, read_blocking};
use onehand_core::issues::{self, sync};
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

impl gpui::EventEmitter<Signal> for IssuesFile {}

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
    pub(super) fn show(&mut self, change: &Change, cx: &mut Context<Self>) {
        let fx = self.drive.show(change, now());
        self.carry(fx, cx);
    }

    /// Write `change` after every change made before it: which change it
    /// is, for the view that made it to hear its write in [`Signal::Wrote`].
    pub(super) fn write(&mut self, change: Change, cx: &mut Context<Self>) -> u64 {
        let (of, write) = self.drive.write(change, now());
        self.carry(vec![write], cx);
        of
    }

    /// Keep the file in step with its forge, if that is due; `asked` when
    /// something asked for it.
    pub(super) fn sync(&mut self, asked: bool, cx: &mut Context<Self>) {
        let fx = self.drive.sync(asked, now());
        self.carry(fx, cx);
    }

    /// Send issue `number` to the forge; what it came to is heard in
    /// [`Signal::PublishFailed`] when it failed.
    pub(super) fn publish(&mut self, number: u64, cx: &mut Context<Self>) {
        let fx = self.drive.publish(number);
        self.carry(fx, cx);
    }

    /// Carry out `fx`, in order.
    fn carry(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in fx {
            match effect {
                Effect::Changed => cx.notify(),
                Effect::Read { token } => {
                    let (root, path) = self.drive.place();
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
                                now: now(),
                            });
                            file.carry(fx, cx);
                        });
                    }));
                }
                Effect::Sync { forge, now: asked } => {
                    let (root, path) = self.drive.place();
                    let at = asked.secs;
                    cx.spawn(async move |file, cx| {
                        let done = cx
                            .background_executor()
                            .spawn(async move { sync::sync_blocking(&path, &root, forge, at) })
                            .await;
                        let _ = file.update(cx, |file: &mut Self, cx| {
                            let fx = file.drive.handle(Input::Synced { done, now: now() });
                            file.carry(fx, cx);
                        });
                    })
                    .detach();
                }
                Effect::Write { of, change, at } => {
                    let (_, path) = self.drive.place();
                    let written = self.writer.write(move || {
                        issues::update_blocking(&path, move |kept| change.apply(kept, at))
                    });
                    // Held strongly: every write lands, whether or not a view
                    // is still there, so what was read meanwhile is let through.
                    let file = cx.entity();
                    cx.spawn(async move |_, cx| {
                        let done = written.await;
                        file.update(cx, |file, cx| {
                            let fx = file.drive.handle(Input::Written {
                                of,
                                done,
                                now: now(),
                            });
                            file.carry(fx, cx);
                        });
                    })
                    .detach();
                }
                Effect::Publish { number, forge } => {
                    let (root, path) = self.drive.place();
                    cx.spawn(async move |file, cx| {
                        let done = cx
                            .background_executor()
                            .spawn(
                                async move { sync::publish_blocking(&path, &root, forge, number) },
                            )
                            .await
                            .map(|_| ());
                        let _ = file.update(cx, |file: &mut Self, cx| {
                            let fx = file.drive.handle(Input::Published { number, done });
                            file.carry(fx, cx);
                        });
                    })
                    .detach();
                }
                Effect::Signal(signal) => cx.emit(signal),
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

/// The time now, as the drive is told it: it reads no clock of its own.
fn now() -> Now {
    Now {
        secs: issues::now(),
        at: std::time::Instant::now(),
    }
}
