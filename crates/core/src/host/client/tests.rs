use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::io::{BufReader, DuplexStream, Lines, ReadHalf, WriteHalf};

use super::*;
use crate::host::wire::Bytes;

/// The host's end of the pipe: it reads asks and writes lines.
struct Host {
    asks: Lines<BufReader<ReadHalf<DuplexStream>>>,
    out: WriteHalf<DuplexStream>,
}

impl Host {
    async fn ask(&mut self) -> Question {
        let line = self.asks.next_line().await.unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, line: FromHost) {
        let mut text = serde_json::to_string(&line).unwrap();
        text.push('\n');
        self.out.write_all(text.as_bytes()).await.unwrap();
    }

    async fn entry(&mut self, seq: u64, entry: Entry) {
        self.timed(seq, 0, entry).await;
    }

    async fn timed(&mut self, seq: u64, at: u64, entry: Entry) {
        self.send(FromHost::Entry {
            session: s1(),
            seq,
            at,
            entry,
        })
        .await;
    }

    async fn answer(&mut self, id: u64, reply: Reply) {
        self.send(FromHost::Answer {
            id,
            result: Ok(reply),
        })
        .await;
    }

    /// Read the `Watch` a `watch` sends, and start its replay; its id.
    async fn replaying(&mut self) -> u64 {
        let Question { id, call } = self.ask().await;
        assert!(matches!(call, Call::Watch { session, after: None } if session == s1()));
        self.send(FromHost::Replay {
            session: s1(),
            after: None,
        })
        .await;
        id
    }

    /// Read the `Watch` a `watch` sends, and answer it on an empty replay.
    async fn watched(&mut self) {
        let id = self.replaying().await;
        self.answer(id, Reply::Done).await;
    }
}

fn s1() -> HostSession {
    HostSession("s1".into())
}

fn build(id: &str) -> Build {
    Build(id.into())
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
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: false,
            forkable: true,
        }),
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
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: true,
            forkable: false,
        }),
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = events.next().await else {
        panic!("no Connected");
    };
    tx.send(AcpRequest::Cancel).unwrap();
    let Question { call, .. } = host.ask().await;
    assert!(matches!(
        call,
        Call::Request { session, request: AcpRequest::Cancel } if session == s1()
    ));
}

