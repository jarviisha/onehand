//! Bringing in what earlier builds kept elsewhere, once, at launch.
//!
//! An earlier build kept each workspace in a folder the person bound it to,
//! named those folders in `state.toml`, and kept a project's offload beside
//! the workspace made from it. This build keeps every workspace under
//! `<config_dir>/workspaces/` and what belongs to a project under
//! `<config_dir>/projects/`. [`migrate_in`] gets from the one to the other.
//!
//! **Nothing is lost on the way.** A workspace folder elsewhere is copied,
//! never moved, and the copy lands whole or not at all: it is made in a
//! staging folder beside its target and renamed into place, so a target that
//! exists is always a whole copy and a crash leaves only the staging folder,
//! which the next run clears and starts again. The old `state.toml` fields
//! are kept until every copy they named has landed. Offload data is renamed,
//! never copied: it can hold a whole checkout.
//!
//! **Safe to run at every launch**, and it is: each step passes over what is
//! already in place, so a second run changes nothing.

use super::stem_for;
use crate::config::{move_into, write_atomic, AppState, WorkspaceConfig, WorkspaceLoad};
use crate::issues::issues_dir;
use std::fs;
use std::path::{Path, PathBuf};

/// How many launches in a row a folder an earlier build named may be missing
/// before it is given up on: enough for a disk left unplugged for a while.
pub(crate) const MISSING_LAUNCHES: u32 = 20;

/// The file a copy carries naming the folder it was copied from, so a later
/// run knows that folder is already in.
pub(super) const FROM: &str = "migrated-from";

/// What a project's offload keeps, in what was its workspace's folder.
const OFFLOAD: [&str; 3] = ["offload.json", "coming-back", "backups"];

/// Bring in what earlier builds kept, under onehand's own root. Blocking;
/// answers the problems met, for the log.
pub fn migrate_storage_blocking() -> Vec<String> {
    migrate_in(&crate::config::config_dir())
}

/// [`migrate_storage_blocking`] for the root `config`.
pub(crate) fn migrate_in(config: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let workspaces = crate::config::workspaces_in(config);
    clear_staging(&workspaces, &mut problems);
    let state_path = config.join("state.toml");
    let mut state = AppState::load_from(&state_path);
    let mut old: Vec<PathBuf> = state.workspace_dir.iter().cloned().collect();
    for dir in &state.recent_workspaces {
        if !old.contains(dir) {
            old.push(dir.clone());
        }
    }
    // The workspace dir mirrored the first recent; the recents' order wins.
    old.sort_by_key(|dir| state.recent_workspaces.iter().position(|d| d == dir));

    // Each folder leaves the old fields once it is in, on its own: one that
    // failed keeps only itself named, so a copy deleted since is never
    // brought back by the retry of another.
    let mut ids = Vec::new();
    let mut kept = Vec::new();
    for dir in &old {
        match bring_in(dir, &workspaces, state.names.get(dir), config) {
            Brought::In { id, left } => {
                ids.push(id);
                if !left.is_empty() {
                    kept.push(dir.clone());
                    problems.extend(left);
                }
            }
            Brought::Nothing => {}
            Brought::Missing => {
                let launches = state.missing.entry(dir.clone()).or_default();
                *launches += 1;
                match *launches >= MISSING_LAUNCHES {
                    true => problems.push(format!(
                        "given up on {}: not there for {MISSING_LAUNCHES} launches",
                        dir.display()
                    )),
                    false => {
                        kept.push(dir.clone());
                        problems.push(format!("{}: not there at this launch", dir.display()));
                    }
                }
            }
            Brought::Later(why) => {
                kept.push(dir.clone());
                problems.push(format!("{}: {why}", dir.display()));
            }
        }
    }
    move_offloads(
        &workspaces,
        &crate::config::projects_in(config),
        &mut problems,
    );

    if !old.is_empty() {
        // The first time, the old order is the order. A folder brought in
        // on a later launch goes after the workspaces opened since: it is
        // not the one the person was last in.
        let mut order = std::mem::take(&mut state.workspaces);
        match order.is_empty() {
            true => order = ids,
            false => order.extend(
                ids.into_iter()
                    .filter(|id| !order.contains(id))
                    .collect::<Vec<_>>(),
            ),
        }
        state.workspaces = order;
        state.missing.retain(|dir, _| kept.contains(dir));
        state.workspace_dir = state.workspace_dir.take().filter(|dir| kept.contains(dir));
        state.recent_workspaces.retain(|dir| kept.contains(dir));
        state.names.retain(|dir, _| kept.contains(dir));
        if let Err(why) = state.save_to(&state_path) {
            problems.push(format!("{}: {why}", state_path.display()));
        }
    }
    problems
}

/// Remove what a copy cut short left behind.
fn clear_staging(workspaces: &Path, problems: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(workspaces) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') && name.ends_with(".migrating") {
            if let Err(why) = fs::remove_dir_all(&path) {
                problems.push(format!("{}: {why}", path.display()));
            }
        }
    }
}

/// What became of one folder an earlier build named.
enum Brought {
    /// It is the workspace `id` under `workspaces`; `left` says what of its
    /// tasks could not be repointed, to be tried again at the next launch.
    In { id: String, left: Vec<String> },
    /// It is there and holds no workspace: nothing to bring in, ever.
    Nothing,
    /// It is not there at this launch: a disk not mounted, or deleted.
    Missing,
    /// It could not be brought in now, for this: unreadable, or a copy that
    /// failed. Tried at the next launch.
    Later(String),
}

