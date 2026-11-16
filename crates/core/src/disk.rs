//! Where a project's files are: this machine's disk, or a host's reached over
//! its attach. The file tree, the editor and the git badges read through this
//! one seam, so none of them asks which it is.

use futures::executor::block_on;
use std::path::Path;
use std::time::SystemTime;

use crate::editor::{self, SaveOutcome};
use crate::gitstat::{self, GitStatus};
use crate::host::client::HostConn;
use crate::tree::{self, TreeEntry};

#[derive(Clone)]
pub enum Disk {
    Local,
    /// A host's, over its connection. Its calls are awaited on whatever
    /// thread asks: the connection's own task keeps their waits.
    Host(HostConn),
}

/// `dir`'s listing on the host reached over `conn`, and how many entries its
/// bound left out, said as a person reads it when it fails: what the tree
/// and the host folder picker both read.
pub async fn read_host_dir(conn: &HostConn, dir: &Path) -> Result<(Vec<TreeEntry>, usize), String> {
    conn.read_dir(dir)
        .await
        .map_err(|e| format!("Could not read {} on {}: {e}", dir.display(), conn.host()))
}

impl Disk {
    /// `dir`'s listing, sorted, and how many entries its bound left out.
    ///
    /// Blocking: never call it on a UI loop, nor on the runtime a host's
    /// connection runs on, whose task it waits for.
    pub fn read_dir(&self, dir: &Path) -> Result<(Vec<TreeEntry>, usize), String> {
        match self {
            Disk::Local => Ok(tree::read_dir_sorted(dir)),
            Disk::Host(conn) => block_on(read_host_dir(conn, dir)),
        }
    }

    /// A file for the editor, and its modification time.
    ///
    /// Blocking: never call it on a UI loop, nor on the runtime a host's
    /// connection runs on, whose task it waits for.
    pub fn read_file(&self, path: &Path) -> Result<(String, Option<SystemTime>), String> {
        match self {
            Disk::Local => editor::read_blocking(path),
            Disk::Host(conn) => {
                block_on(conn.read_file(path)).map_err(|e| format!("On {}: {e}", conn.host()))
            }
        }
    }

    /// Write `text` unless the file changed since `expected`.
    ///
    /// Blocking: never call it on a UI loop, nor on the runtime a host's
    /// connection runs on, whose task it waits for.
    pub fn save_file(
        &self,
        path: &Path,
        text: String,
        expected: Option<SystemTime>,
    ) -> SaveOutcome {
        match self {
            Disk::Local => editor::save_blocking(path, text, expected),
            Disk::Host(conn) => block_on(conn.save_file(path, text, expected))
                .unwrap_or_else(|e| SaveOutcome::Failed(format!("On {}: {e}", conn.host()))),
        }
    }

    /// The repository status of `root`; None when it is not in one, or a
    /// host could not say.
    ///
    /// Blocking: never call it on a UI loop, nor on the runtime a host's
    /// connection runs on, whose task it waits for.
    pub fn git_status(&self, root: &Path) -> Option<GitStatus> {
        match self {
            Disk::Local => gitstat::read_blocking(root),
            Disk::Host(conn) => block_on(conn.git_status(root)).ok().flatten(),
        }
    }

    /// Whether this is a host's disk over an attach that has ended, so
    /// nothing read through it can land.
    pub fn is_closed(&self) -> bool {
        match self {
            Disk::Local => false,
            Disk::Host(conn) => conn.is_closed(),
        }
    }

    /// Whether this is a host's disk whose attach the host closed because
    /// another machine attached.
    pub fn taken(&self) -> bool {
        match self {
            Disk::Local => false,
            Disk::Host(conn) => conn.taken(),
        }
    }

    /// The host's name, or None for this machine.
    pub fn host(&self) -> Option<&str> {
        match self {
            Disk::Local => None,
            Disk::Host(conn) => Some(conn.host()),
        }
    }
}

#[cfg(test)]
mod tests;
