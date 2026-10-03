//! The file each pipeline run keeps until it ends, so a restart can offer to
//! resume it: `<config_dir>/onehand/pipeline-runs/<id>.json`.
//!
//! **Written in the order asked, by one thread.** A task per write lands in
//! any order, so an older snapshot could land over a newer one, or a save
//! land after the run's file was removed and bring a finished run back at
//! the next launch.

use super::run::PipelineRun;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// `<config_dir>/onehand/pipeline-runs/`.
pub fn runs_dir() -> PathBuf {
    crate::config::config_dir().join("pipeline-runs")
}

/// An id no other run has.
pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    nanos.to_string()
}

/// The file in `dir` the run `id` keeps.
pub fn run_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// One write for the writer.
pub enum FileOp {
    Save(PathBuf, Box<PipelineRun>),
    Remove(PathBuf),
}

/// What the writer's thread is handed: a write, or a mark to answer once
/// every write handed over before it has landed.
enum Job {
    Write(FileOp),
    Mark(mpsc::Sender<()>),
}

/// A thread that carries out [`FileOp`]s one at a time, in the order sent.
/// A clone hands work to the same thread, which ends with the last clone.
#[derive(Clone)]
pub struct Writer {
    tx: mpsc::Sender<Job>,
}

impl Writer {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("onehand-pipeline-runs".to_string())
            .spawn(move || {
                for job in rx {
                    match job {
                        Job::Write(op) => {
                            if let Err(why) = carry_out(&op) {
                                eprintln!("onehand: could not write a pipeline run's file: {why}");
                            }
                        }
                        Job::Mark(landed) => {
                            let _ = landed.send(());
                        }
                    }
                }
            })
            .inspect_err(|why| eprintln!("onehand: no thread to write pipeline runs: {why}"))
            .ok();
        Self { tx }
    }

    pub fn send(&self, op: FileOp) {
        if self.tx.send(Job::Write(op)).is_err() {
            eprintln!("onehand: a pipeline run's file was not written: its writer is gone");
        }
    }

    /// Wait up to `within` for every write sent so far to land: what a
    /// process about to exit does last, or a run's last save and its file's
    /// removal die with it. Whether they all landed. Blocking.
    pub fn flush(&self, within: std::time::Duration) -> bool {
        let (mark, landed) = mpsc::channel();
        self.tx.send(Job::Mark(mark)).is_ok() && landed.recv_timeout(within).is_ok()
    }
}

fn carry_out(op: &FileOp) -> Result<(), String> {
    match op {
        FileOp::Save(file, run) => {
            let text = serde_json::to_string_pretty(run).map_err(|err| err.to_string())?;
            crate::config::write_atomic(file, &text)
                .map_err(|err| format!("{}: {err}", file.display()))
        }
        FileOp::Remove(file) => match std::fs::remove_file(file) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {err}", file.display()))
            }
            _ => Ok(()),
        },
    }
}

/// Every run file in `dir`, each read or why it could not be. Blocking.
pub fn load_all_blocking(dir: &Path) -> Vec<(PathBuf, Result<PipelineRun, String>)> {
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
