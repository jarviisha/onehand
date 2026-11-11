//! What a person presses on a task that is read or judged before it is
//! asked of them, its button waiting meanwhile. *Answer the pull request
//! review* reads what it is judged on first. *Remove worktree…* on a task
//! whose pull request merged is judged when it is pressed, asked of the
//! person, judged again as things stand once they say yes, then removed,
//! and kept with the task, whatever became of the window it was pressed in.
//! Never automatic.

use super::{Effect, Signal, TasksDrive};
use crate::task::work::Act;
use crate::worktree::removal::{Facts, Judged, Removed};
use std::path::PathBuf;

impl<W: Copy> TasksDrive<W> {
    /// What a person pressed on task `id` that is still being done.
    pub fn acting(&self, id: &str) -> Option<Act> {
        self.acting.get(id).copied()
    }

    /// *Answer the pull request review* pressed on task `id` in `window`:
    /// what it is judged on read first, its button waiting meanwhile.
    #[must_use]
    pub fn answer_review(&mut self, id: &str, window: W) -> Vec<Effect<W>> {
        if self.task(id).is_none() {
            return Vec::new();
        }
        self.acting.insert(id.to_string(), Act::AnswerReview);
        vec![
            Effect::ReadReview {
                task: id.to_string(),
                window,
            },
            Effect::Refresh,
        ]
    }

    /// Remove task `id`'s worktree and branch, pressed in `window`: judged
    /// first, then asked of the person; `confirmed` once they said yes, when
    /// it is judged again and removed if nothing came up meanwhile.
    #[must_use]
    pub fn remove_worktree(&mut self, id: &str, confirmed: bool, window: W) -> Vec<Effect<W>> {
        let Some(task) = self.task(id) else {
            return Vec::new();
        };
        let judge = Effect::JudgeRemoval {
            task: id.to_string(),
            dir: task.setup.dir.clone(),
            repo: task.setup.repo.clone(),
            branch: task.setup.branch.clone(),
            forge: task.setup.forge.clone(),
            confirmed,
            window,
        };
        self.acting.insert(id.to_string(), Act::RemoveWorktree);
        vec![judge, Effect::Refresh]
    }

    pub(super) fn judged(
        &mut self,
        id: String,
        confirmed: bool,
        window: W,
        repo: PathBuf,
        judged: Option<(Box<Facts>, Judged)>,
        fx: &mut Vec<Effect<W>>,
    ) {
        self.acting.remove(&id);
        fx.push(Effect::Refresh);
        let Some((facts, judged)) = judged else {
            return;
        };
        if !confirmed {
            return fx.push(Effect::ConfirmRemoval {
                task: id,
                facts,
                judged,
                window,
            });
        }
        match judged {
            Judged::Remove { branch, .. } => {
                self.acting.insert(id.clone(), Act::RemoveWorktree);
                fx.push(Effect::RemoveWorktree {
                    task: id,
                    repo,
                    folder: facts.folder,
                    branch,
                    window,
                });
            }
            Judged::Refused(why) => fx.push(Effect::Signal(Signal::NotRemoved { window, why })),
        }
    }

    pub(super) fn worktree_gone(
        &mut self,
        id: String,
        window: W,
        folder: PathBuf,
        branch: Option<String>,
        removed: Result<Removed, String>,
        fx: &mut Vec<Effect<W>>,
    ) {
        self.acting.remove(&id);
        match removed {
            Ok(removed) => {
                // Kept with the task, so nothing offers to remove them again.
                if let Some(task) = self.task_mut(&id) {
                    task.worktree_removed = true;
                    fx.push(Effect::Save(Box::new(task.clone())));
                }
                fx.push(Effect::Signal(Signal::Removed {
                    window,
                    folder,
                    branch,
                    kept: removed.branch_kept,
                }));
            }
            Err(why) => fx.push(Effect::Signal(Signal::RemoveFailed { window, why })),
        }
        fx.push(Effect::Refresh);
    }
}
