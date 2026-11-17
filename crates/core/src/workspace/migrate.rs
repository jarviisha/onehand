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
use std::fs;
use std::path::{Path, PathBuf};

/// The file a copy carries naming the folder it was copied from, so a later
/// run knows that folder is already in.
const FROM: &str = "migrated-from";

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
    let workspaces = config.join("workspaces");
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

    let mut ids = Vec::new();
    let mut landed = true;
    for dir in &old {
        match bring_in(dir, &workspaces, state.names.get(dir), config) {
            Ok(Some(id)) => ids.push(id),
            Ok(None) => {}
            Err(why) => {
                landed = false;
                problems.push(format!("{}: {why}", dir.display()));
            }
        }
    }
    move_offloads(&workspaces, &config.join("projects"), &mut problems);

    if !old.is_empty() {
        for id in std::mem::take(&mut state.workspaces) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        state.workspaces = ids;
        if landed {
            state.workspace_dir = None;
            state.recent_workspaces.clear();
            state.names.clear();
        }
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

/// The workspace an earlier build kept in `dir`, as an id under
/// `workspaces`: copied in when it lives elsewhere, as it is when it is
/// already there, none when `dir` holds no workspace that loads.
fn bring_in(
    dir: &Path,
    workspaces: &Path,
    name: Option<&String>,
    config: &Path,
) -> Result<Option<String>, String> {
    if dir.parent() == Some(workspaces) {
        let id = dir.file_name().unwrap_or_default().to_string_lossy();
        return Ok(dir.is_dir().then(|| id.into_owned()));
    }
    if let Some(id) = copied_from(workspaces, dir) {
        return Ok(Some(id));
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
        WorkspaceLoad::Missing => return Ok(None),
        WorkspaceLoad::Unreadable(why) => return Err(why),
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
        let issues = dir.join("issues");
        if issues.is_dir() {
            copy_dir(&issues, &staging.join("issues"))?;
        }
        move_into(&staging, &target)
    })();
    if let Err(why) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(why.to_string());
    }
    follow_tasks(&dir.join("issues"), &target.join("issues"), config)?;
    Ok(Some(id))
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

/// Copy the files of `from` into `to`, folders within it too.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let into = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &into)?;
        } else {
            fs::copy(&path, &into)?;
        }
    }
    Ok(())
}

/// Point every task that reports into an issue file under `old` at the same
/// file under `new`, so it reports into the copy the app shows.
fn follow_tasks(old: &Path, new: &Path, config: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(config.join("tasks")) else {
        return Ok(());
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
            let text = serde_json::to_string_pretty(&task).map_err(|e| e.to_string())?;
            write_atomic(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
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
