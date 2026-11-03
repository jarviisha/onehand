use std::collections::BTreeMap;

use tokio::io::{BufReader, DuplexStream, Lines, ReadHalf, WriteHalf};

use super::*;

/// The host's end of the pipe: it reads asks and writes lines.
struct Host {
    asks: Lines<BufReader<ReadHalf<DuplexStream>>>,
    out: WriteHalf<DuplexStream>,
}

impl Host {
    async fn ask(&mut self) -> Ask {
        let line = self.asks.next_line().await.unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, line: Line) {
        let mut text = serde_json::to_string(&line).unwrap();
        text.push('\n');
        self.out.write_all(text.as_bytes()).await.unwrap();
    }

    async fn entry(&mut self, seq: u64, entry: Entry) {
        self.send(Line::Entry {
            session: "s1".into(),
            seq,
            entry,
        })
        .await;
    }

    async fn answer(&mut self, id: u64, reply: Reply) {
        self.send(Line::Answer {
            id,
            result: Ok(reply),
        })
        .await;
    }

    /// Read the `Watch` a `watch` sends, and answer it.
    async fn watched(&mut self) {
        let Ask { id, call } = self.ask().await;
        assert!(matches!(call, Call::Watch { session } if session == "s1"));
        self.answer(id, Reply::Done).await;
    }
}

fn pair() -> (HostConn, Host) {
    let (ours, theirs) = tokio::io::duplex(1 << 16);
    let (read, write) = tokio::io::split(ours);
    let (asks, out) = tokio::io::split(theirs);
    (
        HostConn::over(read, write),
        Host {
            asks: BufReader::new(asks).lines(),
            out,
        },
    )
}

#[tokio::test]
async fn watch_maps_entries_to_events() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    host.entry(
        0,
        Entry::Ready {
            resumed: false,
            forkable: true,
        },
    )
    .await;
    host.entry(
        1,
        Entry::Event(AcpEvent::AgentChunk {
            text: "hi".into(),
            message: None,
        }),
    )
    .await;
    host.entry(
        2,
        Entry::Request(AcpRequest::Prompt {
            text: "go".into(),
            attachments: vec![],
        }),
    )
    .await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Connected {
            resumed: false,
            forkable: true,
            ..
        })
    ));
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::AgentChunk { text, .. }) if text == "hi"
    ));
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Echo(AcpRequest::Prompt { text, .. })) if text == "go"
    ));
}

#[tokio::test]
async fn requests_on_tx_go_to_the_host() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    host.entry(
        0,
        Entry::Ready {
            resumed: true,
            forkable: false,
        },
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = events.next().await else {
        panic!("no Connected");
    };
    tx.send(AcpRequest::Cancel).unwrap();
    let Ask { call, .. } = host.ask().await;
    assert!(matches!(
        call,
        Call::Request { session, request: AcpRequest::Cancel } if session == "s1"
    ));
}

#[tokio::test]
async fn closing_the_stream_ends_watch_with_disconnected() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    drop(host);
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why.contains("lost the connection")
    ));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn kicked_ends_watch_saying_so() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    host.send(Line::Kicked).await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why.contains("attached from another machine")
    ));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn open_clears_env() {
    let (conn, mut host) = pair();
    let agent = AgentSpec {
        name: "a".into(),
        command: "agent".into(),
        env: BTreeMap::from([("SECRET".into(), "x".into())]),
        ..AgentSpec::default()
    };
    let opened =
        tokio::spawn(async move { conn.open("/w".into(), agent, Opening::Fresh).await.unwrap() });
    let Ask { id, call } = host.ask().await;
    let Call::Open { agent, .. } = call else {
        panic!("not an Open: {call:?}");
    };
    assert!(agent.env.is_empty());
    assert_eq!(agent.command, "agent");
    host.answer(
        id,
        Reply::Opened {
            session: "s9".into(),
        },
    )
    .await;
    assert_eq!(opened.await.unwrap(), "s9");
}

