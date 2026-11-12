//! *Remove worktree…*: a task's worktree and branch removed after its pull
//! request merged, on a person's press and never otherwise.
//!
//! The rule is core's (`worktree::removal::judge`), and what happens when is
//! the tasks' drive's; what is gathered here is what only the app can see:
//! everything of onehand's using the folder, in every window, and the modal
//! that asks. onehand closes nothing on the person's behalf: each user is
//! named, and the removal waits until they are gone.

use super::Shell;
use crate::state::Shared;
use gpui::{App, Context, Window};
use onehand_core::worktree::removal::{self, Judged, Process};
use std::path::Path;

impl Shell {
    /// Ask whether to remove task `id`'s worktree and branch, its pull
    /// request merged: judged first by the tasks' drive, off the UI loop.
    pub(crate) fn remove_worktree(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::task::remove_worktree(&id, false, window.window_handle(), cx);
    }

    /// Ask the person whether to remove task `id`'s worktree, as the tasks'
    /// drive judged it.
    pub(crate) fn confirm_removal(
        &mut self,
        id: String,
        facts: removal::Facts,
        judged: Judged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = facts.folder.display().to_string();
        let (lines, refused) = match judged {
            Judged::Remove { why, .. } => (vec![why], false),
            Judged::Refused(why) => (why, true),
        };
        // The branch named is the one that would go: the task's own.
        let what = match facts.expected.as_ref().or(facts.branch.as_ref()) {
            Some(branch) => format!("The worktree {folder} and its branch {branch} are deleted."),
            None => format!("The worktree {folder} is deleted; it is on no branch."),
        };
        let ask = super::Confirm {
            id: "remove-worktree",
            title: "Remove this worktree?".into(),
            description: what.into(),
            act: "Remove",
            lines,
            refused,
        };
        let handle = window.window_handle();
        super::confirm_on(cx.entity(), ask, window, cx, move |_, _, cx| {
            crate::task::remove_worktree(&id, true, handle, cx)
        });
    }

    /// Everything of onehand's using `folder`, in every window: a project
    /// open on it with its sessions and terminals, and a task working there;
    /// then every other shell and Neovim, whose directory is asked off the
    /// UI thread.
    pub(crate) fn folder_users(
        &self,
        folder: &Path,
        cx: &Context<Self>,
    ) -> (Vec<String>, Vec<Process>) {
        let (mut users, mut processes) = self.users_here(folder, "this window", cx);
        // This shell is the one being updated, so it is read as `self`.
        let this = cx.entity_id();
        for open in &Shared::global(cx).windows {
            let Some(other) = open.shell.upgrade().filter(|o| o.entity_id() != this) else {
                continue;
            };
            let (more, running) = other.read(cx).users_here(folder, "another window", cx);
            users.extend(more);
            processes.extend(running);
        }
        users.extend(crate::task::each(cx, |task, working| {
            (working && task.setup().dir.starts_with(folder)).then(|| {
                // A command step running there is named by its step.
                match task.runs().last().and_then(|run| run.current()) {
                    Some(step) => format!(
                        "Task “{}”, at its step {}, still working there,",
                        task.brief().title,
                        step.label
                    ),
                    None => format!("Task “{}”, still working there,", task.brief().title),
                }
            })
        }));
        (users, processes)
    }

    /// What of this window uses `folder`: each project open inside it, with
    /// its sessions and terminals.
    fn users_here(&self, folder: &Path, said: &str, cx: &App) -> (Vec<String>, Vec<Process>) {
        // A shell opened on a project inside the folder is said with it;
        // the rest are asked where they are now.
        let processes = self
            .terminal
            .read(cx)
            .processes()
            .into_iter()
            .filter(|(root, _)| !root.starts_with(folder))
            .map(|(_, pid)| Process {
                what: format!("A terminal in {said}"),
                pid,
            })
            .chain(
                self.workbench
                    .read(cx)
                    .processes(cx)
                    .into_iter()
                    .map(|p| Process {
                        what: format!("{} in {said}", p.what),
                        pid: p.pid,
                    }),
            )
            .collect();
        let users = self
            .window
            .workspace
            .roots
            .iter()
            // A host's folder of the same path is not this worktree.
            .filter(|root| {
                root.local_path()
                    .is_some_and(|path| path.starts_with(folder))
            })
            .map(|root| {
                let terminals = self.terminal.read(cx).shells_in(&root.path);
                let mut what = Vec::new();
                if !root.sessions.is_empty() {
                    what.push(count(root.sessions.len(), "session"));
                }
                if terminals > 0 {
                    what.push(count(terminals, "terminal"));
                }
                let what = match what.is_empty() {
                    true => String::new(),
                    false => format!(", with {}", what.join(" and ")),
                };
                format!("Project {}, open in {said}{what},", root.label)
            })
            .collect();
        (users, processes)
    }
}

/// `n` of `thing`, as a person counts them.
fn count(n: usize, thing: &str) -> String {
    match n {
        1 => format!("a {thing}"),
        n => format!("{n} {thing}s"),
    }
}