/// The workspace an earlier build kept in `dir`, as an id under
/// `workspaces`: copied in when it lives elsewhere, as it is when it is
/// already there.
fn bring_in(dir: &Path, workspaces: &Path, name: Option<&String>, config: &Path) -> Brought {
    if dir.parent() == Some(workspaces) {
        let id = dir.file_name().unwrap_or_default().to_string_lossy();
        return match dir.is_dir() {
            true => Brought::In {
                id: id.into_owned(),
                left: Vec::new(),
            },
            false => Brought::Nothing,
        };
    }
    if !dir.exists() {
        return Brought::Missing;
    }
    // Copied by an earlier run: its tasks again, since that run may have
    // stopped between the copy and them.
    if let Some(id) = copied_from(workspaces, dir) {
        let left = follow_tasks(&issues_dir(dir), &issues_dir(&workspaces.join(&id)), config);
        return Brought::In { id, left };
    }
    // The folder named for `dir` can be taken: a workspace once made from
    // the old storage folder itself was kept there. The copy goes beside it.
    let stem = stem_for(dir);
    let id = std::iter::once(stem.clone())
        .chain((2..).map(|n| format!("{stem}-{n}")))
        .find(|id| !workspaces.join(id).exists())
        .unwrap_or(stem);
    let target = workspaces.join(&id);
    let mut cfg = match WorkspaceConfig::load_from(dir) {
        WorkspaceLoad::Found(cfg) => cfg,
        WorkspaceLoad::Missing => return Brought::Nothing,
        WorkspaceLoad::Unreadable(why) => return Brought::Later(why),
    };
    if cfg.name.is_empty() {
        if let Some(name) = name {
            cfg.name = name.clone();
        }
    }
    let staging = workspaces.join(format!(".{id}.migrating"));
    let copied = (|| -> std::io::Result<()> {
        fs::create_dir_all(&staging)?;
        cfg.save_to(&staging)?;
        fs::write(staging.join(FROM), dir.as_os_str().as_encoded_bytes())?;
        let issues = issues_dir(dir);
        if issues.is_dir() {
            copy_dir(&issues, &issues_dir(&staging))?;
        }
        move_into(&staging, &target)
    })();
    if let Err(why) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Brought::Later(why.to_string());
    }
    let left = follow_tasks(&issues_dir(dir), &issues_dir(&target), config);
    Brought::In { id, left }
}

/// The id of the copy already made of `dir`, if one was.
fn copied_from(workspaces: &Path, dir: &Path) -> Option<String> {
    let want = dir.as_os_str().as_encoded_bytes();
    fs::read_dir(workspaces)
        .ok()?
        .flatten()
        .find(|entry| fs::read(entry.path().join(FROM)).is_ok_and(|from| from == want))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

/// Copy the files of `from` into `to`, folders within it too. An entry that
/// cannot be read fails the copy rather than leaving it short; a link is
/// passed over, since following one can loop for ever.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let into = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &into)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &into)?;
        }
    }
    Ok(())
}

/// Point every task that reports into an issue file under `old` at the same
/// file under `new`, so it reports into the copy the app shows; what could
/// not be written, each task file on its own, the rest still done.
fn follow_tasks(old: &Path, new: &Path, config: &Path) -> Vec<String> {
    let mut left = Vec::new();
    let Ok(entries) = fs::read_dir(config.join("tasks")) else {
        return left;
    };
    let (old, new) = (format!("{}/", old.display()), format!("{}/", new.display()));
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(mut task) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if repoint(&mut task, &old, &new) {
            let written = serde_json::to_string_pretty(&task)
                .map_err(|e| e.to_string())
                .and_then(|text| write_atomic(&path, &text).map_err(|e| e.to_string()));
            if let Err(why) = written {
                left.push(format!("{}: {why}", path.display()));
            }
        }
    }
    left
}

/// Every string in `value` that starts with `old`, made to start with `new`;
/// whether any did.
fn repoint(value: &mut serde_json::Value, old: &str, new: &str) -> bool {
    match value {
        serde_json::Value::String(text) => match text.strip_prefix(old) {
            Some(rest) => {
                *text = format!("{new}{rest}");
                true
            }
            None => false,
        },
        serde_json::Value::Array(items) => items
            .iter_mut()
            .fold(false, |any, item| repoint(item, old, new) | any),
        serde_json::Value::Object(fields) => fields
            .values_mut()
            .fold(false, |any, item| repoint(item, old, new) | any),
        _ => false,
    }
}

/// Rename what a project's offload kept in a workspace folder into the
/// project's own folder of the same name, and drop a workspace folder left
/// with nothing in it.
fn move_offloads(workspaces: &Path, projects: &Path, problems: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(workspaces) else {
        return;
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        if !dir.is_dir() || !OFFLOAD.iter().any(|name| dir.join(name).exists()) {
            continue;
        }
        let project = projects.join(dir.file_name().unwrap_or_default());
        for name in OFFLOAD {
            let (from, to) = (dir.join(name), project.join(name));
            if !from.exists() || to.exists() {
                continue;
            }
            let moved = fs::create_dir_all(&project).and_then(|()| move_into(&from, &to));
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
