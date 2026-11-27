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
            BoardLoad::Missing => {}
            BoardLoad::Unreadable(why) => {
                unreadable.insert(id, why);
            }
        }
    }
    (boards, unreadable)
}

/// The project a folder of the store is, by its name.
fn folder_id(dir: &Path) -> Option<ProjectId> {
    let name = dir.is_dir().then(|| dir.file_name())??.to_str()?;
    Some(ProjectId::of_folder(name))
}

#[cfg(test)]
mod tests;
