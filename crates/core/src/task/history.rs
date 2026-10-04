//! How much history is kept: the newest finished tasks of each project, so
//! the task files and the marks they pin do not grow without bound.

use super::{Group, Task};
use std::collections::HashMap;
use std::path::Path;

/// How many finished tasks each project keeps.
pub(crate) const KEPT: usize = 200;

/// The ids of the finished tasks past the newest [`KEPT`] of the project
/// each was started from. A task a person should still look at is never
/// one of them.
pub fn over_cap(tasks: &[(&Task, Group)]) -> Vec<String> {
    let mut by_repo: HashMap<&Path, Vec<&Task>> = HashMap::new();
    for (task, group) in tasks {
        if *group == Group::Finished {
            by_repo.entry(&task.setup.repo).or_default().push(task);
        }
    }
    by_repo
        .into_values()
        .flat_map(|mut finished| {
            finished.sort_by_key(|task| std::cmp::Reverse(task.recency()));
            finished.into_iter().skip(KEPT).map(|task| task.id.clone())
        })
        .collect()
}
