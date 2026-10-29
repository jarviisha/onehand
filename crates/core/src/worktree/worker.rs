//! Which folder a lead's worker may be opened on, and which further folders
//! it may write.
//!
//! **Every path is canonicalized before it is compared.** git answers a
//! common directory relative to where it ran, the same repository reads as
//! different strings from a main checkout and from a linked worktree, and a
//! symlink names a folder under another name. A rule comparing the strings
//! it was given would let one folder pass for another: a worktree for a
//! stranger, or the worktree itself for a harmless run folder.

use super::{git, git_message, LOCAL_LIMIT};
use crate::process::output_within;
use std::path::{Path, PathBuf};

/// A checkout and the repository it belongs to, both canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The checkout's top level.
    pub top: PathBuf,
    /// The repository's common git directory, shared by every worktree.
    pub common: PathBuf,
}

/// The checkout `dir` is in, and its repository. Blocking.
pub fn repo_of_blocking(dir: &Path) -> Result<Repo, String> {
    let ask = |arg: &str| -> Result<PathBuf, String> {
        let out = output_within(git(dir).args(["rev-parse", arg]), LOCAL_LIMIT)
            .map_err(|why| format!("git could not be run in {}: {why}", dir.display()))?;
        if !out.status.success() {
            return Err(format!(
                "{} is not a git checkout: {}",
                dir.display(),
                git_message(&out.stderr)
            ));
        }
        let said = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        let full = if said.is_absolute() {
            said
        } else {
            dir.join(said)
        };
        std::fs::canonicalize(&full).map_err(|why| format!("{}: {why}", full.display()))
    };
    Ok(Repo {
        top: ask("--show-toplevel")?,
        common: ask("--git-common-dir")?,
    })
}

/// `path` as a worker's folder: the top level of a checkout of a repository
/// one of `projects` belongs to. Blocking.
pub fn worktree_of_blocking(path: &Path, projects: &[PathBuf]) -> Result<Repo, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let real = std::fs::canonicalize(path).map_err(|why| format!("{}: {why}", path.display()))?;
    let repo = repo_of_blocking(&real)?;
    if repo.top != real {
        return Err(format!(
            "{} is inside the checkout at {}; name its top level",
            path.display(),
            repo.top.display()
        ));
    }
    // A project that is no checkout at all simply belongs to no repository.
    let ours = projects
        .iter()
        .filter_map(|project| repo_of_blocking(project).ok())
        .any(|project| project.common == repo.common);
    if !ours {
        return Err(format!(
            "{} is a checkout of a repository no project of this window belongs to",
            path.display()
        ));
    }
    Ok(repo)
}

/// `dirs` as folders a worker on `repo` may write, canonical, in order and
/// each once. Refused at the first that is not an existing absolute folder,
/// or that is `/`, `home`, the checkout or inside it, or that holds the
/// checkout or the repository's main checkout. Blocking.
pub fn writable_blocking(
    dirs: &[PathBuf],
    repo: &Repo,
    home: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    let home = home.and_then(|home| std::fs::canonicalize(home).ok());
    let main = match repo.common.file_name() {
        Some(name) if name == ".git" => repo.common.parent().unwrap_or(&repo.common),
        _ => &repo.common,
    };
    let mut kept: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !dir.is_absolute() {
            return Err(format!("{} is not an absolute path", dir.display()));
        }
        let real = std::fs::canonicalize(dir)
            .ok()
            .filter(|real| real.is_dir())
            .ok_or_else(|| format!("{} is not an existing folder", dir.display()))?;
        let why = if real.parent().is_none() {
            Some("the whole file system")
        } else if home.as_deref() == Some(real.as_path()) {
            Some("the home folder")
        } else if real.starts_with(&repo.top) {
            Some("the worker's own checkout")
        } else if repo.top.starts_with(&real) {
            Some("a folder holding the worker's checkout")
        } else if main.starts_with(&real) {
            Some("a folder holding the repository")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(format!(
                "{} (that is {}) cannot be given to a worker: it is {why}",
                dir.display(),
                real.display()
            ));
        }
        if !kept.contains(&real) {
            kept.push(real);
        }
    }
    Ok(kept)
}

/// Everything a worker on `repo` writes besides its checkout: the
/// repository's common git directory when the checkout is a linked worktree
/// (a commit there writes into it), then `writable`.
pub fn worker_dirs(repo: &Repo, writable: Vec<PathBuf>) -> Vec<PathBuf> {
    let linked = !repo.common.starts_with(&repo.top);
    linked
        .then(|| repo.common.clone())
        .into_iter()
        .chain(writable)
        .collect()
}

#[cfg(test)]
mod tests;