#[tokio::test]
async fn closing_the_stream_ends_watch_with_disconnected() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    drop(host);
    assert!(matches!(
        made(&mut events).await,
        Some(AcpEvent::Disconnected(why)) if why.contains("lost the connection")
    ));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn kicked_ends_watch_saying_so() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    host.send(FromHost::Kicked).await;
    assert!(matches!(
        made(&mut events).await,
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
    let Question { id, call } = host.ask().await;
    let Call::Open { agent, .. } = call else {
        panic!("not an Open: {call:?}");
    };
    assert!(agent.env.is_empty());
    assert_eq!(agent.command, "agent");
    host.answer(
        id,
        Reply::Opened {
            session: HostSession("s9".into()),
        },
    )
    .await;
    assert_eq!(opened.await.unwrap().0, "s9");
}

/// A host on build `theirs`, with `busy` mid-turn, answering every `Hello`
/// and counting the `Shutdown`s it is sent until the laptop goes.
fn fake_host(theirs: &str, busy: Vec<HostSession>) -> (HostConn, tokio::task::JoinHandle<usize>) {
    let (conn, mut host) = pair();
    let theirs = theirs.to_string();
    let fake = tokio::spawn(async move {
        let mut shutdowns = 0;
        while let Ok(Some(line)) = host.asks.next_line().await {
            let Question { id, call } = serde_json::from_str(&line).unwrap();
            let reply = match call {
                Call::Hello { .. } => Reply::Welcome {
                    build: build(&theirs),
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

async fn settle_against(
    theirs: &str,
    busy: Vec<HostSession>,
) -> (Result<Settled, HostError>, usize) {
    let (conn, fake) = fake_host(theirs, busy);
    let settled = settle(&conn, &build("ours")).await;
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
    let (settled, shutdowns) = settle_against("old", vec![s1()]).await;
    assert!(matches!(
        settled,
        Err(HostError::Busy { build, busy }) if build.0 == "old" && busy == [s1()]
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
    let again = settle_again(&conn, &build("ours")).await;
    drop(conn);
    assert!(matches!(again, Err(HostError::Protocol(why)) if why.contains("old")));
    assert_eq!(fake.await.unwrap(), 0);

    let (conn, fake) = fake_host("ours", vec![]);
    assert!(settle_again(&conn, &build("ours")).await.is_ok());
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
    let mut events = conn.watch(&s1(), None);
    let Question { id, .. } = host.ask().await;
    host.send(FromHost::Answer {
        id,
        result: Err("no such session".into()),
    })
    .await;
    assert!(matches!(
        made(&mut events).await,
        Some(AcpEvent::Disconnected(why)) if why == "no such session"
    ));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn a_refused_request_is_said_as_an_error() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: false,
            forkable: false,
        }),
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = events.next().await else {
        panic!("no Connected");
    };
    tx.send(AcpRequest::Cancel).unwrap();
    let Question { id, .. } = host.ask().await;
    host.send(FromHost::Answer {
        id,
        result: Err("the session ended".into()),
    })
    .await;
    assert!(matches!(
        made(&mut events).await,
        Some(AcpEvent::Error { text, .. }) if text == "the session ended"
    ));
}

#[tokio::test]
async fn a_live_ready_reuses_the_request_channel() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    let ready = Entry::Ready(Readiness {
        resumed: false,
        forkable: false,
    });
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
    let mut events = conn.watch(&s1(), None);
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
    let ready = Entry::Ready(Readiness {
        resumed: false,
        forkable: false,
    });
    let mut first = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(0, ready.clone()).await;
    host.entry(1, chunk("a")).await;
    assert!(matches!(
        first.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(next_chunk(&mut first).await, "a");

    let mut second = conn.watch(&s1(), None);
    let Question { id, .. } = host.ask().await;
    // Sent live to the first watch before the host read the second.
    host.entry(2, chunk("b")).await;
    host.send(FromHost::Replay {
        session: s1(),
        after: None,
    })
    .await;
    for (seq, entry) in [(0, ready), (1, chunk("a")), (2, chunk("b"))] {
        host.entry(seq, entry).await;
    }
    host.answer(id, Reply::Done).await;
    host.entry(3, chunk("c")).await;
    assert_eq!(next_chunk(&mut second).await, "a");
    assert_eq!(next_chunk(&mut second).await, "b");
    assert!(matches!(
        second.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(next_chunk(&mut second).await, "c");
}

#[tokio::test]
async fn a_replay_whose_first_line_is_unreadable_is_delivered_whole() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    let id = host.replaying().await;
    // Line 0 was torn, or is an entry this build cannot read.
    host.entry(1, chunk("a")).await;
    host.entry(
        2,
        Entry::Event(AcpEvent::Disconnected("the agent died".into())),
    )
    .await;
    host.answer(id, Reply::Done).await;
    assert_eq!(next_chunk(&mut events).await, "a");
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why == "the agent died"
    ));
    drop(host);
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn a_replay_gives_one_live_connected_after_its_last_event() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    let id = host.replaying().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: true,
            forkable: true,
        }),
    )
    .await;
    host.entry(1, chunk("a")).await;
    host.entry(
        2,
        Entry::Event(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        }),
    )
    .await;
    host.answer(id, Reply::Done).await;
    assert_eq!(next_chunk(&mut events).await, "a");
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::TurnEnded { .. })
    ));
    let Some(AcpEvent::Connected {
        tx,
        resumed: true,
        forkable: true,
    }) = events.next().await
    else {
        panic!("no live Connected after the replay");
    };
    tx.send(AcpRequest::Cancel).unwrap();
    let Question { call, .. } = host.ask().await;
    assert!(
        matches!(&call, Call::Request { session, request: AcpRequest::Cancel } if *session == s1()),
        "{call:?}"
    );
    host.entry(3, chunk("b")).await;
    assert_eq!(next_chunk(&mut events).await, "b");
}

