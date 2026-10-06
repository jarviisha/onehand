//! Reading, writing and syncing a project's issues file: every change goes
//! through one read-change-write off the UI loop, and a project kept in step
//! with a forge is synced after it.

use super::{IssuesView, SYNC_GAP};
use gpui::Context;
use onehand_core::connector;
use onehand_core::issues::{self, Issues, sync};
use std::path::PathBuf;

impl IssuesView {
    /// Read the active project's issues.
    ///
    /// The entry is made **here**, while the root is known to be in the
    /// workspace, and the read that lands later only fills it in — one made on
    /// the way back would bring back a project removed in the meantime.
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        let Some((root, file)) = self.file() else {
            return;
        };
        self.roots.entry(root.clone()).or_default();
        let connectors = self.connectors;
        self._load = Some(cx.spawn(async move |view, cx| {
            let (read, forge) = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move {
                        let forge = connector::serving(connectors, &root)
                            .ok()
                            .map(|at| connectors[at]);
                        (issues::load_blocking(&file), forge)
                    }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                state.forge = forge;
                match read {
                    Ok(read) => {
                        state.land(read);
                        view.status = None;
                    }
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
                view.sync(false, cx);
            });
        }));
    }

    /// Keep the active project in step with its forge, if it is kept in step
    /// with one. `now` for a sync something asked for — an edit to send, a
    /// press of *Sync now* — and not for the ones that only come around, which
    /// wait out [`SYNC_GAP`] since the last.
    pub(super) fn sync(&mut self, now: bool, cx: &mut Context<Self>) {
        if let Some(root) = self.root.clone() {
            self.sync_root(root, now, cx);
        }
    }

    /// [`Self::sync`] for `root` in particular, which is what a sync that
    /// comes back for a project needs: the one on screen may have changed
    /// while it ran.
    pub(super) fn sync_root(&mut self, root: PathBuf, now: bool, cx: &mut Context<Self>) {
        let Some(storage) = self.storage.as_deref() else {
            return;
        };
        let file = issues::file_for(storage, &root);
        let Some(state) = self.roots.get_mut(&root) else {
            return;
        };
        let (Some(forge), Some(kept)) = (state.forge, state.issues.as_ref()) else {
            return;
        };
        let wanted = kept.in_step_with(forge.name());
        let due = now
            || state
                .synced
                .as_ref()
                .is_none_or(|(at, _)| issues::now().saturating_sub(*at) >= SYNC_GAP.as_secs());
        if state.syncing && wanted && now {
            state.again = true;
            return;
        }
        if !wanted || !due || state.syncing {
            return;
        }
        state.syncing = true;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { sync::sync_blocking(&file, &root, forge, issues::now()) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                state.syncing = false;
                let again = std::mem::take(&mut state.again);
                let came_to = match done {
                    Ok((kept, report)) => {
                        keep_newer(&mut state.issues, kept);
                        failures(&report).map_or_else(|| Ok(said(&report, forge.name())), Err)
                    }
                    Err(why) => Err(why),
                };
                state.synced = Some((issues::now(), came_to));
                cx.notify();
                if again {
                    view.sync_root(root, true, cx);
                }
            });
        })
        .detach();
    }

    /// Start or stop keeping the active project in step with its forge.
    pub(super) fn set_syncing(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(forge) = self.state_mut().and_then(|state| state.forge) else {
            return;
        };
        self.change(
            move |kept| {
                kept.sync_with(on.then(|| forge.name().to_string()));
                Ok(None)
            },
            cx,
        );
    }

    /// Send issue `number` to the forge, which is the only way anything written
    /// here reaches it.
    pub(super) fn publish(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some((root, file)) = self.file() else {
            return;
        };
        let Some(forge) = self.state_mut().and_then(|state| state.forge) else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { sync::publish_blocking(&file, &root, forge, number) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.status = done.err();
                view.load(cx);
            });
        })
        .detach();
    }

    /// Settle issue `number`'s conflict one way or the other, then sync so the
    /// decision reaches the forge.
    pub(super) fn resolve(&mut self, number: u64, keep_mine: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change(
            move |kept| kept.resolve(number, keep_mine, now).map(|()| Some(number)),
            cx,
        );
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any.
    ///
    /// The whole read-change-write happens off the UI loop in one call, so it
    /// is made against what is on disk rather than against the copy on screen,
    /// and nothing else in this process can write in between.
    ///
    /// On a project kept in step with its forge the change is then synced
    /// straight away, so an edit reaches the forge without waiting for the
    /// timer.
    pub(super) fn change(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some((root, file)) = self.file() else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { issues::update_blocking(&file, change) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                match done {
                    Ok((kept, number)) => {
                        keep_newer(&mut state.issues, kept);
                        if number.is_some() {
                            state.selected = number;
                        }
                        state.form = None;
                        view.status = None;
                        cx.notify();
                        view.sync_root(root, true, cx);
                        return;
                    }
                    // The form stays open with what was typed in it: a refusal
                    // is something to correct, not a reason to start again.
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
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
