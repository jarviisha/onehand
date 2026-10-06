//! What is read of an issue's work off the network: its pull request, read
//! on the background executor, each read keyed to what asked for it and to
//! when, so an answer about another issue, an older request or an older run
//! is never shown.

use super::{IssuesView, READ_AGE};
use gpui::Context;
use onehand_core::connector;
use onehand_core::issues::{self, IssueKey};
use onehand_core::task::work::{IssueWork, PrSeen};
use std::path::Path;

/// What a read of a pull request is about: the issue, the task and its run.
pub(super) type PrAbout = (IssueKey, String, Option<String>);

impl IssuesView {
    /// The issue `number` of `root`, named by the file it is kept in.
    pub(super) fn key(&self, root: &Path, number: u64) -> Option<IssueKey> {
        Some(IssueKey {
            file: issues::file_for(self.storage.as_deref()?, root),
            number,
        })
    }

    /// The pull request `work` should have: the read is about its issue, its
    /// task and its run, and only on a project a forge serves once the run
    /// has reached its pull request step.
    pub(super) fn pr_about(work: &IssueWork) -> Option<PrAbout> {
        let w = &work.work;
        w.has_pull_request()
            .then(|| (work.key.clone(), w.task.clone(), w.run.clone()))
    }

    /// What was last read of `work`'s pull request and when, if the reads are
    /// about it: never another issue's, task's or run's.
    pub(super) fn pr_value(
        &self,
        work: Option<&IssueWork>,
    ) -> Option<&(Option<onehand_core::connector::PullRequest>, u64)> {
        let about = work.and_then(Self::pr_about)?;
        (self.pr.about() == Some(&about)).then_some(self.pr.value.as_ref())?
    }

    /// The pull request of `work`, as last read for it.
    pub(super) fn pr_seen(&self, work: Option<&IssueWork>) -> PrSeen<'_> {
        let Some(about) = work.and_then(Self::pr_about) else {
            return PrSeen::Unread;
        };
        if self.pr.about() != Some(&about) {
            return PrSeen::Unread;
        }
        match (&self.pr.failed, &self.pr.value) {
            (Some(why), _) => PrSeen::Failed(why),
            (None, Some((pr, _))) => PrSeen::Read(pr.as_ref()),
            (None, None) => PrSeen::Unread,
        }
    }

    /// Read the pull request of `work` when it is due: never read for it,
    /// its task moved, or the window came back to the front with what is
    /// shown older than [`READ_AGE`]. Never on a timer of its own.
    pub(super) fn read_pr_if_due(
        &mut self,
        root: &Path,
        work: Option<&IssueWork>,
        cx: &mut Context<Self>,
    ) {
        let returned = self.returned;
        let moved = std::mem::take(&mut self.moved);
        let Some(work) = work else {
            self.shown = None;
            return;
        };
        // Opening an issue reads it, even one read before another was shown.
        let opened = self.shown.as_ref() != Some(&work.key);
        self.shown = Some(work.key.clone());
        let Some(about) = Self::pr_about(work) else {
            return;
        };
        let old = self
            .pr
            .value
            .as_ref()
            .is_none_or(|(_, at)| issues::now().saturating_sub(*at) >= READ_AGE.as_secs());
        let due = self.pr.about() != Some(&about) || opened || moved || (returned && old);
        if due {
            self.read_pr(root, work, about, cx);
        }
    }

    /// Read the pull request of `work` now, whatever was read before.
    pub(super) fn read_pr(
        &mut self,
        root: &Path,
        work: &IssueWork,
        about: PrAbout,
        cx: &mut Context<Self>,
    ) {
        let (Some(name), Some(branch)) = (&work.work.forge, work.work.branch.clone()) else {
            return;
        };
        let generation = self.pr.ask(about);
        let Some(forge) = connector::named(self.connectors, name) else {
            let why = format!("{name} is not a connector this build has");
            self.pr.land(generation, Err(why), issues::now());
            return;
        };
        let root = root.to_path_buf();
        cx.spawn(async move |view, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { forge.pull_request_for_blocking(&root, &branch) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if view.pr.land(generation, answer, issues::now()) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Read the pull request of the issue on screen again, asked by a person.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(number) = self.roots.get(&root).and_then(|state| state.selected) else {
            return;
        };
        let Some(key) = self.key(&root, number) else {
            return;
        };
        let Some(work) = self.works.iter().find(|work| work.key == key).cloned() else {
            return;
        };
        if let Some(about) = Self::pr_about(&work) {
            self.read_pr(&root, &work, about, cx);
            cx.notify();
        }
    }

    /// Whether the work on issue `number` of the project on screen is
    /// running, queued or waiting.
    pub(super) fn work_active(&self, number: u64) -> bool {
        let Some(key) = self.root.as_deref().and_then(|root| self.key(root, number)) else {
            return false;
        };
        self.works.iter().any(|work| {
            work.key == key
                && (work.work.active() || work.earlier.iter().any(|earlier| earlier.active))
        })
    }
}
