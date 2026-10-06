//! The page's pull requests: one read per project of all of them, never one
//! per row, and the branches a capped read missed looked up one by one up to
//! a cap. Read on opening the page, on *Refresh*, and when the window comes
//! back to the front with the reading older than a minute.

use super::{IssuesView, works_of};
use gpui::Context;
use onehand_core::connector::PullRequests;
use onehand_core::issues;
use onehand_core::task::work::list::{PrReads, READ_AGE};
use std::path::PathBuf;

/// How many pull requests one read of a project asks for.
const PR_LIST_CAP: usize = 200;

/// How many branches a capped read missed are looked up one by one, per
/// read; past it a row is not read, and the list says so.
const LOOKUP_CAP: usize = 20;

impl IssuesView {
    /// When the oldest project's pull requests were read, for the list's
    /// head.
    pub(super) fn prs_read_at(&self) -> Option<u64> {
        self.page
            .as_ref()?
            .prs
            .values()
            .filter_map(|read| read.at)
            .min()
    }

    /// Read every project's pull requests when it is due.
    pub(super) fn read_prs_if_due(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let arrived = std::mem::take(&mut page.arrived);
        let old = self
            .prs_read_at()
            .is_none_or(|at| issues::now().saturating_sub(at) >= READ_AGE);
        if arrived || (self.returned && old) {
            self.read_prs(cx);
        }
    }

    /// The projects with work that should have a pull request, each with the
    /// branches it should be on.
    fn wanting_prs(&self) -> Vec<(PathBuf, Vec<String>)> {
        let (Some(page), Some(storage)) = (self.page.as_ref(), self.storage.as_deref()) else {
            return Vec::new();
        };
        page.projects
            .iter()
            .filter_map(|(root, _)| {
                let file = issues::file_for(storage, root);
                let branches: Vec<String> = works_of(&self.works, &file)
                    .filter(|work| work.work.has_pull_request())
                    .filter_map(|work| work.work.branch.clone())
                    .collect();
                (!branches.is_empty()).then(|| (root.clone(), branches))
            })
            .collect()
    }

    /// Read every project's pull requests now.
    pub(super) fn read_prs(&mut self, cx: &mut Context<Self>) {
        for (root, branches) in self.wanting_prs() {
            self.read_project(root, branches, cx);
        }
    }

    /// Look up, one by one, the branches the last capped reads missed.
    pub(super) fn read_missing(&mut self, cx: &mut Context<Self>) {
        for (root, branches) in self.wanting_prs() {
            let Some(page) = self.page.as_ref() else {
                return;
            };
            let wanted = page
                .prs
                .get(&root)
                .map(|read| read.to_look_up(branches.iter().map(String::as_str), LOOKUP_CAP))
                .unwrap_or_default();
            let generation = page.pr_asked.get(&root).copied().unwrap_or_default();
            self.look_up(root, wanted, generation, cx);
        }
    }

    fn read_project(&mut self, root: PathBuf, branches: Vec<String>, cx: &mut Context<Self>) {
        let Some(forge) = self
            .roots
            .get(&root)
            .and_then(|state| state.file.read(cx).forge)
        else {
            return;
        };
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let generation = page.pr_asked.entry(root.clone()).or_default();
        *generation += 1;
        let generation = *generation;
        cx.spawn(async move |view, cx| {
            let answer = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { forge.pull_requests_blocking(&root, PR_LIST_CAP) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(page) = view.page.as_mut() else {
                    return;
                };
                if page.pr_asked.get(&root) != Some(&generation) {
                    return;
                }
                let read = page.prs.entry(root.clone()).or_default();
                landed(read, answer);
                let wanted = read.to_look_up(branches.iter().map(String::as_str), LOOKUP_CAP);
                cx.notify();
                view.look_up(root, wanted, generation, cx);
            });
        })
        .detach();
    }

    /// Look up each of `branches` of `root` on its own, for the read of
    /// `generation`.
    fn look_up(
        &mut self,
        root: PathBuf,
        branches: Vec<String>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(forge) = self
            .roots
            .get(&root)
            .and_then(|state| state.file.read(cx).forge)
        else {
            return;
        };
        for branch in branches {
            let root = root.clone();
            cx.spawn(async move |view, cx| {
                let answer = cx
                    .background_executor()
                    .spawn({
                        let (root, branch) = (root.clone(), branch.clone());
                        async move { forge.pull_request_for_blocking(&root, &branch) }
                    })
                    .await;
                let _ = view.update(cx, |view: &mut Self, cx| {
                    let Some(page) = view.page.as_mut() else {
                        return;
                    };
                    if page.pr_asked.get(&root) != Some(&generation) {
                        return;
                    }
                    // A lookup that failed leaves the row not read, which
                    // the list says.
                    if let (Ok(found), Some(read)) = (answer, page.prs.get_mut(&root)) {
                        read.looked_up.insert(branch, found);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }
}

/// Take a read of a project's pull requests: a failure keeps what was read
/// before, marked with why.
fn landed(read: &mut PrReads, answer: Result<PullRequests, String>) {
    match answer {
        Ok(found) => {
            read.by_branch = found.by_branch.into_iter().collect();
            read.capped = found.capped;
            read.looked_up.clear();
            read.failed = None;
        }
        Err(why) => read.failed = Some(why),
    }
    read.at = Some(issues::now());
}
