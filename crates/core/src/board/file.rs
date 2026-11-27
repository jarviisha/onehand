//! `board.toml`: a project's board, in its project's folder beside
//! `project.toml`, written whole.

use super::model::Board;
use crate::config::write_atomic;
use crate::project::ProjectId;
use std::collections::HashMap;
use std::path::Path;

/// The file a board is kept in, inside its project's folder.
pub const FILE: &str = "board.toml";

/// What reading a project's folder found of its board.
#[derive(Debug)]
pub enum BoardLoad {
    Found(Board),
    /// No board there.
    Missing,
    /// A file there that cannot be read, for this. It is never written over:
    /// a newer onehand may have written it.
    Unreadable(String),
}

/// The board kept in `dir`.
pub fn load_from(dir: &Path) -> BoardLoad {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return BoardLoad::Missing,
        Err(e) => return BoardLoad::Unreadable(format!("{}: {e}", path.display())),
    };
    let board = toml::from_str::<Board>(&text)
        .map_err(|e| e.to_string())
        .and_then(|board| board.shape().map(|()| board));
    match board {
        Ok(board) => BoardLoad::Found(board),
        Err(why) => BoardLoad::Unreadable(format!("{}: {why}", path.display())),
    }
}

/// Write `board` into `dir` whole, or remove it with none. Blocking.
pub fn save_blocking(dir: &Path, board: Option<&Board>) -> std::io::Result<()> {
    let path = dir.join(FILE);
    match board {
        Some(board) => {
            let text = toml::to_string_pretty(board).map_err(std::io::Error::other)?;
            write_atomic(&path, &text)
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            done => done,
        },
    }
}

/// Every board kept under `projects_dir`, by the project whose folder holds
/// it, and why each one that cannot be read cannot. Blocking.
pub fn load_all_blocking(
    projects_dir: &Path,
) -> (HashMap<ProjectId, Board>, HashMap<ProjectId, String>) {
    let mut boards = HashMap::new();
    let mut unreadable = HashMap::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return (boards, unreadable);
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        let Some(id) = folder_id(&dir) else {
            continue;
        };
        match load_from(&dir) {
            BoardLoad::Found(board) => {
                boards.insert(id, board);
            }
            BoardLoad::Missing => {
                if let Some(why) = left_in_project(&dir) {
                    unreadable.insert(id, why);
                }
            }
            BoardLoad::Unreadable(why) => {
                unreadable.insert(id, why);
            }
        }
    }
    (boards, unreadable)
}

/// Move each board still kept in its `project.toml`, the way an earlier
/// build kept it, into a file of its own under `projects_dir`, and answer
/// what could not be moved. Blocking; run at start, before anything reads or
/// writes a project.
///
/// **The key goes only once the file is written**: a project whose board
/// could not be written keeps the key, which every later write of the
/// project keeps too, and the next start tries again. A board file already
/// there wins over the key, which is then dropped; one that cannot be read
/// is left as it is, with the key.
pub fn migrate_blocking(projects_dir: &Path) -> Vec<String> {
    use crate::project::file::{load_from as load_project, save_to, ProjectLoad};
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return problems;
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        let mut project = match load_project(&dir) {
            ProjectLoad::Found(project) => *project,
            ProjectLoad::Missing | ProjectLoad::Unreadable(_) => continue,
        };
        let Some(old) = project.old_board.clone() else {
            continue;
        };
        match load_from(&dir) {
            BoardLoad::Found(_) => {}
            BoardLoad::Unreadable(why) => {
                problems.push(format!("the board was not moved out of its project: {why}"));
                continue;
            }
            BoardLoad::Missing => {
                let since = modified_secs(&dir.join(crate::project::file::FILE));
                let board = Board::made(old.workflow, old.team, &old.todo, since).unadopted();
                if let Err(why) = save_blocking(&dir, Some(&board)) {
                    let at = dir.join(FILE);
                    problems.push(format!("{}: the board was not moved: {why}", at.display()));
                    continue;
                }
            }
        }
        project.old_board = None;
        if let Err(why) = save_to(&project, &dir) {
            let at = dir.join(crate::project::file::FILE);
            problems.push(format!(
                "{}: its board key was not dropped: {why}",
                at.display()
            ));
        }
    }
    problems
}

/// Why the board of the project in `dir` is not read, when its move out of
/// `project.toml` failed and the old key still holds it: taken for none, a
/// board bound anew would start an empty To do the next start's move then
/// puts over the old one.
fn left_in_project(dir: &Path) -> Option<String> {
    use crate::project::file::{load_from as load_project, ProjectLoad};
    match load_project(dir) {
        ProjectLoad::Found(project) => project.old_board.as_ref().map(|_| {
            "it is still kept in its project, where an earlier build left it, and could not \
             be moved; onehand tries again at the next start"
                .to_string()
        }),
        ProjectLoad::Missing | ProjectLoad::Unreadable(_) => None,
    }
}

/// When `path` was last written, in seconds; 0 when that cannot be read.
fn modified_secs(path: &Path) -> u64 {
    (std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok())
    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
    .map_or(0, |since| since.as_secs())
}

/// The project a folder of the store is, by its name.
fn folder_id(dir: &Path) -> Option<ProjectId> {
    let name = dir.is_dir().then(|| dir.file_name())??.to_str()?;
    Some(ProjectId::of_folder(name))
}

#[cfg(test)]
mod tests;
