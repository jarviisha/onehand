//! `project.toml`: one project's own file, in a folder named by its id.

use super::{Project, Projects};
use crate::config::write_atomic;
use std::path::Path;

/// The file a project is kept in, inside its folder.
pub const FILE: &str = "project.toml";

/// What reading a project's folder found.
#[derive(Debug)]
pub enum ProjectLoad {
    Found(Box<Project>),
    /// No file there: a folder holding something else, or none at all.
    Missing,
    /// A file there that cannot be read, for this. It is never written over:
    /// it is somebody's project in a shape this build does not understand.
    Unreadable(String),
}

/// The project kept in `dir`.
pub fn load_from(dir: &Path) -> ProjectLoad {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ProjectLoad::Missing,
        Err(e) => return ProjectLoad::Unreadable(format!("{}: {e}", path.display())),
    };
    match toml::from_str(&text) {
        Ok(project) => ProjectLoad::Found(Box::new(project)),
        Err(e) => ProjectLoad::Unreadable(format!("{}: {e}", path.display())),
    }
}

/// Write `project` into `dir`, whole, creating the folder if need be.
/// Blocking.
pub fn save_to(project: &Project, dir: &Path) -> std::io::Result<()> {
    let text = toml::to_string_pretty(project).map_err(std::io::Error::other)?;
    write_atomic(&dir.join(FILE), &text)
}

/// Every project kept under `projects_dir`, and a problem for each file that
/// could not be read. Blocking.
pub fn load_all_blocking(projects_dir: &Path) -> (Projects, Vec<String>) {
    let mut projects = Projects::default();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir) else {
        return (projects, problems);
    };
    for dir in entries.flatten().map(|entry| entry.path()) {
        if !dir.is_dir() {
            continue;
        }
        match load_from(&dir) {
            ProjectLoad::Found(project) => projects.put(*project),
            ProjectLoad::Missing => {}
            ProjectLoad::Unreadable(why) => problems.push(why),
        }
    }
    (projects, problems)
}
