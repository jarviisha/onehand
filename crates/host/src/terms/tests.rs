use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedReceiver};

use super::*;

fn terms() -> Arc<Mutex<Terms>> {
    Arc::new(Mutex::new(Terms::new("/bin/sh".into(), None)))
}

fn open(terms: &Arc<Mutex<Terms>>, cwd: PathBuf) -> HostTerm {
    Terms::open(terms, cwd, 80, 24).unwrap()
}

/// What came for `term` within 5 s, read until it contains `want`; panics
/// with what came if it never does.
fn wait_for(lines: &mut UnboundedReceiver<FromHost>, term: HostTerm, want: &str) -> String {
    let start = Instant::now();
    let mut text = String::new();
    while start.elapsed() < Duration::from_secs(5) {
        match lines.try_recv() {
            Ok(FromHost::TermOutput { term: t, data }) if t == term => {
                text.push_str(&String::from_utf8_lossy(&data.0));
                if text.contains(want) {
                    return text;
                }
            }
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("no {want:?} in {text:?}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-terms-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn a_terminal_echoes_what_is_typed() {
    let terms = terms();
    let term = open(&terms, std::env::temp_dir());
    let (out, mut lines) = mpsc::unbounded_channel();
    lock(&terms).watch(term, out).unwrap();
    lock(&terms)
        .write(term, b"echo hi-$((1+1))\n".to_vec())
        .unwrap();
    wait_for(&mut lines, term, "hi-2");
    lock(&terms).close(term).unwrap();
}

#[test]
fn a_watch_after_a_detach_replays_what_came_meanwhile() {
    let terms = terms();
    let term = open(&terms, std::env::temp_dir());
    lock(&terms).write(term, b"echo aw''ay\n".to_vec()).unwrap();
    // Typed split, so only the output says `away`, not the echo of the line.
    let start = Instant::now();
    while !String::from_utf8_lossy(
        &lock(&terms).map[&term]
            .kept
            .iter()
            .copied()
            .collect::<Vec<u8>>(),
    )
    .contains("away")
    {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no output in time"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (out, mut lines) = mpsc::unbounded_channel();
    lock(&terms).watch(term, out).unwrap();
    assert!(matches!(
        lines.try_recv(),
        Ok(FromHost::TermOutput { data, .. }) if String::from_utf8_lossy(&data.0).contains("away")
    ));
    lock(&terms).close(term).unwrap();
}

#[test]
fn a_buffer_past_its_cap_says_it_dropped_output() {
    let mut kept = VecDeque::new();
    let mut dropped = false;
    keep(&mut kept, &mut dropped, b"start");
    assert!(!dropped);
    keep(&mut kept, &mut dropped, &vec![b'x'; TERM_KEEP]);
    assert!(dropped);
    assert_eq!(kept.len(), TERM_KEEP);

    let terms = terms();
    let term = open(&terms, std::env::temp_dir());
    {
        let mut this = lock(&terms);
        let t = this.map.get_mut(&term).unwrap();
        keep(&mut t.kept, &mut t.dropped, &vec![b'y'; TERM_KEEP + 10]);
    }
    let (out, mut lines) = mpsc::unbounded_channel();
    lock(&terms).watch(term, out).unwrap();
    let Ok(FromHost::TermOutput { data, .. }) = lines.try_recv() else {
        panic!("no replay");
    };
    assert!(data.0.starts_with(NOT_KEPT.as_bytes()));
    assert!(data.0.len() <= TERM_KEEP + NOT_KEPT.len());
    lock(&terms).close(term).unwrap();
}

#[test]
fn closing_a_terminal_ends_its_shell_and_lists_it_no_more() {
    let terms = terms();
    let term = open(&terms, std::env::temp_dir());
    let (out, mut lines) = mpsc::unbounded_channel();
    lock(&terms).watch(term, out).unwrap();
    assert_eq!(lock(&terms).list().len(), 1);
    lock(&terms).close(term).unwrap();
    let start = Instant::now();
    loop {
        match lines.try_recv() {
            Ok(FromHost::TermExit { term: t }) if t == term => break,
            Ok(_) => {}
            Err(_) => {
                assert!(start.elapsed() < Duration::from_secs(5), "no exit in time");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    assert!(lock(&terms).list().is_empty());
}

#[test]
fn a_terminal_runs_in_its_folder() {
    let dir = scratch("cwd");
    let terms = terms();
    let term = open(&terms, dir.clone());
    let (out, mut lines) = mpsc::unbounded_channel();
    lock(&terms).watch(term, out).unwrap();
    lock(&terms).write(term, b"pwd\n".to_vec()).unwrap();
    wait_for(&mut lines, term, &dir.display().to_string());
    lock(&terms).close(term).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
