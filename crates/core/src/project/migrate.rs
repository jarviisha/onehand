//! Bringing every workspace's projects into the store, once, at launch.
//!
//! An earlier build kept each project as an entry of each workspace file that
//! held it, keyed by its folder, with its facts beside it; a worktree split
//! off a project was an entry of its own. This build keeps each project once,
//! in `projects/<id>/project.toml`, and a workspace's entries only name it.
//! [`migrate_projects_in`] gets from the one to the other.
//!
//! **One repository, one project.** Entries are grouped by what they are a
//! folder of: a host's folder by its host and path, a folder in git by its
//! repository and its place in its checkout, any other by its path. A group
//! is one project: its folder the main checkout's, its other folders its
//! worktrees. A group a project already holds adds to that project.
//!
//! **Nothing is lost on the way, and it is safe to run at every launch.**
//! Projects are written before the workspace files naming them, so a run cut
//! short between the two finds them again; an entry whose project could not
//! be written keeps its own facts; a workspace file is copied aside before
//! its first rewrite; one that cannot be read is never written. Each step
//! passes over what is already in place, so a second run changes nothing.

use super::file::{load_all_blocking, save_to, FILE};
use super::place::repo_key_blocking;
use super::{Folder, MadeBy, Project, ProjectId, Projects, Worktree};
use crate::config::{
    move_into, offload_in, projects_in, workspaces_in, ProjectConfig, WorkspaceConfig,
    WorkspaceLoad,
};
use crate::workspace::{stem_for, HostName, OFFLOAD};
use crate::worktree::current_branch_blocking;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Beside a workspace file, the copy of it made before its first rewrite.
const BEFORE: &str = "onehand-workspace.toml.before-projects";

/// What a group of entries is a folder of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Host(HostName, PathBuf),
    /// A repository's common directory, and the folder's path within its
    /// checkout.
    Repo(PathBuf, PathBuf),
    Folder(PathBuf),
}

/// An entry of a workspace file, by the file's place and its own.
type At = (usize, usize);

/// Bring every workspace's entries under the root `config` into the store,
/// minting new ids from `seed`. Blocking; answers the problems met.
pub(crate) fn migrate_projects_in(config: &Path, seed: u64) -> Vec<String> {
    let mut problems = Vec::new();
    move_offload_data(config, &mut problems);
    let projects_dir = projects_in(config);
    let (mut projects, unread) = load_all_blocking(&projects_dir);
    problems.extend(unread);
    let mut files = workspace_files(config, &mut problems);

    // Each entry no project names, grouped by what it is a folder of; or
    // the project already holding its folder.
    let mut named: Vec<(At, ProjectId)> = Vec::new();
    // Of those, the entries a project already held: an older build may have
    // written facts on them the project does not have.
    let mut found: Vec<(At, ProjectId)> = Vec::new();
    let mut groups: Vec<(Key, Vec<At>)> = Vec::new();
    for (f, (_, cfg)) in files.iter().enumerate() {
        for (e, entry) in cfg.projects.iter().enumerate() {
            if entry
                .id
                .as_ref()
                .is_some_and(|id| projects.get(id).is_some())
            {
                continue;
            }
            let path = canon(entry);
            if let Some((project, _)) = projects.find(entry.host.as_ref(), &path) {
                named.push(((f, e), project.id.clone()));
                found.push(((f, e), project.id.clone()));
                continue;
            }
            let key = key_of(entry, &path);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push((f, e)),
                None => groups.push((key, vec![(f, e)])),
            }
        }
    }

    let mut changed: BTreeSet<ProjectId> = BTreeSet::new();
    for ((f, e), id) in found {
        let Some(project) = projects.get_mut(&id) else {
            continue;
        };
        let before = project.clone();
        take_in(project, &files[f].1.projects[e]);
        if *project != before {
            changed.insert(id);
        }
    }
    let mut n = 0;
    for (key, members) in groups {
        let id = match owner_of(&projects, &key) {
            Some(id) => id,
            None => {
                let id = loop {
                    let id = ProjectId::mint(seed.wrapping_add(n));
                    n += 1;
                    if projects.get(&id).is_none() && !projects_dir.join(id.as_str()).exists() {
                        break id;
                    }
                };
                let home = home_of(&key, &members, &files);
                let host = files[members[0].0].1.projects[members[0].1].host.clone();
                projects.put(Project::new(id.clone(), host, home));
                id
            }
        };
        let Some(project) = projects.get_mut(&id) else {
            continue;
        };
        // The project folder's own entry first, so its facts win.
        let mut members = members;
        members.sort_by_key(|&(f, e)| canon(&files[f].1.projects[e]) != project.path);
        for &(f, e) in &members {
            take_in(project, &files[f].1.projects[e]);
            named.push(((f, e), id.clone()));
        }
        changed.insert(id);
    }

    // Projects first: a workspace file never names one not yet written.
    let mut failed: BTreeSet<ProjectId> = BTreeSet::new();
    for id in &changed {
        let Some(project) = projects.get(id) else {
            continue;
        };
        if let Err(why) = save_to(project, &projects_dir.join(id.as_str())) {
            problems.push(format!("project {}: {why}", project.path.display()));
            failed.insert(id.clone());
        }
    }
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    for ((f, e), id) in named {
        if failed.contains(&id) {
            continue;
        }
        let entry = &mut files[f].1.projects[e];
        entry.id = Some(id);
        entry.check = None;
        entry.offload = None;
        entry.builds = Vec::new();
        touched.insert(f);
    }
    for f in touched {
        let (dir, cfg) = &files[f];
        let before = dir.join(BEFORE);
        let copied = match before.exists() {
            true => Ok(()),
            false => fs::copy(dir.join(WorkspaceConfig::FILE), &before).map(|_| ()),
        };
        if let Err(why) = copied.and_then(|()| cfg.save_to(dir)) {
            problems.push(format!("{}: {why}", dir.display()));
        }
    }
    merge_issues(config, &projects, &mut problems);
    problems
}

