//! The `git status` sweep: every project's repository read again, off the
//! UI loop, and pushed to whatever names a branch or marks a change.

use super::Shell;
use gpui::{Context, SharedString};
use onehand_core::disk::Disk;
use onehand_core::workspace::{HostName, Workspace};
use std::path::PathBuf;

impl Shell {
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
                    let git = shell.window.git.clone();
                    shell
                        .workbench
                        .update(cx, |panel, cx| panel.set_git(git, cx));
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
}

/// The roots a `git status` sweep reads, each with the disk it is read
/// through: those on this machine, and those on a host `live` has an attach
/// to. A host not attached is left out rather than attached for a sweep.
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
                Some(host) => live(host)?,
            };
            Some((root.path.clone(), disk))
        })
        .collect()
}
