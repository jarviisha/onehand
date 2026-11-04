//! The `git status` sweep: every project's repository read again, off the
//! UI loop, and pushed to whatever names a branch or marks a change; and the
//! one call that re-reads everything derived from the files on disk.

use super::Shell;
use gpui::{Context, SharedString};
use onehand_core::disk::Disk;
use onehand_core::workspace::{HostName, Workspace};
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

    /// Refresh `git status` for every root, off the UI loop.
    ///
    /// Uses core's blocking reader on GPUI's background executor rather than
    /// its tokio wrapper: tokio's process driver is not running under GPUI, so
    /// the async path would panic looking for a reactor. A host's status is
    /// asked over its attach in the same pass, blocking there likewise.
    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let roots = git_roots(&self.window.workspace, |host| {
            crate::hosts::live(cx, host).map(|conn| crate::hosts::disk_over(cx, conn))
        });

        // Two refreshes can be in flight at once -- a turn ends while the
        // window is being activated, say -- and `git status` on a big repo does
        // not finish in the order it was asked for. Without this, the slower
        // (older) scan lands last and overwrites the newer snapshot.
        self.git_generation = self.git_generation.wrapping_add(1);
        let generation = self.git_generation;

        cx.spawn(async move |shell, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move {
                    roots
                        .into_iter()
                        .filter_map(|(root, disk)| {
                            disk.git_status(&root).map(|status| (root, status))
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            shell
                .update(cx, |shell, cx| {
                    if shell.git_generation != generation {
                        return;
                    }
                    shell.window.git = scanned.into_iter().collect();
                    shell.tell_workbench_git(cx);
                    // The workspace page's project tiles name each branch.
                    if shell.page_shown(cx) {
                        let local = shell.local_page_projects();
                        let named = local
                            .iter()
                            .map(|p| (p.root.clone(), SharedString::from(p.label.to_string())))
                            .collect();
                        // The Tasks page's or the workspace page's projects,
                        // while it is the page: a filter left on another page
                        // waits for its next showing, which checks it then.
                        if shell.tasks_shown(cx) {
                            shell
                                .tasks_page
                                .update(cx, |page, cx| page.set_projects(local, cx));
                        } else if shell.workspace_shown(cx) {
                            let projects = shell.page_projects();
                            shell
                                .workspace_page
                                .update(cx, |page, cx| page.set_projects(projects, cx));
                        }
                        // The Schedules page's list and filter, likewise.
                        shell
                            .schedules_page
                            .update(cx, |page, cx| page.set_projects(named, None, cx));
                    }
                    // This sweep is also the answer to "is the selected project
                    // a repository", which decides whether its page offers to
                    // split it into a worktree.
                    shell.sync_project_facts(cx);
                    shell.tell_issue_works(cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Hand the Workbench the window's `git status`, for its tree's badges.
    /// A host's folder on screen on a path a folder here also has is given
    /// none, since what is kept under that path is the local one's.
    pub(super) fn tell_workbench_git(&mut self, cx: &mut Context<Self>) {
        let mut git = self.window.git.clone();
        if let Some(root) = self.window.workspace.active_root()
            && crate::state::shares_local_path(&self.window.workspace, root)
        {
            git.remove(&root.path);
        }
        self.workbench
            .update(cx, |panel, cx| panel.set_git(git, cx));
    }
}

/// The roots a `git status` sweep reads, each with the disk it is read
/// through: those on this machine, and those on a host `live` has an attach
/// to. A host not attached is left out rather than attached for a sweep.
///
/// What a sweep reads is kept by path alone, so a host's folder on a path a
/// folder here also has is left out too: its status would be read as the
/// local one's, and the local one's as its.
pub(super) fn git_roots(
    workspace: &Workspace,
    live: impl Fn(&HostName) -> Option<Disk>,
) -> Vec<(PathBuf, Disk)> {
    workspace
        .roots
        .iter()
        .filter_map(|root| {
            let disk = match root.on_host() {
                None => Disk::Local,
                Some(_) if crate::state::shares_local_path(workspace, root) => return None,
                Some(host) => live(host)?,
            };
            Some((root.path.clone(), disk))
        })
        .collect()
}
