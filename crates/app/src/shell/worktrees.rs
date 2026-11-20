//! What a project page's *Worktrees* box asks of the window: a session in a
//! worktree, a checkout git lists taken in, a missing one forgotten, a
//! worktree removed, and the folder new worktrees go in.
//!
//! **Removing one is judged twice**, when its question opens and again when
//! it is answered, since anything can change between the two; the rule is
//! core's (`worktree::removal::judge_own`), the record's change the projects'
//! drive's, and what is gathered here is what only the app sees: everything of
//! onehand's using the folder, in every window.

use super::{Confirm, Shell};
use crate::chat::pane::WorktreeAct;
use gpui::{Context, Window};
use onehand_core::project::{MadeBy, Worktree};
use onehand_core::worktree::removal::{self, Judged, OwnFacts};
use std::path::{Path, PathBuf};

/// What a removal was judged on and came to, and whether its branch would go.
struct Judging {
    facts: OwnFacts,
    judged: Judged,
    merged: bool,
}

impl Shell {
    /// Carry out what the project page's *Worktrees* asked, about the
    /// project on screen.
    pub(super) fn worktree_act(
        &mut self,
        act: WorktreeAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match act {
            WorktreeAct::NewSession(folder) => {
                let idx = self.window.workspace.active_root;
                let _ = self.new_session_in_folder(idx, folder, 0, window, cx);
            }
            WorktreeAct::Remove(folder) => self.ask_remove_worktree(folder, window, cx),
            WorktreeAct::Add { folder, branch } => self.add_worktree(folder, branch, window, cx),
            WorktreeAct::Forget(folder) => {
                if let Some(root) = self.project_here() {
                    crate::projects::forget_worktree(&root, &folder, cx);
                }
            }
            WorktreeAct::PickFolder => self.pick_worktree_folder(cx),
            WorktreeAct::ClearFolder => {
                if let Some(root) = self.project_here() {
                    crate::projects::set_worktree_parent(&root, None, cx);
                }
            }
        }
    }

    /// The project on screen's own folder, when it is on this machine.
    fn project_here(&self) -> Option<PathBuf> {
        self.window
            .workspace
            .active_root()
            .and_then(|root| root.local_path().map(Path::to_path_buf))
    }

    /// Take in `folder`, a checkout git lists, on `branch`, as a worktree of
    /// the project on screen.
    pub(super) fn add_worktree(
        &mut self,
        folder: PathBuf,
        branch: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project_here() else {
            return;
        };
        let worktree = Worktree {
            path: folder,
            branch: branch.unwrap_or_else(|| "HEAD".into()),
            from: None,
            made_by: MadeBy::Added,
            offload: None,
        };
        let _ = crate::projects::record(&root, worktree, cx);
    }

    /// Choose the folder the project on screen's new worktrees go in.
    fn pick_worktree_folder(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project_here() else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let Some(dir) = super::storage::pick_folder(cx).await else {
                return;
            };
            cx.update(|cx| crate::projects::set_worktree_parent(&root, Some(dir), cx));
        })
        .detach();
    }

    /// Judge removing the worktree at `folder` of the project on screen, off
    /// the UI loop, then hand what it came to to `then`.
    fn judge_worktree(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, Judging, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(root) = self.project_here() else {
            return;
        };
        let own = crate::projects::of(&root, cx).map(|project| project.id);
        cx.spawn_in(window, async move |shell, cx| {
            let top = cx
                .background_executor()
                .spawn({
                    let folder = folder.clone();
                    async move { removal::top_blocking(&folder) }
                })
                .await;
            let Ok((users, processes, shared_with, tasks)) = shell.update(cx, |shell, cx| {
                let (users, processes) = shell.folder_users(&top, cx);
                // Another project with a folder inside this checkout, and the
                // tasks whose worktree it is.
                let shared_with =
                    (crate::state::Shared::global(cx).projects).sharing(&top, own.as_ref());
                let tasks = crate::task::each(cx, |task, _| {
                    task.setup()
                        .dir
                        .starts_with(&top)
                        .then(|| task.brief().title.clone())
                });
                (users, processes, shared_with, tasks)
            }) else {
                return;
            };
            let judging = cx
                .background_executor()
                .spawn({
                    let (top, root) = (top.clone(), root.clone());
                    async move {
                        let mut users = users;
                        users.extend(removal::working_in_blocking(&top, &processes));
                        let facts = removal::own_facts_blocking(&top, users, shared_with, tasks);
                        let judged = removal::judge_own(&facts);
                        let merged = facts
                            .branch
                            .as_deref()
                            .is_some_and(|branch| removal::branch_merged_blocking(&root, branch));
                        Judging {
                            facts,
                            judged,
                            merged,
                        }
                    }
                })
                .await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                then(shell, folder, judging, window, cx)
            });
        })
        .detach();
    }

    /// Ask whether to remove the worktree at `folder` of the project on
    /// screen, as it is judged now; remove it on the answer.
    fn ask_remove_worktree(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.judge_worktree(folder, window, cx, |shell, folder, judging, window, cx| {
            let (facts, judged, merged) = (judging.facts, judging.judged, judging.merged);
            let at = facts.folder.display().to_string();
            let named = facts.branch.clone().unwrap_or_else(|| at.clone());
            let branch = match (&facts.branch, merged) {
                (Some(branch), true) => format!(" Its branch {branch} goes with it: it is merged."),
                (Some(branch), false) => format!(" Its branch {branch} is kept: it is not merged."),
                (None, _) => " It is on no branch.".to_string(),
            };
            let (lines, refused) = match judged {
                Judged::Remove { .. } => (Vec::new(), false),
                Judged::Refused(why) => (why, true),
            };
            let ask = Confirm {
                id: "remove-own-worktree",
                title: format!("Remove worktree {named}?").into(),
                description: format!("The worktree {at} is deleted from disk.{branch}").into(),
                act: "Remove",
                lines,
                refused,
                ..Default::default()
            };
            shell.confirm(ask, window, cx, move |shell, window, cx| {
                shell.remove_worktree_now(folder.clone(), window, cx)
            });
        });
    }

    /// Remove the worktree at `folder` of the project on screen, judged
    /// again first, and forget it in the project; how it went is said.
    pub(super) fn remove_worktree_now(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project_here() else {
            return;
        };
        self.judge_worktree(folder, window, cx, move |_, folder, judging, window, cx| {
            use onehand_core::project::drive::Removal;
            let branch = match judging.judged {
                Judged::Remove { branch, .. } => branch,
                Judged::Refused(why) => {
                    let refused = Removal::Refused(why);
                    return crate::projects::worktree_removal(&root, &folder, refused, window, cx);
                }
            };
            let top = judging.facts.folder;
            cx.spawn_in(window, async move |_, cx| {
                let result = cx
                    .background_executor()
                    .spawn({
                        let (root, top, branch) = (root.clone(), top.clone(), branch.clone());
                        async move { removal::remove_own_blocking(&root, &top, branch.as_deref()) }
                    })
                    .await;
                let _ = cx.update(|window, cx| {
                    let done = Removal::Done {
                        top,
                        branch,
                        result,
                    };
                    crate::projects::worktree_removal(&root, &folder, done, window, cx);
                });
            })
            .detach();
        });
    }
}

#[cfg(test)]
mod tests;
