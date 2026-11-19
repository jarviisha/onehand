//! Which project a folder belongs to.
//!
//! **One rule for every way a folder comes in**: the folder picker, a second
//! launch, a conversation resumed, a tool naming a path. A folder a project
//! already holds is that project's. Failing that, a linked checkout of a
//! known project's repository, at that project's own sub-folder of it, is a
//! worktree of that project: a second checkout is never a second project.
//! Anything else is a project of its own.
//!
//! A main checkout is never taken for a worktree, even of a project whose
//! own folder is a linked one: the rule only ever adds to a project, it never
//! changes which folder a project was added from.

use super::{Folder, ProjectId, Projects};
use crate::workspace::HostName;
use crate::worktree::current_branch_blocking;
use crate::worktree::worker::repo_of_blocking;
use std::path::{Path, PathBuf};

/// Where a folder goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// A folder of this project already.
    Known(ProjectId, Folder),
    /// A further checkout of this project's repository, on `branch`.
    WorktreeOf { project: ProjectId, branch: String },
    /// A project of its own.
    New,
}

/// Where `folder` on `host` goes among `projects`. Blocking: git is asked
/// about the folder, and about a project's folder only when the folder is a
/// linked checkout. git failing to answer is the same as no repository.
pub fn place_blocking(projects: &Projects, host: Option<&HostName>, folder: &Path) -> Placement {
    let canon = match host {
        // A host's folder names nothing here to resolve.
        Some(_) => folder.to_path_buf(),
        None => std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()),
    };
    if let Some((project, at)) = projects
        .find(host, &canon)
        .or_else(|| projects.find(host, folder))
    {
        return Placement::Known(project.id.clone(), at);
    }
    if host.is_some() {
        return Placement::New;
    }
    let Some((common, sub, false)) = repo_key_blocking(&canon) else {
        return Placement::New;
    };
    let owner = projects
        .iter()
        .filter(|project| project.host.is_none())
        .find(|project| {
            repo_key_blocking(&project.path).is_some_and(|(c, s, _)| c == common && s == sub)
        });
    match owner {
        Some(project) => Placement::WorktreeOf {
            project: project.id.clone(),
            branch: current_branch_blocking(&canon).unwrap_or_else(|_| "HEAD".to_string()),
        },
        None => Placement::New,
    }
}

/// `folder`'s repository, as what tells checkouts of one repository apart
/// from another's: its common git directory, the folder's path within its
/// checkout, and whether that checkout is the main one. `None` outside git,
/// or when git cannot say. Blocking.
pub(crate) fn repo_key_blocking(folder: &Path) -> Option<(PathBuf, PathBuf, bool)> {
    let canon = std::fs::canonicalize(folder).ok()?;
    let repo = repo_of_blocking(&canon).ok()??;
    let sub = canon.strip_prefix(&repo.top).ok()?.to_path_buf();
    let main = repo.common == repo.top.join(".git");
    Some((repo.common, sub, main))
}

#[cfg(test)]
mod tests;
