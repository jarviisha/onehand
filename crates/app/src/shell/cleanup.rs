//! *Remove worktree…*: a task's worktree and branch removed after its pull
//! request merged, on a person's press and never otherwise.
//!
//! The rule is core's (`worktree::removal::judge`), and what happens when is
//! the tasks' drive's; what is gathered here is what only the app can see:
//! everything of onehand's using the folder, in every window, and the modal
//! that asks. onehand closes nothing on the person's behalf: each user is
//! named, and the removal waits until they are gone.

use super::Shell;
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
            ..Default::default()
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
        for (_, other) in crate::windows::others(cx.entity_id(), cx) {
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
            .filter(|root| root.local_path().is_some())
            // Its folders inside: its own, a worktree of it, a session's.
            .filter_map(|root| {
                let inside: Vec<std::path::PathBuf> = (root.folders_used().into_iter())
                    .filter(|path| path.starts_with(folder))
                    .collect();
                (!inside.is_empty()).then_some((root, inside))
            })
            .filter_map(|(root, inside)| {
                let terminal = self.terminal.read(cx);
                let terminals: usize = inside.iter().map(|path| terminal.shells_in(path)).sum();
                let sessions = (root.sessions.iter())
                    .filter(|session| inside.contains(&session.folder))
                    .count();
                let workbench = self.workbench.read(cx);
                let unsaved: usize = (inside.iter())
                    .map(|path| workbench.unsaved_in(path, false, cx))
                    .sum();
                let what = open_in(sessions, terminals, unsaved);
                // A worktree the row only records, nothing open in it, is no
                // project open on the folder: the row's own folder inside is.
                if what.is_empty() && !root.path.starts_with(folder) {
                    return None;
                }
                let what = match what.is_empty() {
                    true => String::new(),
                    false => format!(", with {}", what.join(" and ")),
                };
                Some(format!("Project {}, open in {said}{what},", root.label))
            })
            .collect();
        (users, processes)
    }
}

/// What a project has open in a folder, as said: its sessions, its shells
/// and the Workbench's unsaved files there; nothing when none.
fn open_in(sessions: usize, terminals: usize, unsaved: usize) -> Vec<String> {
    [
        (sessions, "session"),
        (terminals, "terminal"),
        (unsaved, "unsaved file"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, thing)| count(n, thing))
    .collect()
}

/// `n` of `thing`, as a person counts them.
fn count(n: usize, thing: &str) -> String {
    match n {
        1 if thing.starts_with(['a', 'e', 'i', 'o', 'u']) => format!("an {thing}"),
        1 => format!("a {thing}"),
        n => format!("{n} {thing}s"),
    }
}

#[cfg(test)]
mod users_tests {
    use super::open_in;

    #[test]
    fn unsaved_files_use_a_folder_as_sessions_and_shells_do() {
        assert_eq!(open_in(0, 0, 0), Vec::<String>::new());
        assert_eq!(open_in(1, 0, 3), ["a session", "3 unsaved files"]);
        assert_eq!(open_in(0, 2, 1), ["2 terminals", "an unsaved file"]);
    }
}
