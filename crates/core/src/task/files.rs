//! The file each task keeps, unfinished or in the history:
//! `<config_dir>/onehand/tasks/<id>.json`.
//!
//! **Written in the order asked, by one thread.** A task per write lands in
//! any order, so an older snapshot could land over a newer one and bring back
//! a run as it was before it ended.

use super::Task;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// `<config_dir>/onehand/tasks/`.
pub fn dir() -> PathBuf {
    crate::config::config_dir().join("tasks")
}

/// What the writer's thread is handed: a task to save, or a mark to answer
/// once every save handed over before it has landed.
enum Job {
    Save(Box<Task>),
    Mark(mpsc::Sender<()>),
}

/// A thread that saves tasks into one folder one at a time, in the order
/// sent. A clone hands work to the same thread, which ends with the last
/// clone.
#[derive(Clone)]
pub struct Writer {
    tx: mpsc::Sender<Job>,
}

impl Writer {
    pub fn spawn(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("onehand-tasks".to_string())
            .spawn(move || {
                for job in rx {
                    match job {
                        Job::Save(task) => {
                            if let Err(why) = save_blocking(&dir, &task) {
                                eprintln!("onehand: could not write a task's file: {why}");
                            }
                        }
                        Job::Mark(landed) => {
                            let _ = landed.send(());
                        }
                    }
                }
            })
            .inspect_err(|why| eprintln!("onehand: no thread to write tasks: {why}"))
            .ok();
        Self { tx }
    }

    pub fn save(&self, task: Task) {
        if self.tx.send(Job::Save(Box::new(task))).is_err() {
            eprintln!("onehand: a task's file was not written: its writer is gone");
        }
    }

    /// Wait up to `within` for every save sent so far to land: what a process
    /// about to exit does last, or a run's last save dies with it. Whether
    /// they all landed. Blocking.
    pub fn flush(&self, within: std::time::Duration) -> bool {
        let (mark, landed) = mpsc::channel();
        self.tx.send(Job::Mark(mark)).is_ok() && landed.recv_timeout(within).is_ok()
    }
}

fn save_blocking(dir: &Path, task: &Task) -> Result<(), String> {
    let file = dir.join(format!("{}.json", task.id));
    let text = serde_json::to_string_pretty(task).map_err(|err| err.to_string())?;
    crate::config::write_atomic(&file, &text).map_err(|err| format!("{}: {err}", file.display()))
}

/// Every task file in `dir`, each read or why it could not be. Blocking.
pub fn load_all_blocking(dir: &Path) -> Vec<(PathBuf, Result<Task, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|x| x == "json"))
        .map(|path| {
            let read = std::fs::read_to_string(&path)
                .map_err(|err| err.to_string())
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()));
            (path, read)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Move the runs a build from before tasks kept in `pipeline-runs/` to
/// [`dir`], each as a task of one cut-off run, and say what was left behind.
/// Blocking.
pub fn migrate_old_dir_blocking() -> Vec<String> {
    migrate_blocking(&crate::config::config_dir().join("pipeline-runs"), &dir())
}

/// Move every run file from `old` to `new` as a task of the same id. A task
/// file already in `new` under that name wins as long as it reads: it may
/// have moved on since it was moved.
pub(crate) fn migrate_blocking(old: &Path, new: &Path) -> Vec<String> {
    crate::config::migrate_dir_blocking(
        old,
        new,
        "json",
        |text| {
            let run: crate::workflow::Run =
                serde_json::from_str(text).map_err(|err| err.to_string())?;
            let task = Task {
                id: run.id.clone(),
                brief: run.brief.clone(),
                setup: run.setup.clone(),
                runs: vec![run],
                dismissed: false,
            };
            serde_json::to_string_pretty(&task).map_err(|err| err.to_string())
        },
        |there, _| serde_json::from_str::<Task>(there).is_ok(),
    )
}
