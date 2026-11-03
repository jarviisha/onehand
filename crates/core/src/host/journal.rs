//! A host session's journal: every entry of the session, one JSON line each
//! with the unix second it was journaled at, appended and never rewritten. A line's number is its seq, so a torn last
//! line costs that entry alone and no later seq shifts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::wire::Entry;

const FILE: &str = "journal.jsonl";

/// How far `last` first reaches back from a journal's end; it doubles until
/// it holds a whole line.
const TAIL_CHUNK: u64 = 4096;

/// One journal line.
#[derive(Serialize, Deserialize)]
struct Line<E> {
    at: u64,
    entry: E,
}

/// A line read back, with its time; a bare entry, as lines were before they
/// kept their time, reads with `at` 0.
fn parse(line: &[u8]) -> Option<(u64, Entry)> {
    match serde_json::from_slice::<Line<Entry>>(line) {
        Ok(Line { at, entry }) => Some((at, entry)),
        Err(_) => serde_json::from_slice(line).ok().map(|entry| (0, entry)),
    }
}

pub struct Journal {
    file: File,
    next: u64,
    /// The file's length, kept here so an append need not ask the file;
    /// none once a failed line could not be cut back, after which it is
    /// never cut back again.
    len: Option<u64>,
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
        let mut len = existing.len() as u64;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        if existing.last().is_some_and(|b| *b != b'\n') {
            // A torn tail still counts as a line, and must not fuse with the next entry.
            file.write_all(b"\n")?;
            next += 1;
            len += 1;
        }
        Ok(Journal {
            file,
            next,
            len: Some(len),
        })
    }

    /// Append one entry, journaled at unix second `at`, as one line, flushed;
    /// returns its seq (0-based line number).
    pub fn append(&mut self, at: u64, entry: &Entry) -> io::Result<u64> {
        let mut line = serde_json::to_vec(&Line { at, entry }).map_err(io::Error::other)?;
        line.push(b'\n');
        land(&mut self.file, &mut self.next, &mut self.len, &line)
    }
}

/// A journal file cut back to `len`.
trait Cut {
    fn cut(&mut self, len: u64) -> io::Result<()>;
}

impl Cut for File {
    fn cut(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)
    }
}

/// Write `line` whole to `file`, which is `len` long, and return its seq,
/// `next`; both move on past it. What part of a line that failed landed is
/// cut back, so a watch never meets half an entry. A file that cannot be cut
/// back gets the torn line ended instead, as `open` does, so it costs its seq
/// alone and the next entry does not fuse with it; its length is then not
/// known, and no later line is cut back, so no seq can drift.
// ponytail: after one failed cut-back every later failed append costs a torn
// line and a seq; reopening the journal learns the length again.
fn land<W: Write + Cut>(
    file: &mut W,
    next: &mut u64,
    len: &mut Option<u64>,
    line: &[u8],
) -> io::Result<u64> {
    if let Err(e) = file.write_all(line).and_then(|()| file.flush()) {
        let cut = len.is_some_and(|len| file.cut(len).is_ok());
        if !cut {
            *len = None;
            if file.write_all(b"\n").is_ok() {
                *next += 1;
            }
        }
        return Err(e);
    }
    let seq = *next;
    *next += 1;
    if let Some(len) = len {
        *len += line.len() as u64;
    }
    Ok(seq)
}

/// Entries read back, each with its seq and the unix second it was journaled at.
pub type Entries = Vec<(u64, u64, Entry)>;

/// Where a read of a journal stopped: the seq and the byte offset of the
/// next line it did not read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mark {
    seq: u64,
    at: u64,
}

/// Every readable entry in `dir/journal.jsonl` with its seq and time, in
/// order. A line that does not parse is skipped.
pub fn read(dir: &Path) -> io::Result<Entries> {
    Ok(read_from(dir, Mark::default())?.0)
}

/// Every readable entry on the whole lines from `from` on, with its seq and
/// time, and where they end. A line its newline has not yet ended is left for the next
/// read: it may still be being written, and is torn if it never is.
pub fn read_from(dir: &Path, from: Mark) -> io::Result<(Entries, Mark)> {
    let mut file = match File::open(dir.join(FILE)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), from)),
        Err(e) => return Err(e),
    };
    file.seek(SeekFrom::Start(from.at))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let whole = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |at| at + 1);
    let lines = bytes[..whole].split_inclusive(|b| *b == b'\n');
    let entries = lines
        .clone()
        .enumerate()
        .filter_map(|(i, line)| {
            let (at, entry) = parse(&line[..line.len() - 1])?;
            Some((from.seq + i as u64, at, entry))
        })
        .collect();
    let end = Mark {
        seq: from.seq + lines.count() as u64,
        at: from.at + whole as u64,
    };
    Ok((entries, end))
}

/// The last entry in the journal that parses, read from the file's end
/// alone: whole lines this build cannot read (a newer build's entry, a torn
/// line ended later) are stepped over, as is a torn tail. None for a missing
/// or empty journal, or one with no line that parses.
pub fn last(dir: &Path) -> io::Result<Option<Entry>> {
    let mut file = match File::open(dir.join(FILE)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let size = file.metadata()?.len();
    let mut reach = TAIL_CHUNK;
    loop {
        let start = size.saturating_sub(reach);
        file.seek(SeekFrom::Start(start))?;
        let mut tail = Vec::new();
        (&mut file).take(size - start).read_to_end(&mut tail)?;
        let whole = tail.iter().rposition(|b| *b == b'\n').unwrap_or(0);
        let mut lines = tail[..whole].split(|b| *b == b'\n').rev().peekable();
        while let Some(line) = lines.next() {
            // Read from mid-file, the first line may have begun before `start`.
            if start > 0 && lines.peek().is_none() {
                break;
            }
            if let Some((_, entry)) = parse(line) {
                return Ok(Some(entry));
            }
        }
        if start == 0 {
            return Ok(None);
        }
        reach *= 2;
    }
}

#[cfg(test)]
mod tests;
