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

/// A thread that carries out [`FileOp`]s one at a time, in the order sent.
pub struct Writer {
    tx: Option<mpsc::Sender<FileOp>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<FileOp>();
        let thread = std::thread::Builder::new()
            .name("onehand-pipeline-runs".to_string())
            .spawn(move || {
                for op in rx {
                    if let Err(why) = carry_out(&op) {
                        eprintln!("onehand: could not write a pipeline run's file: {why}");
                    }
                }
            })
            .inspect_err(|why| eprintln!("onehand: no thread to write pipeline runs: {why}"))
            .ok();
        Self {
            tx: Some(tx),
            thread,
        }
    }

    pub fn send(&self, op: FileOp) {
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(op).is_ok());
        if !sent {
            eprintln!("onehand: a pipeline run's file was not written: its writer is gone");
        }
    }

    /// Wait for every write sent so far to land.
    pub fn finish(mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
