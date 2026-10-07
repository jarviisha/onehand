//! *Remove worktree…*: a task's worktree and branch removed after its pull
//! request merged, on a person's press and never otherwise.
//!
//! The rule is core's (`worktree::removal::judge`); what is gathered here is
//! what only the app can see: everything of onehand's using the folder, in
//! every window. The judgement runs when the modal opens and again when it is
//! confirmed, off the UI thread, since anything can change between the two,
//! and onehand closes nothing on the person's behalf: each user is named, and
//! the removal waits until they are gone.

use super::Shell;
use crate::state::Shared;
use gpui::{App, Context, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use onehand_core::connector::PrState;
use onehand_core::task::Task;
use onehand_core::worktree::removal::{self, Judged, Merged, Process};
use std::path::{Path, PathBuf};

/// What a removal reads, gathered on the UI thread before the git and
/// forge reads go off it.
struct Asked {
    repo: PathBuf,
    folder: PathBuf,
    branch: Option<String>,
    forge: Option<&'static dyn onehand_core::connector::Connector>,
    users: Vec<String>,
    /// Every shell and Neovim of every window, each with what it is
    /// called, to be asked where it works now.
    processes: Vec<Process>,
}

impl Asked {
    fn of(task: &Task, folder: PathBuf, (users, processes): (Vec<String>, Vec<Process>)) -> Self {
        Self {
            repo: task.setup.repo.clone(),
            folder,
            branch: task.setup.branch.clone(),
            forge: task.setup.forge.as_deref().and_then(|name| {
                onehand_core::connector::named(crate::plugins::connectors(), name)
            }),
            users,
            processes,
        }
    }

    /// The forge's word on the pull request, then git's on the folder, and
    /// the judgement. Blocking.
    fn judge_blocking(&self) -> (removal::Facts, Judged) {
        let merged = match (self.forge, &self.branch) {
            (Some(forge), Some(branch)) => {
                forge
                    .pull_request_for_blocking(&self.repo, branch)
                    .map(|pr| {
                        pr.filter(|pr| pr.state == PrState::Merged)
                            .map(|pr| Merged {
                                number: pr.number,
                                head: pr.head,
                            })
                    })
            }
            (None, _) => Err("no forge serves the project".to_string()),
            (_, None) => Err("the task has no branch to look its pull request up by".to_string()),
        };
        let mut users = self.users.clone();
        users.extend(removal::working_in_blocking(&self.folder, &self.processes));
        let facts = removal::facts_blocking(&self.folder, self.branch.clone(), merged, users);
        let judged = removal::judge(&facts);
        (facts, judged)
    }
}

impl Shell {
    /// Judge removing task `id`'s worktree, then hand `done` what was
    /// asked and found. The worktree's own top is found first, off the UI
    /// thread, since a project in a folder of its repository works in that
    /// folder of the worktree; what uses it is gathered from there.
    fn judge_removal(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Asked, removal::Facts, Judged, &mut Window, &mut Context<Self>)
        + 'static,
    ) {
        let Some(task) = crate::task::task(id, cx) else {
            return;
        };
        let dir = task.setup.dir.clone();
        cx.spawn_in(window, async move |shell, cx| {
            let top = cx
                .background_executor()
                .spawn(async move { worktree_top_blocking(&dir) })
                .await;
            let Ok(asked) = shell.update(cx, |shell: &mut Self, cx| {
                let users = shell.folder_users(&top, cx);
                Asked::of(&task, top, users)
            }) else {
                return;
            };
            let (asked, facts, judged) = cx
                .background_executor()
                .spawn(async move {
                    let (facts, judged) = asked.judge_blocking();
                    (asked, facts, judged)
                })
                .await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                done(shell, asked, facts, judged, window, cx)
            });
        })
        .detach();
    }

    /// Ask whether to remove task `id`'s worktree and branch, its pull
    /// request merged: judged first, off the UI loop, and listed in a modal.
    pub(crate) fn remove_worktree(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked_for = id.clone();
        self.judge_removal(
            &asked_for,
            window,
            cx,
            move |shell, _, facts, judged, window, cx| {
                shell.confirm_removal(id, facts, judged, window, cx)
            },
        );
    }

    fn confirm_removal(
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
        let ask = super::Ask {
            id: "remove-worktree",
            title: "Remove this worktree?".into(),
            description: what.into(),
            act: "Remove",
            lines,
            refused,
        };
        super::ask_on(cx.entity(), ask, window, cx, move |shell, window, cx| {
            shell.removal_confirmed(id.clone(), window, cx)
        });
    }

    /// The person confirmed: judge again, as things stand now, and remove
    /// only if nothing has come up since the modal opened.
    fn removal_confirmed(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let asked_for = id.clone();
        self.judge_removal(
            &asked_for,
            window,
            cx,
            move |_, asked, _, judged, window, cx| {
                let branch = match judged {
                    Judged::Remove { branch, .. } => branch,
                    Judged::Refused(why) => {
                        let why = format!("Not removed, as things stand now: {}", why.join(" "));
                        window.push_notification(Notification::warning(why), cx);
                        return;
                    }
                };
                cx.spawn_in(window, async move |_, cx| {
                    let done = cx
                        .background_executor()
                        .spawn(async move {
                            removal::remove_blocking(&asked.repo, &asked.folder, branch.as_deref())
                                .map(|removed| (asked.folder, branch, removed))
                        })
                        .await;
                    let _ = cx.update(|window, cx| {
                        if done.is_ok() {
                            crate::task::worktree_removed(&id, cx);
                        }
                        let said = match done {
                            Ok((folder, branch, removed)) => {
                                let folder = folder.display();
                                match (branch, removed.branch_kept) {
                                    (Some(branch), Some(why)) => Notification::warning(format!(
                                        "Removed {folder}; the branch {branch} was kept: {why}"
                                    )),
                                    (Some(branch), None) => Notification::success(format!(
                                        "Removed {folder} and {branch}."
                                    )),
                                    (None, _) => {
                                        Notification::success(format!("Removed {folder}."))
                                    }
                                }
                            }
                            Err(why) => Notification::warning(why),
                        };
                        window.push_notification(said, cx);
                    });
                })
                .detach();
            },
        );
    }

    /// Everything of onehand's using `folder`, in every window: a project
    /// open on it with its sessions and terminals, and a task working there;
    /// then every other shell and Neovim, whose directory is asked off the
    /// UI thread.
    fn folder_users(&self, folder: &Path, cx: &Context<Self>) -> (Vec<String>, Vec<Process>) {
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
            (working && task.setup.dir.starts_with(folder)).then(|| {
                // A command step running there is named by its step.
                match task.runs.last().and_then(|run| run.current()) {
                    Some(step) => format!(
                        "Task “{}”, at its step {}, still working there,",
                        task.brief.title, step.label
                    ),
                    None => format!("Task “{}”, still working there,", task.brief.title),
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
            .filter(|root| root.path.starts_with(folder))
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

/// The top of the worktree `dir` is in, written the way `dir` is, so it is
/// compared with the workspace's own paths as they are kept: git answers
/// with the top made canonical, which a link on the way would not match.
fn worktree_top_blocking(dir: &Path) -> PathBuf {
    let Some(top) = onehand_core::worktree::repo_top_blocking(dir) else {
        return dir.to_path_buf();
    };
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let below = real
        .strip_prefix(&top)
        .map_or(0, |rel| rel.components().count());
    dir.ancestors().nth(below).map_or(top, Path::to_path_buf)
}

/// `n` of `thing`, as a person counts them.
fn count(n: usize, thing: &str) -> String {
    match n {
        1 => format!("a {thing}"),
        n => format!("{n} {thing}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::worktree_top_blocking;
    use std::process::Command;

    /// A project in a folder of its repository works in that folder of the
    /// worktree; the removal is of the worktree's top, written as the
    /// project's path is, through a link included.
    #[cfg(unix)]
    #[test]
    fn the_worktree_top_is_found_from_a_folder_in_it_and_kept_unresolved() {
        let base = std::env::temp_dir().join(format!("onehand-top-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("app/src")).unwrap();
        let git = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .arg("-q")
            .status();
        if !git.is_ok_and(|s| s.success()) {
            return;
        }
        let link = base.join("link");
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        assert_eq!(worktree_top_blocking(&link.join("app/src")), link);
        assert_eq!(worktree_top_blocking(&repo.join("app")), repo);
        let outside = base.join("plain");
        std::fs::create_dir_all(&outside).unwrap();
        assert_eq!(worktree_top_blocking(&outside), outside);
        let _ = std::fs::remove_dir_all(&base);
    }
}