/// Take into each project's issues file what an earlier build kept of them
/// per workspace and per folder: every workspace's file for each folder of
/// the project, its own first. The project's file records each one taken, so
/// a run cut short before the renames that follow never takes one twice;
/// each one taken is renamed `.json.merged` beside where it was, once the
/// project's file has landed. A project on a host keeps no issues here.
fn merge_issues(config: &Path, projects: &Projects, problems: &mut Vec<String>) {
    let workspaces = workspace_dirs(config);
    for project in projects.iter().filter(|project| project.host.is_none()) {
        let folders = std::iter::once(&project.path)
            .chain(project.worktrees.iter().map(|worktree| &worktree.path));
        let sources: Vec<PathBuf> = folders
            .flat_map(|folder| {
                let name = format!("{}.json", stem_for(folder));
                workspaces
                    .iter()
                    .map(move |ws| crate::issues::issues_dir(ws).join(&name))
            })
            .filter(|source| source.is_file())
            .collect();
        if sources.is_empty() {
            continue;
        }
        let file = projects_in(config)
            .join(project.id.as_str())
            .join("issues.json");
        let mut kept = match crate::issues::load_blocking(&file) {
            Ok(kept) => kept,
            Err(why) => {
                problems.push(why);
                continue;
            }
        };
        let mut taken = Vec::new();
        for source in sources {
            match crate::issues::load_blocking(&source) {
                Ok(read) => {
                    crate::issues::merge(&mut kept, read, &source);
                    taken.push(source);
                }
                Err(why) => problems.push(why),
            }
        }
        if let Err(why) = crate::issues::save_blocking(&file, &kept) {
            problems.push(why);
            continue;
        }
        for source in taken {
            let aside = source.with_extension("json.merged");
            if let Err(why) = move_into(&source, &aside) {
                problems.push(format!("{}: {why}", source.display()));
            }
        }
    }
}

