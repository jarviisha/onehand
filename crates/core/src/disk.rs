//! Where a project's files are: this machine's disk, or a host's reached over
//! its attach. The file tree, the editor and the git badges read through this
//! one seam, so none of them asks which it is.

use std::path::Path;
use std::time::SystemTime;

use crate::editor::{self, SaveOutcome};
use crate::gitstat::{self, GitStatus};
use crate::host::client::HostConn;
use crate::tree::{self, TreeEntry};

#[derive(Clone)]
pub enum Disk {
    Local,
    Host(HostDisk),
}

/// A host's connection, with the tokio runtime its calls are polled on.
#[derive(Clone)]
pub struct HostDisk {
    conn: HostConn,
    rt: tokio::runtime::Handle,
}

impl HostDisk {
    pub fn new(conn: HostConn, rt: tokio::runtime::Handle) -> Self {
        Self { conn, rt }
    }
}

impl Disk {
    /// `dir`'s listing, sorted, and how many entries its bound left out.
    ///
    /// Blocking: never call it on a UI loop or a tokio worker.
    pub fn read_dir(&self, dir: &Path) -> Result<(Vec<TreeEntry>, usize), String> {
        match self {
            Disk::Local => Ok(tree::read_dir_sorted(dir)),
            Disk::Host(h) => h
                .rt
                .block_on(h.conn.read_dir(dir))
                .map_err(|e| format!("Could not read {} on {}: {e}", dir.display(), h.conn.host())),
        }
    }

    /// A file for the editor, and its modification time.
    ///
    /// Blocking: never call it on a UI loop or a tokio worker.
    pub fn read_file(&self, path: &Path) -> Result<(String, Option<SystemTime>), String> {
        match self {
            Disk::Local => editor::read_blocking(path),
            Disk::Host(h) => {
                h.rt.block_on(h.conn.read_file(path))
                    .map_err(|e| format!("On {}: {e}", h.conn.host()))
            }
        }
    }

    /// Write `text` unless the file changed since `expected`.
    ///
    /// Blocking: never call it on a UI loop or a tokio worker.
    pub fn save_file(
        &self,
        path: &Path,
        text: String,
        expected: Option<SystemTime>,
    ) -> SaveOutcome {
        match self {
            Disk::Local => editor::save_blocking(path, text, expected),
            Disk::Host(h) => {
                h.rt.block_on(h.conn.save_file(path, text, expected))
                    .unwrap_or_else(|e| SaveOutcome::Failed(format!("On {}: {e}", h.conn.host())))
            }
        }
    }

    /// The repository status of `root`; None when it is not in one, or a
    /// host could not say.
    ///
    /// Blocking: never call it on a UI loop or a tokio worker.
    pub fn git_status(&self, root: &Path) -> Option<GitStatus> {
        match self {
            Disk::Local => gitstat::read_blocking(root),
            Disk::Host(h) => h.rt.block_on(h.conn.git_status(root)).ok().flatten(),
        }
    }

    /// Whether this is a host's disk over an attach that has ended, so
    /// nothing read through it can land.
    pub fn is_closed(&self) -> bool {
        match self {
            Disk::Local => false,
            Disk::Host(h) => h.conn.is_closed(),
        }
    }

    /// The host's name, or None for this machine.
    pub fn host(&self) -> Option<&str> {
        match self {
            Disk::Local => None,
            Disk::Host(h) => Some(h.conn.host()),
        }
    }
}

#[cfg(test)]
mod tests;
