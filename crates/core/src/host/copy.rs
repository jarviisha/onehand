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
        self.scan().map(|(entries, _)| entries).unwrap_or_default()
    }

    /// The copy's whole lines that parse, and whether any whole line did
    /// not; a missing copy is empty.
    fn scan(&self) -> io::Result<(journal::Entries, bool)> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut broken = false;
        let entries = bytes[..whole(&bytes)]
            .split_inclusive(|b| *b == b'\n')
            .filter_map(|line| {
                let parsed = serde_json::from_slice::<Line<Entry>>(&line[..line.len() - 1]);
                broken |= parsed.is_err();
                parsed.ok().map(|Line { seq, at, entry }| (seq, at, entry))
            })
            .collect();
        Ok((entries, broken))
    }

    /// The seq of the copy's last entry, read once and then kept. None for a
    /// copy with a whole line that does not parse: an entry may be missing
    /// below its last, so a watch must take the host's whole journal. None
    /// too, and nothing kept, when the copy cannot be read.
    pub fn last(&self) -> Option<u64> {
        if let Some(last) = self.last.get() {
            return last;
        }
        let Ok((entries, broken)) = self.scan() else {
            return None;
        };
        let last = if broken {
            None
        } else {
            entries.last().map(|(seq, _, _)| *seq)
        };
        self.last.set(Some(last));
        last
    }

    /// Add one entry at the end. One whose seq is not past the last is
    /// already here, and is not written again. A line a crash cut short is
    /// cut off first. When the write fails the copy is dropped (as far as it
    /// can be), so no entry is ever missing below its last: the next watch
    /// fills it again from the host.
    pub fn append(&mut self, seq: u64, at: u64, entry: &Entry) -> io::Result<()> {
        if self.last().is_some_and(|last| seq <= last) {
            return Ok(());
        }
        let written = self.write(seq, at, entry);
        if written.is_err() {
            if self.clear().is_err() {
                self.last.set(None);
            }
        } else {
            self.last.set(Some(Some(seq)));
        }
        written
    }

    fn write(&self, seq: u64, at: u64, entry: &Entry) -> io::Result<()> {
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
            let mut bytes = Vec::new();
            file.seek(SeekFrom::Start(0))?;
            file.read_to_end(&mut bytes)?;
            file.set_len(whole(&bytes) as u64)?;
        }
        file.write_all(&line)
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

/// How much of `bytes` is whole lines.
fn whole(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |at| at + 1)
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

/// What a journal entry says to a chat: nothing for the agent coming up,
/// which is the connection's to tell; a request the person made, echoed; a
/// card the host answered, then its answer echoed.
pub(super) fn as_event(entry: Entry) -> Vec<AcpEvent> {
    match entry {
        Entry::Ready(_) => vec![],
        Entry::Event(event) => vec![event],
        Entry::Request(request) => vec![AcpEvent::Echo(request)],
        Entry::Answered { card, answer } => card
            .map(|card| *card)
            .into_iter()
            .chain([AcpEvent::Echo(answer)])
            .collect(),
    }
}

/// A copy read back as a chat takes it, as a watch's replay is: each entry's
/// event, after `At` whenever its second differs from the last one said. An
/// entry at 0 kept no time and says none.
pub fn events(entries: &journal::Entries) -> Vec<AcpEvent> {
    let mut said = None;
    let mut out = Vec::new();
    for (_, at, entry) in entries {
        let said_now = as_event(entry.clone());
        if said_now.is_empty() {
            continue;
        }
        if *at != 0 && said != Some(*at) {
            said = Some(*at);
            out.push(AcpEvent::At(*at));
        }
        out.extend(said_now);
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
