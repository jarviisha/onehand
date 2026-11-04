use super::*;
use crate::acp::AcpRequest;
use crate::host::wire::Readiness;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-copy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn host() -> HostName {
    HostName("box.example".into())
}

fn session() -> HostSession {
    HostSession("1700000000-0".into())
}

fn chunk(text: &str) -> Entry {
    Entry::Event(AcpEvent::ThoughtChunk(text.into()))
}

fn seqs(entries: &journal::Entries) -> Vec<u64> {
    entries.iter().map(|(s, _, _)| *s).collect()
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

#[test]
fn append_then_read_round_trips() {
    let dir = scratch("round");
    let mut copy = Copy::open(&dir, &host(), &session());
    assert_eq!(copy.last(), None);
    copy.append(0, 10, &chunk("a")).unwrap();
    copy.append(1, 11, &chunk("b")).unwrap();
    copy.append(3, 12, &chunk("c")).unwrap();
    assert_eq!(copy.last(), Some(3));
    let read = Copy::open(&dir, &host(), &session()).read();
    assert_eq!(seqs(&read), [0, 1, 3]);
    assert_eq!(
        read.iter().map(|(_, at, _)| *at).collect::<Vec<_>>(),
        [10, 11, 12]
    );
    let got: Vec<_> = read.into_iter().map(|(_, _, e)| e).collect();
    assert_eq!(json(&got), json(&vec![chunk("a"), chunk("b"), chunk("c")]));
    assert_eq!(Copy::open(&dir, &host(), &session()).last(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_below_last_is_ignored() {
    let dir = scratch("below");
    let mut copy = Copy::open(&dir, &host(), &session());
    copy.append(4, 0, &chunk("a")).unwrap();
    copy.append(4, 0, &chunk("again")).unwrap();
    copy.append(2, 0, &chunk("old")).unwrap();
    let read = copy.read();
    assert_eq!(seqs(&read), [4]);
    assert_eq!(copy.last(), Some(4));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn torn_last_line_is_left_out() {
    let dir = scratch("torn");
    let mut copy = Copy::open(&dir, &host(), &session());
    copy.append(0, 0, &chunk("a")).unwrap();
    copy.append(1, 0, &chunk("b")).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&copy.path)
        .unwrap();
    f.write_all(br#"{"seq":2,"at":0,"entry":{"event":"#)
        .unwrap();
    drop(f);
    let mut copy = Copy::open(&dir, &host(), &session());
    assert_eq!(seqs(&copy.read()), [0, 1]);
    assert_eq!(copy.last(), Some(1));
    copy.append(2, 0, &chunk("c")).unwrap();
    assert_eq!(seqs(&copy.read()), [0, 1, 2]);
    assert_eq!(Copy::open(&dir, &host(), &session()).last(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clear_empties() {
    let dir = scratch("clear");
    let mut copy = Copy::open(&dir, &host(), &session());
    copy.clear().unwrap();
    copy.append(5, 0, &chunk("a")).unwrap();
    copy.clear().unwrap();
    assert!(copy.read().is_empty());
    assert_eq!(copy.last(), None);
    copy.append(0, 0, &chunk("b")).unwrap();
    assert_eq!(seqs(&copy.read()), [0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn events_say_time_once_per_second() {
    let entries: journal::Entries = vec![
        (0, 0, chunk("untimed")),
        (1, 100, chunk("a")),
        (2, 100, chunk("b")),
        (3, 101, chunk("c")),
    ];
    let want = vec![
        AcpEvent::ThoughtChunk("untimed".into()),
        AcpEvent::At(100),
        AcpEvent::ThoughtChunk("a".into()),
        AcpEvent::ThoughtChunk("b".into()),
        AcpEvent::At(101),
        AcpEvent::ThoughtChunk("c".into()),
    ];
    assert_eq!(format!("{:?}", events(&entries)), format!("{want:?}"));
}

#[test]
fn events_drop_ready_and_echo_requests() {
    let ready = Entry::Ready(Readiness {
        resumed: false,
        forkable: true,
    });
    let entries: journal::Entries = vec![
        (0, 0, ready),
        (1, 0, Entry::Request(AcpRequest::Cancel)),
        (2, 0, chunk("a")),
    ];
    let want = vec![
        AcpEvent::Echo(AcpRequest::Cancel),
        AcpEvent::ThoughtChunk("a".into()),
    ];
    assert_eq!(format!("{:?}", events(&entries)), format!("{want:?}"));
}

#[test]
fn list_round_trips() {
    let dir = scratch("list");
    let list = vec![SessionInfo {
        session: session(),
        cwd: "/srv/app".into(),
        agent: "Claude".into(),
        busy: false,
        ended: true,
    }];
    save_list(&dir, &host(), &list).unwrap();
    assert_eq!(load_list(&dir, &host()), list);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_list_is_empty() {
    let dir = scratch("nolist");
    assert!(load_list(&dir, &host()).is_empty());
}
