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
    let mut len = Some(8);
    assert!(land(&mut file, &mut next, &mut len, b"{\"b\":2}\n").is_err());
    assert_eq!(file.bytes, b"{\"a\":1}\n");
    assert_eq!((next, len), (1, Some(8)));
}

#[test]
fn a_line_that_cannot_be_cut_back_is_ended_and_counted() {
    let mut file = Full {
        bytes: b"{\"a\":1}\n".to_vec(),
        room: 4,
        stuck: true,
    };
    let mut next = 1;
    let mut len = Some(8);
    assert!(land(&mut file, &mut next, &mut len, b"{\"b\":2}\n").is_err());
    assert_eq!(file.bytes, b"{\"a\":1}\n{\"b\"\n");
    assert_eq!((next, len), (2, None));
    // Its length no longer known, a later failed line is never cut back to
    // a stale length (which would cut into the line ended above); with no
    // room for its newline either, the next line fuses with it as one.
    file.stuck = false;
    file.room = 3;
    assert!(land(&mut file, &mut next, &mut len, b"{\"c\":3}\n").is_err());
    assert_eq!(file.bytes, b"{\"a\":1}\n{\"b\"\n{\"c");
    assert_eq!((next, len), (2, None));
}

#[test]
fn a_landed_line_moves_the_length_on() {
    let mut file = Full {
        bytes: b"{\"a\":1}\n".to_vec(),
        room: 64,
        stuck: false,
    };
    let (mut next, mut len) = (1, Some(8));
    assert_eq!(
        land(&mut file, &mut next, &mut len, b"{\"b\":2}\n").unwrap(),
        1
    );
    assert_eq!((next, len), (2, Some(16)));
}

#[test]
fn appending_keeps_the_length_without_asking_the_file() {
    let dir = scratch("len");
    let mut j = Journal::open(&dir).unwrap();
    j.append(&chunk("a")).unwrap();
    j.append(&chunk("b")).unwrap();
    let on_disk = std::fs::metadata(dir.join("journal.jsonl")).unwrap().len();
    assert_eq!(j.len, Some(on_disk));
    drop(j);
    assert_eq!(Journal::open(&dir).unwrap().len, Some(on_disk));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn last_of_a_missing_or_empty_journal_is_none() {
    let dir = scratch("last-empty");
    assert!(last(&dir).unwrap().is_none());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("journal.jsonl"), b"").unwrap();
    assert!(last(&dir).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn last_skips_a_torn_tail() {
    let dir = scratch("last-torn");
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
    let got = last(&dir).unwrap().unwrap();
    assert_eq!(
        serde_json::to_string(&got).unwrap(),
        serde_json::to_string(&chunk("b")).unwrap()
    );
    // A torn line alone is no whole line.
    std::fs::write(dir.join("journal.jsonl"), br#"{"event":"#).unwrap();
    assert!(last(&dir).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn last_reaches_back_over_many_chunks() {
    let dir = scratch("last-long");
    let mut j = Journal::open(&dir).unwrap();
    for i in 0..50 {
        j.append(&chunk(&format!("{i}{}", "x".repeat(1000))))
            .unwrap();
    }
    // Longer than a chunk on its own, and a torn tail longer than one after it.
    let long = "y".repeat(3 * TAIL_CHUNK as usize);
    j.append(&chunk(&long)).unwrap();
    drop(j);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    f.write_all("z".repeat(2 * TAIL_CHUNK as usize).as_bytes())
        .unwrap();
    drop(f);
    let got = last(&dir).unwrap().unwrap();
    assert_eq!(
        serde_json::to_string(&got).unwrap(),
        serde_json::to_string(&chunk(&long)).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reading_on_from_a_mark_gives_each_whole_line_once() {
    let dir = scratch("mark");
    let mut j = Journal::open(&dir).unwrap();
    j.append(&chunk("a")).unwrap();
    j.append(&chunk("b")).unwrap();
    // A line still being written: no newline yet.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    f.write_all(br#"{"event":{"ThoughtChunk""#).unwrap();
    let (first, mark) = read_from(&dir, Mark::default()).unwrap();
    assert_eq!(first.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [0, 1]);
    f.write_all(b":\"c\"}}\n").unwrap();
    drop(f);
    j.append(&chunk("d")).unwrap();
    let (rest, end) = read_from(&dir, mark).unwrap();
    assert_eq!(rest.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [2, 3]);
    assert!(read_from(&dir, end).unwrap().0.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn last_steps_back_over_lines_it_cannot_read() {
    let dir = scratch("last-unreadable");
    let mut j = Journal::open(&dir).unwrap();
    j.append(&chunk("a")).unwrap();
    j.append(&Entry::Event(AcpEvent::Disconnected("gone".into())))
        .unwrap();
    drop(j);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    // A newer build's entry, longer than the first reach, then an empty line.
    let newer = format!(
        "{{\"future\":\"{}\"}}\n\n",
        "f".repeat(3 * TAIL_CHUNK as usize)
    );
    f.write_all(newer.as_bytes()).unwrap();
    drop(f);
    assert!(matches!(
        last(&dir).unwrap(),
        Some(Entry::Event(AcpEvent::Disconnected(why))) if why == "gone"
    ));
    // Nothing readable at all.
    std::fs::write(dir.join("journal.jsonl"), b"{\"future\":1}\n\n").unwrap();
    assert!(last(&dir).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