#[tokio::test]
async fn a_replay_that_ended_the_session_gives_no_connected() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    let id = host.replaying().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: false,
            forkable: false,
        }),
    )
    .await;
    host.entry(1, Entry::Event(AcpEvent::Disconnected("gone".into())))
        .await;
    host.answer(id, Reply::Done).await;
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why == "gone"
    ));
    drop(host);
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn what_a_replay_makes_the_chat_send_never_reaches_the_host() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    let id = host.replaying().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: false,
            forkable: false,
        }),
    )
    .await;
    host.entry(
        1,
        Entry::Event(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        }),
    )
    .await;
    // The replay hands the `Chat` no channel, so the card cancels its
    // replayed turn end would send have nowhere to go.
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::TurnEnded { .. })
    ));
    host.answer(id, Reply::Done).await;
    let Some(AcpEvent::Connected { tx: live, .. }) = events.next().await else {
        panic!("no live Connected after the replay");
    };
    live.send(AcpRequest::Cancel).unwrap();
    let Question { call, .. } = host.ask().await;
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
async fn a_refused_watch_forgets_its_request_channel() {
    let (conn, mut host) = pair();
    let mut first = conn.watch(&s1(), None);
    host.watched().await;
    let ready = Entry::Ready(Readiness {
        resumed: false,
        forkable: false,
    });
    host.entry(0, ready.clone()).await;
    let Some(AcpEvent::Connected { tx: held, .. }) = first.next().await else {
        panic!("no Connected");
    };
    let mut refused = conn.watch(&s1(), None);
    let Question { id, .. } = host.ask().await;
    host.send(FromHost::Answer {
        id,
        result: Err("no such session".into()),
    })
    .await;
    assert!(matches!(
        made(&mut refused).await,
        Some(AcpEvent::Disconnected(_))
    ));
    let mut third = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(1, ready).await;
    let Some(AcpEvent::Connected { tx, .. }) = third.next().await else {
        panic!("no Connected");
    };
    assert!(
        !tx.same_channel(&held),
        "a refused watch kept the old channel"
    );
}

