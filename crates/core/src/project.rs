//! A project as onehand keeps it: an id and a name of its own, the folder it
//! was added from, and the worktrees cut from that folder's repository.
//!
//! ```text
//! <data root>/projects/<id>/project.toml
//! ```
//!
//! **The id is the project, not the folder.** A project used to be its folder:
//! its name was the folder's, and every fact about it was kept under the
//! folder's path in each workspace that held it. Renaming one then meant
//! renaming the folder, a folder in two workspaces was two projects, and a
//! worktree split off one was a stranger to it. Here the folder is one fact
//! about the project among others, and every workspace that holds the project
//! holds the same one.
//!
//! **Which worktrees exist is git's.** A project records the ones it knows
//! about (where they came from, who cut them); git is asked what is on disk.

use crate::workspace::{label_for, BuildHost, HostName, Offloaded};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub mod drive;
pub mod file;
pub(crate) mod migrate;
mod place;
pub mod worktrees;
pub use drive::ProjectsDrive;
pub use place::{place_blocking, Placement};

/// A project's id: eight lowercase hex characters, minted once and never
/// derived from a path or a name, so neither a rename nor a move changes it.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    /// The id `seed` gives. Whoever mints one hands in the seed (a clock, a
    /// counter) and tries the next when the id is taken: nothing here reads a
    /// clock or the disk.
    pub fn mint(seed: u64) -> ProjectId {
        ProjectId(format!(
            "{:08x}",
            crate::workspace::fnv1a(&seed.to_le_bytes())
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who cut a worktree, or that git listed it and it was adopted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MadeBy {
    /// Split off the project by a person.
    Split,
    /// Cut for a workflow's run, which keeps its own record of it.
    Run,
    /// Where a lead's worker was opened.
    Worker,
    /// A checkout of the repository onehand did not cut, taken in.
    Added,
}

/// A further checkout of the project folder's repository, as the project
/// records it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Worktree {
    /// The folder sessions run in: the checkout's own top, or the same
    /// sub-folder of it that the project folder is of its repository.
    pub path: PathBuf,
    pub branch: String,
    /// The branch or commit it was cut from, when that is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub made_by: MadeBy,
    /// Where this folder's sessions run while it is offloaded: each folder
    /// of a project is offloaded on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload: Option<Offloaded>,
}

/// One project. Everything but the id, the name and the folder is left out
/// of its file while it holds its default, so a hand-read file says only what
/// was chosen.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// The host the project folder lives on; none for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostName>,
    /// The folder the project was added from.
    pub path: PathBuf,
    /// Where the project folder's sessions run while it is offloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload: Option<Offloaded>,
    /// The command a workflow's work must pass before it goes further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The hosts it builds on, and what comes back from each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builds: Vec<BuildHost>,
    /// The folder new worktrees go in; none for beside the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_parent: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktrees: Vec<Worktree>,
}

/// Which of a project's folders a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    /// The project folder.
    Home,
    /// The worktree at this index of [`Project::worktrees`].
    Worktree(usize),
}

impl Project {
    /// A project on `path`, named after its folder.
    pub fn new(id: ProjectId, host: Option<HostName>, path: PathBuf) -> Project {
        Project {
            id,
            name: label_for(&path),
            host,
            path,
            offload: None,
            check: None,
            builds: Vec::new(),
            worktree_parent: None,
            worktrees: Vec::new(),
        }
    }

    /// Which of this project's folders `path` on `host` is, if any.
    pub fn folder(&self, host: Option<&HostName>, path: &Path) -> Option<Folder> {
        if self.host.as_ref() != host {
            return None;
        }
        if self.path == path {
            return Some(Folder::Home);
        }
        self.worktrees
            .iter()
            .position(|worktree| worktree.path == path)
            .map(Folder::Worktree)
    }

    /// Where `folder`'s sessions run while it is offloaded.
    pub fn offload_of(&self, folder: Folder) -> Option<&Offloaded> {
        match folder {
            Folder::Home => self.offload.as_ref(),
            Folder::Worktree(at) => self.worktrees.get(at)?.offload.as_ref(),
        }
    }

    /// What a row of `folder` is called: the project's name for its own
    /// folder, a worktree's folder name for a worktree, as one has always
    /// been labelled.
    pub fn label_of(&self, folder: Folder) -> String {
        match folder {
            Folder::Home => self.name.clone(),
            Folder::Worktree(at) => self
                .worktrees
                .get(at)
                .map_or_else(|| self.name.clone(), |worktree| label_for(&worktree.path)),
        }
    }
}

/// Every project this process has read, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Projects(BTreeMap<ProjectId, Project>);

impl Projects {
    pub fn get(&self, id: &ProjectId) -> Option<&Project> {
        self.0.get(id)
    }

    /// The project `path` on `host` is a folder of, and which folder.
    pub fn find(&self, host: Option<&HostName>, path: &Path) -> Option<(&Project, Folder)> {
        self.0
            .values()
            .find_map(|project| Some((project, project.folder(host, path)?)))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Project> {
        self.0.values()
    }

    /// Keep `project`, in place of the one with its id.
    pub(crate) fn put(&mut self, project: Project) {
        self.0.insert(project.id.clone(), project);
    }

    pub(crate) fn get_mut(&mut self, id: &ProjectId) -> Option<&mut Project> {
        self.0.get_mut(id)
    }
}

impl FromIterator<Project> for Projects {
    fn from_iter<I: IntoIterator<Item = Project>>(projects: I) -> Self {
        Projects(
            projects
                .into_iter()
                .map(|project| (project.id.clone(), project))
                .collect(),
        )
    }
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