/// Every workspace folder under `config`, sorted, a staging copy left out.
fn workspace_dirs(config: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(workspaces_in(config)) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| {
            dir.is_dir()
                && !dir
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    dirs.sort();
    dirs
}

/// Every workspace file under `config` that reads, sorted by its folder so a
/// run is the same every time; one that does not read is a problem, left as
/// it is.
fn workspace_files(config: &Path, problems: &mut Vec<String>) -> Vec<(PathBuf, WorkspaceConfig)> {
    let dirs = workspace_dirs(config);
    dirs.into_iter()
        .filter_map(|dir| match WorkspaceConfig::load_from(&dir) {
            WorkspaceLoad::Found(cfg) => Some((dir, cfg)),
            WorkspaceLoad::Missing => None,
            WorkspaceLoad::Unreadable(why) => {
                problems.push(format!("{}: {why}", dir.display()));
                None
            }
        })
        .collect()
}

/// `entry`'s folder as projects keep it: resolved when it is one here.
fn canon(entry: &ProjectConfig) -> PathBuf {
    match entry.host {
        Some(_) => entry.path.clone(),
        None => fs::canonicalize(&entry.path).unwrap_or_else(|_| entry.path.clone()),
    }
}

fn key_of(entry: &ProjectConfig, path: &Path) -> Key {
    if let Some(host) = &entry.host {
        return Key::Host(host.clone(), path.to_path_buf());
    }
    match repo_key_blocking(path) {
        Some((common, sub, _)) => Key::Repo(common, sub),
        None => Key::Folder(path.to_path_buf()),
    }
}

/// The project already in the store that `key` names: one on the same
/// repository at the same sub-folder. A folder or a host's folder a project
/// holds was found by its path already.
fn owner_of(projects: &Projects, key: &Key) -> Option<ProjectId> {
    let Key::Repo(common, sub) = key else {
        return None;
    };
    projects
        .iter()
        .filter(|project| project.host.is_none())
        .find(|project| {
            repo_key_blocking(&project.path).is_some_and(|(c, s, _)| c == *common && s == *sub)
        })
        .map(|project| project.id.clone())
}

/// The folder a new project of `key` is added from: the main checkout's,
/// whether a workspace held it or not, when it is on disk; else the first
/// entry met.
fn home_of(key: &Key, members: &[At], files: &[(PathBuf, WorkspaceConfig)]) -> PathBuf {
    let entry = |&(f, e): &At| &files[f].1.projects[e];
    let first = canon(entry(&members[0]));
    let Key::Repo(common, sub) = key else {
        return first;
    };
    if let Some(main) = members
        .iter()
        .map(|at| canon(entry(at)))
        .find(|path| repo_key_blocking(path).is_some_and(|(_, _, main)| main))
    {
        return main;
    }
    let main = common
        .file_name()
        .filter(|name| *name == ".git")
        .and(common.parent())
        .map(|top| top.join(sub))
        .filter(|path| path.is_dir())
        .and_then(|path| fs::canonicalize(path).ok());
    main.unwrap_or(first)
}

/// Take what `entry` says into `project`: its folder as a worktree when it is
/// not the project's own, and the facts the project does not have yet.
fn take_in(project: &mut Project, entry: &ProjectConfig) {
    let path = canon(entry);
    let local = entry.host.is_none();
    if path == project.path {
        if project.offload.is_none() && local {
            project.offload = entry.offload.clone();
        }
    } else if let Some(Folder::Worktree(at)) = project.folder(entry.host.as_ref(), &path) {
        if let Some(worktree) = project
            .worktrees
            .get_mut(at)
            .filter(|w| w.offload.is_none())
        {
            worktree.offload = entry.offload.clone().filter(|_| local);
        }
    } else if project.folder(entry.host.as_ref(), &path).is_none() {
        project.worktrees.push(Worktree {
            branch: current_branch_blocking(&path).unwrap_or_else(|_| "HEAD".into()),
            path,
            from: None,
            made_by: MadeBy::Added,
            offload: entry.offload.clone().filter(|_| local),
        });
    }
    if project.check.is_none() {
        project.check = entry.check.clone();
    }
    if project.builds.is_empty() {
        project.builds = entry.builds.clone();
    }
}

/// Move what an offload kept under `projects/<folder>/`, where an earlier
/// build put it, to `offload/<folder>/`, leaving `projects/` to projects. A
/// folder holding a project file is a project, and stays.
fn move_offload_data(config: &Path, problems: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(projects_in(config)) else {
        return;
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        if !dir.is_dir()
            || dir.join(FILE).exists()
            || !OFFLOAD.iter().any(|name| dir.join(name).exists())
        {
            continue;
        }
        let target = offload_in(config).join(dir.file_name().unwrap_or_default());
        for name in OFFLOAD {
            let (from, to) = (dir.join(name), target.join(name));
            if !from.exists() {
                continue;
            }
            if to.exists() {
                problems.push(format!(
                    "{} kept: {} is there",
                    from.display(),
                    to.display()
                ));
                continue;
            }
            let moved = fs::create_dir_all(&target).and_then(|()| move_into(&from, &to));
            if let Err(why) = moved {
                problems.push(format!("{}: {why}", from.display()));
            }
        }
        if fs::read_dir(&dir).is_ok_and(|mut left| left.next().is_none()) {
            let _ = fs::remove_dir(&dir);
        }
    }
}

#[cfg(test)]
mod tests;
