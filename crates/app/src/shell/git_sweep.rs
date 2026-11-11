//! The `git status` sweep: every project's repository read again, off the
//! UI loop, and pushed to whatever names a branch or marks a change; and the
//! one call that re-reads everything derived from the files on disk.

use super::Shell;
use gpui::{Context, SharedString};
use onehand_core::disk::Disk;
use onehand_core::workspace::drive::{Input, SweepProject};
use std::path::PathBuf;

impl Shell {
    /// Re-read everything derived from the files on disk.
    ///
    /// Two panels read the working tree and neither notices it change: the
    /// rail's `branch · N changed` and the Files tree. Both were seeded once
    /// and left, which made them stale from the first turn
    /// onwards -- the moment they start being worth reading.
    ///
    /// One call for both, because there is one cause. Cheap enough to run on
    /// every turn end: `git status` is one process per root off the UI loop,
    /// and the rescan covers only the directories currently on screen.
    ///
    /// A host project on screen whose attach has ended since its files were
    /// shown is attached again first. One still attaching, failed, or taken
    /// by another machine since, is left to that attach or to *Try again*.
    pub(crate) fn refresh_worktree(&mut self, cx: &mut Context<Self>) {
        if self
            .workbench
            .update(cx, |panel, cx| panel.attach_again(cx))
        {
            self.try_host_again(cx);
        }
        self.refresh_git(cx);

        self.workbench.update(cx, |panel, cx| panel.rescan(cx));
        self.tell_issues_page(&onehand_workbench::Request::Rescan, cx);
        // A run leaving a note or a sync landing changes the issue files too,
        // read again while the workspace page is what is showing.
        if self.workspace_shown(cx) {
            self.workspace_page.update(cx, |page, cx| page.reload(cx));
        }
    }

    /// Refresh `git status` for every root, off the UI loop: which roots, and
    /// which sweep's answer is kept, are the workspace drive's.
    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let sweep = self
            .window
            .workspace
            .sweep(|host| crate::hosts::live(cx, host).is_some());
        self.carry_workspace_quietly(vec![sweep], cx);
    }

    /// Read `roots`' `git status`, off the UI loop, and tell the drive.
    ///
    /// Core's blocking reader on GPUI's background executor rather than its
    /// tokio wrapper: tokio's process driver is not running under GPUI, so
    /// the async path would panic looking for a reactor. A host's status is
    /// asked over its attach in the same pass, blocking there likewise.
    pub(super) fn sweep_git(
        &mut self,
        token: u64,
        roots: Vec<SweepProject>,
        cx: &mut Context<Self>,
    ) {
        let read: Vec<(PathBuf, Disk)> = roots
            .into_iter()
            .filter_map(|root| {
                let disk = match &root.host {
                    None => Disk::Local,
                    Some(host) => crate::hosts::live(cx, host)
                        .map(|conn| crate::hosts::disk_over(cx, conn))?,
                };
                Some((root.path, disk))
            })
            .collect();
        cx.spawn(async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    read.into_iter()
                        .filter_map(|(root, disk)| {
                            disk.git_status(&root).map(|status| (root, status))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            shell
                .update(cx, |shell, cx| {
                    let fx = shell.window.workspace.handle(Input::Swept { token, found });
                    shell.carry_workspace_quietly(fx, cx);
                })
                .ok();
        })
        .detach();
    }

    /// What was read of the projects' `git status` changed: tell whatever
    /// names a branch or marks a change.
    pub(super) fn git_read(&mut self, cx: &mut Context<Self>) {
        self.tell_workbench_git(cx);
        // The workspace page's project tiles name each branch.
        if self.page_shown(cx) {
            let local = self.local_page_projects();
            let named = local
                .iter()
                .map(|p| (p.root.clone(), SharedString::from(p.label.to_string())))
                .collect();
            // The Tasks page's or the workspace page's projects, while it is
            // the page: a filter left on another page waits for its next
            // showing, which checks it then.
            if self.tasks_shown(cx) {
                self.tasks_page
                    .update(cx, |page, cx| page.set_projects(local, cx));
            } else if self.workspace_shown(cx) {
                let projects = self.page_projects();
                self.workspace_page
                    .update(cx, |page, cx| page.set_projects(projects, cx));
            }
            // The Schedules page's list and filter, likewise.
            self.schedules_page
                .update(cx, |page, cx| page.set_projects(named, None, cx));
        }
        // This sweep is also the answer to "is the selected project a
        // repository", which decides whether its page offers to split it
        // into a worktree.
        self.sync_project_facts(cx);
        self.tell_issue_works(cx);
        cx.notify();
    }

    /// Hand the Workbench the window's `git status`, for its tree's badges.
    /// A host's folder on screen on a path a folder here also has is given
    /// none, since what is kept under that path is the local one's.
    pub(super) fn tell_workbench_git(&mut self, cx: &mut Context<Self>) {
        let mut git = self.window.workspace.git().clone();
        if let Some(root) = self.window.workspace.active_root()
            && self.window.workspace.shares_local_path(root)
        {
            git.remove(&root.path);
        }
        self.workbench
            .update(cx, |panel, cx| panel.set_git(git, cx));
    }
}
