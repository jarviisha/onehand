//! A project's worktrees as its page lists them: those it records, each on
//! disk or missing, and the checkouts git lists that no project records.
//!
//! **Which worktrees exist is git's.** The project records what onehand knows
//! of them; git is asked what is on disk.

use super::{Project, Projects};
use crate::worktree::Listed;
use std::path::{Path, PathBuf};

/// One row of a project's worktrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeRow {
    /// The recorded worktree at this folder, by path rather than index, so a
    /// row read before the record changed never names another; `on_disk`
    /// while git lists its checkout.
    Recorded { path: PathBuf, on_disk: bool },
    /// A checkout git lists that no project records, as the folder of it the
    /// project would run in.
    Offered(Listed),
}

/// The rows of `project`'s worktrees: each it records, in its order, then
/// each checkout of `listed` (git's list, none when it could not be read)
/// that is neither the main checkout nor the project's own, holds the
/// project's sub-folder (`exists` says, of the folder it would be), and no
/// project of `others` or `project` records. With git unread, every recorded
/// worktree is drawn as on disk and nothing is offered.
pub fn rows(
    project: &Project,
    listed: Option<&[Listed]>,
    others: &Projects,
    exists: impl Fn(&Path) -> bool,
) -> Vec<WorktreeRow> {
    let recorded = |on_disk: &dyn Fn(&Path) -> bool| {
        (project.worktrees.iter())
            .map(|worktree| WorktreeRow::Recorded {
                path: worktree.path.clone(),
                on_disk: on_disk(&worktree.path),
            })
            .collect::<Vec<_>>()
    };
    let Some(listed) = listed else {
        return recorded(&|_| true);
    };
    // The project folder's own checkout, and its sub-folder of it.
    let own = (listed.iter())
        .filter(|l| project.path.starts_with(&l.path))
        .max_by_key(|l| l.path.components().count());
    let sub = own
        .and_then(|l| project.path.strip_prefix(&l.path).ok())
        .unwrap_or(Path::new(""));
    let folder_of = |l: &Listed| -> PathBuf { l.path.join(sub) };
    let mut rows = recorded(&|path| listed.iter().any(|l| folder_of(l) == path));
    let main = listed.first();
    for l in listed {
        let folder = folder_of(l);
        let taken = project.folder(None, &folder).is_some() || others.find(None, &folder).is_some();
        if Some(l) == main || Some(l) == own || taken || !exists(&folder) {
            continue;
        }
        rows.push(WorktreeRow::Offered(Listed {
            path: folder,
            branch: l.branch.clone(),
        }));
    }
    rows
}

#[cfg(test)]
mod tests;