/// A host on build `theirs`, with `busy` mid-turn, answering every `Hello`
/// and counting the `Shutdown`s it is sent until the laptop goes.
fn fake_host(theirs: &str, busy: Vec<String>) -> (HostConn, tokio::task::JoinHandle<usize>) {
    let (conn, mut host) = pair();
    let theirs = theirs.to_string();
    let fake = tokio::spawn(async move {
        let mut shutdowns = 0;
        while let Ok(Some(line)) = host.asks.next_line().await {
            let Ask { id, call } = serde_json::from_str(&line).unwrap();
            let reply = match call {
                Call::Hello { .. } => Reply::Welcome {
                    build: theirs.clone(),
                    busy: busy.clone(),
                },
                Call::Shutdown => {
                    shutdowns += 1;
                    Reply::Done
                }
                other => panic!("unexpected {other:?}"),
            };
            host.answer(id, reply).await;
        }
        shutdowns
    });
    (conn, fake)
}

async fn settle_against(theirs: &str, busy: Vec<String>) -> (Result<Settled, HostError>, usize) {
    let (conn, fake) = fake_host(theirs, busy);
    let settled = settle(&conn, "ours").await;
    drop(conn);
    (settled, fake.await.unwrap())
}

#[tokio::test]
async fn same_build_settles_at_once() {
    let (settled, shutdowns) = settle_against("ours", vec![]).await;
    assert!(matches!(settled, Ok(Settled::Same)));
    assert_eq!(shutdowns, 0);
}

#[tokio::test]
async fn mismatch_with_busy_sessions_refuses() {
    let (settled, shutdowns) = settle_against("old", vec!["s1".into()]).await;
    assert!(matches!(
        settled,
        Err(HostError::Busy { build, busy }) if build == "old" && busy == ["s1"]
    ));
    assert_eq!(shutdowns, 0);
}

#[tokio::test]
async fn mismatch_idle_shuts_down_and_reattaches() {
    let (settled, shutdowns) = settle_against("old", vec![]).await;
    assert!(matches!(settled, Ok(Settled::Restarted)));
    assert_eq!(shutdowns, 1);
}

#[tokio::test]
async fn after_a_restart_another_build_is_a_fault_not_a_second_shutdown() {
    let (conn, fake) = fake_host("old", vec![]);
    let again = settle_again(&conn, "ours").await;
    drop(conn);
    assert!(matches!(again, Err(HostError::Protocol(why)) if why.contains("old")));
    assert_eq!(fake.await.unwrap(), 0);

    let (conn, fake) = fake_host("ours", vec![]);
    assert!(settle_again(&conn, "ours").await.is_ok());
    drop(conn);
    assert_eq!(fake.await.unwrap(), 0);
}

#[tokio::test]
async fn pending_calls_fail_when_the_stream_closes() {
    let (conn, mut host) = pair();
    let listing = tokio::spawn({
        let conn = conn.clone();
        async move { conn.list().await }
    });
    let opening = tokio::spawn(async move {
        conn.open("/w".into(), AgentSpec::default(), Opening::Fresh)
            .await
    });
    host.ask().await;
    host.ask().await;
    drop(host);
    assert!(matches!(
        listing.await.unwrap(),
        Err(HostError::Unreachable(why)) if why.contains("lost the connection")
    ));
    assert!(matches!(
        opening.await.unwrap(),
        Err(HostError::Unreachable(why)) if why.contains("lost the connection")
    ));
}

#[tokio::test]
async fn a_refused_watch_ends_in_one_disconnected() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    let Ask { id, .. } = host.ask().await;
    host.send(Line::Answer {
        id,
        result: Err("no such session".into()),
    })
    .await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why == "no such session"
    ));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn a_refused_request_is_said_as_an_error() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    host.entry(
        0,
        Entry::Ready {
            resumed: false,
            forkable: false,
        },
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = events.next().await else {
        panic!("no Connected");
    };
    tx.send(AcpRequest::Cancel).unwrap();
    let Ask { id, .. } = host.ask().await;
    host.send(Line::Answer {
        id,
        result: Err("the session ended".into()),
    })
    .await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Error { text, .. }) if text == "the session ended"
    ));
}

