//! Who may work where: one task at a time in each place, the rest waiting
//! in the order they asked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The place tasks wait for, by the checkout git sees: two folders of one
/// checkout, or one reached through a symlink, are one place. A folder
/// outside git is its own. Blocking.
pub fn place_blocking(dir: &Path) -> PathBuf {
    crate::worktree::repo_top_blocking(dir)
        .or_else(|| std::fs::canonicalize(dir).ok())
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Which task holds each place, and which wait for one.
#[derive(Debug, Default)]
pub struct Queue {
    held: HashMap<PathBuf, String>,
    waiting: Vec<(PathBuf, String)>,
}

impl Queue {
    /// `task` asks for `place`: whether it has it now. One that does not
    /// waits, first in, first out.
    pub fn ask(&mut self, place: PathBuf, task: String) -> bool {
        if self.held.get(&place).is_some_and(|holder| *holder != task) {
            self.waiting.push((place, task));
            return false;
        }
        self.held.insert(place, task);
        true
    }

    /// `task` has stopped working: its place goes straight to the first task
    /// waiting for it, which is handed back to be started.
    pub fn release(&mut self, task: &str) -> Option<String> {
        let place = self
            .held
            .iter()
            .find(|(_, holder)| *holder == task)
            .map(|(place, _)| place.clone())?;
        self.held.remove(&place);
        let next = self.waiting.iter().position(|(at, _)| *at == place)?;
        let (place, next) = self.waiting.remove(next);
        self.held.insert(place, next.clone());
        Some(next)
    }

    /// `task` no longer waits: whether it was waiting.
    pub fn call_off(&mut self, task: &str) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|(_, waiting)| waiting != task);
        self.waiting.len() != before
    }

    /// Whether `task` waits for a place.
    pub fn queued(&self, task: &str) -> bool {
        self.waiting.iter().any(|(_, waiting)| waiting == task)
    }

    /// The task holding the place `task` waits for, while it waits.
    pub fn holder_of(&self, task: &str) -> Option<&str> {
        let (place, _) = self.waiting.iter().find(|(_, waiting)| waiting == task)?;
        self.held.get(place).map(String::as_str)
    }

    /// Whether `task` holds a place.
    pub fn holds(&self, task: &str) -> bool {
        self.held.values().any(|holder| holder == task)
    }
}

/// What a task just queued for its place is told: the task holding the
/// place, by its title, so a person knows which one to finish or stop, and
/// the place by its folder's name.
pub fn queued_said(title: &str, holder: Option<&str>, place: &str) -> String {
    match holder {
        Some(holder) => format!("{title} is queued behind {holder}, working in {place}"),
        None => format!("{title} is queued behind the task working in {place}"),
    }
}