#[tokio::test]
async fn watching_again_ends_the_first_watch() {
    let (conn, mut host) = pair();
    let mut first = conn.watch(&s1(), None);
    let _second = conn.watch(&s1(), None);
    host.watched().await;
    host.watched().await;
    assert!(matches!(
        made(&mut first).await,
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
    tx.send(Cmd::Watch(s1(), watched, None)).ok().unwrap();
    refuse_queued(&mut rx, "gone");
    assert!(matches!(answer.await, Ok(Err(HostError::Unreachable(why))) if why == "gone"));
    assert!(matches!(
        made(&mut events).await,
        Some(AcpEvent::Disconnected(why)) if why == "gone"
    ));
    assert!(tx.send(Cmd::Request(s1(), AcpRequest::Cancel)).is_err());
}

#[tokio::test]
async fn a_card_answered_during_a_rewatch_reaches_the_host() {
    let (conn, mut host) = pair();
    let mut first = conn.watch(&s1(), None);
    host.watched().await;
    host.entry(
        0,
        Entry::Ready(Readiness {
            resumed: false,
            forkable: false,
        }),
    )
    .await;
    let Some(AcpEvent::Connected { tx, .. }) = first.next().await else {
        panic!("no Connected");
    };
    let _second = conn.watch(&s1(), None);
    let Question { call, .. } = host.ask().await;
    assert!(matches!(call, Call::Watch { .. }), "{call:?}");
    // The second watch is still replaying: its Watch has no answer yet.
    tx.send(AcpRequest::PermissionResponse {
        rpc_id: serde_json::json!(7),
        option_id: Some("allow".into()),
    })
    .unwrap();
    let Question { call, .. } = tokio::time::timeout(Duration::from_secs(5), host.ask())
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

#[tokio::test(start_paused = true)]
async fn an_unanswered_hello_ends_at_its_deadline() {
    let (conn, mut host) = pair();
    let said = tokio::spawn({
        let conn = conn.clone();
        async move { hello(&conn, &build("ours")).await }
    });
    let Question { id, call } = host.ask().await;
    assert!(matches!(call, Call::Hello { .. }));
    assert!(matches!(
        said.await.unwrap(),
        Err(HostError::Protocol(why)) if why == "the host did not answer within 30 s"
    ));
    // Its answer, come late, is let go, and the connection still serves.
    host.answer(id, Reply::Done).await;
    let listing = tokio::spawn(async move { conn.list().await });
    let Question { id, .. } = host.ask().await;
    host.answer(id, Reply::Sessions(vec![])).await;
    assert!(listing.await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn a_watch_starts_only_at_its_own_marker() {
    let (conn, mut host) = pair();
    let mut first = conn.watch(&s1(), None);
    let mut second = conn.watch(&s1(), None);
    let Question { id: one, .. } = host.ask().await;
    let Question { id: two, .. } = host.ask().await;
    // As the host sends them: each watch's replay, then its answer, in turn.
    // Told apart by their text, so the test sees whose replay was taken.
    for (id, text) in [(one, "first's"), (two, "second's")] {
        host.send(FromHost::Replay {
            session: s1(),
            after: None,
        })
        .await;
        host.entry(0, chunk(text)).await;
        host.entry(1, chunk("b")).await;
        host.answer(id, Reply::Done).await;
    }
    host.entry(2, chunk("c")).await;
    assert!(matches!(
        made(&mut first).await,
        Some(AcpEvent::Disconnected(why)) if why.contains("watched elsewhere")
    ));
    assert_eq!(next_chunk(&mut second).await, "second's");
    assert_eq!(next_chunk(&mut second).await, "b");
    assert_eq!(next_chunk(&mut second).await, "c");
    drop(host);
    assert!(matches!(
        made(&mut second).await,
        Some(AcpEvent::Disconnected(why)) if why.contains("lost the connection")
    ));
}

#[tokio::test]
async fn watch_says_the_time_before_entries_whose_second_changed() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    host.watched().await;
    host.timed(0, 100, chunk("a")).await;
    host.timed(1, 100, chunk("b")).await;
    host.timed(2, 105, chunk("c")).await;
    assert!(matches!(events.next().await, Some(AcpEvent::At(100))));
    assert_eq!(next_chunk(&mut events).await, "a");
    assert_eq!(next_chunk(&mut events).await, "b");
    assert!(matches!(events.next().await, Some(AcpEvent::At(105))));
    assert_eq!(next_chunk(&mut events).await, "c");
}

#[tokio::test]
async fn open_errors_name_the_host() {
    let (conn, mut host) = pair();
    let opening = tokio::spawn(async move {
        conn.open("/w".into(), AgentSpec::default(), Opening::Fresh)
            .await
    });
    let Question { id, .. } = host.ask().await;
    host.send(FromHost::Answer {
        id,
        result: Err("`claude` is not on this machine — install it there".into()),
    })
    .await;
    let opened = opening.await.unwrap();
    assert!(
        matches!(&opened, Err(HostError::Protocol(why))
            if why == "On vps: `claude` is not on this machine — install it there"),
        "{opened:?}"
    );
}

/// Every failure of a call names the host but a busy one, which is said where
/// the attach was turned down.
#[test]
fn every_open_error_but_busy_names_the_host() {
    let named = |e| on_host("vps", e).to_string();
    assert_eq!(
        named(HostError::Unsupported("Plan9 mips".into())),
        "On vps: Plan9 mips"
    );
    assert_eq!(named(HostError::Unreachable("gone".into())), "On vps: gone");
    assert_eq!(named(HostError::Protocol("no".into())), "On vps: no");
    let busy = HostError::Busy {
        build: Build("old".into()),
        busy: Vec::new(),
    };
    assert_eq!(named(busy), "the host runs another build (old)");
}

#[tokio::test]
async fn is_dir_asks_the_host() {
    let (conn, mut host) = pair();
    let probing = tokio::spawn(async move { conn.is_dir(Path::new("/srv/app")).await });
    let Question { id, call } = host.ask().await;
    assert!(
        matches!(&call, Call::Probe { path } if path == Path::new("/srv/app")),
        "{call:?}"
    );
    host.answer(id, Reply::Probed { is_dir: true }).await;
    assert!(probing.await.unwrap().unwrap());
}

/// A service on build `theirs`, with `busy` mid-turn, as one attach finds
/// it: it answers every `Hello`, and exits on `Shutdown`, counted in `shutdowns`.
fn service(theirs: &str, busy: Vec<HostSession>, shutdowns: Arc<AtomicUsize>) -> HostConn {
    let (conn, mut host) = pair();
    let theirs = build(theirs);
    tokio::spawn(async move {
        while let Ok(Some(line)) = host.asks.next_line().await {
            let Question { id, call } = serde_json::from_str(&line).unwrap();
            match call {
                Call::Hello { .. } => {
                    let welcome = Reply::Welcome {
                        build: theirs.clone(),
                        busy: busy.clone(),
                    };
                    host.answer(id, welcome).await;
                }
                Call::Shutdown => {
                    shutdowns.fetch_add(1, Ordering::SeqCst);
                    host.answer(id, Reply::Done).await;
                    return;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    });
    conn
}

/// Restart onto `ours` against services that come up on `builds` in turn,
/// each with a session mid-turn; the restart's end and the shutdowns sent.
async fn restart_against(builds: [&'static str; 2]) -> (Result<HostConn, HostError>, usize) {
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let mut builds = builds.into_iter();
    let count = shutdowns.clone();
    let dial = move || {
        let (_, said) = oneshot::channel();
        let theirs = builds.next().expect("no more than two attaches");
        Ok((service(theirs, vec![s1()], count.clone()), said))
    };
    let restarted = restart_over(dial, &build("ours")).await;
    (restarted, shutdowns.load(Ordering::SeqCst))
}

#[tokio::test]
async fn restart_shuts_down_a_busy_host_and_reattaches() {
    let (restarted, shutdowns) = restart_against(["old", "ours"]).await;
    assert!(restarted.is_ok(), "{:?}", restarted.err());
    assert_eq!(shutdowns, 1);
}

#[tokio::test]
async fn a_restart_that_finds_another_build_again_fails() {
    let (restarted, shutdowns) = restart_against(["old", "old"]).await;
    assert!(
        matches!(&restarted, Err(HostError::Protocol(why)) if why.contains("old")),
        "{:?}",
        restarted.err()
    );
    assert_eq!(shutdowns, 1);
}

/// The next event, one the client made itself: the `At` of now it comes after
/// is taken first.
async fn made(events: &mut (impl Stream<Item = AcpEvent> + Unpin)) -> Option<AcpEvent> {
    assert!(matches!(events.next().await, Some(AcpEvent::At(_))));
    events.next().await
}

#[tokio::test]
async fn what_the_client_says_itself_is_stamped_now() {
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), None);
    let id = host.replaying().await;
    host.timed(0, 100, chunk("a")).await;
    host.answer(id, Reply::Done).await;
    assert!(matches!(events.next().await, Some(AcpEvent::At(100))));
    assert_eq!(next_chunk(&mut events).await, "a");
    drop(host);
    assert!(matches!(events.next().await, Some(AcpEvent::At(now)) if now > 100));
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Disconnected(why)) if why.contains("lost the connection")
    ));
}

#[tokio::test]
async fn a_rewatch_says_the_time_again() {
    let (conn, mut host) = pair();
    let mut first = conn.watch(&s1(), None);
    host.watched().await;
    host.timed(0, 100, chunk("a")).await;
    assert!(matches!(first.next().await, Some(AcpEvent::At(100))));
    assert_eq!(next_chunk(&mut first).await, "a");
    let mut second = conn.watch(&s1(), None);
    let id = host.replaying().await;
    host.timed(0, 100, chunk("a")).await;
    host.answer(id, Reply::Done).await;
    assert!(matches!(second.next().await, Some(AcpEvent::At(100))));
    assert_eq!(next_chunk(&mut second).await, "a");
}

#[tokio::test]
async fn restart_on_our_build_sends_no_shutdown() {
    let (restarted, shutdowns) = restart_against(["ours", "ours"]).await;
    assert!(restarted.is_ok(), "{:?}", restarted.err());
    assert_eq!(shutdowns, 0);
}

#[tokio::test]
async fn a_connection_says_it_is_closed_once_its_stream_ends() {
    let (conn, host) = pair();
    assert!(!conn.is_closed());
    drop(host);
    // The pump notices the end on its next read.
    for _ in 0..100 {
        if conn.is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the connection never said it was closed");
}

/// A connection the host closed because another machine attached says it
/// was taken; one whose link merely went away does not.
#[tokio::test]
async fn a_kicked_connection_says_it_was_taken_and_a_dropped_one_does_not() {
    async fn closed(conn: &HostConn) {
        for _ in 0..100 {
            if conn.is_closed() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the connection never said it was closed");
    }
    let (kicked, mut host) = pair();
    host.send(FromHost::Kicked).await;
    closed(&kicked).await;
    assert!(kicked.taken());

    let (dropped, host) = pair();
    drop(host);
    closed(&dropped).await;
    assert!(!dropped.taken());
}

#[tokio::test]
async fn read_dir_takes_the_listing() {
    let (conn, mut host) = pair();
    let reading = tokio::spawn(async move { conn.read_dir(Path::new("/srv/app")).await });
    let Question { id, call } = host.ask().await;
    assert!(
        matches!(&call, Call::ReadDir { path } if path == Path::new("/srv/app")),
        "{call:?}"
    );
    let entry = |name: &str, is_dir| TreeEntry {
        path: Path::new("/srv/app").join(name),
        name: name.into(),
        is_dir,
    };
    let entries = vec![entry("src", true), entry("a.rs", false)];
    let listing = Reply::Listing {
        entries: entries.clone(),
        cut: 3,
    };
    host.answer(id, listing).await;
    assert_eq!(reading.await.unwrap().unwrap(), (entries, 3));
}

#[tokio::test]
async fn a_refused_read_file_is_a_protocol_error() {
    let (conn, mut host) = pair();
    let reading = tokio::spawn(async move { conn.read_file(Path::new("/srv/app/a.rs")).await });
    let Question { id, call } = host.ask().await;
    assert!(matches!(call, Call::ReadFile { .. }), "{call:?}");
    host.send(FromHost::Answer {
        id,
        result: Err("No such file".into()),
    })
    .await;
    match reading.await.unwrap() {
        Err(HostError::Protocol(why)) => assert!(why.contains("No such file"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_unexpected_answer_to_save_is_refused() {
    let (conn, mut host) = pair();
    let saving = tokio::spawn(async move {
        conn.save_file(Path::new("/srv/app/a.rs"), "x".into(), None)
            .await
    });
    let Question { id, call } = host.ask().await;
    assert!(matches!(call, Call::SaveFile { .. }), "{call:?}");
    host.answer(id, Reply::Done).await;
    assert!(matches!(saving.await.unwrap(), Err(HostError::Protocol(_))));
}

fn output(term: u64, text: &str) -> FromHost {
    FromHost::TermOutput {
        term: HostTerm(term),
        data: Bytes(text.into()),
    }
}

/// Everything `reader` gives until it ends, read on a thread of its own.
async fn read_to_end(mut reader: TermReader) -> String {
    use std::io::Read as _;
    let read = std::thread::spawn(move || {
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        text
    });
    tokio::task::spawn_blocking(move || read.join().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn term_output_reads_in_order_and_ends_at_exit() {
    let (conn, mut host) = pair();
    let reader = conn.term_watch(HostTerm(1));
    let Question { id, call } = host.ask().await;
    assert!(matches!(call, Call::TermWatch { term: HostTerm(1) }));
    host.send(output(1, "ab")).await;
    host.send(output(1, "cd")).await;
    host.answer(id, Reply::Done).await;
    host.send(FromHost::TermExit { term: HostTerm(1) }).await;
    assert_eq!(read_to_end(reader).await, "abcd");
}

#[tokio::test]
async fn a_newer_watch_of_a_term_ends_the_first_reader() {
    let (conn, mut host) = pair();
    let first = conn.term_watch(HostTerm(1));
    let second = conn.term_watch(HostTerm(1));
    host.ask().await;
    host.ask().await;
    host.send(output(1, "x")).await;
    host.send(FromHost::TermExit { term: HostTerm(1) }).await;
    assert_eq!(read_to_end(first).await, "");
    assert_eq!(read_to_end(second).await, "x");
}

#[tokio::test]
async fn writes_reach_the_host_in_order() {
    use std::io::Write as _;
    let (conn, mut host) = pair();
    let mut writer = conn.term_writer(HostTerm(2));
    let wrote = std::thread::spawn(move || {
        for i in 0..100 {
            writer.write_all(i.to_string().as_bytes()).unwrap();
        }
    });
    for i in 0..100 {
        let Question { call, .. } = host.ask().await;
        let Call::TermWrite { term, data } = call else {
            panic!("not a write: {call:?}")
        };
        assert_eq!(term, HostTerm(2));
        assert_eq!(data.0, i.to_string().into_bytes());
    }
    wrote.join().unwrap();
}

#[tokio::test]
async fn a_lost_connection_ends_every_terminal_reader() {
    let (conn, mut host) = pair();
    let reader = conn.term_watch(HostTerm(1));
    host.ask().await;
    drop(host);
    assert_eq!(read_to_end(reader).await, "");
}

#[test]
fn a_recording_connection_keeps_what_was_fired_and_needs_no_runtime() {
    let (conn, recorded) = HostConn::recording("vps");
    conn.term_resize(HostTerm(3), 80, 24);
    conn.term_close(HostTerm(3));
    let fired = recorded.fired();
    assert!(matches!(
        fired.as_slice(),
        [
            Call::TermResize {
                term: HostTerm(3),
                cols: 80,
                rows: 24
            },
            Call::TermClose { term: HostTerm(3) }
        ]
    ));
    assert!(recorded.fired().is_empty());
}

#[tokio::test]
async fn home_reads_the_reply() {
    let (conn, mut host) = pair();
    let asking = tokio::spawn(async move { conn.home().await });
    let Question { id, call } = host.ask().await;
    assert!(matches!(call, Call::Home), "{call:?}");
    host.answer(id, Reply::Home("/home/me".into())).await;
    assert_eq!(asking.await.unwrap().unwrap(), Path::new("/home/me"));
}

#[tokio::test(start_paused = true)]
async fn clone_waits_past_ask_wait() {
    let (conn, mut host) = pair();
    let cloning = tokio::spawn(async move {
        conn.clone_repo("https://x/o/repo.git", Path::new("/srv/repo"))
            .await
    });
    let Question { id, call } = host.ask().await;
    assert!(
        matches!(&call, Call::Clone { url, into }
            if url == "https://x/o/repo.git" && into == Path::new("/srv/repo")),
        "{call:?}"
    );
    tokio::time::sleep(Duration::from_secs(60)).await;
    host.answer(id, Reply::Done).await;
    assert!(cloning.await.unwrap().is_ok());
}

#[tokio::test]
async fn a_refused_clone_says_the_hosts_words() {
    let (conn, mut host) = pair();
    let cloning = tokio::spawn(async move {
        conn.clone_repo("https://x/o/repo.git", Path::new("/srv/repo"))
            .await
    });
    let Question { id, .. } = host.ask().await;
    host.send(FromHost::Answer {
        id,
        result: Err("there is already /srv/repo".into()),
    })
    .await;
    match cloning.await.unwrap() {
        Err(HostError::Protocol(why)) => assert_eq!(why, "there is already /srv/repo"),
        other => panic!("{other:?}"),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn copy_in(root: &Path) -> super::super::copy::Copy {
    super::super::copy::Copy::open(root, &crate::workspace::HostName("vps".into()), &s1())
}

fn copy_seqs(root: &Path) -> Vec<u64> {
    copy_in(root)
        .read()
        .iter()
        .map(|(seq, _, _)| *seq)
        .collect()
}

/// A copy holding seqs 0..=2: `Ready`, then chunks `a` and `b`.
fn copy_of_three(root: &Path) -> super::super::copy::Copy {
    let mut copy = copy_in(root);
    let ready = Entry::Ready(Readiness {
        resumed: false,
        forkable: false,
    });
    copy.append(0, 0, &ready).unwrap();
    copy.append(1, 0, &chunk("a")).unwrap();
    copy.append(2, 0, &chunk("b")).unwrap();
    copy
}

#[tokio::test]
async fn watch_with_copy_yields_copy_then_tail() {
    let root = scratch("tail");
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), Some(copy_of_three(&root)));
    let Question { id, call } = host.ask().await;
    assert!(
        matches!(call, Call::Watch { after: Some(2), .. }),
        "{call:?}"
    );
    host.send(FromHost::Replay {
        session: s1(),
        after: Some(2),
    })
    .await;
    host.entry(3, chunk("c")).await;
    host.entry(4, chunk("d")).await;
    host.answer(id, Reply::Done).await;
    for want in ["a", "b", "c", "d"] {
        assert_eq!(next_chunk(&mut events).await, want);
    }
    assert!(matches!(
        events.next().await,
        Some(AcpEvent::Connected { .. })
    ));
    assert_eq!(copy_seqs(&root), [0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn watch_with_longer_copy_drops_it() {
    let root = scratch("longer");
    let (conn, mut host) = pair();
    let mut events = conn.watch(&s1(), Some(copy_of_three(&root)));
    let Question { id, .. } = host.ask().await;
    // The host's journal is shorter than the copy: it replays from the start.
    host.send(FromHost::Replay {
        session: s1(),
        after: None,
    })
    .await;
    host.entry(0, chunk("x")).await;
    host.entry(1, chunk("y")).await;
    host.answer(id, Reply::Done).await;
    assert_eq!(next_chunk(&mut events).await, "x");
    assert_eq!(next_chunk(&mut events).await, "y");
    // Nothing from the copy came between them.
    host.entry(2, chunk("z")).await;
    assert_eq!(next_chunk(&mut events).await, "z");
    let seqs = copy_seqs(&root);
    assert_eq!(seqs, [0, 1, 2]);
    let texts: Vec<_> = super::super::copy::events(&copy_in(&root).read())
        .into_iter()
        .map(|e| match e {
            AcpEvent::AgentChunk { text, .. } => text,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(texts, ["x", "y", "z"]);
}

#[tokio::test]
async fn watch_with_a_broken_copy_clears_it_and_asks_for_all() {
    let root = scratch("broken");
    drop(copy_of_three(&root));
    let path = root.join("vps").join("s1.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.insert_str(0, "not json\n");
    std::fs::write(&path, text).unwrap();
    let (conn, mut host) = pair();
    let _events = conn.watch(&s1(), Some(copy_in(&root)));
    let Question { call, .. } = host.ask().await;
    assert!(matches!(call, Call::Watch { after: None, .. }), "{call:?}");
    assert!(!path.exists());
}