#[tokio::test]
async fn a_live_ready_reuses_the_request_channel() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    let ready = Entry::Ready {
        resumed: false,
        forkable: false,
    };
    host.entry(0, ready.clone()).await;
    host.entry(1, ready).await;
    let Some(AcpEvent::Connected { tx: first, .. }) = events.next().await else {
        panic!("no Connected");
    };
    let Some(AcpEvent::Connected { tx: second, .. }) = events.next().await else {
        panic!("no second Connected");
    };
    assert!(first.same_channel(&second));
}

#[tokio::test]
async fn a_session_that_ends_ends_its_watch() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    host.watched().await;
    host.entry(
        0,
        Entry::Event(AcpEvent::Disconnected("the agent died".into())),
    )
    .await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why == "the agent died"
    ));
    drop(host);
    assert!(events.next().await.is_none());
}

fn chunk(text: &str) -> Entry {
    Entry::Event(AcpEvent::AgentChunk {
        text: text.into(),
        message: None,
    })
}

async fn next_chunk(events: &mut (impl Stream<Item = AcpEvent> + Unpin)) -> String {
    match events.next().await {
        Some(AcpEvent::AgentChunk { text, .. }) => text,
        other => panic!("not a chunk: {other:?}"),
    }
}

#[tokio::test]
async fn watching_again_skips_what_the_first_watch_had_in_flight() {
    let (conn, mut host) = pair();
    let ready = Entry::Ready {
        resumed: false,
        forkable: false,
    };
    let mut first = conn.watch("s1");
    host.watched().await;
    host.entry(0, ready.clone()).await;
    host.entry(1, chunk("a")).await;
    assert!(matches!(
        first.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(next_chunk(&mut first).await, "a");

    let mut second = conn.watch("s1");
    let Ask { id, .. } = host.ask().await;
    // Sent live to the first watch before the host read the second.
    host.entry(2, chunk("b")).await;
    for (seq, entry) in [(0, ready), (1, chunk("a")), (2, chunk("b"))] {
        host.entry(seq, entry).await;
    }
    host.answer(id, Reply::Done).await;
    host.entry(3, chunk("c")).await;
    assert!(matches!(
        second.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(next_chunk(&mut second).await, "a");
    assert_eq!(next_chunk(&mut second).await, "b");
    assert!(matches!(
        second.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(next_chunk(&mut second).await, "c");
}

#[tokio::test]
async fn what_a_replay_makes_the_chat_send_never_reaches_the_host() {
    let (conn, mut host) = pair();
    let mut events = conn.watch("s1");
    let Ask { id, .. } = host.ask().await;
    host.entry(
        0,
        Entry::Ready {
            resumed: false,
            forkable: false,
        },
    )
    .await;
    host.entry(
        1,
        Entry::Event(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        }),
    )
    .await;
    // What a chat does with a replayed turn end that left a card open.
    let Some(AcpEvent::Connected { tx: replayed, .. }) = events.next().await else {
        panic!("no Connected");
    };
    let _ = replayed.send(AcpRequest::PermissionResponse {
        rpc_id: serde_json::json!(1),
        option_id: None,
    });
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::TurnEnded { .. })
    ));
    host.answer(id, Reply::Done).await;
    let Some(AcpEvent::Connected { tx: live, .. }) = events.next().await else {
        panic!("no live Connected after the replay");
    };
    let _ = replayed.send(AcpRequest::PermissionResponse {
        rpc_id: serde_json::json!(1),
        option_id: None,
    });
    live.send(AcpRequest::Cancel).unwrap();
    let Ask { call, .. } = host.ask().await;
    assert!(
        matches!(
            call,
            Call::Request {
                request: AcpRequest::Cancel,
                ..
            }
        ),
        "{call:?}"
    );
}

#[tokio::test]
async fn watching_again_ends_the_first_watch() {
    let (conn, mut host) = pair();
    let mut first = conn.watch("s1");
    let _second = conn.watch("s1");
    host.watched().await;
    host.watched().await;
    assert!(matches!(
        first.next().await,
        Some(AcpEvent::Disconnected(why)) if why.contains("watched elsewhere")
    ));
    assert!(first.next().await.is_none());
}

#[tokio::test]
async fn what_was_queued_as_the_stream_ended_is_refused() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (asked, answer) = oneshot::channel();
    let (watched, mut events) = events::unbounded();
    tx.send(Cmd::Ask(Call::List, asked)).ok().unwrap();
    tx.send(Cmd::Watch("s1".into(), watched)).ok().unwrap();
    refuse_queued(&mut rx, "gone");
    assert!(matches!(answer.await, Ok(Err(HostError::Unreachable(why))) if why == "gone"));
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why == "gone"
    ));
    assert!(tx
        .send(Cmd::Request("s1".into(), AcpRequest::Cancel))
        .is_err());
}

