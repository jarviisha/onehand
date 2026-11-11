//! What is read of an issue's work off the network and off git, carried for
//! the view's drive (`WorkDrive`), which says when each read is due and
//! which answers are kept: each read runs on the background executor and its
//! answer is reported back with the time it landed.

use super::IssuesView;
use gpui::Context;
use onehand_core::issues::{self, IssueKey};
use onehand_core::task::marks;
use onehand_core::task::work::IssueWork;
use onehand_core::task::work::drive::{Effect, Input, PR_LIST_CAP};
use onehand_core::task::work::left::left_blocking;
use std::path::Path;

impl IssuesView {
    /// The issue `number` of `root`, named by the file it is kept in.
    pub(super) fn key(&self, root: &Path, number: u64) -> Option<IssueKey> {
        Some(IssueKey {
            file: issues::file_for(self.storage.as_deref()?, root),
            number,
        })
    }

    /// Carry out the drive's effects, in order, each off the UI loop.
    pub(super) fn carry(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::ReadPr {
                    generation,
                    root,
                    forge,
                    branch,
                } => self.read(cx, move || Input::Pr {
                    generation,
                    answer: forge.pull_request_for_blocking(&root, &branch),
                    now: issues::now(),
                }),
                Effect::ReadPrs {
                    generation,
                    root,
                    forge,
                    branches,
                } => self.read(cx, move || Input::Prs {
                    generation,
                    answer: forge.pull_requests_blocking(&root, PR_LIST_CAP),
                    root,
                    forge,
                    branches,
                    now: issues::now(),
                }),
                Effect::LookUp {
                    generation,
                    root,
                    forge,
                    branch,
                } => self.read(cx, move || Input::LookedUp {
                    generation,
                    answer: forge.pull_request_for_blocking(&root, &branch),
                    root,
                    branch,
                }),
                Effect::ReadLeft { generation, work } => self.read(cx, move || Input::Left {
                    generation,
                    left: left_blocking(&work),
                    now: issues::now(),
                }),
                Effect::ReadReviewFiles {
                    generation,
                    dir,
                    span,
                } => self.read(cx, move || Input::ReviewFiles {
                    generation,
                    read: marks::changes_blocking(&dir, &span.0, &span.1),
                    now: issues::now(),
                }),
            }
        }
        cx.notify();
    }

    /// Run `read` off the UI loop and report what it found to the drive.
    fn read(&mut self, cx: &mut Context<Self>, read: impl FnOnce() -> Input + Send + 'static) {
        cx.spawn(async move |view, cx| {
            let input = cx.background_executor().spawn(async move { read() }).await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let effects = view.work.handle(input);
                view.carry(effects, cx);
            });
        })
        .detach();
    }

    /// Read the pull request of the issue on screen again, asked by a person.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(key)) = (self.root.clone(), self.selected_key()) else {
            return;
        };
        let effects = self.work.refresh(&root, &key, issues::now());
        self.carry(effects, cx);
    }

    /// Read again what the issue on screen left, asked by a person.
    pub(super) fn refresh_left(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.selected_key() else {
            return;
        };
        let effects = self.work.refresh_left(&key);
        self.carry(effects, cx);
    }

    /// Whether closing issue `number` of the project on screen asks first.
    pub(super) fn closing_asks(&self, number: u64) -> bool {
        let Some(key) = self.root.as_deref().and_then(|root| self.key(root, number)) else {
            return false;
        };
        self.work.work_of(&key).is_some_and(IssueWork::closing_asks)
    }
}
