//! A host session's journal: every entry of the session, one JSON line each,
//! appended and never rewritten. A line's number is its seq, so a torn last
//! line costs that entry alone and no later seq shifts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use super::wire::Entry;

const FILE: &str = "journal.jsonl";

pub struct Journal {
    file: File,
    next: u64,
}

impl Journal {
    /// Open `dir/journal.jsonl` for appending, creating `dir`; `next` continues after what is there.
    pub fn open(dir: &Path) -> io::Result<Journal> {
        fs::create_dir_all(dir)?;
        let path = dir.join(FILE);
        let existing = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut next = existing.iter().filter(|b| **b == b'\n').count() as u64;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        if existing.last().is_some_and(|b| *b != b'\n') {
            // A torn tail still counts as a line, and must not fuse with the next entry.
            file.write_all(b"\n")?;
            next += 1;
        }
        Ok(Journal { file, next })
    }

    /// Append one entry as one line, flushed; returns its seq (0-based line number).
    pub fn append(&mut self, entry: &Entry) -> io::Result<u64> {
        let mut line = serde_json::to_vec(entry).map_err(io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()?;
        let seq = self.next;
        self.next += 1;
        Ok(seq)
    }
}

/// Every readable entry in `dir/journal.jsonl` with its seq, in order. A line that does not parse is skipped.
pub fn read(dir: &Path) -> io::Result<Vec<(u64, Entry)>> {
    let text = match fs::read(dir.join(FILE)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(text
        .split(|b| *b == b'\n')
        .enumerate()
        .filter_map(|(seq, line)| Some((seq as u64, serde_json::from_slice(line).ok()?)))
        .collect())
}

#[cfg(test)]
mod tests;