/// A scratch directory for an install, run here through `sh`.
fn install_dir(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("onehand-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.display().to_string()
}

const HERE: HostAddr = HostAddr {
    ssh: String::new(),
    local: true,
};

#[tokio::test]
async fn a_copy_that_names_its_build_is_moved_into_place() {
    let dir = install_dir("good");
    let script = b"#!/bin/sh\necho abc\n";
    run(&HERE, &copy_command(&dir, "abc"), script)
        .await
        .unwrap();
    let path = Path::new(&dir).join("onehand-host");
    assert_eq!(std::fs::read(&path).unwrap(), script);
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_cut_short_copy_is_never_installed() {
    let dir = install_dir("short");
    // What a copy cut off mid-way leaves: a file that cannot name the build.
    let script = b"#!/bin/sh\necho ab";
    let copied = run(&HERE, &copy_command(&dir, "abc"), script).await;
    assert!(matches!(copied, Err(HostError::Unreachable(why)) if why.contains("cut short")));
    assert!(!Path::new(&dir).join("onehand-host").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_copy_that_cannot_run_here_says_why_and_is_never_installed() {
    let dir = install_dir("old-glibc");
    // What a binary built against a newer C library prints as it fails to load.
    let script = b"#!/bin/sh\necho \"version 'GLIBC_2.39' not found\" >&2\nexit 1\n";
    let copied = run(&HERE, &copy_command(&dir, "abc"), script).await;
    assert!(
        matches!(&copied, Err(HostError::Unreachable(why))
            if why.contains("GLIBC_2.39") && !why.contains("cut short")),
        "{copied:?}"
    );
    assert!(!Path::new(&dir).join("onehand-host").exists());
    assert!(!Path::new(&dir).join("onehand-host.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_probe_is_read_after_its_marker_whatever_an_rc_file_prints() {
    let out = format!("Welcome to the box!\n{PROBE}\nLinux x86_64\npresent\n");
    assert_eq!(probed(&out), ("Linux x86_64", true));
    let out = format!("{PROBE} is a word too\n{PROBE}\nDarwin arm64\n");
    assert_eq!(probed(&out), ("Darwin arm64", false));
    assert_eq!(probed("banner only\n"), ("", false));
}

#[tokio::test]
async fn a_card_answered_during_a_rewatch_reaches_the_host() {
    let (conn, mut host) = pair();
    let mut first = conn.watch("s1");
    host.watched().await;
    host.entry(
        0,
        Entry::Ready {
            resumed: false,
            forkable: false,
        },
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = first.next().await else {
        panic!("no Connected");
    };
    let _second = conn.watch("s1");
    let Ask { call, .. } = host.ask().await;
    assert!(matches!(call, Call::Watch { .. }), "{call:?}");
    // The second watch is still replaying: its Watch has no answer yet.
    tx.send(AcpRequest::PermissionResponse {
        rpc_id: serde_json::json!(7),
        option_id: Some("allow".into()),
    })
    .unwrap();
    let Ask { call, .. } = tokio::time::timeout(Duration::from_secs(5), host.ask())
        .await
        .expect("the answer reached the host");
    assert!(
        matches!(
            call,
            Call::Request {
                request: AcpRequest::PermissionResponse { .. },
                ..
            }
        ),
        "{call:?}"
    );
}
