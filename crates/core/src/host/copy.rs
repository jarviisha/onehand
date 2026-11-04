//! The laptop's own copy of a host session's journal: the journal's lines,
//! each with the seq the host gave it, kept at
//! `<root>/<host>/<session>.jsonl`. It lets a host session be read while the
//! host is not connected, and a later watch carry on from where it ends.
//! Beside the copies, `sessions.json` keeps the host's last session list.

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::journal;
use super::wire::{Entry, HostSession, SessionInfo};
use crate::acp::AcpEvent;
use crate::chat::store::sanitize;
use crate::workspace::HostName;

const LIST: &str = "sessions.json";

/// Where every host's copies live.
pub fn copies_dir() -> PathBuf {
    crate::config::config_dir().join("hosts")
}

fn host_dir(root: &Path, host: &HostName) -> PathBuf {
    root.join(sanitize(&host.0))
}

/// One copy line.
#[derive(Serialize, Deserialize)]
struct Line<E> {
    seq: u64,
    at: u64,
    entry: E,
}

/// One session's copy.
pub struct Copy {
    path: PathBuf,
    /// The last whole line's seq, once read; none inside for an empty copy.
    last: Cell<Option<Option<u64>>>,
}

impl Copy {
    /// The copy of `session` on `host` under `root`; nothing is read yet.
    pub fn open(root: &Path, host: &HostName, session: &HostSession) -> Copy {
        let path = host_dir(root, host).join(format!("{}.jsonl", sanitize(&session.0)));
        Copy {
            path,
            last: Cell::new(None),
        }
    }

    /// Every entry on a whole line that parses, in order. A line a crash cut
    /// short is left out, as is the whole copy when it cannot be read.
    pub fn read(&self) -> journal::Entries {
        let Ok(bytes) = fs::read(&self.path) else {
            return Vec::new();
        };
        let whole = bytes
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |at| at + 1);
        bytes[..whole]
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice::<Line<Entry>>(line).ok())
            .map(|Line { seq, at, entry }| (seq, at, entry))
            .collect()
    }

    /// The seq of the copy's last entry, read once and then kept.
    pub fn last(&self) -> Option<u64> {
        if let Some(last) = self.last.get() {
            return last;
        }
        let last = self.read().last().map(|(seq, _, _)| *seq);
        self.last.set(Some(last));
        last
    }

    /// Add one entry at the end. One whose seq is not past the last is
    /// already here, and is not written again.
    pub fn append(&mut self, seq: u64, at: u64, entry: &Entry) -> io::Result<()> {
        if self.last().is_some_and(|last| seq <= last) {
            return Ok(());
        }
        let mut line = serde_json::to_vec(&Line { seq, at, entry }).map_err(io::Error::other)?;
        line.push(b'\n');
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)?;
        if torn(&mut file)? {
            // A line a crash cut short must not fuse with this one.
            line.insert(0, b'\n');
        }
        file.write_all(&line)?;
        self.last.set(Some(Some(seq)));
        Ok(())
    }

    /// Drop the whole copy.
    pub fn clear(&mut self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        self.last.set(Some(None));
        Ok(())
    }
}

/// Whether `file` ends partway through a line.
fn torn(file: &mut File) -> io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut end = [0];
    file.read_exact(&mut end)?;
    Ok(end[0] != b'\n')
}

/// What a journal entry says to a chat: none for the agent coming up, which
/// is the connection's to tell; a request the person made, echoed.
pub(super) fn as_event(entry: Entry) -> Option<AcpEvent> {
    match entry {
        Entry::Ready(_) => None,
        Entry::Event(event) => Some(event),
        Entry::Request(request) => Some(AcpEvent::Echo(request)),
    }
}

/// A copy read back as a chat takes it, as a watch's replay is: each entry's
/// event, after `At` whenever its second differs from the last one said. An
/// entry at 0 kept no time and says none.
pub fn events(entries: &journal::Entries) -> Vec<AcpEvent> {
    let mut said = None;
    let mut out = Vec::new();
    for (_, at, entry) in entries {
        let Some(event) = as_event(entry.clone()) else {
            continue;
        };
        if *at != 0 && said != Some(*at) {
            said = Some(*at);
            out.push(AcpEvent::At(*at));
        }
        out.push(event);
    }
    out
}

/// Keep `host`'s session list, whole.
pub fn save_list(root: &Path, host: &HostName, list: &[SessionInfo]) -> io::Result<()> {
    let text = serde_json::to_string(list).map_err(io::Error::other)?;
    crate::config::write_atomic(&host_dir(root, host).join(LIST), &text)
}

/// `host`'s kept session list; empty when none was kept or it cannot be read.
pub fn load_list(root: &Path, host: &HostName) -> Vec<SessionInfo> {
    fs::read(host_dir(root, host).join(LIST))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
