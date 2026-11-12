//! The page's pull requests: one read per project of all of them, never one
//! per row, and the branches a capped read missed looked up one by one up to
//! a cap. When each is due is the view's drive's (`WorkDrive`); this hands
//! it the page's projects.

use super::IssuesView;
use gpui::Context;
use onehand_core::task::work::drive::Project;

impl IssuesView {
    /// The page's projects, each with its issues file and the forge serving
    /// it once that file is read.
    fn pr_projects(&self, cx: &Context<Self>) -> Vec<Project> {
        let (Some(page), Some(storage)) = (self.page.as_ref(), self.storage.as_deref()) else {
            return Vec::new();
        };
        page.projects
            .iter()
            .map(|(root, _)| {
                let forge = self
                    .roots
                    .get(root)
                    .and_then(|state| state.file.read(cx).forge);
                Project::new(root, storage, forge)
            })
            .collect()
    }

    /// Read every project's pull requests when it is due.
    pub(super) fn read_prs_if_due(&mut self, cx: &mut Context<Self>) {
        let projects = self.pr_projects(cx);
        let effects = self.work.page_drawn(&projects, onehand_core::Now::read());
        self.carry(effects, cx);
    }

    /// Read every project's pull requests now.
    pub(in crate::view) fn read_prs(&mut self, cx: &mut Context<Self>) {
        let projects = self.pr_projects(cx);
        let effects = self.work.read_prs(&projects);
        self.carry(effects, cx);
    }

    /// Look up, one by one, the branches the last capped reads missed.
    pub(super) fn read_missing(&mut self, cx: &mut Context<Self>) {
        let projects = self.pr_projects(cx);
        let effects = self.work.read_missing(&projects);
        self.carry(effects, cx);
    }
}
