use std::io::Write;
use std::path::PathBuf;

use super::*;
use crate::acp::AcpEvent;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-journal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn chunk(text: &str) -> Entry {
    Entry::Event(AcpEvent::ThoughtChunk(text.into()))
}

#[test]
fn appends_and_reads_back_in_order() {
    let dir = scratch("order");
    let mut j = Journal::open(&dir).unwrap();
    let seqs: Vec<u64> = ["a", "b", "c"]
        .iter()
        .map(|t| j.append(&chunk(t)).unwrap())
        .collect();
    assert_eq!(seqs, [0, 1, 2]);
    let read = read(&dir).unwrap();
    let want: Vec<_> = ["a", "b", "c"].iter().map(|t| chunk(t)).collect();
    let got: Vec<_> = read.iter().map(|(_, e)| e.clone()).collect();
    assert_eq!(read.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(
        serde_json::to_string(&got).unwrap(),
        serde_json::to_string(&want).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reopening_continues_the_count() {
    let dir = scratch("reopen");
    let mut j = Journal::open(&dir).unwrap();
    for t in ["a", "b", "c"] {
        j.append(&chunk(t)).unwrap();
    }
    drop(j);
    let mut j = Journal::open(&dir).unwrap();
    assert_eq!(j.append(&chunk("d")).unwrap(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn torn_last_line_costs_only_itself() {
    let dir = scratch("torn");
    let mut j = Journal::open(&dir).unwrap();
    j.append(&chunk("a")).unwrap();
    j.append(&chunk("b")).unwrap();
    drop(j);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    f.write_all(br#"{"event":{"agent_chunk":"#).unwrap();
    drop(f);
    assert_eq!(read(&dir).unwrap().len(), 2);
    let mut j = Journal::open(&dir).unwrap();
    assert_eq!(j.append(&chunk("c")).unwrap(), 3);
    let read = read(&dir).unwrap();
    assert_eq!(read.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [0, 1, 3]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that takes `room` more bytes, then fails, as a full disk does
/// mid-write; and that cannot be cut back when `stuck`.
struct Full {
    bytes: Vec<u8>,
    room: usize,
    stuck: bool,
}

impl Write for Full {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.room == 0 {
            return Err(std::io::Error::other("no space left"));
        }
        let n = buf.len().min(self.room);
        self.bytes.extend_from_slice(&buf[..n]);
        self.room -= n;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Cut for Full {
    fn cut(&mut self, len: u64) -> std::io::Result<()> {
        if self.stuck {
            // Room again for the newline that ends the torn line.
            self.room = 1;
            return Err(std::io::Error::other("cannot cut"));
        }
        self.bytes.truncate(len as usize);
        Ok(())
    }
}

#[test]
fn a_failed_append_leaves_no_partial_line() {
    let mut file = Full {
        bytes: b"{\"a\":1}\n".to_vec(),
        room: 4,
        stuck: false,
    };
    let mut next = 1;
    assert!(land(&mut file, &mut next, 8, b"{\"b\":2}\n").is_err());
    assert_eq!(file.bytes, b"{\"a\":1}\n");
    assert_eq!(next, 1);
}

#[test]
fn a_line_that_cannot_be_cut_back_is_ended_and_counted() {
    let mut file = Full {
        bytes: b"{\"a\":1}\n".to_vec(),
        room: 4,
        stuck: true,
    };
    let mut next = 1;
    assert!(land(&mut file, &mut next, 8, b"{\"b\":2}\n").is_err());
    assert_eq!(file.bytes, b"{\"a\":1}\n{\"b\"\n");
    assert_eq!(next, 2);
}
