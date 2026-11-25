//! Room at a limited step: how many tasks of a project may be at one step of
//! one workflow at once, and the rest waiting before it in the order they
//! arrived. Kept in memory only: a run cut off by a restart asks again when
//! it is resumed.

use crate::workflow::Template;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What room is counted by: runs of the same workflow, in the same
/// repository, at the same step.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoomKey {
    pub repo: PathBuf,
    pub workflow: String,
    pub step: String,
}

impl RoomKey {
    /// The room of `template`'s step at `at`, for runs in `repo`; none
    /// past its steps.
    pub(crate) fn at(repo: &Path, template: &Template, at: usize) -> Option<Self> {
        Some(Self {
            repo: repo.to_path_buf(),
            workflow: template.id.clone(),
            step: template.steps.get(at)?.id.clone(),
        })
    }
}

/// The room at a limited step: its limit, how many tasks hold room there
/// and how many wait for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    pub limit: u32,
    pub holding: usize,
    pub waiting: usize,
}

/// Which task holds room where, and which wait for some.
#[derive(Debug, Default)]
pub(crate) struct Rooms {
    held: HashMap<String, RoomKey>,
    /// Oldest first, each with the limit its run asked under.
    waiting: Vec<(String, RoomKey, u32)>,
}

impl Rooms {
    /// `task` asks for room at `key`, where `limit` may hold it: where it
    /// gave room up, and the tasks given room now, itself among them when
    /// there was room and nobody waiting ahead. One already waiting keeps
    /// its place; one asking for another step gives up the room it held.
    pub(crate) fn ask(
        &mut self,
        task: &str,
        key: RoomKey,
        limit: u32,
    ) -> (Option<RoomKey>, Vec<String>) {
        if self.waiting.iter().any(|(waiting, _, _)| waiting == task) {
            return (None, Vec::new());
        }
        let left = self.held.remove(task);
        self.waiting.push((task.to_string(), key, limit));
        (left, self.grant())
    }

    /// `task` holds room nowhere and waits for none: where it held room, if
    /// it did, and the tasks given the room that freed.
    pub(crate) fn leave(&mut self, task: &str) -> (Option<RoomKey>, Vec<String>) {
        let held = self.held.remove(task);
        let before = self.waiting.len();
        self.waiting.retain(|(waiting, _, _)| waiting != task);
        let given = match held.is_some() || self.waiting.len() != before {
            true => self.grant(),
            false => Vec::new(),
        };
        (held, given)
    }

    /// The step `task` waits for room at, while it waits.
    pub(crate) fn waits(&self, task: &str) -> Option<&RoomKey> {
        self.waiting
            .iter()
            .find(|(waiting, _, _)| waiting == task)
            .map(|(_, key, _)| key)
    }

    /// How many tasks hold room at `key`, and how many wait for it.
    pub(crate) fn at(&self, key: &RoomKey) -> (usize, usize) {
        let waiting = self.waiting.iter().filter(|(_, at, _)| at == key).count();
        (self.holding(key), waiting)
    }

    fn holding(&self, key: &RoomKey) -> usize {
        self.held.values().filter(|at| *at == key).count()
    }

    /// Give room to whoever waits first at each step while it has some; one
    /// that finds none holds back everyone behind it at that step.
    fn grant(&mut self) -> Vec<String> {
        let mut given = Vec::new();
        let mut full = HashSet::new();
        let mut i = 0;
        while i < self.waiting.len() {
            let (_, key, limit) = &self.waiting[i];
            if full.contains(key) || self.holding(key) >= *limit as usize {
                full.insert(key.clone());
                i += 1;
                continue;
            }
            let (task, key, _) = self.waiting.remove(i);
            self.held.insert(task.clone(), key);
            given.push(task);
        }
        given
    }
}
